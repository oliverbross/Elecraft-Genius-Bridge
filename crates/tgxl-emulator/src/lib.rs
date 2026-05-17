use anyhow::Context;
use bridge_core::{
    parse_client_command, response_line, ConnectionState, ManualTuneRequest, SharedState,
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

    loop {
        let (socket, peer) = listener.accept().await?;
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
    let outcome = handle_command(cmd.seq, &cmd.command, state).await;
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
    {
        let mut guard = state.write().await;
        guard.clients.tgxl_connected = true;
        guard.clients.tgxl_client_count += 1;
    }
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
                    let outcome = handle_command(cmd.seq, &cmd.command, &state).await;
                    if outcome.unknown {
                        stats.unknown_commands += 1;
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
                        increment_responses(&state).await;
                    }
                }
                Err(err) => {
                    stats.parse_failures += 1;
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
    }
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

async fn handle_command(seq: u32, command: &str, state: &SharedState) -> CommandOutcome {
    match command {
        "info" => CommandOutcome::ok(response_line(seq, 0, info_body())),
        "status" => CommandOutcome::ok(response_line(seq, 0, status_body(state).await)),
        "autotune" => {
            {
                let mut guard = state.write().await;
                guard.desired.tuner_autotune_requested = true;
            }
            let pushes = vec![state_push(state).await];
            sleep(Duration::from_millis(800)).await;
            CommandOutcome::with_pushes(response_line(seq, 0, status_body(state).await), pushes)
        }
        _ if command.starts_with("activate ant=") => {
            let ant = command
                .trim_start_matches("activate ant=")
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=3).contains(n));
            if let Some(ant) = ant {
                let mut guard = state.write().await;
                guard.desired.tuner_selected_antenna = Some(ant);
                CommandOutcome {
                    response: response_line(seq, 0, status_body_from_tuner(&guard.tuner)),
                    pushes: vec![state_push_from_tuner(&guard.tuner)],
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
        _ if command.starts_with("tune relay=") => {
            let result = apply_relay_command(command, state).await;
            match result {
                Ok(()) => CommandOutcome::with_pushes(
                    response_line(seq, 0, status_body(state).await),
                    vec![state_push(state).await],
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

fn info_body() -> String {
    format!(
        "model=TunerGeniusXL serial_num=EGB-TGXL version={VERSION} firmware={VERSION} one_by_three=1 capabilities=direct_tcp,status,autotune,ant,manual_tune"
    )
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
    if relay > 2 {
        return Err("invalid_relay");
    }

    let mut guard = state.write().await;
    guard.desired.tuner_manual_tune = Some(ManualTuneRequest { relay, movement });
    Ok(())
}

async fn status_body(state: &SharedState) -> String {
    let guard = state.read().await;
    status_body_from_tuner(&guard.tuner)
}

fn status_body_from_tuner(tuner: &bridge_core::TunerState) -> String {
    let fwd = watts_to_dbm(tuner.forward_power_watts);
    let swr = protocol_swr_value(tuner.swr);
    let degraded = matches!(
        tuner.connection_state,
        ConnectionState::Disconnected | ConnectionState::Degraded | ConnectionState::Error
    );
    format!(
        "operate={} bypass={} tuning={} relayC1={} relayL={} relayC2={} antA={} one_by_three=1 fwd={fwd:.4} swr={swr:.4} connection_state={} fault={}",
        bool_int(tuner.operate),
        bool_int(tuner.bypass),
        bool_int(tuner.tuning || degraded),
        tuner.relay_c1,
        tuner.relay_l,
        tuner.relay_c2,
        tuner.selected_antenna.unwrap_or(0),
        tuner.connection_state.as_str(),
        tuner.fault.as_deref().unwrap_or(if degraded {
            "device_degraded"
        } else {
            ""
        }),
    )
}

async fn state_push(state: &SharedState) -> String {
    let guard = state.read().await;
    state_push_from_tuner(&guard.tuner)
}

fn state_push_from_tuner(tuner: &bridge_core::TunerState) -> String {
    format!("S0|state {}\n", status_body_from_tuner(tuner))
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
    transcript.write_line(device, direction, line).await
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
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
        0.0
    } else {
        10.0 * (watts * 1000.0).log10()
    }
}

fn protocol_swr_value(swr: f32) -> f32 {
    if swr.is_finite() && swr >= 1.0 {
        swr
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::state::shared_mock_state;

    #[tokio::test]
    async fn status_contains_aethersdr_fields() {
        let state = shared_mock_state();
        let body = status_body(&state).await;
        assert!(body.contains("operate=0"));
        assert!(body.contains("relayC1="));
        assert!(body.contains("one_by_three=1"));
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
        let body = status_body(&state).await;
        assert_eq!(
            response_line(2, 0, body),
            "R2|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=1.0000 connection_state=connected fault=\n"
        );
    }

    #[tokio::test]
    async fn mock_status_never_reports_negative_or_extreme_swr() {
        let state = shared_mock_state();
        let body = status_body(&state).await;
        assert!(body.contains("swr=1.0000"));
        assert!(!body.contains("swr=-"));
        assert!(!body.contains("swr=32."));
    }

    #[test]
    fn golden_tgxl_info_response_is_stable() {
        assert_eq!(
            response_line(1, 0, info_body()),
            "R1|0|model=TunerGeniusXL serial_num=EGB-TGXL version=0.1.0-egb-tgxl firmware=0.1.0-egb-tgxl one_by_three=1 capabilities=direct_tcp,status,autotune,ant,manual_tune\n"
        );
    }
}
