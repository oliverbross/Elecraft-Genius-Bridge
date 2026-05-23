use anyhow::Context;
use bridge_core::{
    append_evidence_json, append_evidence_line, parse_client_command, response_line,
    ConnectionState, LifecycleState, ManualTuneRequest, ProtocolClientSession, SharedState,
};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;
use tracing::{debug, info, warn};

const VERSION: &str = "0.1.0-egb-tgxl";

#[derive(Debug, Clone, Default)]
pub struct EmulatorOptions {
    pub protocol_trace: bool,
    pub transcript_dir: Option<PathBuf>,
    pub transcript_rotate_bytes: u64,
    pub aethersdr_compat: bool,
    pub control_profile: String,
    pub strict_emulation: bool,
    pub startup_delay: Duration,
    pub force_presence_test: bool,
}

pub async fn run(bind_addr: SocketAddr, state: SharedState) -> anyhow::Result<()> {
    run_with_options(bind_addr, state, EmulatorOptions::default()).await
}

pub async fn run_with_options(
    bind_addr: SocketAddr,
    state: SharedState,
    options: EmulatorOptions,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind TGXL emulator on {bind_addr}"))?;
    info!(%bind_addr, "TGXL emulator listening");
    append_evidence_line(
        "listener-startup.log",
        format!("TGXL listener started bind_addr={bind_addr}"),
    );

    loop {
        let (socket, peer) = listener.accept().await?;
        append_evidence_line(
            "listener-startup.log",
            format!("TGXL accept peer={peer} bind_addr={bind_addr}"),
        );
        let state = state.clone();
        let options = options.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_client(socket, peer, state, options).await {
                warn!(%peer, error = %err, "TGXL client ended with error");
            }
        });
    }
}

pub async fn replay_line(
    line: &str,
    state: &SharedState,
) -> Result<Vec<String>, bridge_core::ProtocolError> {
    let cmd = parse_client_command(line)?;
    let outcome = handle_command(cmd.seq, &cmd.command, state, false, "readonly").await;
    let mut lines = vec![outcome.response];
    lines.extend(outcome.pushes);
    Ok(lines)
}

async fn handle_client(
    socket: TcpStream,
    peer: SocketAddr,
    state: SharedState,
    options: EmulatorOptions,
) -> anyhow::Result<()> {
    let session_id = {
        let mut guard = state.write().await;
        guard.clients.tgxl_connected = true;
        guard.clients.tgxl_client_count += 1;
        guard.clients.tgxl_session_started_count =
            guard.clients.tgxl_session_started_count.saturating_add(1);
        guard.clients.next_session_id = guard.clients.next_session_id.saturating_add(1);
        let id = guard.clients.next_session_id;
        guard.clients.tgxl_sessions.push(ProtocolClientSession::new(
            id,
            "TGXL",
            peer,
            timestamp_millis(),
        ));
        guard.lifecycle.tgxl.transition(
            LifecycleState::TcpConnected,
            format!("TGXL TCP client connected: {peer}"),
        );
        guard.lifecycle.aether_client.transition(
            LifecycleState::TcpConnected,
            format!("Aether client connected to TGXL: {peer}"),
        );
        id
    };
    append_evidence_json(
        "client-sessions.jsonl",
        &serde_json::json!({
            "event": "client_connected",
            "protocol": "TGXL",
            "peer": peer.to_string(),
            "session_id": session_id,
        }),
    );
    maybe_start_strict_startup(&state, &options).await;
    if options.force_presence_test {
        info!(
            event_id = "tgxl_direct_presence_test",
            protocol = "TGXL",
            connection_id = %peer,
            "TGXL direct presence test mode enabled"
        );
        apply_direct_presence_test_state(&state).await;
    }
    info!(event_id = "client_connected", protocol = "TGXL", connection_id = %peer, "TGXL client connected");

    let result = async {
        let (reader, mut writer) = socket.into_split();
        let mut transcript = Transcript::new(
            "tgxl",
            peer,
            options.transcript_dir.clone(),
            options.transcript_rotate_bytes,
        )
        .await;
        let mut stats = SessionStats::new(peer);
        let greeting = format!("V{VERSION}");
        write_protocol_line(
            &mut writer,
            &mut transcript,
            "TGXL",
            "TX >",
            &greeting,
            options.protocol_trace,
        )
        .await
        .context("failed to write TGXL version greeting")?;
        stats.responses_sent += 1;
        update_tgxl_session_response(&state, session_id, 0).await;
        increment_responses(&state).await;

        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await? {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            trace_protocol_line(
                &mut transcript,
                "TGXL",
                "RX <",
                &line,
                options.protocol_trace,
            )
            .await?;
            debug!(%peer, command_line_len = line.len(), "TGXL command received");
            match parse_client_command(&line) {
                Ok(cmd) => {
                    stats.commands_received += 1;
                    increment_commands(&state).await;
                    if let Some(previous) = stats.last_sequence {
                        if cmd.seq <= previous {
                            stats.unexpected_sequences += 1;
                            increment_unexpected_sequence(&state).await;
                            warn!(
                                event_id = "protocol_parse_failure",
                                protocol = "TGXL",
                                raw_line = %line,
                                connection_id = %peer,
                                parser_reason = "unexpected_sequence",
                                previous_sequence = previous,
                                current_sequence = cmd.seq,
                                "unexpected TGXL command sequence"
                            );
                        }
                    }
                    stats.last_sequence = Some(cmd.seq);
                    stats.observe_command(&cmd.command);
                    update_tgxl_session_command(&state, session_id, &cmd.command).await;
                    let command_started = Instant::now();
                    let outcome = handle_command(
                        cmd.seq,
                        &cmd.command,
                        &state,
                        options.aethersdr_compat,
                        &options.control_profile,
                    )
                    .await;
                    if outcome.unknown {
                        stats.unknown_commands += 1;
                        update_tgxl_session_unknown(&state, session_id).await;
                        increment_unknown(&state).await;
                        warn!(
                            event_id = "protocol_parse_failure",
                            protocol = "TGXL",
                            raw_line = %line,
                            connection_id = %peer,
                            parser_reason = "unknown_command",
                            "unknown TGXL command"
                        );
                    }
                    if outcome.unsupported {
                        stats.unsupported_features += 1;
                        increment_unsupported(&state).await;
                    }
                    debug!(%peer, command = %cmd.command, "TGXL command handled");
                    let latency_ms = duration_millis_u64(command_started.elapsed());
                    write_protocol_line(
                        &mut writer,
                        &mut transcript,
                        "TGXL",
                        "TX >",
                        outcome.response.trim_end(),
                        options.protocol_trace,
                    )
                    .await?;
                    stats.responses_sent += 1;
                    update_tgxl_session_response(&state, session_id, latency_ms).await;
                    increment_responses(&state).await;
                    for push in outcome.pushes {
                        write_protocol_line(
                            &mut writer,
                            &mut transcript,
                            "TGXL",
                            "TX >",
                            push.trim_end(),
                            options.protocol_trace,
                        )
                        .await?;
                        stats.responses_sent += 1;
                        update_tgxl_session_response(&state, session_id, 0).await;
                        increment_responses(&state).await;
                    }
                }
                Err(err) => {
                    stats.parse_failures += 1;
                    update_tgxl_session_parse_failure(&state, session_id).await;
                    increment_parse_failure(&state).await;
                    warn!(
                        event_id = "protocol_parse_failure",
                        protocol = "TGXL",
                        raw_line = %line,
                        connection_id = %peer,
                        parser_reason = %err,
                        "invalid TGXL command frame"
                    );
                }
            }
        }
        stats.log_summary("TGXL");
        Ok::<(), anyhow::Error>(())
    }
    .await;

    {
        let mut guard = state.write().await;
        guard.clients.tgxl_client_count = guard.clients.tgxl_client_count.saturating_sub(1);
        guard.clients.tgxl_connected = guard.clients.tgxl_client_count > 0;
        guard
            .clients
            .tgxl_sessions
            .retain(|session| session.id != session_id);
        guard.clients.tgxl_last_disconnect_reason = Some(match &result {
            Ok(()) => "client_closed".to_string(),
            Err(err) => err.to_string(),
        });
        let reason = guard
            .clients
            .tgxl_last_disconnect_reason
            .clone()
            .unwrap_or_else(|| "client_closed".to_string());
        if guard.clients.tgxl_client_count == 0 {
            guard.lifecycle.tgxl.transition(
                LifecycleState::Degraded,
                format!("TGXL client disconnected: {reason}"),
            );
        }
    }
    append_evidence_json(
        "disconnect-events.jsonl",
        &serde_json::json!({
            "event": "client_disconnected",
            "protocol": "TGXL",
            "peer": peer.to_string(),
            "session_id": session_id,
            "reason": result.as_ref().err().map(|err| err.to_string()).unwrap_or_else(|| "client_closed".to_string()),
        }),
    );
    info!(event_id = "client_disconnected", protocol = "TGXL", connection_id = %peer, "TGXL client disconnected");
    result
}

