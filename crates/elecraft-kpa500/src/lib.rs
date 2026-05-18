use anyhow::{Context, Result};
use bridge_core::{push_capability, AmpOperatingState, ConnectionState, SharedState};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, timeout};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, info, trace, warn};

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
    pub unsolicited: Vec<String>,
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
    pub allow_rf_risk: bool,
    pub control_verify_delay: Duration,
    pub transcript_dir: Option<PathBuf>,
    pub transcript_rotate_bytes: u64,
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
                guard.amp.runtime.record_poll_success(0);
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
        let mut backoff = Duration::from_secs(1);
        loop {
            match tokio_serial::new(self.settings.com_port.clone(), self.settings.baud)
                .open_native_async()
            {
                Ok(mut port) => {
                    backoff = Duration::from_secs(1);
                    info!(port = %self.settings.com_port, baud = self.settings.baud, "KPA500 serial connected");
                    let mut transcript = SerialTranscript::open(
                        "KPA500",
                        &self.settings.com_port,
                        &self.settings.transcript_dir,
                        self.settings.transcript_rotate_bytes,
                    )
                    .await;
                    {
                        let mut guard = self.state.write().await;
                        guard.amp.connected = true;
                        guard.amp.connection_state = ConnectionState::Connecting;
                    }
                    self.discover_capabilities(&mut port, &mut transcript).await;
                    loop {
                        if let Err(err) = self
                            .process_desired_control(&mut port, &mut transcript)
                            .await
                        {
                            warn!(
                                event_id = "command_blocked_by_safety",
                                device = "KPA500",
                                error = %err,
                                "KPA500 desired control request was not applied"
                            );
                        }
                        if let Err(err) = self.poll_status_on_port(&mut port, &mut transcript).await
                        {
                            warn!(event_id = "serial_disconnected", device = "KPA500", error = %err, "KPA500 poll failed; reconnecting");
                            let mut guard = self.state.write().await;
                            guard.amp.connected = false;
                            guard.amp.connection_state = ConnectionState::Degraded;
                            guard.amp.runtime.poll_failure_count =
                                guard.amp.runtime.poll_failure_count.saturating_add(1);
                            guard.amp.runtime.reconnect_count =
                                guard.amp.runtime.reconnect_count.saturating_add(1);
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
                    guard.amp.runtime.reconnect_count =
                        guard.amp.runtime.reconnect_count.saturating_add(1);
                }
            }
            warn!(
                event_id = "reconnect_attempt",
                device = "KPA500",
                backoff_ms = backoff.as_millis(),
                "KPA500 reconnect attempt scheduled"
            );
            sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
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
                    unsolicited: Vec::new(),
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
            self.settings.transcript_rotate_bytes,
        )
        .await;
        self.poll_status_on_port(&mut port, &mut transcript).await
    }

    pub async fn set_operate(&self) -> Result<()> {
        let result = self.set_operate_verified().await?;
        if result.verify_result != Some(CommandResultState::Verified) {
            anyhow::bail!(
                "KPA500 operate was not verified: send={:?} verify={:?} response={:?}",
                result.send_result,
                result.verify_result,
                result.verification_response
            );
        }
        Ok(())
    }

    pub async fn set_operate_verified(&self) -> Result<ControlCommandResult> {
        self.ensure_can_send(CMD_OPERATE)?;
        if self.settings.mock {
            let mut guard = self.state.write().await;
            guard.amp.operate = true;
            guard.amp.state = AmpOperatingState::Operate;
            return Ok(ControlCommandResult {
                command: CMD_OPERATE,
                send_result: CommandResultState::SentNoAck,
                verify_result: Some(CommandResultState::Verified),
                verification_response: Some("^OS1;".to_string()),
                final_state: Some(AmpOperatingState::Operate),
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
            self.settings.transcript_rotate_bytes,
        )
        .await;
        self.send_ackless_verified(&mut port, &mut transcript, CMD_OPERATE, true)
            .await
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
            self.settings.transcript_rotate_bytes,
        )
        .await;
        self.send_ackless_verified(&mut port, &mut transcript, CMD_STANDBY, false)
            .await
    }

    async fn process_desired_control(
        &self,
        port: &mut SerialStream,
        transcript: &mut SerialTranscript,
    ) -> Result<()> {
        let desired = {
            let mut guard = self.state.write().await;
            guard.desired.amp_operate.take()
        };
        let Some(operate) = desired else {
            return Ok(());
        };
        let command = if operate { CMD_OPERATE } else { CMD_STANDBY };
        info!(
            event_id = "pgxl_control_mapping",
            requested_operate = operate,
            kpa_command = command.wire,
            safety = ?command.safety,
            dry_run = self.settings.dry_run,
            allow_rf_risk = self.settings.allow_rf_risk,
            "mapping PGXL/Flex amplifier control to KPA500"
        );
        let result = self
            .send_ackless_verified(port, transcript, command, operate)
            .await?;
        info!(
            event_id = "pgxl_control_result",
            command = command.label,
            send_result = ?result.send_result,
            verify_result = ?result.verify_result,
            verification_response = ?result.verification_response,
            final_state = ?result.final_state,
            "KPA500 mapped control completed"
        );
        Ok(())
    }

    async fn send_ackless_verified(
        &self,
        port: &mut SerialStream,
        transcript: &mut SerialTranscript,
        command: ElecraftCommand,
        expected_operate: bool,
    ) -> Result<ControlCommandResult> {
        self.ensure_can_send(command)?;
        send_no_ack_command(port, command, transcript).await?;
        sleep(self.settings.control_verify_delay).await;
        match send_command(
            port,
            CMD_OPERATE_STATUS,
            Duration::from_millis(1000),
            transcript,
        )
        .await
        {
            Ok(response) => {
                let mut amp = bridge_core::AmpState::default();
                parse_kpa500_response(&response, &mut amp);
                let verified = amp.operate == expected_operate
                    && matches!(
                        (expected_operate, amp.state),
                        (false, AmpOperatingState::Standby) | (true, AmpOperatingState::Operate)
                    );
                if verified {
                    let mut guard = self.state.write().await;
                    parse_kpa500_response(&response, &mut guard.amp);
                    Ok(ControlCommandResult {
                        command,
                        send_result: CommandResultState::SentNoAck,
                        verify_result: Some(CommandResultState::Verified),
                        verification_response: Some(response),
                        final_state: Some(amp.state),
                    })
                } else {
                    Ok(ControlCommandResult {
                        command,
                        send_result: CommandResultState::SentNoAck,
                        verify_result: Some(CommandResultState::VerifyFailed),
                        verification_response: Some(response),
                        final_state: Some(amp.state),
                    })
                }
            }
            Err(err) => Ok(ControlCommandResult {
                command,
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
            self.settings.transcript_rotate_bytes,
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
        if command.safety == CommandSafety::RfRisk && !self.settings.allow_rf_risk {
            warn!(
                event_id = "blocked_rf_risk_control",
                device = "KPA500",
                command = command.label,
                wire = command.wire,
                "blocked RF-risk KPA500 command because allow_rf_risk is false"
            );
            anyhow::bail!(
                "KPA500 RF-risk command {} requires allow_rf_risk",
                command.label
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
        let started = Instant::now();
        let mut outcomes = Vec::with_capacity(read_only_poll_commands().len());
        for command in read_only_poll_commands() {
            match send_command_collect(port, *command, Duration::from_millis(1000), transcript)
                .await
            {
                Ok(read) => {
                    debug!(command = command.label, response = %read.response, unsolicited_count = read.unsolicited.len(), "KPA500 status response");
                    outcomes.push(CommandOutcome {
                        command: *command,
                        response: Some(read.response),
                        error: None,
                        unsolicited: read.unsolicited,
                    });
                }
                Err(err) => {
                    warn!(device = "KPA500", command = command.label, error = %err, "KPA500 read-only command failed; continuing");
                    outcomes.push(CommandOutcome {
                        command: *command,
                        response: None,
                        error: Some(err.to_string()),
                        unsolicited: Vec::new(),
                    });
                }
            }
        }
        {
            let mut guard = self.state.write().await;
            guard.amp.connected = true;
            guard.amp.connection_state = ConnectionState::Connected;
            guard.amp.last_serial_command_at = Some(SystemTime::now());
            if outcomes.iter().any(|outcome| outcome.response.is_some()) {
                guard.amp.last_serial_response_at = Some(SystemTime::now());
                guard.amp.last_successful_poll_at = Some(SystemTime::now());
                guard
                    .amp
                    .runtime
                    .record_poll_success(duration_millis_u64(started.elapsed()));
            } else {
                guard.amp.connection_state = ConnectionState::Degraded;
                guard.amp.runtime.poll_failure_count =
                    guard.amp.runtime.poll_failure_count.saturating_add(1);
            }
            for outcome in &outcomes {
                for unsolicited in &outcome.unsolicited {
                    parse_kpa500_response(unsolicited, &mut guard.amp);
                }
                if let Some(response) = &outcome.response {
                    push_capability(&mut guard.amp.capabilities, outcome.command.label);
                    parse_kpa500_response(response, &mut guard.amp);
                }
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
    if (10..=990).contains(&value) {
        return Some(f32::from(value) / 10.0);
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
    Ok(send_command_collect(port, command, wait, transcript)
        .await?
        .response)
}

struct SerialRead {
    response: String,
    unsolicited: Vec<String>,
}

async fn send_command_collect(
    port: &mut SerialStream,
    command: ElecraftCommand,
    wait: Duration,
    transcript: &mut SerialTranscript,
) -> Result<SerialRead> {
    transcript.write_line("TX", command.wire).await;
    port.write_all(command.wire.as_bytes())
        .await
        .with_context(|| format!("failed to write serial command {}", command.label))?;
    port.flush().await.context("failed to flush serial port")?;

    let expected_prefixes = expected_prefixes(command);
    let mut unsolicited = Vec::new();
    let response = timeout(wait, async {
        loop {
            let response = read_serial_line(port).await?;
            transcript.write_line("RX", &response).await;
            if matches_expected_response(&response, expected_prefixes) {
                debug!(
                    device = "KPA500",
                    command = command.label,
                    expected_prefix = ?expected_prefixes,
                    received = %response,
                    classification = "matched",
                    "KPA500 serial response matched command"
                );
                break Ok::<String, std::io::Error>(response);
            }
            let classification = if response == command.wire {
                "echo_only"
            } else if is_unsolicited_response(&response) {
                "unsolicited"
            } else {
                "mismatched"
            };
            if classification == "unsolicited" || classification == "echo_only" {
                trace!(
                    device = "KPA500",
                    command = command.label,
                    expected_prefix = ?expected_prefixes,
                    received = %response,
                    classification,
                    "KPA500 serial response did not match current command"
                );
            } else {
                debug!(
                    device = "KPA500",
                    command = command.label,
                    expected_prefix = ?expected_prefixes,
                    received = %response,
                    classification,
                    "KPA500 serial response did not match current command"
                );
            }
            unsolicited.push(response);
        }
    })
    .await
    .context("serial response timed out")?
    .context("failed reading serial response")?;

    Ok(SerialRead {
        response,
        unsolicited,
    })
}

async fn read_serial_line(port: &mut SerialStream) -> std::io::Result<String> {
    let mut buf = Vec::new();
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
    Ok(String::from_utf8_lossy(&buf).trim().to_string())
}

fn expected_prefixes(command: ElecraftCommand) -> &'static [&'static str] {
    match command.label {
        "read_firmware" => &["^RVM"],
        "read_serial_number" => &["^SN"],
        "read_operate_status" => &["^OS"],
        "read_power_swr" => &["^WS"],
        "read_temperature" => &["^TM"],
        "read_volts_current" => &["^VI"],
        "read_fault" => &["^FL"],
        _ => &[],
    }
}

fn matches_expected_response(response: &str, expected_prefixes: &[&str]) -> bool {
    expected_prefixes
        .iter()
        .any(|prefix| response.starts_with(prefix) && response.ends_with(';'))
}

fn is_unsolicited_response(response: &str) -> bool {
    const PREFIXES: &[&str] = &["^RVM", "^SN", "^OS", "^WS", "^TM", "^VI", "^FL"];
    PREFIXES
        .iter()
        .any(|prefix| response.starts_with(prefix) && response.ends_with(';'))
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
    dir: Option<PathBuf>,
    device: String,
    safe_port: String,
    session_ts: u128,
    index: u64,
    bytes_written: u64,
    rotate_bytes: u64,
}

impl SerialTranscript {
    fn disabled() -> Self {
        Self {
            file: None,
            dir: None,
            device: String::new(),
            safe_port: String::new(),
            session_ts: 0,
            index: 0,
            bytes_written: 0,
            rotate_bytes: 0,
        }
    }

    async fn open(device: &str, port: &str, dir: &Option<PathBuf>, rotate_bytes: u64) -> Self {
        let Some(dir) = dir else {
            return Self::disabled();
        };
        if let Err(err) = create_dir_all(dir).await {
            warn!(device, error = %err, "serial transcript directory could not be created");
            return Self::disabled();
        }
        let ts = timestamp_millis();
        let safe_port = port.replace([':', '\\', '/', '.'], "_");
        let path = transcript_path(dir, device, ts, &safe_port, 0);
        match File::create(&path).await {
            Ok(file) => Self {
                file: Some(file),
                dir: Some(dir.clone()),
                device: device.to_lowercase(),
                safe_port,
                session_ts: ts,
                index: 0,
                bytes_written: 0,
                rotate_bytes,
            },
            Err(err) => {
                warn!(device, path = %path.display(), error = %err, "serial transcript file could not be opened");
                Self::disabled()
            }
        }
    }

    async fn write_line(&mut self, direction: &str, line: &str) {
        if self.file.is_none() {
            return;
        }
        let row = format!("{} {direction} {line}\n", timestamp_millis());
        if self.rotate_bytes > 0
            && self.bytes_written.saturating_add(row.len() as u64) > self.rotate_bytes
        {
            self.rotate().await;
        }
        let Some(file) = &mut self.file else {
            return;
        };
        if let Err(err) = file.write_all(row.as_bytes()).await {
            warn!(error = %err, "serial transcript write failed");
            self.file = None;
            return;
        }
        self.bytes_written = self.bytes_written.saturating_add(row.len() as u64);
        if let Err(err) = file.flush().await {
            warn!(error = %err, "serial transcript flush failed");
            self.file = None;
        }
    }

    async fn rotate(&mut self) {
        let Some(dir) = &self.dir else {
            return;
        };
        self.index = self.index.saturating_add(1);
        let path = transcript_path(
            dir,
            &self.device,
            self.session_ts,
            &self.safe_port,
            self.index,
        );
        match File::create(&path).await {
            Ok(file) => {
                self.file = Some(file);
                self.bytes_written = 0;
            }
            Err(err) => {
                warn!(path = %path.display(), error = %err, "serial transcript rotation failed");
                self.file = None;
            }
        }
    }
}

fn transcript_path(dir: &Path, device: &str, ts: u128, safe_port: &str, index: u64) -> PathBuf {
    dir.join(format!(
        "{}-{}-{}-{}.log",
        device.to_lowercase(),
        ts,
        safe_port,
        index
    ))
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
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
        parse_kpa500_response("^WS500 015;", &mut amp);
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
    fn parser_handles_live_power_swr_tenths() {
        let fixture = include_str!("../../../tests/fixtures/kpa500-live-ws-com21.txt");
        let mut amp = bridge_core::AmpState::default();
        for line in fixture.lines().filter(|line| line.starts_with('^')) {
            parse_kpa500_response(line, &mut amp);
        }
        assert_eq!(amp.forward_power_watts, 30.0);
        assert_eq!(amp.swr, 1.1);
    }

    #[test]
    fn response_matching_ignores_out_of_order_kpa_lines() {
        let fixture = include_str!("../../../tests/fixtures/kpa500-out-of-order-com21.txt");
        assert!(fixture.contains("RX ^FL00;"));
        assert!(fixture.contains("RX ^WS000 000;"));
        assert!(!matches_expected_response(
            "^FL00;",
            expected_prefixes(CMD_OPERATE_STATUS)
        ));
        assert!(is_unsolicited_response("^FL00;"));
        assert!(matches_expected_response(
            "^OS0;",
            expected_prefixes(CMD_OPERATE_STATUS)
        ));
        assert!(!matches_expected_response(
            "^OS0;",
            expected_prefixes(CMD_POWER_SWR)
        ));
        assert!(matches_expected_response(
            "^WS030 011;",
            expected_prefixes(CMD_POWER_SWR)
        ));
    }

    #[test]
    fn parser_rejects_impossible_swr_values() {
        let mut amp = bridge_core::AmpState::default();
        parse_kpa500_response("^WS000 009;", &mut amp);
        assert_eq!(amp.swr, 1.0);
        parse_kpa500_response("^WS000 991;", &mut amp);
        assert_eq!(amp.swr, 1.0);
        parse_kpa500_response("^WS000 014;", &mut amp);
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
