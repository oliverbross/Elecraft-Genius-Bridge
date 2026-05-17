use anyhow::Context;
use bridge_core::{
    parse_client_command, response_line, AmpOperatingState, ConnectionState, SharedState,
};
use std::net::SocketAddr;
use std::path::PathBuf;
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
    pub strict_emulation: bool,
    pub startup_delay: Duration,
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

    loop {
        let (socket, peer) = listener.accept().await?;
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
    Ok(handle_command(cmd.seq, &cmd.command, state).await.response)
}

async fn handle_client(
    socket: TcpStream,
    peer: SocketAddr,
    state: SharedState,
    options: EmulatorOptions,
) -> anyhow::Result<()> {
    {
        let mut guard = state.write().await;
        guard.clients.pgxl_connected = true;
        guard.clients.pgxl_client_count += 1;
    }
    maybe_start_strict_startup(&state, &options).await;
    info!(event_id = "client_connected", protocol = "PGXL", connection_id = %peer, "PGXL client connected");

    let result = async {
        let (reader, mut writer) = socket.into_split();
        let mut transcript = Transcript::new("pgxl", peer, options.transcript_dir.clone()).await?;
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
            debug!(%peer, command_line_len = line.len(), "PGXL command received");
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
                    let outcome = handle_command(cmd.seq, &cmd.command, &state).await;
                    if outcome.unknown {
                        stats.unknown_commands += 1;
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
    }
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

async fn handle_command(seq: u32, command: &str, state: &SharedState) -> CommandOutcome {
    match command {
        "info" => CommandOutcome::ok(response_line(seq, 0, info_body())),
        "status" => CommandOutcome::ok(response_line(seq, 0, status_body(state).await)),
        // AetherSDR currently routes PGXL operate/standby through the Flex radio
        // amplifier API, not direct TCP. These direct commands are accepted only
        // as desired-state requests for manual harness testing.
        "operate" => {
            let mut guard = state.write().await;
            guard.desired.amp_operate = Some(true);
            CommandOutcome::ok(response_line(seq, 0, status_body_from_amp(&guard.amp)))
        }
        "standby" => {
            let mut guard = state.write().await;
            guard.desired.amp_operate = Some(false);
            CommandOutcome::ok(response_line(seq, 0, status_body_from_amp(&guard.amp)))
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

fn info_body() -> String {
    format!(
        "model=PowerGeniusXL serial_num=EGB-PGXL version={VERSION} firmware={VERSION} capabilities=direct_tcp,status"
    )
}

async fn status_body(state: &SharedState) -> String {
    let guard = state.read().await;
    status_body_from_amp(&guard.amp)
}

fn status_body_from_amp(amp: &bridge_core::AmpState) -> String {
    let degraded = matches!(
        amp.connection_state,
        ConnectionState::Disconnected | ConnectionState::Degraded | ConnectionState::Error
    );
    let state = if degraded {
        "FAULT"
    } else {
        amp.state.pgxl_state()
    };
    let peakfwd = watts_to_dbm(amp.forward_power_watts);
    let swr_rl = swr_to_return_loss_db(amp.swr);
    let fault = amp
        .fault
        .as_deref()
        .unwrap_or(if degraded { "device_degraded" } else { "" });
    format!(
        "state={state} peakfwd={peakfwd:.4} swr={swr_rl:.4} temp={:.1} id={:.1} vac={} meffa={} fault={fault} connection_state={}",
        amp.temperature_c,
        amp.drain_current_amps,
        amp.mains_volts,
        amp.meffa,
        amp.connection_state.as_str()
    )
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
}

impl Transcript {
    async fn new(device: &str, peer: SocketAddr, dir: Option<PathBuf>) -> anyhow::Result<Self> {
        let Some(dir) = dir else {
            return Ok(Self { file: None });
        };
        create_dir_all(&dir).await?;
        let ts = timestamp_millis();
        let peer = peer.to_string().replace([':', '.'], "_");
        let path = dir.join(format!("{device}-{ts}-{peer}.log"));
        let file = File::create(path).await?;
        Ok(Self { file: Some(file) })
    }

    async fn write_line(
        &mut self,
        device: &str,
        direction: &str,
        line: &str,
    ) -> anyhow::Result<()> {
        if let Some(file) = &mut self.file {
            let row = format!("{} {device} {direction} {line}\n", timestamp_millis());
            file.write_all(row.as_bytes()).await?;
            file.flush().await?;
        }
        Ok(())
    }
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

fn watts_to_dbm(watts: f32) -> f32 {
    if watts <= 0.0 {
        0.0
    } else {
        10.0 * (watts * 1000.0).log10()
    }
}

fn swr_to_return_loss_db(swr: f32) -> f32 {
    if swr <= 1.0 {
        30.0
    } else {
        let rho = ((swr - 1.0) / (swr + 1.0)).clamp(0.000_001, 0.999_999);
        -20.0 * rho.log10()
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
        assert!(body.contains("state=STANDBY"));
        assert!(body.contains("peakfwd="));
        assert!(body.contains("meffa="));
    }

    #[tokio::test]
    async fn golden_pgxl_mock_status_response_is_stable() {
        let state = shared_mock_state();
        let body = status_body(&state).await;
        assert_eq!(
            response_line(2, 0, body),
            "R2|0|state=STANDBY peakfwd=0.0000 swr=32.2557 temp=32.0 id=0.0 vac=230 meffa=OK fault= connection_state=connected\n"
        );
    }

    #[test]
    fn golden_pgxl_info_response_is_stable() {
        assert_eq!(
            response_line(1, 0, info_body()),
            "R1|0|model=PowerGeniusXL serial_num=EGB-PGXL version=0.1.0-egb-pgxl firmware=0.1.0-egb-pgxl capabilities=direct_tcp,status\n"
        );
    }
}