async fn apply_direct_presence_test_state(state: &SharedState) {
    let mut guard = state.write().await;
    guard.tuner.connected = true;
    guard.tuner.connection_state = ConnectionState::Connected;
    guard.tuner.operate = true;
    guard.tuner.bypass = false;
    guard.tuner.tuning = false;
    guard.tuner.selected_antenna = Some(0);
    guard.tuner.relay_c1 = 20;
    guard.tuner.relay_l = 35;
    guard.tuner.relay_c2 = 20;
    guard.tuner.forward_power_watts = 0.0;
    guard.tuner.swr = 1.0;
    guard.tuner.fault = None;
    guard.tuner.firmware_version = Some(VERSION.to_string());
    for capability in ["direct-presence-test", "direct-tcp", "one-by-three"] {
        if !guard
            .tuner
            .capabilities
            .iter()
            .any(|existing| existing == capability)
        {
            guard.tuner.capabilities.push(capability.to_string());
        }
    }
    guard.tuner.last_successful_poll_at = Some(SystemTime::now());
}

struct CommandOutcome {
    response: String,
    pushes: Vec<String>,
    unknown: bool,
    unsupported: bool,
}

impl CommandOutcome {
    fn ok(response: String) -> Self {
        Self {
            response,
            pushes: Vec::new(),
            unknown: false,
            unsupported: false,
        }
    }

    fn with_pushes(response: String, pushes: Vec<String>) -> Self {
        Self {
            response,
            pushes,
            unknown: false,
            unsupported: false,
        }
    }
}

