use anyhow::{Context, Result};
use bridge_core::{ConnectionState, SharedState};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, timeout};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, info, warn};

const DISCOVERY_BAUDS: [u32; 5] = [38400, 19200, 9600, 4800, 38400];

// KAT500 Serial Command Reference mappings.
// `ST` is intentionally absent: the KAT500 uses ST for SWR thresholds, not
// generic device status.
const CMD_NULL: ElecraftCommand = ElecraftCommand {
    label: "null_wake",
    wire: ";",
    safety: CommandSafety::ReadOnly,
    verified: true,
};
const CMD_FIRMWARE: ElecraftCommand = ElecraftCommand {
    label: "read_firmware",
    wire: "RV;",
    safety: CommandSafety::ReadOnly,
    verified: true,
};
const CMD_SERIAL_NUMBER: ElecraftCommand = ElecraftCommand {
    label: "read_serial_number",
    wire: "SN;",
    safety: CommandSafety::ReadOnly,
    verified: true,
};
const CMD_ANTENNA_STATUS: ElecraftCommand = ElecraftCommand {
    label: "read_antenna",
    wire: "AN;",
    safety: CommandSafety::ReadOnly,
    verified: true,
};
const CMD_BYPASS_STATUS: ElecraftCommand = ElecraftCommand {
    label: "read_bypass",
    wire: "BYP;",
    safety: CommandSafety::ReadOnly,
    verified: true,
};
const CMD_MODE: ElecraftCommand = ElecraftCommand {
    label: "read_mode",
    wire: "MD;",
    safety: CommandSafety::ReadOnly,
    verified: true,
};
const CMD_TUNE_POLL: ElecraftCommand = ElecraftCommand {
    label: "read_tune_poll",
    wire: "TP;",
    safety: CommandSafety::ReadOnly,
    verified: true,
};
const CMD_FAULT: ElecraftCommand = ElecraftCommand {
    label: "read_fault",
    wire: "FLT;",
    safety: CommandSafety::ReadOnly,
    verified: true,
};
const CMD_VSWR: ElecraftCommand = ElecraftCommand {
    label: "read_vswr",
    wire: "VSWR;",
    safety: CommandSafety::ReadOnly,
    verified: true,
};
const CMD_FORWARD_ADC: ElecraftCommand = ElecraftCommand {
    label: "read_forward_adc",
    wire: "VFWD;",
    safety: CommandSafety::ReadOnly,
    verified: true,
};
const CMD_AUTOTUNE: ElecraftCommand = ElecraftCommand {
    label: "autotune",
    wire: "T;",
    safety: CommandSafety::RfRisk,
    verified: true,
};
const CMD_BYPASS_ON: ElecraftCommand = ElecraftCommand {
    label: "set_bypass_on",
    wire: "BYPB;",
    safety: CommandSafety::StateChangeSafe,
    verified: true,
};
const CMD_BYPASS_OFF: ElecraftCommand = ElecraftCommand {
    label: "set_bypass_off",
    wire: "BYPN;",
    safety: CommandSafety::StateChangeSafe,
    verified: true,
};
const CMD_ANTENNA_SELECT: ElecraftCommand = ElecraftCommand {
    label: "set_antenna",
    wire: "AN<n>;",
    safety: CommandSafety::StateChangeSafe,
    verified: true,
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
        CMD_NULL,
        CMD_FIRMWARE,
        CMD_SERIAL_NUMBER,
        CMD_ANTENNA_STATUS,
        CMD_BYPASS_STATUS,
        CMD_MODE,
        CMD_TUNE_POLL,
        CMD_FAULT,
        CMD_VSWR,
        CMD_FORWARD_ADC,
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
            let mut skip_until_next_poll = false;
            {
                let mut guard = self.state.write().await;
                guard.tuner.connected = true;
                guard.tuner.connection_state = ConnectionState::Connected;
                guard.tuner.last_serial_response_at = Some(SystemTime::now());
                guard.tuner.last_successful_poll_at = Some(SystemTime::now());
                if guard.tuner.fault.as_deref() == Some("mock_tgxl_fault") {
                    guard.tuner.connected = false;
                    guard.tuner.connection_state = ConnectionState::Degraded;
                    skip_until_next_poll = true;
                } else if let Some(antenna) = guard.desired.tuner_selected_antenna {
                    guard.tuner.selected_antenna = Some(antenna.saturating_sub(1));
                }
                if !skip_until_next_poll {
                    if let Some(bypass) = guard.desired.tuner_bypass {
                        guard.tuner.bypass = bypass;
                    }
                }
                if !skip_until_next_poll && guard.desired.tuner_autotune_requested {
                    guard.tuner.tuning = true;
                    guard.desired.tuner_autotune_requested = false;
                }
                if !skip_until_next_poll {
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
                }
                if !skip_until_next_poll && guard.tuner.tuning {
                    guard.tuner.swr = 1.15;
                    guard.tuner.tuning = false;
                }
            }
            sleep(self.settings.polling_interval).await;
        }
    }

    async fn run_serial_loop(self) {
        loop {
            let mut transcript = SerialTranscript::open(
                "KAT500",
                &self.settings.com_port,
                &self.settings.transcript_dir,
            )
            .await;
            match self.open_with_discovery(&mut transcript).await {
                Ok((mut port, baud)) => {
                    info!(port = %self.settings.com_port, baud, "KAT500 serial connected");
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
        let mut transcript = SerialTranscript::disabled();
        let _ = self.open_with_discovery(&mut transcript).await?;
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
        let mut transcript = SerialTranscript::open(
            "KAT500",
            &self.settings.com_port,
            &self.settings.transcript_dir,
        )
        .await;
        let (mut port, _) = self.open_with_discovery(&mut transcript).await?;
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

    async fn open_with_discovery(
        &self,
        transcript: &mut SerialTranscript,
    ) -> Result<(SerialStream, u32)> {
        let mut bauds = vec![self.settings.baud];
        for baud in DISCOVERY_BAUDS {
            if !bauds.contains(&baud) {
                bauds.push(baud);
            }
        }
        let mut last_error = None;
        for baud in bauds {
            info!(
                event_id = "kat500_baud_discovery_attempt",
                port = %self.settings.com_port,
                baud,
                "trying KAT500 baud/wake discovery"
            );
            match tokio_serial::new(self.settings.com_port.clone(), baud).open_native_async() {
                Ok(mut port) => {
                    match wake_kat500(&mut port, transcript, 4, Duration::from_millis(1000)).await {
                        Ok(response) => {
                            info!(
                                event_id = "kat500_baud_discovery_success",
                                port = %self.settings.com_port,
                                baud,
                                response = %response,
                                "KAT500 wake/null probe returned bytes"
                            );
                            return Ok((port, baud));
                        }
                        Err(err) => {
                            warn!(
                                event_id = "kat500_baud_discovery_no_response",
                                port = %self.settings.com_port,
                                baud,
                                error = %err,
                                "KAT500 wake/null probe did not return bytes"
                            );
                            last_error = Some(err);
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        event_id = "kat500_baud_discovery_open_failed",
                        port = %self.settings.com_port,
                        baud,
                        error = %err,
                        "KAT500 serial open failed during baud discovery"
                    );
                    last_error = Some(anyhow::Error::new(err));
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("KAT500 baud discovery did not run")))
            .with_context(|| {
                format!(
                    "failed KAT500 baud/wake discovery on {}",
                    self.settings.com_port
                )
            })
    }

    async fn discover_capabilities(
        &self,
        port: &mut SerialStream,
        transcript: &mut SerialTranscript,
    ) {
        match send_command(port, CMD_FIRMWARE, Duration::from_millis(1000), transcript).await {
            Ok(response) => {
                info!(event_id = "serial_connected", device = "KAT500", response = %response, "KAT500 read-only capability discovery succeeded");
                let mut guard = self.state.write().await;
                parse_kat500_response(&response, &mut guard.tuner);
                push_capability(&mut guard.tuner.capabilities, CMD_FIRMWARE.label);
            }
            Err(err) => {
                warn!(event_id = "serial_connected", device = "KAT500", error = %err, "KAT500 read-only capability discovery did not return a version");
            }
        }
        match send_command(
            port,
            CMD_SERIAL_NUMBER,
            Duration::from_millis(1000),
            transcript,
        )
        .await
        {
            Ok(response) => {
                let mut guard = self.state.write().await;
                parse_kat500_response(&response, &mut guard.tuner);
                push_capability(&mut guard.tuner.capabilities, CMD_SERIAL_NUMBER.label);
            }
            Err(err) => {
                warn!(event_id = "serial_connected", device = "KAT500", error = %err, "KAT500 serial number query did not return a response");
            }
        }
    }

    async fn poll_status_on_port(
        &self,
        port: &mut SerialStream,
        transcript: &mut SerialTranscript,
    ) -> Result<()> {
        let commands = [
            CMD_ANTENNA_STATUS,
            CMD_BYPASS_STATUS,
            CMD_MODE,
            CMD_TUNE_POLL,
            CMD_FAULT,
            CMD_VSWR,
            CMD_FORWARD_ADC,
        ];
        let mut responses = Vec::with_capacity(commands.len());
        for command in commands {
            let response = send_command(port, command, Duration::from_millis(1000), transcript)
                .await
                .with_context(|| format!("KAT500 {} timed out or failed", command.label))?;
            debug!(command = command.label, response = %response, "KAT500 status response");
            responses.push((command, response));
        }
        let mut guard = self.state.write().await;
        guard.tuner.connected = true;
        guard.tuner.connection_state = ConnectionState::Connected;
        guard.tuner.last_serial_command_at = Some(SystemTime::now());
        guard.tuner.last_serial_response_at = Some(SystemTime::now());
        guard.tuner.last_successful_poll_at = Some(SystemTime::now());
        for (command, response) in responses {
            push_capability(&mut guard.tuner.capabilities, command.label);
            parse_kat500_response(&response, &mut guard.tuner);
        }
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

fn push_capability(capabilities: &mut Vec<String>, capability: &str) {
    if !capabilities.iter().any(|existing| existing == capability) {
        capabilities.push(capability.to_string());
    }
}

fn parse_kat500_response(response: &str, tuner: &mut bridge_core::TunerState) {
    let response = response.trim();
    if let Some(raw) = response
        .strip_prefix("RV")
        .and_then(|value| value.strip_suffix(';'))
    {
        if !raw.is_empty() {
            tuner.firmware_version = Some(raw.to_string());
        }
        return;
    }
    if let Some(raw) = response
        .strip_prefix("SN")
        .and_then(|value| value.strip_suffix(';'))
    {
        let serial = raw.trim();
        if !serial.is_empty() {
            tuner.serial_number = Some(serial.to_string());
        }
        return;
    }
    if let Some(raw) = response
        .strip_prefix("AN")
        .and_then(|value| value.strip_suffix(';'))
    {
        match raw.parse::<u8>() {
            Ok(antenna @ 0..=3) => tuner.selected_antenna = Some(antenna),
            _ => warn_invalid("antenna", raw, response),
        }
        return;
    }
    if response == "BYPB;" {
        tuner.bypass = true;
        return;
    }
    if response == "BYPN;" {
        tuner.bypass = false;
        return;
    }
    if let Some(raw) = response
        .strip_prefix("MD")
        .and_then(|value| value.strip_suffix(';'))
    {
        match raw {
            "B" => {
                tuner.bypass = true;
                tuner.operate = false;
            }
            "M" | "A" => {
                tuner.operate = true;
            }
            _ => warn_invalid("mode", raw, response),
        }
        return;
    }
    if let Some(raw) = response
        .strip_prefix("TP")
        .and_then(|value| value.strip_suffix(';'))
    {
        match raw {
            "0" => tuner.tuning = false,
            "1" => tuner.tuning = true,
            _ => warn_invalid("tuning", raw, response),
        }
        return;
    }
    if let Some(raw) = response
        .strip_prefix("FLT")
        .and_then(|value| value.strip_suffix(';'))
    {
        match raw.parse::<u16>() {
            Ok(0) => tuner.fault = None,
            Ok(code) => tuner.fault = Some(format!("KAT500 fault {code}")),
            Err(_) => warn_invalid("fault", raw, response),
        }
        return;
    }
    if let Some(raw) = response
        .strip_prefix("VSWR ")
        .and_then(|value| value.strip_suffix(';'))
    {
        parse_vswr(raw, response, tuner);
        return;
    }
    if let Some(raw) = response
        .strip_prefix("VFWD ")
        .and_then(|value| value.strip_suffix(';'))
    {
        match raw.parse::<u16>() {
            Ok(value) if value <= 4095 => {
                tuner.forward_power_watts = 0.0;
            }
            _ => warn_invalid("forward_adc", raw, response),
        }
    }
}

fn parse_vswr(raw: &str, response: &str, tuner: &mut bridge_core::TunerState) {
    match raw.parse::<f32>() {
        Ok(0.0) => tuner.swr = 1.0,
        Ok(value) if (1.0..=99.99).contains(&value) => tuner.swr = value,
        Ok(_) | Err(_) => warn_invalid("swr", raw, response),
    }
}

fn warn_invalid(field: &str, value: &str, response: &str) {
    warn!(
        event_id = "invalid_serial_value",
        device = "KAT500",
        field,
        value = value,
        response,
        "rejected invalid KAT500 serial value"
    );
}

async fn send_command(
    port: &mut SerialStream,
    command: ElecraftCommand,
    wait: Duration,
    transcript: &mut SerialTranscript,
) -> Result<String> {
    send_wire_command(port, command.label, command.wire, wait, transcript).await
}

async fn wake_kat500(
    port: &mut SerialStream,
    transcript: &mut SerialTranscript,
    attempts: usize,
    wait: Duration,
) -> Result<String> {
    let mut last_error = None;
    for _ in 0..attempts {
        match send_command(port, CMD_NULL, wait, transcript).await {
            Ok(response) => return Ok(response),
            Err(err) => {
                last_error = Some(err);
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("KAT500 wake probe did not run")))
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
        verified: antenna <= 3,
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
    fn parser_handles_documented_read_only_responses() {
        let mut tuner = bridge_core::TunerState::default();
        parse_kat500_response("RV02.16;", &mut tuner);
        parse_kat500_response("SN 12345;", &mut tuner);
        parse_kat500_response("AN3;", &mut tuner);
        parse_kat500_response("BYPB;", &mut tuner);
        parse_kat500_response("MDA;", &mut tuner);
        parse_kat500_response("TP1;", &mut tuner);
        parse_kat500_response("FLT4;", &mut tuner);
        parse_kat500_response("VSWR 1.25;", &mut tuner);
        parse_kat500_response("VFWD 1234;", &mut tuner);
        assert_eq!(tuner.firmware_version.as_deref(), Some("02.16"));
        assert_eq!(tuner.serial_number.as_deref(), Some("12345"));
        assert_eq!(tuner.selected_antenna, Some(3));
        assert!(tuner.bypass);
        assert!(tuner.operate);
        assert!(tuner.tuning);
        assert_eq!(tuner.fault.as_deref(), Some("KAT500 fault 4"));
        assert_eq!(tuner.swr, 1.25);
    }

    #[test]
    fn parser_rejects_impossible_swr_values() {
        let mut tuner = bridge_core::TunerState::default();
        parse_kat500_response("VSWR -1.0;", &mut tuner);
        assert_eq!(tuner.swr, 1.0);
        parse_kat500_response("VSWR 1000.0;", &mut tuner);
        assert_eq!(tuner.swr, 1.0);
        parse_kat500_response("VSWR 1.3;", &mut tuner);
        assert_eq!(tuner.swr, 1.3);
    }

    #[test]
    fn parser_treats_vswr_zero_as_no_rf_baseline() {
        let mut tuner = bridge_core::TunerState::default();
        parse_kat500_response("VSWR 0.0;", &mut tuner);
        assert_eq!(tuner.swr, 1.0);
    }

    #[test]
    fn all_commands_have_safety_classification() {
        assert!(command_map()
            .iter()
            .all(|command| !command.label.is_empty() && !command.wire.is_empty()));
        assert_eq!(CMD_FIRMWARE.safety, CommandSafety::ReadOnly);
        assert_eq!(CMD_AUTOTUNE.safety, CommandSafety::RfRisk);
        assert_eq!(antenna_command(1).safety, CommandSafety::StateChangeSafe);
        assert!(command_map()
            .iter()
            .filter(|command| command.safety != CommandSafety::DestructiveOrUnknown)
            .all(|command| command.verified));
    }

    #[test]
    fn kat500_does_not_use_st_as_generic_status() {
        for command in command_map() {
            assert_ne!(command.wire, "ST;");
            assert_ne!(command.wire, "BP1;");
            assert_ne!(command.wire, "BP0;");
        }
    }
}
