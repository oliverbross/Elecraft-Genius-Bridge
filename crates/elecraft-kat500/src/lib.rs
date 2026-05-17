use anyhow::{Context, Result};
use bridge_core::{ConnectionState, SharedState};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, timeout};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, info, warn};

// UNVERIFIED ELECRAFT COMMAND MAPPING.
// These strings are intentionally isolated until an official KAT500 command
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
const CMD_AUTOTUNE: ElecraftCommand = ElecraftCommand {
    label: "autotune",
    wire: "T;",
    safety: CommandSafety::RfRisk,
    verified: false,
};
const CMD_BYPASS_ON: ElecraftCommand = ElecraftCommand {
    label: "set_bypass_on",
    wire: "BP1;",
    safety: CommandSafety::StateChangeSafe,
    verified: false,
};
const CMD_BYPASS_OFF: ElecraftCommand = ElecraftCommand {
    label: "set_bypass_off",
    wire: "BP0;",
    safety: CommandSafety::StateChangeSafe,
    verified: false,
};
const CMD_ANTENNA_SELECT: ElecraftCommand = ElecraftCommand {
    label: "set_antenna",
    wire: "AN<n>;",
    safety: CommandSafety::StateChangeSafe,
    verified: false,
};
const CMD_MANUAL_TUNE: ElecraftCommand = ElecraftCommand {
    label: "manual_tune_relay_move",
    wire: "UNVERIFIED_MANUAL_TUNE;",
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
        CMD_AUTOTUNE,
        CMD_BYPASS_ON,
        CMD_BYPASS_OFF,
        CMD_ANTENNA_SELECT,
        CMD_MANUAL_TUNE,
    ]
}

#[derive(Debug, Clone)]
pub struct Kat500Settings {
    pub com_port: String,
    pub baud: u32,
    pub polling_interval: Duration,
    pub mock: bool,
    pub dry_run: bool,
    pub transcript_dir: Option<PathBuf>,
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
                guard.tuner.connection_state = ConnectionState::Connected;
                guard.tuner.last_serial_response_at = Some(SystemTime::now());
                guard.tuner.last_successful_poll_at = Some(SystemTime::now());
                if let Some(antenna) = guard.desired.tuner_selected_antenna {
                    guard.tuner.selected_antenna = Some(antenna.saturating_sub(1));
                }
                if let Some(bypass) = guard.desired.tuner_bypass {
                    guard.tuner.bypass = bypass;
                }
                if guard.desired.tuner_autotune_requested {
                    guard.tuner.tuning = true;
                    guard.desired.tuner_autotune_requested = false;
                }
                if let Some(manual) = guard.desired.tuner_manual_tune.take() {
                    let step = if manual.movement >= 0 { 1 } else { -1 };
                    let target = match manual.relay {
                        0 => Some(&mut guard.tuner.relay_c1),
                        1 => Some(&mut guard.tuner.relay_l),
                        2 => Some(&mut guard.tuner.relay_c2),
                        _ => None,
                    };
                    if let Some(target) = target {
                        *target = (*target + step).clamp(0, 255);
                    }
                }
                if guard.tuner.tuning {
                    guard.tuner.swr = 1.15;
                    guard.tuner.tuning = false;
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
                    let mut transcript = SerialTranscript::open(
                        "KAT500",
                        &self.settings.com_port,
                        &self.settings.transcript_dir,
                    )
                    .await;
                    {
                        let mut guard = self.state.write().await;
                        guard.tuner.connected = true;
                        guard.tuner.connection_state = ConnectionState::Connecting;
                    }
                    self.discover_capabilities(&mut port, &mut transcript).await;
                    loop {
                        if let Err(err) = self.poll_status_on_port(&mut port, &mut transcript).await
                        {
                            warn!(event_id = "serial_disconnected", device = "KAT500", error = %err, "KAT500 poll failed; reconnecting");
                            let mut guard = self.state.write().await;
                            guard.tuner.connected = false;
                            guard.tuner.connection_state = ConnectionState::Degraded;
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
                    guard.tuner.connection_state = ConnectionState::Disconnected;
                }
            }
            warn!(
                event_id = "reconnect_attempt",
                device = "KAT500",
                "KAT500 reconnect attempt scheduled"
            );
            sleep(Duration::from_secs(5)).await;
        }
    }

    pub async fn connect(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.tuner.connected = true;
            guard.tuner.connection_state = ConnectionState::Connected;
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
        guard.tuner.connection_state = ConnectionState::Disconnected;
    }

    pub async fn poll_status(&self) -> Result<()> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.tuner.connected = true;
            guard.tuner.connection_state = ConnectionState::Connected;
            guard.tuner.last_serial_response_at = Some(SystemTime::now());
            guard.tuner.last_successful_poll_at = Some(SystemTime::now());
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
        let mut transcript = SerialTranscript::open(
            "KAT500",
            &self.settings.com_port,
            &self.settings.transcript_dir,
        )
        .await;
        self.poll_status_on_port(&mut port, &mut transcript).await
    }

    pub async fn autotune(&self) -> Result<()> {
        self.ensure_can_send(CMD_AUTOTUNE)?;
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
        let mut transcript = SerialTranscript::open(
            "KAT500",
            &self.settings.com_port,
            &self.settings.transcript_dir,
        )
        .await;
        send_command(
            &mut port,
            CMD_AUTOTUNE,
            Duration::from_secs(5),
            &mut transcript,
        )
        .await?;
        Ok(())
    }

    pub async fn set_antenna(&self, antenna: u8) -> Result<()> {
        let command = antenna_command(antenna);
        self.ensure_can_send(command)?;
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
        let mut transcript = SerialTranscript::open(
            "KAT500",
            &self.settings.com_port,
            &self.settings.transcript_dir,
        )
        .await;
        send_dynamic_command(
            &mut port,
            command,
            Duration::from_millis(750),
            &mut transcript,
        )
        .await?;
        Ok(())
    }

    pub async fn set_bypass(&self, on: bool) -> Result<()> {
        let command = if on { CMD_BYPASS_ON } else { CMD_BYPASS_OFF };
        self.ensure_can_send(command)?;
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
        let mut transcript = SerialTranscript::open(
            "KAT500",
            &self.settings.com_port,
            &self.settings.transcript_dir,
        )
        .await;
        send_command(
            &mut port,
            command,
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
                device = "KAT500",
                command = command.label,
                wire = command.wire,
                safety = ?command.safety,
                "blocked serial command because dry-run mode is enabled"
            );
            anyhow::bail!(
                "KAT500 dry-run blocked {} ({:?})",
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
                info!(event_id = "serial_connected", device = "KAT500", response = %response, "KAT500 read-only capability discovery succeeded");
                let mut guard = self.state.write().await;
                guard.tuner.firmware_version = Some(response.clone());
                if !guard
                    .tuner
                    .capabilities
                    .iter()
                    .any(|capability| capability == "read_version")
                {
                    guard.tuner.capabilities.push("read_version".to_string());
                }
            }
            Err(err) => {
                warn!(event_id = "serial_connected", device = "KAT500", error = %err, "KAT500 read-only capability discovery did not return a version");
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
        debug!(response = %response, "KAT500 status response");
        let mut guard = self.state.write().await;
        guard.tuner.connected = true;
        guard.tuner.connection_state = ConnectionState::Connected;
        guard.tuner.last_serial_command_at = Some(SystemTime::now());
        guard.tuner.last_serial_response_at = Some(SystemTime::now());
        guard.tuner.last_successful_poll_at = Some(SystemTime::now());
        if !guard
            .tuner
            .capabilities
            .iter()
            .any(|capability| capability == "poll_status")
        {
            guard.tuner.capabilities.push("poll_status".to_string());
        }
        parse_unverified_status(&response, &mut guard.tuner);
        sleep(self.settings.polling_interval).await;
        Ok(())
    }

    pub async fn autotune_serial(port: &mut SerialStream) -> Result<String> {
        let mut transcript = SerialTranscript::disabled();
        send_command(port, CMD_AUTOTUNE, Duration::from_secs(5), &mut transcript).await
    }

    pub async fn set_bypass_serial(port: &mut SerialStream, on: bool) -> Result<String> {
        let mut transcript = SerialTranscript::disabled();
        let command = if on { CMD_BYPASS_ON } else { CMD_BYPASS_OFF };
        send_command(port, command, Duration::from_millis(750), &mut transcript).await
    }

    pub async fn set_antenna_serial(port: &mut SerialStream, antenna: u8) -> Result<String> {
        let mut transcript = SerialTranscript::disabled();
        send_dynamic_command(
            port,
            antenna_command(antenna),
            Duration::from_millis(750),
            &mut transcript,
        )
        .await
    }
}