async fn handle_command(
    seq: u32,
    command: &str,
    state: &SharedState,
    aethersdr_compat: bool,
    control_profile: &str,
) -> CommandOutcome {
    match command {
        _ if command.starts_with("auth ") => CommandOutcome::ok(response_line(seq, 0, "auth OK")),
        "btl" => CommandOutcome {
            response: response_line(seq, 5, "error=bootloader_unsupported_by_bridge"),
            pushes: Vec::new(),
            unknown: false,
            unsupported: true,
        },
        "info" => CommandOutcome::ok(response_line(seq, 0, info_body(aethersdr_compat))),
        "status" => {
            CommandOutcome::ok(status_line(seq, state, aethersdr_compat, control_profile).await)
        }
        "ifconf read" | "network" | "network read" => {
            CommandOutcome::ok(response_line(seq, 0, network_body()))
        }
        _ if command.starts_with("ifconf set ") => CommandOutcome::ok(response_line(seq, 0, "0")),
        "setup read" => CommandOutcome::ok(response_line(seq, 0, setup_body())),
        _ if command.starts_with("setup set ") => CommandOutcome::ok(response_line(seq, 0, "")),
        "catradio read" => CommandOutcome::with_pushes(
            response_line(seq, 0, catradio_body(1)),
            vec![response_line(seq, 0, catradio_body(2))],
        ),
        _ if command.starts_with("catradio get ") => {
            let ch = command_channel(command).unwrap_or(1).clamp(1, 2);
            CommandOutcome::ok(response_line(seq, 0, catradio_body(ch)))
        }
        _ if command.starts_with("catradio set ") => CommandOutcome::ok(response_line(seq, 0, "")),
        "flexradio list" => {
            let (radio, final_response) = flexradio_list_lines(seq, state).await;
            CommandOutcome::with_pushes(radio, vec![final_response])
        }
        "flexradio read" => {
            let (ch1, ch2) = flexradio_read_lines(seq, state).await;
            CommandOutcome::with_pushes(ch1, vec![ch2])
        }
        _ if command.starts_with("flexradio get ") => {
            let ch = command_channel(command).unwrap_or(1).clamp(1, 2);
            CommandOutcome::ok(flexradio_get_line(seq, state, ch).await)
        }
        _ if command.starts_with("flexradio set ") => {
            record_flexradio_set(command, state).await;
            CommandOutcome::ok(response_line(seq, 0, ""))
        }
        "save" => CommandOutcome::ok(response_line(seq, 0, "")),
        _ if command.starts_with("operate set=") => {
            let requested = command_bool(command, "set");
            if let Some(operate) = requested {
                let mut guard = state.write().await;
                guard.controls.aethersdr_button_command_seen = true;
                guard.controls.control_requested_count =
                    guard.controls.control_requested_count.saturating_add(1);
                guard.controls.last_tgxl_control_command = Some(command.to_string());
                guard.controls.last_mapped_elecraft_action =
                    Some(format!("TGXL operate set={}", bool_int(operate)));
                guard.controls.last_safety_decision =
                    Some("tgxl_operate_state_requested".to_string());
                guard.tuner.operate = operate;
                CommandOutcome::ok(response_line(seq, 0, ""))
            } else {
                CommandOutcome {
                    response: response_line(seq, 2, "error=invalid_operate"),
                    pushes: Vec::new(),
                    unknown: false,
                    unsupported: true,
                }
            }
        }
        _ if command.starts_with("bypass set=") => {
            let requested = command_bool(command, "set");
            if let Some(bypass) = requested {
                let mut guard = state.write().await;
                guard.controls.aethersdr_button_command_seen = true;
                guard.controls.control_requested_count =
                    guard.controls.control_requested_count.saturating_add(1);
                guard.controls.last_tgxl_control_command = Some(command.to_string());
                guard.controls.last_mapped_elecraft_action = Some(if bypass {
                    "KAT500 BYP;".to_string()
                } else {
                    "KAT500 BYPN;".to_string()
                });
                guard.controls.last_safety_decision = Some("desired_bypass_requested".to_string());
                guard.desired.tuner_bypass = Some(bypass);
                drop(guard);
                append_evidence_json(
                    "control-events.jsonl",
                    &serde_json::json!({
                        "protocol": "TGXL",
                        "raw": command,
                        "mapped_action": if bypass { "KAT500 BYP;" } else { "KAT500 BYPN;" },
                        "safety_decision": "desired_bypass_requested",
                    }),
                );
                CommandOutcome::ok(response_line(seq, 0, ""))
            } else {
                CommandOutcome {
                    response: response_line(seq, 2, "error=invalid_bypass"),
                    pushes: Vec::new(),
                    unknown: false,
                    unsupported: true,
                }
            }
        }
        "autotune" => {
            {
                let mut guard = state.write().await;
                guard.controls.aethersdr_button_command_seen = true;
                guard.controls.control_requested_count =
                    guard.controls.control_requested_count.saturating_add(1);
                guard.controls.last_tgxl_control_command = Some(command.to_string());
                guard.controls.last_mapped_elecraft_action = Some("KAT500 T;".to_string());
                guard.controls.last_safety_decision =
                    Some("desired_autotune_requested".to_string());
                guard.controls.tune_requested_count =
                    guard.controls.tune_requested_count.saturating_add(1);
                guard.lifecycle.tune.transition(
                    bridge_core::state::TuneLifecycleState::TuneRequested,
                    "TGXL autotune command received",
                );
                guard.desired.tuner_autotune_requested = true;
            }
            append_evidence_json(
                "control-events.jsonl",
                &serde_json::json!({
                    "protocol": "TGXL",
                    "raw": command,
                    "mapped_action": "KAT500 T;",
                    "safety_decision": "desired_autotune_requested",
                }),
            );
            append_evidence_line(
                "tgxl-control-commands.log",
                format!("RX {command} -> KAT500 T; desired_autotune_requested"),
            );
            let pushes = vec![state_push(state, aethersdr_compat, control_profile).await];
            sleep(Duration::from_millis(800)).await;
            CommandOutcome::with_pushes(response_line(seq, 0, ""), pushes)
        }
        _ if command.starts_with("activate ant=") => {
            let ant = command
                .trim_start_matches("activate ant=")
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=3).contains(n));
            if let Some(ant) = ant {
                let mut guard = state.write().await;
                guard.controls.aethersdr_button_command_seen = true;
                guard.controls.control_requested_count =
                    guard.controls.control_requested_count.saturating_add(1);
                guard.controls.last_tgxl_control_command = Some(command.to_string());
                guard.controls.last_mapped_elecraft_action = Some(format!("KAT500 AN{ant};"));
                guard.controls.last_safety_decision = Some("desired_antenna_requested".to_string());
                guard.desired.tuner_selected_antenna = Some(ant);
                append_evidence_json(
                    "control-events.jsonl",
                    &serde_json::json!({
                        "protocol": "TGXL",
                        "raw": command,
                        "mapped_action": format!("KAT500 AN{ant};"),
                        "safety_decision": "desired_antenna_requested",
                    }),
                );
                append_evidence_line(
                    "tgxl-control-commands.log",
                    format!("RX {command} -> KAT500 AN{ant}; desired_antenna_requested"),
                );
                CommandOutcome {
                    response: response_line(seq, 0, ""),
                    pushes: vec![state_push_from_tuner(
                        &guard,
                        aethersdr_compat,
                        control_profile,
                    )],
                    unknown: false,
                    unsupported: false,
                }
            } else {
                CommandOutcome {
                    response: response_line(seq, 2, "error=invalid_antenna"),
                    pushes: Vec::new(),
                    unknown: false,
                    unsupported: true,
                }
            }
        }
        _ if command.starts_with("activate ch=") => {
            let ch = command
                .trim_start_matches("activate ch=")
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=2).contains(n));
            if let Some(ch) = ch {
                let mut guard = state.write().await;
                guard.controls.aethersdr_button_command_seen = true;
                guard.controls.control_requested_count =
                    guard.controls.control_requested_count.saturating_add(1);
                guard.controls.last_tgxl_control_command = Some(command.to_string());
                guard.controls.last_mapped_elecraft_action =
                    Some(format!("TGXL active channel set to {ch}"));
                guard.controls.last_safety_decision =
                    Some("tgxl_active_channel_requested".to_string());
                append_evidence_json(
                    "control-events.jsonl",
                    &serde_json::json!({
                        "protocol": "TGXL",
                        "raw": command,
                        "mapped_action": format!("TGXL active channel set to {ch}"),
                        "safety_decision": "tgxl_active_channel_requested",
                    }),
                );
                CommandOutcome::ok(response_line(seq, 0, ""))
            } else {
                CommandOutcome {
                    response: response_line(seq, 2, "error=invalid_channel"),
                    pushes: Vec::new(),
                    unknown: false,
                    unsupported: true,
                }
            }
        }
        _ if command.starts_with("tune relay=") => {
            let result = apply_relay_command(command, state).await;
            match result {
                Ok(()) => CommandOutcome::with_pushes(
                    response_line(seq, 0, ""),
                    vec![state_push(state, aethersdr_compat, control_profile).await],
                ),
                Err(error) => CommandOutcome {
                    response: response_line(seq, 2, format!("error={error}")),
                    pushes: Vec::new(),
                    unknown: false,
                    unsupported: true,
                },
            }
        }
        _ => CommandOutcome {
            response: response_line(seq, 1, "error=unknown_command"),
            pushes: Vec::new(),
            unknown: true,
            unsupported: false,
        },
    }
}

