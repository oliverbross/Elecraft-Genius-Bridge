use anyhow::{Context, Result};
use bridge_core::{AmpOperatingState, SharedState};
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, timeout};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, info, warn};

// UNVERIFIED ELECRAFT COMMAND MAPPING.
// These strings are intentionally isolated until an official KPA500 command
// reference is added to docs/ or verified against real hardware.
const CMD_STATUS: &str = "ST;";
const CMD_OPERATE: &str = "OP1;";
const CMD_STANDBY: &str = "OP0;";
const CMD_CLEAR_FAULT: &str = "FC;";

#[derive(Debug, Clone)]
pub struct Kpa500Settings {
    pub com_port: String,
    pub baud: u32,
    pub polling_interval: Duration,
    pub mock: bool,
}

pub struct Kpa500Driver {
    settings: Kpa500Settings,
    state: SharedState,
}

impl Kpa500Driver {
    pub fn new(settings: Kpa500Settings, state: SharedState) -> Self {
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
        info!("KPA500 mock driver started");
        loop {
            {
                let mut guard = self.state.write().await;
                guard.amp.connected = true;
                guard.amp.last_serial_response_at = Some(SystemTime::now());
                if guard.amp.operate {
                    guard.amp.state = AmpOperatingState::Idle;
                    guard.amp.mains_volts = 230;
                    guard.amp.temperature_c = (guard.amp.temperature_c + 0.1).min(45.0);
                    guard.amp.meffa = "OK".to_string();
                } else {
                    guard.amp.state = AmpOperatingState::Standby;
                    guard.amp.forward_power_watts = 0.0;
                    guard.amp.drain_current_amps = 0.0;
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
                    info!(port = %self.settings.com_port, baud = self.settings.baud, "KPA500 serial connected");
                    {
                        let mut guard = self.state.write().await;
                        guard.amp.connected = true;
                    }
                    loop {
                        if let Err(err) = self.poll_status_on_port(&mut port).await {
                            warn!(error = %err, "KPA500 poll failed; reconnecting");
                            break;
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        port = %self.settings.com_port,
                        error = %err,
                        "KPA500 serial open failed; retrying"
                    );
                    let mut guard = self.state.write().await;
                    guard.amp.connected = false;
                }
            }
            sleep(Duration::from_secs(5)).await;
        }
    }

    pub async fn connect(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.connected = true;
            return Ok(());
        }
        let _port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| {
                format!(
                    "failed to open KPA500 serial port {} at {} baud",
                    self.settings.com_port, self.settings.baud
                )
            })?;
        Ok(())
    }

    pub async fn disconnect(&self) {
        let mut guard = self.state.write().await;
        guard.amp.connected = false;
    }

    pub async fn poll_status(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.connected = true;
            guard.amp.last_serial_response_at = Some(SystemTime::now());
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| format!("failed to open KPA500 serial port {}", self.settings.com_port))?;
        self.poll_status_on_port(&mut port).await
    }

    pub async fn set_operate(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.operate = true;
            guard.amp.state = AmpOperatingState::Operate;
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| format!("failed to open KPA500 serial port {}", self.settings.com_port))?;
        Self::set_operate_serial(&mut port).await?;
        Ok(())
    }

    pub async fn set_standby(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.operate = false;
            guard.amp.state = AmpOperatingState::Standby;
            guard.amp.forward_power_watts = 0.0;
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| format!("failed to open KPA500 serial port {}", self.settings.com_port))?;
        Self::set_standby_serial(&mut port).await?;
        Ok(())
    }

    pub async fn clear_fault(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.fault = None;
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| format!("failed to open KPA500 serial port {}", self.settings.com_port))?;
        Self::clear_fault_serial(&mut port).await?;
        Ok(())
    }

    async fn poll_status_on_port(&self, port: &mut SerialStream) -> Result<()> {
        let response = send_command(port, CMD_STATUS, Duration::from_millis(750)).await?;
        debug!(response = %response, "KPA500 status response");
        let mut guard = self.state.write().await;
        guard.amp.connected = true;
        guard.amp.last_serial_command_at = Some(SystemTime::now());
        guard.amp.last_serial_response_at = Some(SystemTime::now());
        parse_unverified_status(&response, &mut guard.amp);
        sleep(self.settings.polling_interval).await;
        Ok(())
    }

    pub async fn set_operate_serial(port: &mut SerialStream) -> Result<String> {
        send_command(port, CMD_OPERATE, Duration::from_millis(750)).await
    }

    pub async fn set_standby_serial(port: &mut SerialStream) -> Result<String> {
        send_command(port, CMD_STANDBY, Duration::from_millis(750)).await
    }

    pub async fn clear_fault_serial(port: &mut SerialStream) -> Result<String> {
        send_command(port, CMD_CLEAR_FAULT, Duration::from_millis(750)).await
    }
}

fn parse_unverified_status(response: &str, amp: &mut bridge_core::AmpState) {
    // Placeholder parser. Keep this conservative until real KPA500 responses
    // are documented or captured.
    if response.contains("FAULT") {
        amp.state = AmpOperatingState::Fault;
        amp.fault = Some(response.to_string());
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
    fn unverified_fault_parser_sets_fault() {
        let mut amp = bridge_core::AmpState::default();
        parse_unverified_status("FAULT;", &mut amp);
        assert_eq!(amp.state, AmpOperatingState::Fault);
        assert!(amp.fault.is_some());
    }
}
