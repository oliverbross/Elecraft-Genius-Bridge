use anyhow::Context;
use bridge_core::{
    append_evidence_json, append_evidence_line, parse_client_command, response_line,
    AmpOperatingState, ConnectionState, LifecycleState, ProtocolClientSession, SharedState,
};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;
use tracing::{debug, info, warn};

const VERSION: &str = "0.1.0-egb-pgxl";

#[derive(Debug, Clone, Default)]
pub struct EmulatorOptions {
    pub protocol_trace: bool,
    pub transcript_dir: Option<PathBuf>,
    pub transcript_rotate_bytes: u64,
    pub aethersdr_compat: bool,
    pub compat_profile: String,
    pub status_profile: String,
    pub strict_emulation: bool,
    pub startup_delay: Duration,
    pub force_direct_connected_test: bool,
}

impl EmulatorOptions {
    fn effective_aethersdr_compat(&self) -> bool {
        self.aethersdr_compat
            || matches!(
                self.compat_profile.as_str(),
                "" | "aethersdr" | "smartsdr" | "permissive"
            )
    }
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
        .with_context(|| format!("failed to bind PGXL emulator on {bind_addr}"))?;
    info!(%bind_addr, "PGXL emulator listening");
    {
        let mut guard = state.write().await;
        guard.clients.pgxl_listener_ready_at_ms = Some(timestamp_millis());
    }
    append_evidence_line(
        "listener-startup.log",
        format!("PGXL listener started bind_addr={bind_addr}"),
    );

    loop {
        let (socket, peer) = listener.accept().await?;
        append_evidence_line(
            "listener-startup.log",
            format!("PGXL accept peer={peer} bind_addr={bind_addr}"),
        );
        let state = state.clone();
        let options = options.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_client(socket, peer, state, options).await {
                warn!(%peer, error = %err, "PGXL client ended with error");
            }
        });
    }
}

pub async fn replay_line(
    line: &str,
    state: &SharedState,
) -> Result<String, bridge_core::ProtocolError> {
    let cmd = parse_client_command(line)?;
    Ok(
        handle_command(cmd.seq, &cmd.command, state, false, "status_current")
            .await
            .response,
    )
}

