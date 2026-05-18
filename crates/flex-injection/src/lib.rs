use anyhow::{Context, Result};
use bridge_core::SharedState;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct FlexInjectionSettings {
    pub radio_addr: SocketAddr,
    pub amplifier_ip: IpAddr,
    pub amplifier_port: u16,
    pub amplifier_model: String,
    pub serial: String,
    pub handle_label: String,
    pub ant_map: String,
    pub reconnect_initial: Duration,
    pub reconnect_max: Duration,
    pub ping_interval: Duration,
}

impl FlexInjectionSettings {
    pub fn amplifier_create_command(&self) -> String {
        amplifier_create_command(
            self.amplifier_ip,
            self.amplifier_port,
            &self.amplifier_model,
            &self.serial,
            &self.ant_map,
        )
    }
}

pub async fn run(settings: FlexInjectionSettings, state: SharedState) {
    let mut backoff = settings.reconnect_initial.max(Duration::from_millis(100));
    loop {
        match run_session(&settings, state.clone()).await {
            Ok(()) => {
                warn!("Flex amplifier injection session ended");
            }
            Err(err) => {
                warn!(
                    error = %err,
                    retry_ms = backoff.as_millis(),
                    "Flex amplifier injection session failed"
                );
            }
        }
        sleep(backoff).await;
        backoff = (backoff * 2).min(settings.reconnect_max.max(settings.reconnect_initial));
    }
}

async fn run_session(settings: &FlexInjectionSettings, state: SharedState) -> Result<()> {
    info!(
        radio = %settings.radio_addr,
        amplifier_ip = %settings.amplifier_ip,
        amplifier_port = settings.amplifier_port,
        model = %settings.amplifier_model,
        serial = %settings.serial,
        handle_label = %settings.handle_label,
        "Flex amplifier injection connecting"
    );

    let stream = TcpStream::connect(settings.radio_addr)
        .await
        .with_context(|| format!("failed to connect to Flex API at {}", settings.radio_addr))?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader).lines();
    let mut session = FlexSession::default();

    let create = settings.amplifier_create_command();
    let mut create_sent = false;
    let mut next_seq = 1_u32;
    let mut ping_timer = Box::pin(sleep(settings.ping_interval));

    loop {
        tokio::select! {
            line = timeout(Duration::from_secs(60), reader.next_line()) => {
                let line = match line {
                    Ok(Ok(Some(line))) => line,
                    Ok(Ok(None)) => anyhow::bail!("Flex API closed connection"),
                    Ok(Err(err)) => return Err(err).context("failed to read Flex API line"),
                    Err(_) => anyhow::bail!("timed out waiting for Flex API traffic"),
                };
                session.observe_line(&line);
                trace_flex_rx(&line);

                if session.has_handle && !create_sent {
                    send_command(&mut writer, next_seq, &create).await?;
                    info!(
                        seq = next_seq,
                        command = %create,
                        "Flex amplifier object creation sent"
                    );
                    next_seq = next_seq.saturating_add(1);
                    create_sent = true;
                }

                if let Some((seq, code, body)) = parse_response(&line) {
                    if seq == 1 && code == "0" {
                        info!(
                            seq,
                            body = %body,
                            "Flex amplifier object creation accepted"
                        );
                        log_amp_snapshot(&state).await;
                    } else if seq == 1 {
                        warn!(
                            seq,
                            code = %code,
                            body = %body,
                            "Flex amplifier object creation rejected"
                        );
                    }
                }
            }
            () = &mut ping_timer => {
                if create_sent {
                    send_command(&mut writer, next_seq, "ping").await?;
                    debug!(seq = next_seq, "Flex injection ping sent");
                    next_seq = next_seq.saturating_add(1);
                    log_amp_snapshot(&state).await;
                }
                ping_timer.as_mut().reset(tokio::time::Instant::now() + settings.ping_interval);
            }
        }
    }
}

async fn send_command(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    seq: u32,
    command: &str,
) -> Result<()> {
    let line = format!("C{seq}|{command}\n");
    trace_flex_tx(line.trim_end());
    writer
        .write_all(line.as_bytes())
        .await
        .context("failed to write Flex API command")?;
    Ok(())
}

#[derive(Debug, Default)]
struct FlexSession {
    has_handle: bool,
    handle: Option<String>,
    version: Option<String>,
}

impl FlexSession {
    fn observe_line(&mut self, line: &str) {
        if let Some(version) = line.strip_prefix('V') {
            self.version = Some(version.trim().to_string());
            info!(version = %version.trim(), "Flex API version received");
        } else if let Some(handle) = line.strip_prefix('H') {
            let handle = handle.trim().to_string();
            self.handle = Some(handle.clone());
            self.has_handle = true;
            info!(%handle, "Flex API client handle received");
        }
    }
}

async fn log_amp_snapshot(state: &SharedState) {
    let guard = state.read().await;
    let amp = &guard.amp;
    debug!(
        connection_state = amp.connection_state.as_str(),
        operate = amp.operate,
        pgxl_state = amp.state.pgxl_state(),
        peakfwd_watts = amp.forward_power_watts,
        swr = amp.swr,
        temp_c = amp.temperature_c,
        fault = amp.fault.as_deref().unwrap_or(""),
        "Flex injection amplifier telemetry snapshot"
    );
}

fn trace_flex_rx(line: &str) {
    debug!(line = %line, "FLEX RX <");
}

fn trace_flex_tx(line: &str) {
    debug!(line = %line, "FLEX TX >");
}

pub fn amplifier_create_command(
    amplifier_ip: IpAddr,
    amplifier_port: u16,
    model: &str,
    serial: &str,
    ant_map: &str,
) -> String {
    format!(
        "amplifier create ip={amplifier_ip} port={amplifier_port} model={} serial_num={} ant={}",
        sanitize_token(model),
        sanitize_token(serial),
        sanitize_token(ant_map)
    )
}

fn sanitize_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '|')
        .collect()
}

pub fn parse_response(line: &str) -> Option<(u32, String, String)> {
    let rest = line.strip_prefix('R')?;
    let mut parts = rest.splitn(3, '|');
    let seq = parts.next()?.parse().ok()?;
    let code = parts.next()?.to_string();
    let body = parts.next().unwrap_or("").to_string();
    Some((seq, code, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_amplifier_create_command() {
        let cmd = amplifier_create_command(
            "192.168.1.50".parse().unwrap(),
            9008,
            "PowerGeniusXL",
            "EGB-KPA500",
            "ANT1:PORTA,ANT2:NONE",
        );
        assert_eq!(
            cmd,
            "amplifier create ip=192.168.1.50 port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:NONE"
        );
    }

    #[test]
    fn strips_unsafe_token_characters() {
        let cmd = amplifier_create_command(
            "127.0.0.1".parse().unwrap(),
            9008,
            "Power Genius|XL",
            "EGB KPA500",
            "ANT1:PORTA, ANT2:NONE",
        );
        assert!(cmd.contains("model=PowerGeniusXL"));
        assert!(cmd.contains("serial_num=EGBKPA500"));
        assert!(cmd.contains("ant=ANT1:PORTA,ANT2:NONE"));
    }

    #[test]
    fn parses_radio_response() {
        assert_eq!(
            parse_response("R44|0|32"),
            Some((44, "0".to_string(), "32".to_string()))
        );
        assert_eq!(parse_response("S0|amplifier 0x1 model=PowerGeniusXL"), None);
    }
}
