use anyhow::{Context, Result};
use bridge_core::{AmpOperatingState, ConnectionState, SharedState};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, timeout};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, info, warn};

// KPA500 Programmer Reference caret-prefixed command set.
// Keep all mappings isolated here so hardware transcripts can correct them
// without touching PGXL/TGXL protocol code.
const CMD_FIRMWARE: ElecraftCommand = ElecraftCommand {
    label: "read_firmware",
    wire: "^RVM;",
    safety: CommandSafety::ReadOnly,
    verified: true,
    expects_response: true,
    requires_post_verify: false,
};
const CMD_SERIAL_NUMBER: ElecraftCommand = ElecraftCommand {
    label: "read_serial_number",
    wire: "^SN;",
    safety: CommandSafety::ReadOnly,
    verified: true,
    expects_response: true,
    requires_post_verify: false,
};
const CMD_OPERATE_STATUS: ElecraftCommand = ElecraftCommand {
    label: "read_operate_status",
    wire: "^OS;",
    safety: CommandSafety::ReadOnly,
    verified: true,
    expects_response: true,
    requires_post_verify: false,
};
const CMD_POWER_SWR: ElecraftCommand = ElecraftCommand {
    label: "read_power_swr",
    wire: "^WS;",
    safety: CommandSafety::ReadOnly,
    verified: true,
    expects_response: true,
    requires_post_verify: false,
};
const CMD_TEMPERATURE: ElecraftCommand = ElecraftCommand {
    label: "read_temperature",
    wire: "^TM;",
    safety: CommandSafety::ReadOnly,
    verified: true,
    expects_response: true,
    requires_post_verify: false,
};
const CMD_VOLTS_CURRENT: ElecraftCommand = ElecraftCommand {
    label: "read_volts_current",
    wire: "^VI;",
    safety: CommandSafety::ReadOnly,
    verified: true,
    expects_response: true,
    requires_post_verify: false,
};
const CMD_FAULT: ElecraftCommand = ElecraftCommand {
    label: "read_fault",
    wire: "^FL;",
    safety: CommandSafety::ReadOnly,
    verified: true,
    expects_response: true,
    requires_post_verify: false,
};
const CMD_OPERATE: ElecraftCommand = ElecraftCommand {
    label: "set_operate",
    wire: "^OS1;",
    safety: CommandSafety::RfRisk,
    verified: true,
    expects_response: false,
    requires_post_verify: true,
};
const CMD_STANDBY: ElecraftCommand = ElecraftCommand {
    label: "set_standby",
    wire: "^OS0;",
    safety: CommandSafety::StateChangeSafe,
    verified: true,
    expects_response: false,
    requires_post_verify: true,
};
const CMD_CLEAR_FAULT: ElecraftCommand = ElecraftCommand {
    label: "clear_fault",
    wire: "^FLC;",
    safety: CommandSafety::DestructiveOrUnknown,
    verified: true,
    expects_response: false,
    requires_post_verify: true,
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
    pub expects_response: bool,
    pub requires_post_verify: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    pub command: ElecraftCommand,
    pub response: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandResultState {
    Acknowledged,
    Verified,
    SentNoAck,
    VerifyFailed,
    Timeout,
    ParseFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlCommandResult {
    pub command: ElecraftCommand,
    pub send_result: CommandResultState,
    pub verify_result: Option<CommandResultState>,
    pub verification_response: Option<String>,
    pub final_state: Option<AmpOperatingState>,
}

pub fn command_map() -> &'static [ElecraftCommand] {
    &[
        CMD_FIRMWARE,
        CMD_SERIAL_NUMBER,
        CMD_OPERATE_STATUS,
        CMD_POWER_SWR,
        CMD_TEMPERATURE,
        CMD_VOLTS_CURRENT,
        CMD_FAULT,
        CMD_OPERATE,
        CMD_STANDBY,
        CMD_CLEAR_FAULT,
    ]
}

pub fn read_only_discovery_commands() -> &'static [ElecraftCommand] {
    &[CMD_FIRMWARE, CMD_SERIAL_NUMBER]
}

pub fn read_only_poll_commands() -> &'static [ElecraftCommand] {
    &[
        CMD_OPERATE_STATUS,
        CMD_POWER_SWR,
        CMD_TEMPERATURE,
        CMD_VOLTS_CURRENT,
        CMD_FAULT,
    ]
}

