use anyhow::{Context, Result};
use bridge_core::{AmpOperatingState, ConnectionState, SharedState};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, timeout};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, info, warn};

// UNVERIFIED ELECRAFT COMMAND MAPPING.
// These strings are intentionally isolated until an official KPA500 command
// reference is added to docs/ or verified against real hardware.
const CMD_STATUS: ElecraftCommand = ElecraftCommand {
    label: "poll_status",
    wire: "ST;",
    safety: CommandSafety::ReadOnly,
    verified: false,
};
const CMD_VERSION: ElecraftCommand = ElecraftCommand {
    label: "read_version",
    wire: "RV;",
    safety: CommandSafety::ReadOnly,
    verified: false,
};
const CMD_OPERATE: ElecraftCommand = ElecraftCommand {
    label: "set_operate",
    wire: "OP1;",
    safety: CommandSafety::RfRisk,
    verified: false,
};
const CMD_STANDBY: ElecraftCommand = ElecraftCommand {
    label: "set_standby",
    wire: "OP0;",
    safety: CommandSafety::StateChangeSafe,
    verified: false,
};
const CMD_CLEAR_FAULT: ElecraftCommand = ElecraftCommand {
    label: "clear_fault",
    wire: "FC;",
    safety: CommandSafety::DestructiveOrUnknown,
    verified: false,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSafety {
    ReadOnly,
    StateChangeSafe,
    RfRisk,
    DestructiveOrUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElecraftCommand {
    pub label: &'static str,
    pub wire: &'static str,
    pub safety: CommandSafety,
    pub verified: bool,
}

pub fn command_map() -> &'static [ElecraftCommand] {
    &[
        CMD_STATUS,
        CMD_VERSION,
        CMD_OPERATE,
        CMD_STANDBY,
        CMD_CLEAR_FAULT,
    ]
}

#[derive(Debug, Clone)]
pub struct Kpa500Settings {
    pub com_port: String,
    pub baud: u32,
    pub polling_interval: Duration,
    pub mock: bool,
    pub dry_run: bool,
    pub transcript_dir: Option<PathBuf>,
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
                guard.amp.connection_state = ConnectionState::Connected;
                guard.amp.last_serial_response_at = Some(SystemTime::now());
                guard.amp.last_successful_poll_at = Some(SystemTime::now());
                if let Some(operate) = guard.desired.amp_operate {
                    guard.amp.operate = operate;
                }
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
                    let mut transcript = SerialTranscript::open(
                        "KPA500",
                        &self.settings.com_port,
                        &self.settings.transcript_dir,
                    )
                    .await;
                    {
                        let mut guard = self.state.write().await;
                        guard.amp.connected = true;
                        guard.amp.connection_state = ConnectionState::Connecting;
                    }
                    self.discover_capabilities(&mut port, &mut transcript).await;
                    loop {
                        if let Err(err) = self.poll_status_on_port(&mut port, &mut transcript).await
                        {
                            warn!(event_id = "serial_disconnected", device = "KPA500", error = %err, "KPA500 poll failed; reconnecting");
                            let mut guard = self.state.write().await;
                            guard.amp.connected = false;
                            guard.amp.connection_state = ConnectionState::Degraded;
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
                    guard.amp.connection_state = ConnectionState::Disconnected;
                }
            }
            warn!(
                event_id = "reconnect_attempt",
                device = "KPA500",
                "KPA500 reconnect attempt scheduled"
            );
            sleep(Duration::from_secs(5)).await;
        }
    }

    pub async fn connect(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.connected = true;
            guard.amp.connection_state = ConnectionState::Connected;
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
        guard.amp.connection_state = ConnectionState::Disconnected;
    }

    pub async fn poll_status(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.connected = true;
            guard.amp.connection_state = ConnectionState::Connected;
            guard.amp.last_serial_response_at = Some(SystemTime::now());
            guard.amp.last_successful_poll_at = Some(SystemTime::now());
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| {
                format!(
                    "failed to open KPA500 serial port {}",
                    self.settings.com_port
                )
            })?;
        let mut transcript = SerialTranscript::open(
            "KPA500",
            &self.settings.com_port,
            &self.settings.transcript_dir,
        )
        .await;
        self.poll_status_on_port(&mut port, &mut transcript).await
    }

    pub async fn set_operate(&self) -> Result<()> {
        self.ensure_can_send(CMD_OPERATE)?;
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.operate = true;
            guard.amp.state = AmpOperatingState::Operate;
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| {
                format!(
                    "failed to open KPA500 serial port {}",
                    self.settings.com_port
                )
            })?;
        let mut transcript = SerialTranscript::open(
            "KPA500",
            &self.settings.com_port,
            &self.settings.transcript_dir,
        )
        .await;
        send_command(
            &mut port,
            CMD_OPERATE,
            Duration::from_millis(750),
            &mut transcript,
        )
        .await?;
        Ok(())
    }

    pub async fn set_standby(&self) -> Result<()> {
        self.ensure_can_send(CMD_STANDBY)?;
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.operate = false;
            guard.amp.state = AmpOperatingState::Standby;
            guard.amp.forward_power_watts = 0.0;
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| {
                format!(
                    "failed to open KPA500 serial port {}",
                    self.settings.com_port
                )
            })?;
        let mut transcript = SerialTranscript::open(
            "KPA500",
            &self.settings.com_port,
            &self.settings.transcript_dir,
        )
        .await;
        send_command(
            &mut port,
            CMD_STANDBY,
            Duration::from_millis(750),
            &mut transcript,
        )
        .await?;
        Ok(())
    }

    pub async fn clear_fault(&self) -> Result<()> {
        self.ensure_can_send(CMD_CLEAR_FAULT)?;
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.fault = None;
            return Ok(());
        }
        let mut port = tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
            .open_native_async()
            .with_context(|| {
                format!(
                    "failed to open KPA500 serial port {}",
                    self.settings.com_port
                )
            })?;
        let mut transcript = SerialTranscript::open(
            "KPA500",
            &self.settings.com_port,
            &self.settings.transcript_dir,
        )
        .await;
        send_command(
            &mut port,
            CMD_CLEAR_FAULT,
            Duration::from_millis(750),
            &mut transcript,
        )
        .await?;
        Ok(())
    }

    fn ensure_can_send(&self, command: ElecraftCommand) -> Result<()> {
        if self.settings.dry_run && command.safety != CommandSafety::ReadOnly {
            warn!(
                event_id = "command_blocked_by_safety",
                device = "KPA500",
                command = command.label,
                wire = command.wire,
                safety = ?command.safety,
                "blocked serial command because dry-run mode is enabled"
            );
            anyhow::bail!(
                "KPA500 dry-run blocked {} ({:?})",
                command.label,
                command.safety
            );
        }
        Ok(())
    }

    async fn discover_capabilities(
        &self,
        port: &mut SerialStream,
        transcript: &mut SerialTranscript,
    ) {
        match send_command(port, CMD_VERSION, Duration::from_millis(750), transcript).await {
            Ok(response) => {
                info!(event_id = "serial_connected", device = "KPA500", response = %response, "KPA500 read-only capability discovery succeeded");
                let mut guard = self.state.write().await;
                guard.amp.firmware_version = Some(response.clone());
                if !guard
                    .amp
                    .capabilities
                    .iter()
                    .any(|capability| capability == "read_version")
                {
                    guard.amp.capabilities.push("read_version".to_string());
                }
            }
            Err(err) => {
                warn!(event_id = "serial_connected", device = "KPA500", error = %err, "KPA500 read-only capability discovery did not return a version");
            }
        }
    }

    async fn poll_status_on_port(
        &self,
        port: &mut SerialStream,
        transcript: &mut SerialTranscript,
    ) -> Result<()> {
        let response =
            send_command(port, CMD_STATUS, Duration::from_millis(750), transcript).await?;
        debug!(response = %response, "KPA500 status response");
        let mut guard = self.state.write().await;
        guard.amp.connected = true;
        guard.amp.connection_state = ConnectionState::Connected;
        guard.amp.last_serial_command_at = Some(SystemTime::now());
        guard.amp.last_serial_response_at = Some(SystemTime::now());
        guard.amp.last_successful_poll_at = Some(SystemTime::now());
        if !guard
            .amp
            .capabilities
            .iter()
            .any(|capability| capability == "poll_status")
        {
            guard.amp.capabilities.push("poll_status".to_string());
        }
        parse_unverified_status(&response, &mut guard.amp);
        sleep(self.settings.polling_interval).await;
        Ok(())
    }

    pub async fn set_operate_serial(port: &mut SerialStream) -> Result<String> {
        let mut transcript = SerialTranscript::disabled();
        send_command(
            port,
            CMD_OPERATE,
            Duration::from_millis(750),
            &mut transcript,
        )
        .await
    }

    pub async fn set_standby_serial(port: &mut SerialStream) -> Result<String> {
        let mut transcript = SerialTranscript::disabled();
        send_command(
            port,
            CMD_STANDBY,
            Duration::from_millis(750),
            &mut transcript,
        )
        .await
    }

    pub async fn clear_fault_serial(port: &mut SerialStream) -> Result<String> {
        let mut transcript = SerialTranscript::disabled();
        send_command(
            port,
            CMD_CLEAR_FAULT,
            Duration::from_millis(750),
            &mut transcript,
        )
        .await
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

async fn send_command(
    port: &mut SerialStream,
    command: ElecraftCommand,
    wait: Duration,
    transcript: &mut SerialTranscript,
) -> Result<String> {
    transcript.write_line("TX", command.wire).await;
    port.write_all(command.wire.as_bytes())
        .await
        .with_context(|| format!("failed to write serial command {}", command.label))?;
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

    let response = String::from_utf8_lossy(&buf).trim().to_string();
    transcript.write_line("RX", &response).await;
    Ok(response)
}

struct SerialTranscript {
    file: Option<File>,
}

impl SerialTranscript {
    fn disabled() -> Self {
        Self { file: None }
    }

    async fn open(device: &str, port: &str, dir: &Option<PathBuf>) -> Self {
        let Some(dir) = dir else {
            return Self::disabled();
        };
        if let Err(err) = create_dir_all(dir).await {
            warn!(device, error = %err, "serial transcript directory could not be created");
            return Self::disabled();
        }
        let ts = timestamp_millis();
        let safe_port = port.replace([':', '\\', '/', '.'], "_");
        let path = dir.join(format!(
            "{}-{}-{}.log",
            device.to_lowercase(),
            ts,
            safe_port
        ));
        match File::create(&path).await {
            Ok(file) => Self { file: Some(file) },
            Err(err) => {
                warn!(device, path = %path.display(), error = %err, "serial transcript file could not be opened");
                Self::disabled()
            }
        }
    }

    async fn write_line(&mut self, direction: &str, line: &str) {
        let Some(file) = &mut self.file else {
            return;
        };
        let row = format!("{} {direction} {line}\n", timestamp_millis());
        if let Err(err) = file.write_all(row.as_bytes()).await {
            warn!(error = %err, "serial transcript write failed");
            self.file = None;
            return;
        }
        if let Err(err) = file.flush().await {
            warn!(error = %err, "serial transcript flush failed");
            self.file = None;
        }
    }
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
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

    #[test]
    fn all_commands_have_safety_classification() {
        assert!(command_map()
            .iter()
            .all(|command| !command.label.is_empty() && !command.wire.is_empty()));
        assert_eq!(CMD_STATUS.safety, CommandSafety::ReadOnly);
        assert_eq!(CMD_OPERATE.safety, CommandSafety::RfRisk);
        assert!(command_map().iter().all(|command| !command.verified));
    }
}