async fn maybe_start_strict_startup(state: &SharedState, options: &EmulatorOptions) {
    if !options.strict_emulation || options.startup_delay.is_zero() {
        return;
    }

    {
        let mut guard = state.write().await;
        guard.tuner.connection_state = ConnectionState::Connecting;
        guard.tuner.connected = false;
        guard.tuner.tuning = true;
        guard.tuner.fault = None;
    }

    let state = state.clone();
    let delay = options.startup_delay;
    tokio::spawn(async move {
        sleep(delay).await;
        let mut guard = state.write().await;
        guard.tuner.connection_state = ConnectionState::Connected;
        guard.tuner.connected = true;
        guard.tuner.tuning = false;
        guard.tuner.last_successful_poll_at = Some(SystemTime::now());
        info!(
            event_id = "strict_startup_complete",
            protocol = "TGXL",
            startup_delay_ms = delay.as_millis(),
            "TGXL strict emulation startup complete"
        );
    });
}

fn info_body(aethersdr_compat: bool) -> String {
    if aethersdr_compat {
        format!(
            "info serial=EGB-TGXL serial_num=EGB-TGXL version={VERSION} nickname=Tuner_Genius_XL 3way=1 model=TunerGeniusXL one_by_three=1"
        )
    } else {
        format!(
            "info serial=EGB-TGXL serial_num=EGB-TGXL version={VERSION} firmware={VERSION} nickname=Tuner_Genius_XL 3way=1 model=TunerGeniusXL one_by_three=1 capabilities=direct_tcp,status,autotune,ant,manual_tune,flexradio,catradio,setup"
        )
    }
}

fn network_body() -> &'static str {
    "ifconf dhcp=1 ip=0.0.0.0 netmask=255.255.255.0 gateway=0.0.0.0"
}

fn setup_body() -> &'static str {
    "setup nickname=Tuner_Genius_XL code= backlight=128 bypass1=0 bypass2=0 tuneptt1=1 tuneptt2=1"
}

fn catradio_body(ch: u8) -> String {
    format!("catradio ch={ch} active=0 type=KENWOOD baud=4800 control=8N2 civ=0")
}

async fn flexradio_list_lines(seq: u32, state: &SharedState) -> (String, String) {
    let guard = state.read().await;
    let serial = guard
        .radio_context
        .radio_serial
        .as_deref()
        .unwrap_or("EGB-FLEX");
    let nickname = guard
        .radio_context
        .radio_nickname
        .as_deref()
        .unwrap_or("FlexRadio");
    let callsign = guard.radio_context.radio_callsign.as_deref().unwrap_or("");
    (
        response_line(
            seq,
            0,
            format!("radio serial={serial} nickname={nickname} callsign={callsign}"),
        ),
        response_line(seq, 0, ""),
    )
}

async fn flexradio_read_lines(seq: u32, state: &SharedState) -> (String, String) {
    (
        flexradio_get_line(seq, state, 1).await,
        flexradio_get_line(seq, state, 2).await,
    )
}

async fn flexradio_get_line(seq: u32, state: &SharedState, ch: u8) -> String {
    let guard = state.read().await;
    let serial = guard
        .radio_context
        .radio_serial
        .as_deref()
        .unwrap_or("EGB-FLEX");
    let antenna = if ch == 2 {
        guard.radio_context.tx_antenna.as_deref().unwrap_or("ANT2")
    } else {
        guard.radio_context.tx_antenna.as_deref().unwrap_or("ANT1")
    };
    response_line(
        seq,
        0,
        format!("flexradio ch={ch} active=1 serial={serial} antenna={antenna} source=LAN"),
    )
}