async fn handle_client(
    socket: TcpStream,
    peer: SocketAddr,
    state: SharedState,
    options: EmulatorOptions,
) -> anyhow::Result<()> {
    let session_id = {
        let mut guard = state.write().await;
        guard.clients.pgxl_connected = true;
        guard.clients.pgxl_client_count += 1;
        let connected_at_ms = timestamp_millis();
        if guard.clients.pgxl_first_accept_at_ms.is_none() {
            guard.clients.pgxl_first_accept_at_ms = Some(connected_at_ms);
            guard.clients.pgxl_reannounce_count_at_first_accept =
                Some(guard.flex_injection.amplifier_reannounce_count);
            guard.clients.pgxl_sub_amp_all_count_at_first_accept =
                Some(guard.flex_injection.sub_amplifier_all_command_count);
            guard.clients.pgxl_last_amp_status_before_accept =
                guard.flex_injection.last_amplifier_status_line.clone();
        }
        guard.clients.pgxl_session_started_count =
            guard.clients.pgxl_session_started_count.saturating_add(1);
        let id = guard.clients.next_session_id;
        guard.clients.next_session_id = guard.clients.next_session_id.saturating_add(1);
        guard.clients.pgxl_sessions.push(ProtocolClientSession::new(
            id,
            "PGXL",
            peer,
            connected_at_ms,
        ));
        guard.lifecycle.pgxl.transition(
            LifecycleState::TcpConnected,
            format!("PGXL TCP client connected: {peer}"),
        );
        guard.lifecycle.aether_client.transition(
            LifecycleState::TcpConnected,
            format!("Aether client connected to PGXL: {peer}"),
        );
        id
    };
    append_evidence_json(
        "client-sessions.jsonl",
        &serde_json::json!({
            "event": "client_connected",
            "protocol": "PGXL",
            "peer": peer.to_string(),
            "session_id": session_id,
        }),
    );
    if options.force_direct_connected_test {
        info!(
            protocol = "PGXL",
            connection_id = %peer,
            "PGXL direct connected diagnostic mode enabled"
        );
    }
    info!(
        protocol = "PGXL",
        connection_id = %peer,
        compat_profile = %options.compat_profile,
        aethersdr_compat = options.effective_aethersdr_compat(),
        "PGXL session compatibility profile selected"
    );
    maybe_start_strict_startup(&state, &options).await;
    info!(event_id = "client_connected", protocol = "PGXL", connection_id = %peer, "PGXL client connected");

    let result = async {
        let (reader, mut writer) = socket.into_split();
        let mut transcript = Transcript::new(
            "pgxl-session",
            peer,
            options
                .transcript_dir
                .clone()
                .or_else(|| Some(PathBuf::from("logs/protocol"))),
            options.transcript_rotate_bytes,
        )
        .await;
        let mut stats = SessionStats::new(peer);
        let greeting = format!("V{VERSION}");
        write_protocol_line(
            &mut writer,
            &mut transcript,
            "PGXL",
            "TX >",
            &greeting,
            options.protocol_trace,
        )
        .await
        .context("failed to write PGXL version greeting")?;
        stats.responses_sent += 1;
        update_pgxl_session_response(&state, session_id, 0).await;
        increment_responses(&state).await;

        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await? {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            trace_protocol_line(
                &mut transcript,
                "PGXL",
                "RX <",
                &line,
                options.protocol_trace,
            )
            .await?;
            debug!(
                %peer,
                command_line_len = line.len(),
                raw_hex = %hex_bytes(line.as_bytes()),
                "PGXL command received"
            );
            match parse_client_command(&line) {
                Ok(cmd) => {
                    stats.commands_received += 1;
                    update_pgxl_session_command(&state, session_id, &cmd.command).await;
                    increment_commands(&state).await;
                    if let Some(previous) = stats.last_sequence {
                        if cmd.seq <= previous {
                            stats.unexpected_sequences += 1;
                            increment_unexpected_sequence(&state).await;
                            warn!(
                                event_id = "protocol_parse_failure",
                                protocol = "PGXL",
                                raw_line = %line,
                                connection_id = %peer,
                                parser_reason = "unexpected_sequence",
                                previous_sequence = previous,
                                current_sequence = cmd.seq,
                                "unexpected PGXL command sequence"
                            );
                        }
                    }
                    stats.last_sequence = Some(cmd.seq);
                    stats.observe_command(&cmd.command);
                    let command_started = Instant::now();
                    let outcome = handle_command(
                        cmd.seq,
                        &cmd.command,
                        &state,
                        options.effective_aethersdr_compat(),
                        &options.status_profile,
                    )
                    .await;
                    update_pgxl_session_response(
                        &state,
                        session_id,
                        duration_millis_u64(command_started.elapsed()),
                    )
                    .await;
                    if outcome.unknown {
                        stats.unknown_commands += 1;
                        update_pgxl_session_unknown(&state, session_id).await;
                        increment_unknown(&state).await;
                        warn!(
                            event_id = "protocol_parse_failure",
                            protocol = "PGXL",
                            raw_line = %line,
                            connection_id = %peer,
                            parser_reason = "unknown_command",
                            "unknown PGXL command"
                        );
                    }
                    if outcome.unsupported {
                        stats.unsupported_features += 1;
                        increment_unsupported(&state).await;
                    }
                    debug!(%peer, command = %cmd.command, "PGXL command handled");
                    write_protocol_line(
                        &mut writer,
                        &mut transcript,
                        "PGXL",
                        "TX >",
                        outcome.response.trim_end(),
                        options.protocol_trace,
                    )
                    .await?;
                    stats.responses_sent += 1;
                    increment_responses(&state).await;
                }
                Err(err) => {
                    stats.parse_failures += 1;
                    update_pgxl_session_parse_failure(&state, session_id).await;
                    increment_parse_failure(&state).await;
                    warn!(
                        event_id = "protocol_parse_failure",
                        protocol = "PGXL",
                        raw_line = %line,
                        connection_id = %peer,
                        parser_reason = %err,
                        "invalid PGXL command frame"
                    );
                }
            }
        }
        stats.log_summary("PGXL");
        Ok::<(), anyhow::Error>(())
    }
    .await;

    {
        let mut guard = state.write().await;
        guard.clients.pgxl_client_count = guard.clients.pgxl_client_count.saturating_sub(1);
        guard.clients.pgxl_connected = guard.clients.pgxl_client_count > 0;
        guard
            .clients
            .pgxl_sessions
            .retain(|session| session.id != session_id);
        guard.clients.pgxl_last_disconnect_reason = Some(match &result {
            Ok(()) => "client_closed".to_string(),
            Err(err) => err.to_string(),
        });
        let reason = guard
            .clients
            .pgxl_last_disconnect_reason
            .clone()
            .unwrap_or_else(|| "client_closed".to_string());
        if guard.clients.pgxl_client_count == 0 {
            guard.lifecycle.pgxl.transition(
                LifecycleState::Degraded,
                format!("PGXL client disconnected: {reason}"),
            );
        }
    }
    append_evidence_json(
        "disconnect-events.jsonl",
        &serde_json::json!({
            "event": "client_disconnected",
            "protocol": "PGXL",
            "peer": peer.to_string(),
            "session_id": session_id,
            "reason": result.as_ref().err().map(|err| err.to_string()).unwrap_or_else(|| "client_closed".to_string()),
        }),
    );
    info!(event_id = "client_disconnected", protocol = "PGXL", connection_id = %peer, "PGXL client disconnected");
    result
}