fn parse_unverified_status(response: &str, tuner: &mut bridge_core::TunerState) {
    // Placeholder parser. Keep this conservative until real KAT500 responses
    // are documented or captured.
    if response.contains("BYP") {
        tuner.bypass = true;
    }
}

async fn send_command(
    port: &mut SerialStream,
    command: ElecraftCommand,
    wait: Duration,
    transcript: &mut SerialTranscript,
) -> Result<String> {
    send_wire_command(port, command.label, command.wire, wait, transcript).await
}

async fn send_dynamic_command(
    port: &mut SerialStream,
    command: ElecraftCommand,
    wait: Duration,
    transcript: &mut SerialTranscript,
) -> Result<String> {
    send_wire_command(port, command.label, command.wire, wait, transcript).await
}

async fn send_wire_command(
    port: &mut SerialStream,
    label: &str,
    wire: &str,
    wait: Duration,
    transcript: &mut SerialTranscript,
) -> Result<String> {
    transcript.write_line("TX", wire).await;
    port.write_all(wire.as_bytes())
        .await
        .with_context(|| format!("failed to write serial command {label}"))?;
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

fn antenna_command(antenna: u8) -> ElecraftCommand {
    let wire = match antenna {
        1 => "AN1;",
        2 => "AN2;",
        3 => "AN3;",
        _ => "AN?;",
    };
    ElecraftCommand {
        label: "set_antenna",
        wire,
        safety: CommandSafety::StateChangeSafe,
        verified: false,
    }
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
    fn unverified_bypass_parser_sets_bypass() {
        let mut tuner = bridge_core::TunerState::default();
        parse_unverified_status("BYP;", &mut tuner);
        assert!(tuner.bypass);
    }

    #[test]
    fn all_commands_have_safety_classification() {
        assert!(command_map()
            .iter()
            .all(|command| !command.label.is_empty() && !command.wire.is_empty()));
        assert_eq!(CMD_STATUS.safety, CommandSafety::ReadOnly);
        assert_eq!(CMD_AUTOTUNE.safety, CommandSafety::RfRisk);
        assert_eq!(antenna_command(1).safety, CommandSafety::StateChangeSafe);
        assert!(command_map().iter().all(|command| !command.verified));
    }
}