async fn record_flexradio_set(command: &str, state: &SharedState) {
    let mut guard = state.write().await;
    guard.controls.aethersdr_button_command_seen = true;
    guard.controls.control_requested_count =
        guard.controls.control_requested_count.saturating_add(1);
    guard.controls.last_tgxl_control_command = Some(command.to_string());
    guard.controls.last_mapped_elecraft_action = Some("TGXL flexradio config update".to_string());
    guard.controls.last_safety_decision = Some("tgxl_flexradio_config_accepted".to_string());
    if let Some(serial) = command_token(command, "serial") {
        guard.radio_context.radio_serial = Some(serial.to_string());
    }
    if let Some(antenna) = command_token(command, "antenna") {
        guard.radio_context.tx_antenna = Some(antenna.to_string());
    }
    guard.radio_context.source = Some("tgxl_flexradio_set".to_string());
    guard.radio_context.updated_at = Some(SystemTime::now());
    drop(guard);
    append_evidence_line(
        "tgxl-flexradio-command-samples.log",
        format!("RX {command} -> accepted"),
    );
}

async fn apply_relay_command(command: &str, state: &SharedState) -> Result<(), &'static str> {
    let mut relay = None;
    let mut movement = None;
    for part in command.split_whitespace() {
        if let Some(value) = part.strip_prefix("relay=") {
            relay = value.parse::<u8>().ok();
        } else if let Some(value) = part.strip_prefix("move=") {
            movement = value.parse::<i32>().ok();
        }
    }

    let relay = relay.ok_or("missing_relay")?;
    let movement = movement.ok_or("missing_move")?;
    let internal_relay = match relay {
        // Compatibility with older AetherSDR traces that used zero-based relay ids.
        0 => 0,
        // Official TGXL API: 1=C1, 2=L, 3=C2.
        1..=3 => relay - 1,
        _ => return Err("invalid_relay"),
    };
    if !matches!(movement, -1 | 1) {
        return Err("invalid_move");
    }

    let mut guard = state.write().await;
    guard.controls.aethersdr_button_command_seen = true;
    guard.controls.control_requested_count =
        guard.controls.control_requested_count.saturating_add(1);
    guard.controls.last_tgxl_control_command = Some(command.to_string());
    guard.controls.last_mapped_elecraft_action = Some(format!(
        "KAT500 manual relay={} move={} (unverified)",
        internal_relay, movement
    ));
    guard.controls.last_safety_decision = Some("desired_manual_tune_requested".to_string());
    guard.desired.tuner_manual_tune = Some(ManualTuneRequest {
        relay: internal_relay,
        movement,
    });
    append_evidence_json(
        "control-events.jsonl",
        &serde_json::json!({
            "protocol": "TGXL",
            "raw": command,
            "mapped_action": format!("KAT500 manual relay={internal_relay} move={movement}"),
            "safety_decision": "desired_manual_tune_requested",
        }),
    );
    append_evidence_line(
        "tgxl-control-commands.log",
        format!("RX {command} -> KAT500 manual relay={internal_relay} move={movement} desired_manual_tune_requested"),
    );
    Ok(())
}

async fn status_line(
    seq: u32,
    state: &SharedState,
    aethersdr_compat: bool,
    control_profile: &str,
) -> String {
    let (body, advertised_operate) = {
        let guard = state.read().await;
        (
            status_body_from_state(&guard, aethersdr_compat, control_profile),
            advertised_operate_from_tuner(&guard.tuner, aethersdr_compat, control_profile),
        )
    };
    let should_update = {
        let guard = state.read().await;
        guard.flex_injection.last_advertised_tgxl_operate != Some(advertised_operate)
    };
    if should_update {
        let mut guard = state.write().await;
        guard.flex_injection.last_advertised_tgxl_operate = Some(advertised_operate);
    }
    let line = format!("S{seq}|status {body}\n");
    append_evidence_line("tgxl-status-samples.log", line.trim_end());
    line
}

fn status_body_from_state(
    state: &bridge_core::state::BridgeState,
    aethersdr_compat: bool,
    control_profile: &str,
) -> String {
    let tuner = &state.tuner;
    let fwd = watts_to_dbm(tuner.forward_power_watts);
    let peak = fwd;
    let max = fwd.max(0.0);
    let swr = protocol_swr_value(tuner.swr);
    let degraded = matches!(
        tuner.connection_state,
        ConnectionState::Disconnected | ConnectionState::Degraded | ConnectionState::Error
    );
    let operate = advertised_operate_from_tuner(tuner, aethersdr_compat, control_profile);
    let frequency_mhz = state
        .radio_context
        .frequency_hz
        .map(|hz| hz as f64 / 1_000_000.0)
        .unwrap_or(0.0);
    let band = band_number(state.radio_context.band);
    let flex = state
        .radio_context
        .radio_nickname
        .as_deref()
        .unwrap_or("FlexRadio");
    let mode = if state.radio_context.frequency_hz.is_some() {
        1
    } else {
        0
    };
    let ant = tuner.selected_antenna.unwrap_or(0);
    let status = format!(
        "fwd={fwd:.2} peak={peak:.2} max={max:.2} swr={swr:.4} \
pttA=0 bandA={band} modeA={mode} flexA={flex} freqA={frequency_mhz:.6} bypassA={} bypassRxA=0 antA={ant} \
pttB=0 bandB=0 modeB=0 flexB= freqB=0.000 bypassB=0 bypassRxB=0 antB=0 \
state={} active=1 tuning={} bypass={} ag=0 relayC1={} relayL={} relayC2={}",
        bool_int(tuner.bypass),
        bool_int(operate),
        bool_int(tuner.tuning || degraded),
        bool_int(tuner.bypass),
        tuner.relay_c1,
        tuner.relay_l,
        tuner.relay_c2
    );
    if aethersdr_compat {
        status
    } else {
        format!(
            "{status} connection_state={} fault={}",
            tuner.connection_state.as_str(),
            tuner
                .fault
                .as_deref()
                .unwrap_or(if degraded { "device_degraded" } else { "" }),
        )
    }
}

