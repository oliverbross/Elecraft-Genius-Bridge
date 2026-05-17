use anyhow::{Context, Result};
use bridge_core::SharedState;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, timeout};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, info, warn};

// UNVERIFIED ELECRAFT COMMAND MAPPING.
// These strings are intentionally isolated until an official KAT500 command
// reference is added to docs/ or verified against real hardware.
const CMD_STATUS: &str = "ST;";
const CMD_AUTOTUNE: &str = "T;";
const CMD_BYPASS_ON: &str = "BP1;";
const CMD_BYPASS_OFF: &str = "BP0;";

#[derive(Debug, Clone)]
pub struct Kat500Settings {
    pub com_port: String,
    pub baud: u32,
    pub polling_interval: Duration,
    pub mock: bool,
}

pub struct Kat500Driver {
    settings: Kat500Settings,
    state: SharedState,
}

impl Kat500Driver {
    pub fn new(settings: Kat500Settings, state: SharedState) -> Self {
        Self { settings, state }
    }

    pub async fn run(self) {
        if self.settings.mock {
            self.run_mock().await;
        } else {
            self.run_serial_loop().await;
        }
    }

    async fn run_mock(self) {
        info!("KAT500 mock driver started");
        loop {
            {
                let mut guard = self.state.write().await;
                guard.tuner.connected = true;
                guard.tuner.last_serial_response_at = Some(SystemTime::now());
                if guard.tuner.tuning {
                    guard.tuner.swr = 1.15;
                }
            }
            sleep(self.settings.polling_interval).await;
        }
    }

    async fn run_serial_loop(self) {
        loop {
            match tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
                .open_native_async()
            {
                Ok(mut port) => {
                    info!(port = %self.settings.com_port, baud = self.settings.baud, "KAT500 serial connected");
                    {
                        let mut guard = self.state.write().await;
                        guard.tuner.connected = true;
                    }
                    loop {
                        if let Err(err) = self.poll_status_on_port(&mut port).await {
                            warn!(error = %err, "KAT500 poll failed; reconnecting");
                            break;
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        port = %self.settings.com_port,
                        error = %err,
                        "KAT500 serial open failed; retrying"
                    );
                    let mut guard = self.state.write().await;
                    guard.tuner.connected = false;
                }
            }
            sleep(Duration::from_secs(5)).await;
        }
    }

    pub async fn connect(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.tuner.connected = true;
            return Ok(());
        }
        let _port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| {
                format!(
                    "failed to open KAT500 serial port {} at {} baud",
                    self.settings.com_port, self.settings.baud
                )
            })?;
        Ok(())
    }

    pub async fn disconnect(&self) {
        let mut guard = self.state.write().await;
        guard.tuner.connected = false;
    }

    pub async fn poll_status(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.tuner.connected = true;
            guard.tuner.last_serial_response_at = Some(SystemTime::now());
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| {
                format!(
                    "failed to open KAT500 serial port {}",
                    self.settings.com_port
                )
            })?;
        self.poll_status_on_port(&mut port).await
    }

    pub async fn autotune(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.tuner.tuning = true;
            guard.tuner.swr = 1.15;
            guard.tuner.tuning = false;
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| {
                format!(
                    "failed to open KAT500 serial port {}",
                    self.settings.com_port
                )
            })?;
        Self::autotune_serial(&mut port).await?;
        Ok(())
    }

    pub async fn set_antenna(&self, antenna: u8) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.tuner.selected_antenna = Some(antenna.saturating_sub(1));
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| {
                format!(
                    "failed to open KAT500 serial port {}",
                    self.settings.com_port
                )
            })?;
        Self::set_antenna_serial(&mut port, antenna).await?;
        Ok(())
    }

    pub async fn set_bypass(&self, on: bool) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.tuner.bypass = on;
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| {
                format!(
                    "failed to open KAT500 serial port {}",
                    self.settings.com_port
                )
            })?;
        Self::set_bypass_serial(&mut port, on).await?;
        Ok(())
    }

    async fn poll_status_on_port(&self, port: &mut SerialStream) -> Result<()> {
        let response = send_command(port, CMD_STATUS, Duration::from_millis(750)).await?;
        debug!(response = %response, "KAT500 status response");
        let mut guard = self.state.write().await;
        guard.tuner.connected = true;
        guard.tuner.last_serial_command_at = Some(SystemTime::now());
        guard.tuner.last_serial_response_at = Some(SystemTime::now());
        parse_unverified_status(&response, &mut guard.tuner);
        sleep(self.settings.polling_interval).await;
        Ok(())
    }

    pub async fn autotune_serial(port: &mut SerialStream) -> Result<String> {
        send_command(port, CMD_AUTOTUNE, Duration::from_secs(5)).await
    }

    pub async fn set_bypass_serial(port: &mut SerialStream, on: bool) -> Result<String> {
        send_command(
            port,
            if on { CMD_BYPASS_ON } else { CMD_BYPASS_OFF },
            Duration::from_millis(750),
        )
        .await
    }

    pub async fn set_antenna_serial(port: &mut SerialStream, antenna: u8) -> Result<String> {
        let command = format!("AN{};", antenna);
        send_command(port, &command, Duration::from_millis(750)).await
    }
}

fn parse_unverified_status(response: &str, tuner: &mut bridge_core::TunerState) {
    // Placeholder parser. Keep this conservative until real KAT500 responses
    // are documented or captured.
    if response.contains("BYP") {
        tuner.bypass = true;
    }
}

async fn send_command(port: &mut SerialStream, command: &str, wait: Duration) -> Result<String> {
    port.write_all(command.as_bytes())
        .await
        .with_context(|| format!("failed to write serial command {command}"))?;
    port.flush().await.context("failed to flush serial port")?;

    let mut buf = Vec::new();
    timeout(wait, async {
        loop {
            let mut byte = [0_u8; 1];
            let n = port.read(&mut byte).await?;
            if n == 0 {
                continue;
            }
            buf.push(byte[0]);
            if byte[0] == b';' || byte[0] == b'\n' {
                break;
            }
        }
        Ok::<(), std::io::Error>(())
    })
    .await
    .context("serial response timed out")?
    .context("failed reading serial response")?;

    Ok(String::from_utf8_lossy(&buf).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unverified_bypass_parser_sets_bypass() {
        let mut tuner = bridge_core::TunerState::default();
        parse_unverified_status("BYP;", &mut tuner);
        assert!(tuner.bypass);
    }
}
