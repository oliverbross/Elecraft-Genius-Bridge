use anyhow::Context;
use bridge_core::{parse_client_command, response_line, SharedState};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

const VERSION: &str = "0.1.0-egb-tgxl";

#[derive(Debug, Clone, Default)]
pub struct EmulatorOptions {
    pub protocol_trace: bool,
    pub transcript_dir: Option<PathBuf>,
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
    info!(%peer, "TGXL client connected");

    let result = async {
        let (reader, mut writer) = socket.into_split();
        let mut transcript = Transcript::new("tgxl", peer, options.transcript_dir.clone()).await?;
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
                    let outcome = handle_command(cmd.seq, &cmd.command, &state).await;
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
                    }
                }
                Err(err) => {
                    warn!(%peer, %err, line = %line, "invalid TGXL command frame");
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    {
        let mut guard = state.write().await;
        guard.clients.tgxl_client_count = guard.clients.tgxl_client_count.saturating_sub(1);
        guard.clients.tgxl_connected = guard.clients.tgxl_client_count > 0;
    }
    info!(%peer, "TGXL client disconnected");
    result
}

struct CommandOutcome {
    response: String,
    pushes: Vec<String>,
}

async fn handle_command(seq: u32, command: &str, state: &SharedState) -> CommandOutcome {
    match command {
        "info" => CommandOutcome {
            response: response_line(
                seq,
                0,
                format!("model=TunerGeniusXL serial_num=EGB-TGXL version={VERSION} one_by_three=1"),
            ),
            pushes: Vec::new(),
        },
        "status" => CommandOutcome {
            response: response_line(seq, 0, status_body(state).await),
            pushes: Vec::new(),
        },
        "autotune" => {
            {
                let mut guard = state.write().await;
                guard.tuner.tuning = true;
            }
            let mut pushes = vec![state_push(state).await];
            sleep(Duration::from_millis(800)).await;
            {
                let mut guard = state.write().await;
                guard.tuner.tuning = false;
                guard.tuner.swr = 1.15;
            }
            pushes.push(state_push(state).await);
            CommandOutcome {
                response: response_line(seq, 0, status_body(state).await),
                pushes,
            }
        }
        _ if command.starts_with("activate ant=") => {
            let ant = command
                .trim_start_matches("activate ant=")
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=3).contains(n));
            if let Some(ant) = ant {
                let mut guard = state.write().await;
                guard.tuner.selected_antenna = Some(ant - 1);
                CommandOutcome {
                    response: response_line(seq, 0, status_body_from_tuner(&guard.tuner)),
                    pushes: vec![state_push_from_tuner(&guard.tuner)],
                }
            } else {
                CommandOutcome {
                    response: response_line(seq, 2, "error=invalid_antenna"),
                    pushes: Vec::new(),
                }
            }
        }
        _ if command.starts_with("tune relay=") => {
            let result = apply_relay_command(command, state).await;
            match result {
                Ok(()) => CommandOutcome {
                    response: response_line(seq, 0, status_body(state).await),
                    pushes: vec![state_push(state).await],
                },
                Err(error) => CommandOutcome {
                    response: response_line(seq, 2, format!("error={error}")),
                    pushes: Vec::new(),
                },
            }
        }
        _ => {
            warn!(%command, "unknown TGXL command");
            CommandOutcome {
                response: response_line(seq, 1, "error=unknown_command"),
                pushes: Vec::new(),
            }
        }
    }
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
    let step = if movement >= 0 { 1 } else { -1 };

    let mut guard = state.write().await;
    let target = match relay {
        0 => &mut guard.tuner.relay_c1,
        1 => &mut guard.tuner.relay_l,
        2 => &mut guard.tuner.relay_c2,
        _ => unreachable!(),
    };
    *target = (*target + step).clamp(0, 255);
    Ok(())
}

async fn status_body(state: &SharedState) -> String {
    let guard = state.read().await;
    status_body_from_tuner(&guard.tuner)
}

fn status_body_from_tuner(tuner: &bridge_core::TunerState) -> String {
    let fwd = watts_to_dbm(tuner.forward_power_watts);
    let swr = -swr_to_return_loss_db(tuner.swr);
    format!(
        "operate={} bypass={} tuning={} relayC1={} relayL={} relayC2={} antA={} one_by_three=1 fwd={fwd:.4} swr={swr:.4}",
        bool_int(tuner.operate),
        bool_int(tuner.bypass),
        bool_int(tuner.tuning),
        tuner.relay_c1,
        tuner.relay_l,
        tuner.relay_c2,
        tuner.selected_antenna.unwrap_or(0),
    )
}

async fn state_push(state: &SharedState) -> String {
    let guard = state.read().await;
    state_push_from_tuner(&guard.tuner)
}

fn state_push_from_tuner(tuner: &bridge_core::TunerState) -> String {
    format!("S0|state {}\n", status_body_from_tuner(tuner))
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
        assert_eq!(guard.tuner.relay_c1, 21);
    }

    #[tokio::test]
    async fn golden_tgxl_mock_status_response_is_stable() {
        let state = shared_mock_state();
        let body = status_body(&state).await;
        assert_eq!(
            response_line(2, 0, body),
            "R2|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=-32.2557\n"
        );
    }
}
