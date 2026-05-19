use anyhow::{Context, Result};
use bridge_core::{
    append_evidence_json, append_evidence_line, ConnectionState, FlexMeterHandle, SharedState,
};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant, SystemTime};
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
    pub full_pgxl_registration: bool,
    pub create_meters: bool,
    pub create_interlock: bool,
    pub allow_rf_risk: bool,
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
    {
        let mut guard = state.write().await;
        guard.flex_injection.enabled = true;
        guard.flex_injection.connection_state = ConnectionState::Connecting;
    }
    loop {
        match run_session(&settings, state.clone()).await {
            Ok(()) => {
                warn!("Flex amplifier injection session ended");
                backoff = settings.reconnect_initial.max(Duration::from_millis(100));
            }
            Err(err) => {
                warn!(
                    error = %err,
                    retry_ms = backoff.as_millis(),
                    "Flex amplifier injection session failed"
                );
            }
        }
        {
            let mut guard = state.write().await;
            if guard.flex_injection.connection_state == ConnectionState::Connected {
                backoff = settings.reconnect_initial.max(Duration::from_millis(100));
            }
            guard.flex_injection.connection_state = ConnectionState::Degraded;
            guard.flex_injection.degraded_reason =
                Some("session ended; reconnect pending".to_string());
            guard.flex_injection.amplifier_handle = None;
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

    {
        let mut guard = state.write().await;
        guard.flex_injection.connection_state = ConnectionState::Connected;
        guard.flex_injection.degraded_reason = None;
        guard.flex_injection.last_command = None;
        guard.flex_injection.last_response = None;
    }

    let registration = registration_commands(settings);
    let mut registration_sent = false;
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
                if let Some(handle) = session.handle.clone() {
                    let mut guard = state.write().await;
                    guard.flex_injection.client_handle = Some(handle);
                }
                trace_flex_rx(&line);

                if session.has_handle && !registration_sent {
                    for item in &registration {
                        send_tracked_command(
                            &mut writer,
                            &mut session,
                            &state,
                            &mut next_seq,
                            item.clone(),
                        )
                        .await?;
                    }
                    registration_sent = true;
                }

                if let Some((seq, code, body)) = parse_response(&line) {
                    session
                        .observe_response(settings, &state, seq, &code, &body)
                        .await;
                }

                if let Some(status) = parse_amplifier_status(&line) {
                    observe_tuner_presence(&state, &status).await;
                    if session.observe_amplifier_status(settings, &status) {
                        set_amplifier_handle(&state, &status.handle).await;
                        handle_amplifier_status(
                            settings,
                            &state,
                            &mut writer,
                            &mut next_seq,
                            &status,
                        )
                        .await?;
                    }
                }
            }
            () = &mut ping_timer => {
                if registration_sent {
                    send_tracked_command(
                        &mut writer,
                        &mut session,
                        &state,
                        &mut next_seq,
                        PendingCommand::new("ping", "ping", PendingKind::Ping),
                    )
                    .await?;
                    {
                        let mut guard = state.write().await;
                        guard.flex_injection.ping_count =
                            guard.flex_injection.ping_count.saturating_add(1);
                    }
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

async fn send_tracked_command(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    session: &mut FlexSession,
    state: &SharedState,
    next_seq: &mut u32,
    pending: PendingCommand,
) -> Result<()> {
    let expired = session.cleanup_pending(Duration::from_secs(120), 128);
    if expired > 0 {
        let mut guard = state.write().await;
        guard.flex_injection.expired_pending_count = guard
            .flex_injection
            .expired_pending_count
            .saturating_add(expired);
    }
    let seq = *next_seq;
    send_command(writer, seq, &pending.command).await?;
    info!(
        seq,
        label = %pending.label,
        command = %pending.command,
        "Flex PGXL registration command sent"
    );
    session.pending.insert(seq, pending.clone());
    {
        let mut guard = state.write().await;
        guard.flex_injection.last_command = Some(pending.command);
        guard.flex_injection.pending_count = session.pending.len();
    }
    *next_seq = next_seq.saturating_add(1);
    Ok(())
}

#[derive(Debug, Default)]
struct FlexSession {
    has_handle: bool,
    handle: Option<String>,
    version: Option<String>,
    amplifier_handle: Option<String>,
    pending: HashMap<u32, PendingCommand>,
}

impl FlexSession {
    fn cleanup_pending(&mut self, ttl: Duration, max_size: usize) -> u64 {
        let now = Instant::now();
        let before = self.pending.len();
        self.pending
            .retain(|_, pending| now.duration_since(pending.created_at) <= ttl);
        while self.pending.len() > max_size {
            if let Some(seq) = self.pending.keys().next().copied() {
                self.pending.remove(&seq);
            } else {
                break;
            }
        }
        (before.saturating_sub(self.pending.len())) as u64
    }

    fn observe_line(&mut self, line: &str) {
        if let Some(version) = line.strip_prefix('V') {
            self.version = Some(version.trim().to_string());
            info!(version = %version.trim(), "Flex API version received");
        } else if let Some(handle) = line.strip_prefix('H') {
            let handle = handle.trim().to_string();
            self.handle = Some(handle.clone());
            self.has_handle = true;
            // The radio assigns this connection handle before accepting any
            // object creation commands from the external amplifier client.
            info!(%handle, "Flex API client handle received");
        }
    }

    async fn observe_response(
        &mut self,
        _settings: &FlexInjectionSettings,
        state: &SharedState,
        seq: u32,
        code: &str,
        body: &str,
    ) {
        let pending = self.pending.remove(&seq);
        let label = pending
            .as_ref()
            .map(|pending| pending.label.as_str())
            .unwrap_or("unknown");
        if code == "0" {
            info!(seq, label, body, "Flex PGXL registration command accepted");
        } else {
            warn!(
                seq,
                label, code, body, "Flex PGXL registration command rejected"
            );
        }

        {
            let mut guard = state.write().await;
            guard.flex_injection.last_response = Some(format!("R{seq}|{code}|{body}"));
            guard.flex_injection.pending_count = self.pending.len();
            if code == "0" {
                guard.flex_injection.command_success_count =
                    guard.flex_injection.command_success_count.saturating_add(1);
            } else {
                guard.flex_injection.command_failure_count =
                    guard.flex_injection.command_failure_count.saturating_add(1);
            }
        }

        let Some(pending) = pending else {
            return;
        };
        if code != "0" {
            if matches!(pending.kind, PendingKind::Ping) {
                let mut guard = state.write().await;
                guard.flex_injection.ping_failure_count =
                    guard.flex_injection.ping_failure_count.saturating_add(1);
                guard.flex_injection.degraded_reason =
                    Some(format!("Flex ping rejected: {code} {body}"));
            }
            return;
        }
        match pending.kind {
            PendingKind::AmplifierCreate => {
                if let Some(handle) = response_object_id(body) {
                    set_amplifier_handle(state, handle).await;
                }
            }
            PendingKind::MeterCreate { name } => {
                if let Some(handle) = response_object_id(body) {
                    upsert_meter_handle(state, name, handle).await;
                }
            }
            PendingKind::InterlockCreate => {
                if let Some(handle) = response_object_id(body) {
                    let mut guard = state.write().await;
                    guard.flex_injection.interlock_handle = Some(handle.to_string());
                }
            }
            PendingKind::KeepaliveEnable | PendingKind::Subscription => {}
            PendingKind::Ping => {
                let mut guard = state.write().await;
                if code == "0" {
                    guard.flex_injection.degraded_reason = None;
                    guard.flex_injection.connection_state = ConnectionState::Connected;
                } else {
                    guard.flex_injection.ping_failure_count =
                        guard.flex_injection.ping_failure_count.saturating_add(1);
                    guard.flex_injection.degraded_reason =
                        Some(format!("Flex ping rejected: {code} {body}"));
                }
            }
        }
    }

    fn observe_amplifier_status(
        &mut self,
        settings: &FlexInjectionSettings,
        status: &AmplifierStatus,
    ) -> bool {
        let model_match = status
            .value("model")
            .is_some_and(|model| model == settings.amplifier_model);
        let serial_match = status
            .value("serial_num")
            .is_some_and(|serial| serial == settings.serial);
        let known_handle = self.amplifier_handle.as_deref() == Some(status.handle.as_str());
        if model_match || serial_match || known_handle {
            if self.amplifier_handle.as_deref() != Some(status.handle.as_str()) {
                self.amplifier_handle = Some(status.handle.clone());
                info!(
                    amplifier_handle = %status.handle,
                    model = status.value("model").unwrap_or(""),
                    serial = status.value("serial_num").unwrap_or(""),
                    "Flex amplifier object handle observed"
                );
            }
            true
        } else {
            false
        }
    }
}

async fn set_amplifier_handle(state: &SharedState, handle: &str) {
    let mut guard = state.write().await;
    guard.flex_injection.amplifier_handle = Some(handle.to_string());
}

async fn upsert_meter_handle(state: &SharedState, name: &str, handle: &str) {
    let mut guard = state.write().await;
    if let Some(existing) = guard
        .flex_injection
        .meter_handles
        .iter_mut()
        .find(|meter| meter.name == name)
    {
        existing.handle = handle.to_string();
    } else {
        guard.flex_injection.meter_handles.push(FlexMeterHandle {
            name: name.to_string(),
            handle: handle.to_string(),
        });
    }
}

async fn observe_tuner_presence(state: &SharedState, status: &AmplifierStatus) {
    let is_tuner = status
        .value("model")
        .is_some_and(|model| model == "TunerGeniusXL");
    let disappearance = status.tuner_disappearance_reason();
    let mut guard = state.write().await;

    if is_tuner && disappearance.is_none() {
        let appeared = guard.flex_injection.tuner_handle.as_deref() != Some(status.handle.as_str());
        guard.flex_injection.tuner_handle = Some(status.handle.clone());
        guard.flex_injection.tuner_last_seen_at = Some(SystemTime::now());
        if appeared {
            guard.flex_injection.tuner_appeared_count =
                guard.flex_injection.tuner_appeared_count.saturating_add(1);
            info!(
                event_id = "smartsdr_tuner_presence_appeared",
                tuner_handle = %status.handle,
                raw = %status.raw,
            "Flex tuner presence appeared"
            );
            append_evidence_json(
                "disconnect-events.jsonl",
                &serde_json::json!({
                    "event": "tuner_registered",
                    "handle": status.handle,
                    "raw": status.raw,
                }),
            );
        }
    }

    if let Some(reason) = disappearance {
        let matches_known_handle =
            guard.flex_injection.tuner_handle.as_deref() == Some(status.handle.as_str());
        if is_tuner || matches_known_handle {
            guard.flex_injection.tuner_disappeared_count = guard
                .flex_injection
                .tuner_disappeared_count
                .saturating_add(1);
            guard.flex_injection.last_tuner_disappearance_reason = Some(reason.to_string());
            guard.flex_injection.tuner_handle = None;
            warn!(
                event_id = "smartsdr_tuner_presence_disappeared",
                tuner_handle = %status.handle,
                reason,
                raw = %status.raw,
                "Flex tuner presence disappeared"
            );
            append_evidence_json(
                "disconnect-events.jsonl",
                &serde_json::json!({
                    "event": "smartsdr_tuner_disconnected",
                    "handle": status.handle,
                    "reason": reason,
                    "raw": status.raw,
                }),
            );
        }
    }
}

async fn handle_amplifier_status(
    settings: &FlexInjectionSettings,
    state: &SharedState,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    next_seq: &mut u32,
    status: &AmplifierStatus,
) -> Result<()> {
    let requested = status.requested_operate();
    debug!(
        amplifier_handle = %status.handle,
        requested_operate = ?requested,
        raw = %status.raw,
        "Flex amplifier status observed"
    );
    let Some(operate) = requested else {
        return Ok(());
    };

    if operate && !settings.allow_rf_risk {
        warn!(
            event_id = "blocked_rf_risk_control",
            amplifier_handle = %status.handle,
            raw = %status.raw,
            "Flex requested amplifier operate while RF-risk control is disabled"
        );
        let command = format!("amplifier set {} operate=0", status.handle);
        send_command(writer, *next_seq, &command).await?;
        info!(
            seq = *next_seq,
            command = %command,
            "Flex amplifier operate request reverted to standby"
        );
        *next_seq = (*next_seq).saturating_add(1);
        return Ok(());
    }

    {
        let mut guard = state.write().await;
        if guard.desired.amp_operate != Some(operate) {
            guard.desired.amp_operate = Some(operate);
        }
    }
    info!(
        event_id = "pgxl_control_mapping",
        amplifier_handle = %status.handle,
        requested_operate = operate,
        allow_rf_risk = settings.allow_rf_risk,
        "Flex amplifier state mapped to KPA500 desired state"
    );
    Ok(())
}

#[derive(Debug, Clone)]
struct PendingCommand {
    label: String,
    command: String,
    kind: PendingKind,
    created_at: Instant,
}

impl PendingCommand {
    fn new(label: impl Into<String>, command: impl Into<String>, kind: PendingKind) -> Self {
        Self {
            label: label.into(),
            command: command.into(),
            kind,
            created_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
enum PendingKind {
    AmplifierCreate,
    MeterCreate { name: &'static str },
    InterlockCreate,
    KeepaliveEnable,
    Subscription,
    Ping,
}

#[derive(Debug, Clone)]
struct AmplifierStatus {
    raw: String,
    handle: String,
    kvs: Vec<(String, String)>,
}

impl AmplifierStatus {
    fn value(&self, key: &str) -> Option<&str> {
        self.kvs
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }

    fn requested_operate(&self) -> Option<bool> {
        if let Some(value) = self.value("operate") {
            return match value {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            };
        }
        let state = self.value("state")?.to_ascii_uppercase();
        Some(matches!(
            state.as_str(),
            "IDLE" | "OPERATE" | "TRANSMIT" | "TRANSMIT_A" | "TRANSMIT_B"
        ))
    }

    fn tuner_disappearance_reason(&self) -> Option<&'static str> {
        for key in ["connected", "online", "present"] {
            if let Some(value) = self.value(key) {
                if matches!(value, "0" | "false" | "False" | "FALSE") {
                    return Some(match key {
                        "connected" => "connected=0",
                        "online" => "online=0",
                        "present" => "present=0",
                        _ => "presence_false",
                    });
                }
            }
        }
        for key in ["removed", "deleted"] {
            if let Some(value) = self.value(key) {
                if matches!(value, "1" | "true" | "True" | "TRUE") {
                    return Some(if key == "removed" {
                        "removed=1"
                    } else {
                        "deleted=1"
                    });
                }
            }
        }
        if let Some(state) = self.value("state") {
            if matches!(
                state.to_ascii_uppercase().as_str(),
                "REMOVED" | "DISCONNECTED" | "OFFLINE" | "UNKNOWN"
            ) {
                return Some("state_removed_or_disconnected");
            }
        }
        None
    }
}

fn parse_amplifier_status(line: &str) -> Option<AmplifierStatus> {
    if !line.starts_with('S') {
        return None;
    }
    let body = line.split_once('|')?.1;
    let rest = body.strip_prefix("amplifier ")?;
    let mut parts = rest.split_whitespace();
    let handle = parts.next()?.to_string();
    let kvs = parts
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect::<Vec<_>>();
    Some(AmplifierStatus {
        raw: line.to_string(),
        handle,
        kvs,
    })
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
    append_evidence_line("flex-rx.log", line);
}

fn trace_flex_tx(line: &str) {
    debug!(line = %line, "FLEX TX >");
    append_evidence_line("flex-tx.log", line);
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

pub fn registration_command_lines(settings: &FlexInjectionSettings) -> Vec<String> {
    registration_commands(settings)
        .into_iter()
        .map(|pending| pending.command)
        .collect()
}

fn registration_commands(settings: &FlexInjectionSettings) -> Vec<PendingCommand> {
    let mut commands = Vec::new();
    commands.push(PendingCommand::new(
        "amplifier_create",
        settings.amplifier_create_command(),
        PendingKind::AmplifierCreate,
    ));
    if settings.full_pgxl_registration && settings.create_meters {
        for meter in amp_meter_create_commands() {
            commands.push(PendingCommand::new(
                format!("meter_create_{}", meter.name),
                meter.command,
                PendingKind::MeterCreate { name: meter.name },
            ));
        }
    }
    if settings.full_pgxl_registration && settings.create_interlock {
        commands.push(PendingCommand::new(
            "interlock_create",
            interlock_create_command(&settings.serial),
            PendingKind::InterlockCreate,
        ));
    }
    if settings.full_pgxl_registration {
        commands.push(PendingCommand::new(
            "keepalive_enable",
            "keepalive enable",
            PendingKind::KeepaliveEnable,
        ));
    }
    commands.push(PendingCommand::new(
        "sub_amplifier_all",
        "sub amplifier all",
        PendingKind::Subscription,
    ));
    commands
}

#[derive(Debug, Clone, Copy)]
struct MeterCreateCommand {
    name: &'static str,
    command: &'static str,
}

fn amp_meter_create_commands() -> &'static [MeterCreateCommand] {
    &[
        MeterCreateCommand {
            name: "FWD",
            command: "meter create name=FWD type=AMP min=30.0 max=63.01 units=DBM",
        },
        MeterCreateCommand {
            name: "RL",
            command: "meter create name=RL type=AMP min=34.0 max=60.0 units=DB",
        },
        MeterCreateCommand {
            name: "DRV",
            command: "meter create name=DRV type=AMP min=10.0 max=50.00 units=DBM",
        },
        MeterCreateCommand {
            name: "ID",
            command: "meter create name=ID type=AMP min=0.0 max=70.0 units=AMPS",
        },
        MeterCreateCommand {
            name: "TEMP",
            command: "meter create name=TEMP type=AMP min=0.0 max=100.0 units=TEMP_C",
        },
    ]
}

fn interlock_create_command(serial: &str) -> String {
    format!(
        "interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial={}",
        sanitize_token(serial)
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

fn response_object_id(body: &str) -> Option<&str> {
    let first = body.split('|').next()?.trim();
    if first.is_empty() || first.eq_ignore_ascii_case("OK") || first.contains(char::is_whitespace) {
        None
    } else {
        Some(first)
    }
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
        assert_eq!(response_object_id("32|OK"), Some("32"));
        assert_eq!(response_object_id("OK"), None);
    }

    #[test]
    fn parses_amplifier_status_operate_request() {
        let status = parse_amplifier_status(
            "S1A2B|amplifier 0x42000001 model=PowerGeniusXL serial_num=EGB-KPA500 operate=1",
        )
        .unwrap();
        assert_eq!(status.handle, "0x42000001");
        assert_eq!(status.value("model"), Some("PowerGeniusXL"));
        assert_eq!(status.requested_operate(), Some(true));

        let standby =
            parse_amplifier_status("S1A2B|amplifier 0x42000001 model=PowerGeniusXL state=STANDBY")
                .unwrap();
        assert_eq!(standby.requested_operate(), Some(false));
    }

    #[test]
    fn ignores_response_lines_when_parsing_amplifier_status() {
        assert!(parse_amplifier_status("R44|0|amplifier 0x42000001 model=PowerGeniusXL").is_none());
    }

    #[tokio::test]
    async fn tracks_flex_tuner_presence_changes() {
        let state = bridge_core::state::shared_default_state();
        let appeared = parse_amplifier_status(
            "S1A2B|amplifier 0x54000001 model=TunerGeniusXL ip=192.168.0.10 connected=1",
        )
        .unwrap();
        observe_tuner_presence(&state, &appeared).await;
        {
            let guard = state.read().await;
            assert_eq!(
                guard.flex_injection.tuner_handle.as_deref(),
                Some("0x54000001")
            );
            assert_eq!(guard.flex_injection.tuner_appeared_count, 1);
            assert_eq!(guard.flex_injection.tuner_disappeared_count, 0);
            assert!(guard.flex_injection.tuner_last_seen_at.is_some());
        }

        let disappeared =
            parse_amplifier_status("S1A2B|amplifier 0x54000001 model=TunerGeniusXL connected=0")
                .unwrap();
        observe_tuner_presence(&state, &disappeared).await;
        let guard = state.read().await;
        assert_eq!(guard.flex_injection.tuner_handle, None);
        assert_eq!(guard.flex_injection.tuner_disappeared_count, 1);
        assert_eq!(
            guard
                .flex_injection
                .last_tuner_disappearance_reason
                .as_deref(),
            Some("connected=0")
        );
    }

    #[test]
    fn full_pgxl_registration_sequence_matches_reference_order() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
        };
        let commands = registration_command_lines(&settings);
        assert_eq!(
            commands[0],
            "amplifier create ip=192.168.1.50 port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB"
        );
        assert!(commands
            .contains(&"meter create name=FWD type=AMP min=30.0 max=63.01 units=DBM".to_string()));
        assert!(commands
            .contains(&"meter create name=RL type=AMP min=34.0 max=60.0 units=DB".to_string()));
        assert!(commands
            .contains(&"meter create name=DRV type=AMP min=10.0 max=50.00 units=DBM".to_string()));
        assert!(commands
            .contains(&"meter create name=ID type=AMP min=0.0 max=70.0 units=AMPS".to_string()));
        assert!(commands.contains(
            &"meter create name=TEMP type=AMP min=0.0 max=100.0 units=TEMP_C".to_string()
        ));
        assert!(commands.contains(
            &"interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=EGB-KPA500"
                .to_string()
        ));
        assert!(commands.contains(&"keepalive enable".to_string()));
        assert_eq!(
            commands.last().map(String::as_str),
            Some("sub amplifier all")
        );
    }
}