#[derive(Debug, Clone)]
pub struct Kpa500Settings {
    pub com_port: String,
    pub baud: u32,
    pub polling_interval: Duration,
    pub mock: bool,
    pub dry_run: bool,
    pub control_verify_delay: Duration,
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
            let mut skip_until_next_poll = false;
            {
                let mut guard = self.state.write().await;
                guard.amp.connected = true;
                guard.amp.connection_state = ConnectionState::Connected;
                guard.amp.last_serial_response_at = Some(SystemTime::now());
                guard.amp.last_successful_poll_at = Some(SystemTime::now());
                if guard.amp.fault.as_deref() == Some("mock_pgxl_fault") {
                    guard.amp.connected = false;
                    guard.amp.connection_state = ConnectionState::Degraded;
                    guard.amp.state = AmpOperatingState::Fault;
                    skip_until_next_poll = true;
                } else if let Some(operate) = guard.desired.amp_operate {
                    guard.amp.operate = operate;
                }
                if !skip_until_next_poll && guard.amp.operate {
                    guard.amp.state = AmpOperatingState::Idle;
                    guard.amp.pa_voltage_volts = 68.0;
                    guard.amp.temperature_c = (guard.amp.temperature_c + 0.1).min(45.0);
                    guard.amp.meffa = "OK".to_string();
                } else if !skip_until_next_poll {
                    guard.amp.state = AmpOperatingState::Standby;
                    guard.amp.forward_power_watts = 0.0;
                    guard.amp.pa_current_amps = 0.0;
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
        let outcomes = self.poll_status_outcomes().await?;
        if outcomes.iter().any(|outcome| outcome.error.is_some()) {
            let failed = outcomes
                .iter()
                .filter(|outcome| outcome.error.is_some())
                .map(|outcome| outcome.command.label)
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("KPA500 read-only poll had failed commands: {failed}");
        }
        Ok(())
    }

    pub async fn poll_status_outcomes(&self) -> Result<Vec<CommandOutcome>> {
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.connected = true;
            guard.amp.connection_state = ConnectionState::Connected;
            guard.amp.last_serial_response_at = Some(SystemTime::now());
            guard.amp.last_successful_poll_at = Some(SystemTime::now());
            return Ok(read_only_poll_commands()
                .iter()
                .map(|command| CommandOutcome {
                    command: *command,
                    response: Some("MOCK;".to_string()),
                    error: None,
                })
                .collect());
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

    pub async fn set_standby(&self) -> Result<ControlCommandResult> {
        self.ensure_can_send(CMD_STANDBY)?;
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.operate = false;
            guard.amp.state = AmpOperatingState::Standby;
            guard.amp.forward_power_watts = 0.0;
            return Ok(ControlCommandResult {
                command: CMD_STANDBY,
                send_result: CommandResultState::SentNoAck,
                verify_result: Some(CommandResultState::Verified),
                verification_response: Some("^OS0;".to_string()),
                final_state: Some(AmpOperatingState::Standby),
            });
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
        send_no_ack_command(&mut port, CMD_STANDBY, &mut transcript).await?;
        sleep(self.settings.control_verify_delay).await;
        match send_command(
            &mut port,
            CMD_OPERATE_STATUS,
            Duration::from_millis(1000),
            &mut transcript,
        )
        .await
        {
            Ok(response) => {
                let mut amp = bridge_core::AmpState::default();
                parse_kpa500_response(&response, &mut amp);
                let verified = !amp.operate && matches!(amp.state, AmpOperatingState::Standby);
                if verified {
                    let mut guard = self.state.write().await;
                    parse_kpa500_response(&response, &mut guard.amp);
                    Ok(ControlCommandResult {
                        command: CMD_STANDBY,
                        send_result: CommandResultState::SentNoAck,
                        verify_result: Some(CommandResultState::Verified),
                        verification_response: Some(response),
                        final_state: Some(AmpOperatingState::Standby),
                    })
                } else {
                    Ok(ControlCommandResult {
                        command: CMD_STANDBY,
                        send_result: CommandResultState::SentNoAck,
                        verify_result: Some(CommandResultState::VerifyFailed),
                        verification_response: Some(response),
                        final_state: Some(amp.state),
                    })
                }
            }
            Err(err) => Ok(ControlCommandResult {
                command: CMD_STANDBY,
                send_result: CommandResultState::SentNoAck,
                verify_result: Some(CommandResultState::Timeout),
                verification_response: Some(err.to_string()),
                final_state: None,
            }),
        }
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
        match send_command(port, CMD_FIRMWARE, Duration::from_millis(1000), transcript).await {
            Ok(response) => {
                info!(event_id = "serial_connected", device = "KPA500", response = %response, "KPA500 read-only capability discovery succeeded");
                let mut guard = self.state.write().await;
                parse_kpa500_response(&response, &mut guard.amp);
                push_capability(&mut guard.amp.capabilities, CMD_FIRMWARE.label);
            }
            Err(err) => {
                warn!(event_id = "serial_connected", device = "KPA500", error = %err, "KPA500 read-only capability discovery did not return a version");
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
                parse_kpa500_response(&response, &mut guard.amp);
                push_capability(&mut guard.amp.capabilities, CMD_SERIAL_NUMBER.label);
            }
            Err(err) => {
                warn!(event_id = "serial_connected", device = "KPA500", error = %err, "KPA500 read-only capability discovery did not return a serial number");
            }
        }
    }

    async fn poll_status_on_port(
        &self,
        port: &mut SerialStream,
        transcript: &mut SerialTranscript,
    ) -> Result<Vec<CommandOutcome>> {
        let mut outcomes = Vec::with_capacity(read_only_poll_commands().len());
        for command in read_only_poll_commands() {
            match send_command(port, *command, Duration::from_millis(1000), transcript).await {
                Ok(response) => {
                    debug!(command = command.label, response = %response, "KPA500 status response");
                    outcomes.push(CommandOutcome {
                        command: *command,
                        response: Some(response),
                        error: None,
                    });
                }
                Err(err) => {
                    warn!(device = "KPA500", command = command.label, error = %err, "KPA500 read-only command failed; continuing");
                    outcomes.push(CommandOutcome {
                        command: *command,
                        response: None,
                        error: Some(err.to_string()),
                    });
                }
            }
        }
        let mut guard = self.state.write().await;
        guard.amp.connected = true;
        guard.amp.connection_state = ConnectionState::Connected;
        guard.amp.last_serial_command_at = Some(SystemTime::now());
        if outcomes.iter().any(|outcome| outcome.response.is_some()) {
            guard.amp.last_serial_response_at = Some(SystemTime::now());
            guard.amp.last_successful_poll_at = Some(SystemTime::now());
        } else {
            guard.amp.connection_state = ConnectionState::Degraded;
        }
        for outcome in &outcomes {
            if let Some(response) = &outcome.response {
                push_capability(&mut guard.amp.capabilities, outcome.command.label);
                parse_kpa500_response(response, &mut guard.amp);
            }
        }
        sleep(self.settings.polling_interval).await;
        Ok(outcomes)
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

fn push_capability(capabilities: &mut Vec<String>, capability: &str) {
    if !capabilities.iter().any(|existing| existing == capability) {
        capabilities.push(capability.to_string());
    }
}

fn parse_kpa500_response(response: &str, amp: &mut bridge_core::AmpState) {
    let response = response.trim();
    if let Some(raw) = response
        .strip_prefix("^RVM")
        .and_then(|value| value.strip_suffix(';'))
    {
        if !raw.is_empty() {
            amp.firmware_version = Some(raw.to_string());
        }
        return;
    }
    if let Some(raw) = response
        .strip_prefix("^SN")
        .and_then(|value| value.strip_suffix(';'))
    {
        if !raw.is_empty() {
            amp.serial_number = Some(raw.to_string());
        }
        return;
    }
    if let Some(raw) = response
        .strip_prefix("^OS")
        .and_then(|value| value.strip_suffix(';'))
    {
        match raw {
            "0" => {
                amp.operate = false;
                amp.state = AmpOperatingState::Standby;
            }
            "1" => {
                amp.operate = true;
                amp.state = AmpOperatingState::Operate;
            }
            _ => warn!(
                event_id = "invalid_serial_value",
                device = "KPA500",
                field = "operate_status",
                value = raw,
                response,
                "rejected unknown KPA500 operate status"
            ),
        }
        return;
    }
    if let Some(raw) = response
        .strip_prefix("^WS")
        .and_then(|value| value.strip_suffix(';'))
    {
        parse_power_swr(raw, response, amp);
        return;
    }
    if let Some(raw) = response
        .strip_prefix("^TM")
        .and_then(|value| value.strip_suffix(';'))
    {
        parse_temperature(raw, response, amp);
        return;
    }
    if let Some(raw) = response
        .strip_prefix("^VI")
        .and_then(|value| value.strip_suffix(';'))
    {
        parse_volts_current(raw, response, amp);
        return;
    }
    if let Some(raw) = response
        .strip_prefix("^FL")
        .and_then(|value| value.strip_suffix(';'))
    {
        parse_fault(raw, response, amp);
    }
}

fn parse_power_swr(raw: &str, response: &str, amp: &mut bridge_core::AmpState) {
    let mut parts = raw.split_whitespace();
    let Some(power_raw) = parts.next() else {
        warn_invalid("power_swr", raw, response);
        return;
    };
    let Some(swr_raw) = parts.next() else {
        warn_invalid("power_swr", raw, response);
        return;
    };
    match power_raw.parse::<u16>() {
        Ok(power) if power <= 1000 => amp.forward_power_watts = f32::from(power),
        _ => warn_invalid("forward_power_watts", power_raw, response),
    }
    match parse_kpa_swr(swr_raw) {
        Some(swr) => amp.swr = swr,
        None => warn_invalid("swr", swr_raw, response),
    }
}

fn parse_kpa_swr(raw: &str) -> Option<f32> {
    let value = raw.parse::<u16>().ok()?;
    if value == 0 {
        return Some(1.0);
    }
    if (100..=990).contains(&value) {
        return Some(f32::from(value) / 100.0);
    }
    None
}

fn parse_temperature(raw: &str, response: &str, amp: &mut bridge_core::AmpState) {
    match raw.parse::<i16>() {
        Ok(value) if (-40..=150).contains(&value) => amp.temperature_c = f32::from(value),
        _ => warn_invalid("temperature_c", raw, response),
    }
}

fn parse_volts_current(raw: &str, response: &str, amp: &mut bridge_core::AmpState) {
    let mut parts = raw.split_whitespace();
    let Some(volts_raw) = parts.next() else {
        warn_invalid("volts_current", raw, response);
        return;
    };
    let Some(current_raw) = parts.next() else {
        warn_invalid("volts_current", raw, response);
        return;
    };
    match volts_raw.parse::<u16>() {
        Ok(value) if value <= 999 => amp.pa_voltage_volts = f32::from(value) / 10.0,
        _ => warn_invalid("pa_voltage_volts", volts_raw, response),
    }
    match current_raw.parse::<u16>() {
        Ok(value) if value <= 999 => amp.pa_current_amps = f32::from(value) / 10.0,
        _ => warn_invalid("pa_current_amps", current_raw, response),
    }
}

fn parse_fault(raw: &str, response: &str, amp: &mut bridge_core::AmpState) {
    match raw.parse::<u16>() {
        Ok(0) => {
            amp.fault = None;
            if amp.operate {
                amp.state = AmpOperatingState::Operate;
            } else {
                amp.state = AmpOperatingState::Standby;
            }
        }
        Ok(code) => {
            amp.state = AmpOperatingState::Fault;
            amp.fault = Some(format!("KPA500 fault {code}"));
        }
        Err(_) => warn_invalid("fault", raw, response),
    }
}

fn warn_invalid(field: &str, value: &str, response: &str) {
    warn!(
        event_id = "invalid_serial_value",
        device = "KPA500",
        field,
        value = value,
        response,
        "rejected invalid KPA500 serial value"
    );
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

async fn send_no_ack_command(
    port: &mut SerialStream,
    command: ElecraftCommand,
    transcript: &mut SerialTranscript,
) -> Result<()> {
    transcript.write_line("TX", command.wire).await;
    port.write_all(command.wire.as_bytes())
        .await
        .with_context(|| format!("failed to write serial command {}", command.label))?;
    port.flush().await.context("failed to flush serial port")?;
    Ok(())
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
        let mut amp = bridge_core::AmpState::default();
        parse_kpa500_response("^RVM01.23;", &mut amp);
        parse_kpa500_response("^SN12345;", &mut amp);
        parse_kpa500_response("^OS1;", &mut amp);
        parse_kpa500_response("^WS500 150;", &mut amp);
        parse_kpa500_response("^TM034;", &mut amp);
        parse_kpa500_response("^VI689 125;", &mut amp);
        parse_kpa500_response("^FL07;", &mut amp);
        assert_eq!(amp.firmware_version.as_deref(), Some("01.23"));
        assert_eq!(amp.serial_number.as_deref(), Some("12345"));
        assert!(amp.operate);
        assert_eq!(amp.forward_power_watts, 500.0);
        assert_eq!(amp.swr, 1.5);
        assert_eq!(amp.temperature_c, 34.0);
        assert_eq!(amp.pa_voltage_volts, 68.9);
        assert_eq!(amp.pa_current_amps, 12.5);
        assert_eq!(amp.state, AmpOperatingState::Fault);
        assert_eq!(amp.fault.as_deref(), Some("KPA500 fault 7"));
    }

    #[test]
    fn parser_handles_real_com21_rvm_fixture() {
        let fixture = include_str!("../../../tests/fixtures/kpa500-rvm-com21.txt");
        let mut amp = bridge_core::AmpState::default();
        for line in fixture.lines().filter(|line| !line.trim().is_empty()) {
            parse_kpa500_response(line, &mut amp);
        }
        assert_eq!(amp.firmware_version.as_deref(), Some("01.54"));
    }

    #[test]
    fn parser_handles_real_com21_readonly_fixture() {
        let fixture = include_str!("../../../tests/fixtures/kpa500-readonly-com21.txt");
        let mut amp = bridge_core::AmpState::default();
        for line in fixture.lines().filter(|line| !line.trim().is_empty()) {
            parse_kpa500_response(line, &mut amp);
        }
        assert!(!amp.operate);
        assert_eq!(amp.state, AmpOperatingState::Standby);
        assert_eq!(amp.forward_power_watts, 0.0);
        assert_eq!(amp.swr, 1.0);
        assert_eq!(amp.temperature_c, 30.0);
        assert_eq!(amp.pa_voltage_volts, 68.9);
        assert_eq!(amp.pa_current_amps, 0.0);
        assert_eq!(amp.fault, None);
    }

    #[test]
    fn parser_treats_no_rf_swr_zero_as_safe_baseline() {
        let mut amp = bridge_core::AmpState::default();
        parse_kpa500_response("^WS000 000;", &mut amp);
        assert_eq!(amp.forward_power_watts, 0.0);
        assert_eq!(amp.swr, 1.0);
    }

    #[test]
    fn parser_rejects_impossible_swr_values() {
        let mut amp = bridge_core::AmpState::default();
        parse_kpa500_response("^WS000 099;", &mut amp);
        assert_eq!(amp.swr, 1.0);
        parse_kpa500_response("^WS000 991;", &mut amp);
        assert_eq!(amp.swr, 1.0);
        parse_kpa500_response("^WS000 140;", &mut amp);
        assert_eq!(amp.swr, 1.4);
    }

    #[test]
    fn all_commands_have_safety_classification() {
        assert!(command_map()
            .iter()
            .all(|command| !command.label.is_empty() && !command.wire.is_empty()));
        assert_eq!(CMD_OPERATE_STATUS.safety, CommandSafety::ReadOnly);
        assert_eq!(CMD_OPERATE.safety, CommandSafety::RfRisk);
        assert!(command_map().iter().all(|command| command.verified));
    }

    #[test]
    fn removed_placeholder_commands_are_not_active_for_kpa500() {
        for command in command_map() {
            assert_ne!(command.wire, "ST;");
            assert_ne!(command.wire, "RV;");
            assert_ne!(command.wire, "OP1;");
            assert_ne!(command.wire, "OP0;");
            assert_ne!(command.wire, "FC;");
        }
    }

    #[test]
    fn standby_control_fixture_contains_only_safe_command() {
        let fixture = include_str!("../../../tests/fixtures/kpa500-standby-control-com21.txt");
        for line in fixture.lines().filter(|line| !line.trim().is_empty()) {
            assert!(
                line.ends_with("^OS0;"),
                "unexpected unsafe standby-control fixture line: {line}"
            );
            assert!(!line.contains("^OS1;"));
            assert!(!line.contains("^FLC;"));
        }
    }

    #[test]
    fn standby_noack_fixture_uses_post_verify() {
        let fixture = include_str!("../../../tests/fixtures/kpa500-standby-noack-verify-com21.txt");
        assert!(fixture.contains("TX ^OS0;"));
        assert!(fixture.contains("RX <no ack>"));
        assert!(fixture.contains("TX ^OS;"));
        assert!(fixture.contains("RX ^OS0;"));
        assert!(!fixture.contains("^OS1;"));
    }

    #[test]
    fn standby_command_metadata_is_ackless_and_verified() {
        let standby = command_map()
            .iter()
            .find(|command| command.label == "set_standby")
            .unwrap();
        let operate_status = command_map()
            .iter()
            .find(|command| command.label == "read_operate_status")
            .unwrap();
        assert!(!standby.expects_response);
        assert!(standby.requires_post_verify);
        assert!(operate_status.expects_response);
    }
}