struct CommandOutcome {
    response: String,
    unknown: bool,
    unsupported: bool,
}

impl CommandOutcome {
    fn ok(response: String) -> Self {
        Self {
            response,
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
    status_profile: &str,
) -> CommandOutcome {
    match command {
        "info" => CommandOutcome::ok(response_line(seq, 0, info_body(aethersdr_compat))),
        "status" => {
            let body = status_body(state, aethersdr_compat, status_profile).await;
            let state_value = status_field(&body, "state").unwrap_or("UNKNOWN");
            let timestamp_ms = timestamp_millis();
            {
                let mut guard = state.write().await;
                guard.flex_injection.last_pgxl_status_state_at_ms = Some(timestamp_ms);
                guard.flex_injection.last_pgxl_status_state = Some(state_value.to_string());
                let updates = guard
                    .flex_injection
                    .record_pgxl_status_observed(timestamp_ms, state_value);
                drop(guard);
                for transition in updates {
                    append_evidence_json("kpa-state-transition-latency.jsonl", &transition);
                }
            }
            append_evidence_json(
                "amp-state-reflection-events.jsonl",
                &serde_json::json!({
                    "event": "pgxl_status_response",
                    "timestamp_ms": timestamp_ms,
                    "seq": seq,
                    "state": state_value,
                    "body": body,
                }),
            );
            CommandOutcome::ok(response_line(seq, 0, body))
        }
        "setup read" | "setup" => CommandOutcome::ok(response_line(seq, 0, setup_body())),
        "ifconf read" | "network" | "network read" => {
            CommandOutcome::ok(response_line(seq, 0, ifconf_body()))
        }
        _ if command.starts_with("ifconf ") && command != "ifconf read" => CommandOutcome {
            response: response_line(seq, 5, "error=network_config_unsupported_by_bridge"),
            unknown: false,
            unsupported: true,
        },
        _ if command.starts_with("catradio read=") => {
            let slice = command
                .strip_prefix("catradio read=")
                .unwrap_or("A")
                .chars()
                .next()
                .unwrap_or('A');
            CommandOutcome::ok(response_line(seq, 0, catradio_body(slice)))
        }
        _ if command.starts_with("catradio ") => CommandOutcome {
            response: response_line(seq, 5, "error=catradio_config_unsupported_by_bridge"),
            unknown: false,
            unsupported: true,
        },
        _ if command.starts_with("flexradio read=") => {
            let slice = command
                .strip_prefix("flexradio read=")
                .unwrap_or("A")
                .chars()
                .next()
                .unwrap_or('A');
            CommandOutcome::ok(response_line(seq, 0, flexradio_body(slice, state).await))
        }
        _ if command.starts_with("flexradio ") => CommandOutcome {
            response: response_line(seq, 5, "error=flexradio_config_unsupported_by_bridge"),
            unknown: false,
            unsupported: true,
        },
        "save" => CommandOutcome {
            response: response_line(seq, 5, "error=save_reboot_unsupported_by_bridge"),
            unknown: false,
            unsupported: true,
        },
        // AetherSDR currently routes PGXL operate/standby through the Flex radio
        // amplifier API, not direct TCP. These direct commands are accepted only
        // as desired-state requests for manual harness testing.
        "operate" => {
            let mut guard = state.write().await;
            guard.controls.aethersdr_button_command_seen = true;
            guard.controls.amp_control_command_seen = true;
            guard.controls.any_control_event_seen = true;
            guard.controls.control_requested_count =
                guard.controls.control_requested_count.saturating_add(1);
            guard.controls.last_pgxl_control_command = Some(command.to_string());
            guard.controls.last_mapped_elecraft_action = Some("KPA500 ^OS1;".to_string());
            guard.controls.last_safety_decision = Some("desired_operate_requested".to_string());
            guard.desired.amp_operate = Some(true);
            drop(guard);
            append_evidence_json(
                "control-events.jsonl",
                &serde_json::json!({
                    "protocol": "PGXL",
                    "raw": command,
                    "mapped_action": "KPA500 ^OS1;",
                    "safety_decision": "desired_operate_requested",
                }),
            );
            append_evidence_line(
                "pgxl-control-commands.log",
                format!("RX {command} -> KPA500 ^OS1; desired_operate_requested"),
            );
            let guard = state.read().await;
            CommandOutcome::ok(response_line(
                seq,
                0,
                status_body_from_amp(&guard.amp, aethersdr_compat, status_profile),
            ))
        }
        "standby" => {
            let mut guard = state.write().await;
            guard.controls.aethersdr_button_command_seen = true;
            guard.controls.amp_control_command_seen = true;
            guard.controls.any_control_event_seen = true;
            guard.controls.control_requested_count =
                guard.controls.control_requested_count.saturating_add(1);
            guard.controls.last_pgxl_control_command = Some(command.to_string());
            guard.controls.last_mapped_elecraft_action = Some("KPA500 ^OS0;".to_string());
            guard.controls.last_safety_decision = Some("desired_standby_requested".to_string());
            guard.desired.amp_operate = Some(false);
            drop(guard);
            append_evidence_json(
                "control-events.jsonl",
                &serde_json::json!({
                    "protocol": "PGXL",
                    "raw": command,
                    "mapped_action": "KPA500 ^OS0;",
                    "safety_decision": "desired_standby_requested",
                }),
            );
            append_evidence_line(
                "pgxl-control-commands.log",
                format!("RX {command} -> KPA500 ^OS0; desired_standby_requested"),
            );
            let guard = state.read().await;
            CommandOutcome::ok(response_line(
                seq,
                0,
                status_body_from_amp(&guard.amp, aethersdr_compat, status_profile),
            ))
        }
        _ => CommandOutcome {
            response: response_line(seq, 1, "error=unknown_command"),
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
        guard.amp.connection_state = ConnectionState::Connecting;
        guard.amp.connected = false;
        guard.amp.state = AmpOperatingState::PowerUp;
        guard.amp.fault = None;
    }

    let state = state.clone();
    let delay = options.startup_delay;
    tokio::spawn(async move {
        sleep(delay).await;
        let mut guard = state.write().await;
        guard.amp.connection_state = ConnectionState::Connected;
        guard.amp.connected = true;
        if guard.amp.state == AmpOperatingState::PowerUp {
            guard.amp.state = AmpOperatingState::Standby;
        }
        guard.amp.last_successful_poll_at = Some(SystemTime::now());
        info!(
            event_id = "strict_startup_complete",
            protocol = "PGXL",
            startup_delay_ms = delay.as_millis(),
            "PGXL strict emulation startup complete"
        );
    });
}

fn info_body(aethersdr_compat: bool) -> String {
    if aethersdr_compat {
        format!("model=PowerGeniusXL serial_num=EGB-PGXL version={VERSION}")
    } else {
        format!(
            "model=PowerGeniusXL serial_num=EGB-PGXL version={VERSION} firmware={VERSION} capabilities=direct_tcp,status"
        )
    }
}

fn setup_body() -> &'static str {
    "nickname=Elecraft_Genius_Bridge fan=auto meffa=OK led=128"
}

fn ifconf_body() -> &'static str {
    "address=0.0.0.0 netmask=255.255.255.0 gateway=0.0.0.0 dhcp=true gateway-status=0.0.0.0"
}

fn catradio_body(slice: char) -> String {
    let slice = normalized_amp_slice(slice);
    format!("ampslice={slice} active=0 type=None baud=0 parity=N stopbits=1 civ=0")
}

async fn flexradio_body(slice: char, state: &SharedState) -> String {
    let slice = normalized_amp_slice(slice);
    let guard = state.read().await;
    let serial = guard
        .radio_context
        .radio_serial
        .as_deref()
        .unwrap_or("EGB-FLEX");
    let antenna = guard.radio_context.tx_antenna.as_deref().unwrap_or("ANT1");
    format!("ampslice={slice} serial={serial} txant={antenna} ptt=LAN active=1")
}

fn normalized_amp_slice(slice: char) -> char {
    match slice.to_ascii_uppercase() {
        'B' => 'B',
        _ => 'A',
    }
}

async fn status_body(state: &SharedState, aethersdr_compat: bool, status_profile: &str) -> String {
    let (body, advertised_state) = {
        let guard = state.read().await;
        (
            status_body_from_amp(&guard.amp, aethersdr_compat, status_profile),
            advertised_state_from_amp(&guard.amp).to_string(),
        )
    };
    let should_update = {
        let guard = state.read().await;
        guard.flex_injection.last_advertised_pgxl_state.as_deref()
            != Some(advertised_state.as_str())
    };
    if should_update {
        let mut guard = state.write().await;
        guard.flex_injection.last_advertised_pgxl_state = Some(advertised_state);
    }
    body
}

fn status_body_from_amp(
    amp: &bridge_core::AmpState,
    aethersdr_compat: bool,
    status_profile: &str,
) -> String {
    let degraded = matches!(
        amp.connection_state,
        ConnectionState::Disconnected | ConnectionState::Degraded | ConnectionState::Error
    );
    let state = match status_profile {
        "status_realistic_operate" => "OPERATE",
        "status_realistic_standby" => "STANDBY",
        _ => advertised_state_from_amp(amp),
    };
    let peakfwd = watts_to_dbm(amp.forward_power_watts);
    let swr = protocol_swr_value(amp.swr);
    let fault = amp
        .fault
        .as_deref()
        .unwrap_or(if degraded { "device_degraded" } else { "" });
    let meffa = pgxl_meffa_value(amp);
    let mut native = format!(
        "state={state} peakfwd={peakfwd:.4} swr={swr:.4} temp={:.1} id={:.1} vac={} meffa={}",
        amp.temperature_c,
        amp.pa_current_amps,
        pgxl_vac_value(amp),
        meffa
    );
    match status_profile {
        "status_control_fields" | "status_operate_capable" => {
            native.push_str(" operate_capable=1 standby_capable=1");
        }
        "status_rich_metered" | "status_realistic_operate" | "status_realistic_standby" => {
            native.push_str(&format!(
                " operate_capable=1 standby_capable=1 control=1 fwd={:.1} rl={:.2} drv=0.0 current={:.1} voltage={:.1}",
                amp.forward_power_watts,
                swr,
                amp.pa_current_amps,
                amp.pa_voltage_volts
            ));
        }
        "status_real_pgxl_like" => {
            native.push_str(&format!(
                " model=PowerGeniusXL serial_num=EGB-KPA500 firmware={VERSION} operate_capable=1 standby_capable=1"
            ));
        }
        _ => {}
    }
    if aethersdr_compat {
        native
    } else {
        format!(
            "{native} fault={fault} connection_state={}",
            amp.connection_state.as_str()
        )
    }
}

fn advertised_state_from_amp(amp: &bridge_core::AmpState) -> &'static str {
    if !amp.first_poll_completed
        && amp.startup_state_policy.as_deref() == Some("wait_for_first_kpa_poll")
        && !amp_has_recent_poll(amp)
    {
        return "UNKNOWN";
    }
    if amp.fault.is_some() || amp.state == bridge_core::AmpOperatingState::Fault {
        "FAULT"
    } else {
        amp.state.pgxl_state()
    }
}