fn advertised_operate_from_tuner(
    tuner: &bridge_core::TunerState,
    aethersdr_compat: bool,
    control_profile: &str,
) -> bool {
    let control_ready = matches!(
        control_profile,
        "control_ready" | "verbose_control" | "tgxl_control_ready" | "tgxl_verbose_control"
    );
    if control_ready {
        true
    } else if aethersdr_compat {
        false
    } else {
        tuner.operate
    }
}

async fn state_push(state: &SharedState, aethersdr_compat: bool, control_profile: &str) -> String {
    let guard = state.read().await;
    state_push_from_tuner(&guard, aethersdr_compat, control_profile)
}

fn state_push_from_tuner(
    state: &bridge_core::state::BridgeState,
    aethersdr_compat: bool,
    control_profile: &str,
) -> String {
    format!(
        "S0|status {}\n",
        status_body_from_state(state, aethersdr_compat, control_profile)
    )
}

fn band_number(band: bridge_core::Band) -> u16 {
    match band {
        bridge_core::Band::M160 => 160,
        bridge_core::Band::M80 => 80,
        bridge_core::Band::M60 => 60,
        bridge_core::Band::M40 => 40,
        bridge_core::Band::M30 => 30,
        bridge_core::Band::M20 => 20,
        bridge_core::Band::M17 => 17,
        bridge_core::Band::M15 => 15,
        bridge_core::Band::M12 => 12,
        bridge_core::Band::M10 => 10,
        bridge_core::Band::M6 => 6,
        bridge_core::Band::Unknown => 0,
    }
}

fn command_token<'a>(command: &'a str, key: &str) -> Option<&'a str> {
    command
        .split_whitespace()
        .find_map(|part| part.strip_prefix(&format!("{key}=")))
}

fn command_channel(command: &str) -> Option<u8> {
    command_token(command, "ch")?.parse().ok()
}

fn command_bool(command: &str, key: &str) -> Option<bool> {
    match command_token(command, key)? {
        "1" | "true" | "True" | "TRUE" => Some(true),
        "0" | "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

struct SessionStats {
    peer: SocketAddr,
    started_at: Instant,
    commands_received: u64,
    responses_sent: u64,
    parse_failures: u64,
    unknown_commands: u64,
    unsupported_features: u64,
    unexpected_sequences: u64,
    status_poll_count: u64,
    total_status_poll_gap: Duration,
    last_status_poll_at: Option<Instant>,
    last_command_at: Option<Instant>,
    max_command_gap: Duration,
    idle_gap_count: u64,
    first_commands: Vec<String>,
    last_sequence: Option<u32>,
}

impl SessionStats {
    fn new(peer: SocketAddr) -> Self {
        Self {
            peer,
            started_at: Instant::now(),
            commands_received: 0,
            responses_sent: 0,
            parse_failures: 0,
            unknown_commands: 0,
            unsupported_features: 0,
            unexpected_sequences: 0,
            status_poll_count: 0,
            total_status_poll_gap: Duration::ZERO,
            last_status_poll_at: None,
            last_command_at: None,
            max_command_gap: Duration::ZERO,
            idle_gap_count: 0,
            first_commands: Vec::new(),
            last_sequence: None,
        }
    }

    fn observe_command(&mut self, command: &str) {
        let now = Instant::now();
        if self.first_commands.len() < 8 {
            self.first_commands.push(command.to_string());
        }
        if let Some(previous) = self.last_command_at {
            let gap = now.saturating_duration_since(previous);
            self.max_command_gap = self.max_command_gap.max(gap);
            if gap >= Duration::from_secs(2) {
                self.idle_gap_count += 1;
            }
        }
        self.last_command_at = Some(now);

        if command == "status" {
            if let Some(previous) = self.last_status_poll_at {
                self.total_status_poll_gap += now.saturating_duration_since(previous);
            }
            self.last_status_poll_at = Some(now);
            self.status_poll_count += 1;
        }
    }

    fn average_poll_interval_ms(&self) -> Option<u128> {
        if self.status_poll_count <= 1 {
            None
        } else {
            Some(self.total_status_poll_gap.as_millis() / u128::from(self.status_poll_count - 1))
        }
    }

    fn log_summary(&self, protocol: &str) {
        info!(
            event_id = "client_disconnected",
            protocol,
            connection_id = %self.peer,
            session_duration_ms = self.started_at.elapsed().as_millis(),
            commands_received = self.commands_received,
            responses_sent = self.responses_sent,
            parse_failures = self.parse_failures,
            unknown_commands = self.unknown_commands,
            unsupported_features = self.unsupported_features,
            unexpected_sequences = self.unexpected_sequences,
            average_poll_interval_ms = self.average_poll_interval_ms().unwrap_or_default(),
            max_command_gap_ms = self.max_command_gap.as_millis(),
            idle_gap_count = self.idle_gap_count,
            first_commands = ?self.first_commands,
            "client session summary"
        );
    }
}

async fn increment_commands(state: &SharedState) {
    state.write().await.protocol.tgxl.commands_received += 1;
}

async fn increment_responses(state: &SharedState) {
    state.write().await.protocol.tgxl.responses_sent += 1;
}

async fn increment_parse_failure(state: &SharedState) {
    state.write().await.protocol.tgxl.parse_failures += 1;
}

async fn increment_unknown(state: &SharedState) {
    state.write().await.protocol.tgxl.unknown_commands += 1;
}

async fn update_tgxl_session_command(state: &SharedState, session_id: u64, command: &str) {
    let mut guard = state.write().await;
    if let Some(session) = guard
        .clients
        .tgxl_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.record_command(command);
    }
}

async fn update_tgxl_session_response(state: &SharedState, session_id: u64, latency_ms: u64) {
    let mut guard = state.write().await;
    if let Some(session) = guard
        .clients
        .tgxl_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.record_response(latency_ms);
    }
}

