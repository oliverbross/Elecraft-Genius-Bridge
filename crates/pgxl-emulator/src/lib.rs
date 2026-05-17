use anyhow::Context;
use bridge_core::{parse_client_command, response_line, SharedState};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, info, warn};

const VERSION: &str = "0.1.0-egb-pgxl";

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
    info!(%peer, "PGXL client connected");

    let result = async {
        let (reader, mut writer) = socket.into_split();
        let mut transcript = Transcript::new("pgxl", peer, options.transcript_dir.clone()).await?;
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
                    let response = handle_command(cmd.seq, &cmd.command, &state).await;
                    debug!(%peer, command = %cmd.command, "PGXL command handled");
                    write_protocol_line(
                        &mut writer,
                        &mut transcript,
                        "PGXL",
                        "TX >",
                        response.trim_end(),
                        options.protocol_trace,
                    )
                    .await?;
                }
                Err(err) => {
                    warn!(%peer, %err, line = %line, "invalid PGXL command frame");
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    {
        let mut guard = state.write().await;
        guard.clients.pgxl_client_count = guard.clients.pgxl_client_count.saturating_sub(1);
        guard.clients.pgxl_connected = guard.clients.pgxl_client_count > 0;
    }
    info!(%peer, "PGXL client disconnected");
    result
}

async fn handle_command(seq: u32, command: &str, state: &SharedState) -> String {
    match command {
        "info" => response_line(
            seq,
            0,
            format!("model=PowerGeniusXL serial_num=EGB-PGXL version={VERSION}"),
        ),
        "status" => response_line(seq, 0, status_body(state).await),
        // AetherSDR currently routes PGXL operate/standby through the Flex radio
        // amplifier API, not direct TCP. These direct commands are accepted only
        // as a conservative extension for manual harness testing.
        "operate" => {
            let mut guard = state.write().await;
            guard.amp.operate = true;
            guard.amp.state = bridge_core::AmpOperatingState::Operate;
            response_line(seq, 0, status_body_from_amp(&guard.amp))
        }
        "standby" => {
            let mut guard = state.write().await;
            guard.amp.operate = false;
            guard.amp.state = bridge_core::AmpOperatingState::Standby;
            guard.amp.forward_power_watts = 0.0;
            response_line(seq, 0, status_body_from_amp(&guard.amp))
        }
        _ => {
            warn!(%command, "unknown PGXL command");
            response_line(seq, 1, "error=unknown_command")
        }
    }
}

async fn status_body(state: &SharedState) -> String {
    let guard = state.read().await;
    status_body_from_amp(&guard.amp)
}

fn status_body_from_amp(amp: &bridge_core::AmpState) -> String {
    let state = amp.state.pgxl_state();
    let peakfwd = watts_to_dbm(amp.forward_power_watts);
    let swr_rl = swr_to_return_loss_db(amp.swr);
    let fault = amp.fault.as_deref().unwrap_or("");
    format!(
        "state={state} peakfwd={peakfwd:.4} swr={swr_rl:.4} temp={:.1} id={:.1} vac={} meffa={} fault={fault}",
        amp.temperature_c, amp.drain_current_amps, amp.mains_volts, amp.meffa
    )
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
            "R2|0|state=STANDBY peakfwd=0.0000 swr=32.2557 temp=32.0 id=0.0 vac=230 meffa=OK fault=\n"
        );
    }
}