fn amp_has_recent_poll(amp: &bridge_core::AmpState) -> bool {
    if amp.runtime.poll_success_count == 0 {
        return false;
    }
    amp.last_successful_poll_at
        .and_then(|poll| SystemTime::now().duration_since(poll).ok())
        .is_some_and(|elapsed| elapsed <= Duration::from_secs(10))
}

fn pgxl_meffa_value(amp: &bridge_core::AmpState) -> &str {
    if amp.fault.is_some() || amp.state == bridge_core::AmpOperatingState::Fault {
        amp.meffa.as_str()
    } else if amp.meffa == "UNKNOWN" || amp.meffa.trim().is_empty() {
        "OK"
    } else {
        amp.meffa.as_str()
    }
}

fn pgxl_vac_value(amp: &bridge_core::AmpState) -> u16 {
    // KPA500 ^VI reports internal PA supply voltage, not AC mains voltage.
    // Avoid publishing values such as 68.9 as PGXL VAC until a validated
    // PGXL mapping exists.
    if amp.pa_voltage_volts >= 100.0 {
        amp.pa_voltage_volts.round().clamp(0.0, f32::from(u16::MAX)) as u16
    } else {
        0
    }
}

fn status_field<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    body.split_whitespace()
        .filter_map(|token| token.split_once('='))
        .find_map(|(field, value)| (field == key).then_some(value))
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
    state.write().await.protocol.pgxl.commands_received += 1;
}