async fn update_tgxl_session_parse_failure(state: &SharedState, session_id: u64) {
    let mut guard = state.write().await;
    if let Some(session) = guard
        .clients
        .tgxl_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.parse_failures = session.parse_failures.saturating_add(1);
    }
}

async fn update_tgxl_session_unknown(state: &SharedState, session_id: u64) {
    let mut guard = state.write().await;
    if let Some(session) = guard
        .clients
        .tgxl_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.unknown_commands = session.unknown_commands.saturating_add(1);
    }
}

async fn increment_unsupported(state: &SharedState) {
    state.write().await.protocol.tgxl.unsupported_features += 1;
}

async fn increment_unexpected_sequence(state: &SharedState) {
    state.write().await.protocol.tgxl.unexpected_sequences += 1;
}

struct Transcript {
    file: Option<File>,
    dir: Option<PathBuf>,
    device: String,
    safe_peer: String,
    session_ts: u128,
    index: u64,
    bytes_written: u64,
    rotate_bytes: u64,
}

impl Transcript {
    fn disabled() -> Self {
        Self {
            file: None,
            dir: None,
            device: String::new(),
            safe_peer: String::new(),
            session_ts: 0,
            index: 0,
            bytes_written: 0,
            rotate_bytes: 0,
        }
    }

    async fn new(device: &str, peer: SocketAddr, dir: Option<PathBuf>, rotate_bytes: u64) -> Self {
        let Some(dir) = dir else {
            return Self::disabled();
        };
        if let Err(err) = create_dir_all(&dir).await {
            warn!(device, error = %err, "protocol transcript directory could not be created");
            return Self::disabled();
        }
        let ts = timestamp_millis();
        let safe_peer = peer.to_string().replace([':', '.'], "_");
        let path = transcript_path(&dir, device, ts, &safe_peer, 0);
        match File::create(&path).await {
            Ok(file) => Self {
                file: Some(file),
                dir: Some(dir),
                device: device.to_string(),
                safe_peer,
                session_ts: ts,
                index: 0,
                bytes_written: 0,
                rotate_bytes,
            },
            Err(err) => {
                warn!(device, path = %path.display(), error = %err, "protocol transcript file could not be opened");
                Self::disabled()
            }
        }
    }

    async fn write_line(
        &mut self,
        device: &str,
        direction: &str,
        line: &str,
    ) -> anyhow::Result<()> {
        if self.file.is_none() {
            return Ok(());
        }
        let row = format!("{} {device} {direction} {line}\n", timestamp_millis());
        if self.rotate_bytes > 0
            && self.bytes_written.saturating_add(row.len() as u64) > self.rotate_bytes
        {
            self.rotate().await;
        }
        let Some(file) = &mut self.file else {
            return Ok(());
        };
        if let Err(err) = file.write_all(row.as_bytes()).await {
            warn!(device, error = %err, "protocol transcript write failed");
            self.file = None;
            return Ok(());
        }
        self.bytes_written = self.bytes_written.saturating_add(row.len() as u64);
        if let Err(err) = file.flush().await {
            warn!(device, error = %err, "protocol transcript flush failed");
            self.file = None;
        }
        Ok(())
    }

    async fn rotate(&mut self) {
        let Some(dir) = &self.dir else {
            return;
        };
        self.index = self.index.saturating_add(1);
        let path = transcript_path(
            dir,
            &self.device,
            self.session_ts,
            &self.safe_peer,
            self.index,
        );
        match File::create(&path).await {
            Ok(file) => {
                self.file = Some(file);
                self.bytes_written = 0;
            }
            Err(err) => {
                warn!(path = %path.display(), error = %err, "protocol transcript rotation failed");
                self.file = None;
            }
        }
    }
}

fn transcript_path(dir: &Path, device: &str, ts: u128, safe_peer: &str, index: u64) -> PathBuf {
    dir.join(format!("{device}-{ts}-{safe_peer}-{index}.log"))
}

async fn write_protocol_line<W>(
    writer: &mut W,
    transcript: &mut Transcript,
    device: &str,
    direction: &str,
    line: &str,
    protocol_trace: bool,
) -> anyhow::Result<()>
where
    W: AsyncWrite + Unpin,
{
    trace_protocol_line(transcript, device, direction, line, protocol_trace).await?;
    writer.write_all(format!("{line}\n").as_bytes()).await?;
    Ok(())
}

async fn trace_protocol_line(
    transcript: &mut Transcript,
    device: &str,
    direction: &str,
    line: &str,
    protocol_trace: bool,
) -> anyhow::Result<()> {
    if protocol_trace {
        info!("{device} {direction} {line}");
    }
    append_evidence_line("tgxl-protocol.log", format!("{device} {direction} {line}"));
    transcript.write_line(device, direction, line).await
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn bool_int(value: bool) -> u8 {
    if value {
        1
    } else {
        0
    }
}

fn watts_to_dbm(watts: f32) -> f32 {
    if watts <= 0.0 {
        -120.0
    } else {
        10.0 * (watts * 1000.0).log10()
    }
}

fn protocol_swr_value(swr: f32) -> f32 {
    if swr.is_finite() && swr > 1.0 {
        let rho = ((swr - 1.0) / (swr + 1.0)).clamp(0.001, 0.999);
        20.0 * rho.log10()
    } else {
        -30.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::state::shared_mock_state;

    #[tokio::test]
    async fn status_contains_aethersdr_fields() {
        let state = shared_mock_state();
        let line = status_line(2, &state, false, "readonly").await;
        assert!(line.starts_with("S2|status "));
        assert!(line.contains("state=0"));
        assert!(line.contains("freqA=14.200000"));
        assert!(line.contains("bandA=20"));
        assert!(line.contains("relayC1="));
    }

    #[tokio::test]
    async fn status_uses_current_flex_frequency_and_band() {
        let state = shared_mock_state();
        {
            let mut guard = state.write().await;
            guard.radio_context.frequency_hz = Some(10_125_000);
            guard.radio_context.band = bridge_core::Band::M30;
        }
        let line = status_line(95, &state, false, "control_ready").await;
        assert!(line.contains("S95|status "));
        assert!(line.contains("freqA=10.125000"));
        assert!(line.contains("bandA=30"));
        assert!(line.contains("state=1"));
    }

    #[tokio::test]
    async fn relay_command_updates_state() {
        let state = shared_mock_state();
        apply_relay_command("tune relay=0 move=1", &state)
            .await
            .unwrap();
        let guard = state.read().await;
        assert_eq!(
            guard.desired.tuner_manual_tune,
            Some(ManualTuneRequest {
                relay: 0,
                movement: 1
            })
        );
    }

    #[tokio::test]
    async fn golden_tgxl_mock_status_response_is_stable() {
        let state = shared_mock_state();
        assert_eq!(
            status_line(2, &state, false, "readonly").await,
            "S2|status fwd=-120.00 peak=-120.00 max=0.00 swr=-30.0000 pttA=0 bandA=20 modeA=1 flexA=FlexRadio freqA=14.200000 bypassA=0 bypassRxA=0 antA=0 pttB=0 bandB=0 modeB=0 flexB= freqB=0.000 bypassB=0 bypassRxB=0 antB=0 state=0 active=1 tuning=0 bypass=0 ag=0 relayC1=20 relayL=35 relayC2=20 connection_state=connected fault=\n"
        );
    }

    #[tokio::test]
    async fn mock_status_reports_return_loss_not_swr_ratio() {
        let state = shared_mock_state();
        let line = status_line(2, &state, false, "readonly").await;
        assert!(line.contains("swr=-30.0000"));
        assert!(!line.contains("swr=32."));
    }

    #[test]
    fn golden_tgxl_info_response_is_stable() {
        assert_eq!(
            response_line(1, 0, info_body(false)),
            "R1|0|info serial=EGB-TGXL serial_num=EGB-TGXL version=0.1.0-egb-tgxl firmware=0.1.0-egb-tgxl nickname=Tuner_Genius_XL 3way=1 model=TunerGeniusXL one_by_three=1 capabilities=direct_tcp,status,autotune,ant,manual_tune,flexradio,catradio,setup\n"
        );
    }

    #[tokio::test]
    async fn aethersdr_compat_status_removes_unverified_fields() {
        let state = shared_mock_state();
        assert_eq!(
            status_line(2, &state, true, "readonly").await,
            "S2|status fwd=-120.00 peak=-120.00 max=0.00 swr=-30.0000 pttA=0 bandA=20 modeA=1 flexA=FlexRadio freqA=14.200000 bypassA=0 bypassRxA=0 antA=0 pttB=0 bandB=0 modeB=0 flexB= freqB=0.000 bypassB=0 bypassRxB=0 antB=0 state=0 active=1 tuning=0 bypass=0 ag=0 relayC1=20 relayL=35 relayC2=20\n"
        );
    }

    #[tokio::test]
    async fn control_ready_profile_advertises_operate_for_ui_enablement() {
        let state = shared_mock_state();
        let body = status_line(2, &state, true, "control_ready").await;
        assert!(body.contains("state=1"));
        let body = status_line(2, &state, true, "verbose_control").await;
        assert!(body.contains("state=1"));
        let body = status_line(2, &state, true, "tgxl_control_ready").await;
        assert!(body.contains("state=1"));
    }

    #[test]
    fn aethersdr_compat_info_removes_capabilities() {
        assert_eq!(
            response_line(1, 0, info_body(true)),
            "R1|0|info serial=EGB-TGXL serial_num=EGB-TGXL version=0.1.0-egb-tgxl nickname=Tuner_Genius_XL 3way=1 model=TunerGeniusXL one_by_three=1\n"
        );
    }

    #[tokio::test]
    async fn flexradio_read_uses_radio_context() {
        let state = shared_mock_state();
        {
            let mut guard = state.write().await;
            guard.radio_context.radio_serial = Some("4315-5050-6700-6206".to_string());
            guard.radio_context.tx_antenna = Some("ANT2".to_string());
        }
        let line = flexradio_get_line(7, &state, 1).await;
        assert_eq!(
            line,
            "R7|0|flexradio ch=1 active=1 serial=4315-5050-6700-6206 antenna=ANT2 source=LAN\n"
        );
    }

    #[tokio::test]
    async fn official_safe_tgxl_commands_have_stable_responses() {
        let state = shared_mock_state();
        assert_eq!(
            replay_line("C1|auth test", &state).await.unwrap(),
            vec!["R1|0|auth OK\n"]
        );
        assert_eq!(
            replay_line("C2|ifconf set ip=192.168.1.10", &state)
                .await
                .unwrap(),
            vec!["R2|0|0\n"]
        );
        assert_eq!(
            replay_line("C3|setup set nickname=Tuner_Genius_XL", &state)
                .await
                .unwrap(),
            vec!["R3|0|\n"]
        );
        assert_eq!(
            replay_line("C4|save", &state).await.unwrap(),
            vec!["R4|0|\n"]
        );
        assert_eq!(
            replay_line("C5|btl", &state).await.unwrap(),
            vec!["R5|5|error=bootloader_unsupported_by_bridge\n"]
        );
    }

    #[tokio::test]
    async fn official_one_based_relay_ids_are_accepted() {
        let state = shared_mock_state();
        replay_line("C6|tune relay=3 move=-1", &state)
            .await
            .unwrap();
        let guard = state.read().await;
        assert_eq!(
            guard.desired.tuner_manual_tune,
            Some(ManualTuneRequest {
                relay: 2,
                movement: -1,
            })
        );
    }
}