async fn increment_responses(state: &SharedState) {
    state.write().await.protocol.pgxl.responses_sent += 1;
}

async fn increment_parse_failure(state: &SharedState) {
    state.write().await.protocol.pgxl.parse_failures += 1;
}

async fn increment_unknown(state: &SharedState) {
    state.write().await.protocol.pgxl.unknown_commands += 1;
}

async fn increment_unsupported(state: &SharedState) {
    state.write().await.protocol.pgxl.unsupported_features += 1;
}

async fn increment_unexpected_sequence(state: &SharedState) {
    state.write().await.protocol.pgxl.unexpected_sequences += 1;
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
    let framed = format!("{line}\n");
    append_evidence_line(
        "pgxl-protocol.log",
        format!("{device} TX raw_hex={}", hex_bytes(framed.as_bytes())),
    );
    writer.write_all(framed.as_bytes()).await?;
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
    append_evidence_line(
        "pgxl-protocol.log",
        format!(
            "{device} {direction} {line} raw_hex={}",
            hex_bytes(line.as_bytes())
        ),
    );
    transcript.write_line(device, direction, line).await
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

async fn update_pgxl_session_command(state: &SharedState, session_id: u64, command: &str) {
    let mut guard = state.write().await;
    if let Some(session) = guard
        .clients
        .pgxl_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.record_command(command);
    }
}

async fn update_pgxl_session_response(state: &SharedState, session_id: u64, latency_ms: u64) {
    let mut guard = state.write().await;
    if let Some(session) = guard
        .clients
        .pgxl_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.record_response(latency_ms);
    }
}

async fn update_pgxl_session_parse_failure(state: &SharedState, session_id: u64) {
    let mut guard = state.write().await;
    if let Some(session) = guard
        .clients
        .pgxl_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.parse_failures = session.parse_failures.saturating_add(1);
    }
}

async fn update_pgxl_session_unknown(state: &SharedState, session_id: u64) {
    let mut guard = state.write().await;
    if let Some(session) = guard
        .clients
        .pgxl_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.unknown_commands = session.unknown_commands.saturating_add(1);
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
        let body = status_body(&state, false, "status_current").await;
        assert!(body.contains("state=STANDBY"));
        assert!(body.contains("peakfwd="));
        assert!(body.contains("meffa="));
    }

    #[tokio::test]
    async fn golden_pgxl_mock_status_response_is_stable() {
        let state = shared_mock_state();
        let body = status_body(&state, false, "status_current").await;
        assert_eq!(
            response_line(2, 0, body),
            "R2|0|state=STANDBY peakfwd=-120.0000 swr=-30.0000 temp=32.0 id=0.0 vac=0 meffa=OK fault= connection_state=connected\n"
        );
    }

    #[tokio::test]
    async fn mock_status_never_reports_extreme_swr() {
        let state = shared_mock_state();
        let body = status_body(&state, false, "status_current").await;
        assert!(body.contains("swr=-30.0000"));
        assert!(!body.contains("swr=32."));
    }

    #[tokio::test]
    async fn pgxl_status_follows_live_amp_state() {
        let state = shared_mock_state();
        {
            let mut guard = state.write().await;
            guard.amp.operate = true;
            guard.amp.state = bridge_core::AmpOperatingState::Operate;
        }
        let body = status_body(&state, true, "status_current").await;
        assert!(body.contains("state=OPERATE"));
        let guard = state.read().await;
        assert_eq!(
            guard.flex_injection.last_advertised_pgxl_state.as_deref(),
            Some("OPERATE")
        );
    }

    #[tokio::test]
    async fn pgxl_status_does_not_report_fault_for_no_kpa_fault() {
        let state = bridge_core::state::shared_default_state();
        {
            let mut guard = state.write().await;
            guard.amp.connection_state = ConnectionState::Degraded;
            guard.amp.connected = false;
            guard.amp.operate = true;
            guard.amp.state = bridge_core::AmpOperatingState::Operate;
            guard.amp.fault = None;
            guard.amp.meffa = "UNKNOWN".to_string();
        }
        let body = status_body(&state, true, "status_current").await;
        assert!(body.contains("state=OPERATE"));
        assert!(body.contains("meffa=OK"));
        assert!(!body.contains("state=FAULT"));
    }

    #[tokio::test]
    async fn pgxl_status_maps_kpa_telemetry_fields() {
        let state = bridge_core::state::shared_default_state();
        {
            let mut guard = state.write().await;
            guard.amp.connection_state = ConnectionState::Connected;
            guard.amp.connected = true;
            guard.amp.operate = true;
            guard.amp.state = bridge_core::AmpOperatingState::Operate;
            guard.amp.forward_power_watts = 30.0;
            guard.amp.swr = 1.1;
            guard.amp.temperature_c = 35.0;
            guard.amp.pa_voltage_volts = 68.7;
            guard.amp.pa_current_amps = 0.0;
            guard.amp.fault = None;
            guard.amp.meffa = "OK".to_string();
        }
        let body = status_body(&state, true, "status_current").await;
        assert!(body.contains("state=OPERATE"));
        assert!(body.contains("temp=35.0"));
        assert!(body.contains("id=0.0"));
        assert!(body.contains("vac=0"));
        assert!(body.contains("meffa=OK"));
        assert!(body.contains("peakfwd=44.7712"));
    }

    #[tokio::test]
    async fn healthy_kpa_state_never_emits_pgxl_unknown() {
        let state = bridge_core::state::shared_default_state();
        {
            let mut guard = state.write().await;
            guard.amp.startup_state_policy = Some("wait_for_first_kpa_poll".to_string());
            guard.amp.first_poll_completed = false;
            guard.amp.connection_state = ConnectionState::Connected;
            guard.amp.connected = true;
            guard.amp.operate = true;
            guard.amp.state = bridge_core::AmpOperatingState::Operate;
            guard.amp.temperature_c = 38.0;
            guard.amp.pa_voltage_volts = 68.9;
            guard.amp.pa_current_amps = 0.0;
            guard.amp.swr = 1.0;
            guard.amp.meffa = "OK".to_string();
            guard.amp.last_successful_poll_at = Some(SystemTime::now());
            guard.amp.runtime.record_poll_success(25);
        }
        let body = status_body(&state, true, "status_current").await;
        assert!(body.contains("state=OPERATE"));
        assert!(!body.contains("state=UNKNOWN"));
        assert!(body.contains("temp=38.0"));
        assert!(body.contains("meffa=OK"));
    }

    #[test]
    fn golden_pgxl_info_response_is_stable() {
        assert_eq!(
            response_line(1, 0, info_body(false)),
            "R1|0|model=PowerGeniusXL serial_num=EGB-PGXL version=0.1.0-egb-pgxl firmware=0.1.0-egb-pgxl capabilities=direct_tcp,status\n"
        );
    }

    #[tokio::test]
    async fn aethersdr_compat_status_removes_unverified_fields() {
        let state = shared_mock_state();
        let body = status_body(&state, true, "status_current").await;
        assert_eq!(
            response_line(2, 0, body),
            "R2|0|state=STANDBY peakfwd=-120.0000 swr=-30.0000 temp=32.0 id=0.0 vac=0 meffa=OK\n"
        );
    }

    #[test]
    fn aethersdr_compat_info_removes_capabilities() {
        assert_eq!(
            response_line(1, 0, info_body(true)),
            "R1|0|model=PowerGeniusXL serial_num=EGB-PGXL version=0.1.0-egb-pgxl\n"
        );
    }

    #[tokio::test]
    async fn pgxl_readonly_config_paths_have_stable_responses() {
        let state = shared_mock_state();
        assert!(replay_line("C10|setup read", &state)
            .await
            .unwrap()
            .contains("nickname=Elecraft_Genius_Bridge"));
        assert!(replay_line("C11|ifconf read", &state)
            .await
            .unwrap()
            .contains("dhcp=true"));
        assert!(replay_line("C12|catradio read=A", &state)
            .await
            .unwrap()
            .contains("ampslice=A"));
        assert!(replay_line("C13|flexradio read=B", &state)
            .await
            .unwrap()
            .contains("ampslice=B"));
        assert_eq!(
            replay_line("C14|save", &state).await.unwrap(),
            "R14|5|error=save_reboot_unsupported_by_bridge\n"
        );
    }
}
