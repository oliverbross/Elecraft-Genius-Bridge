use anyhow::{Context, Result};
use bridge_core::{
    append_evidence_json, append_evidence_line, Band, ConnectionState, FlexMeterHandle,
    LifecycleState, SharedState,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
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
    pub amplifier_status_profile: String,
    pub trace_amplifier_advertisements: bool,
    pub pgxl_force_operate_advertisement: bool,
    pub flex_force_operate_via_radio: bool,
    pub pgxl_connect_assist: bool,
    pub amplifier_startup_state_policy: String,
    pub wait_first_kpa_poll_timeout: Duration,
    pub full_pgxl_registration: bool,
    pub create_meters: bool,
    pub create_interlock: bool,
    pub disable_amp_interlock: bool,
    pub allow_rf_risk: bool,
    pub reconnect_initial: Duration,
    pub reconnect_max: Duration,
    pub ping_interval: Duration,
    pub tuner_presence_refresh: bool,
    pub tuner_refresh_interval: Duration,
    pub amplifier_reannounce_interval: Duration,
    pub pgxl_startup_trigger_strategy: String,
    pub aethersdr_open_trigger_variant: String,
}

impl FlexInjectionSettings {
    pub fn amplifier_create_command(&self) -> String {
        amplifier_create_command(
            self.amplifier_ip,
            self.amplifier_port,
            &self.amplifier_model,
            &self.serial,
            &self.ant_map,
            &self.amplifier_status_profile,
            &self.aethersdr_open_trigger_variant,
        )
    }

    pub fn amplifier_create_command_with_state(&self, state_value: &str) -> String {
        amplifier_create_command_with_state_for_variant(
            self.amplifier_ip,
            self.amplifier_port,
            &self.amplifier_model,
            &self.serial,
            &self.ant_map,
            &self.amplifier_status_profile,
            &self.aethersdr_open_trigger_variant,
            Some(state_value),
        )
    }

    fn lab_forces_operate_advertisement(&self) -> bool {
        self.pgxl_force_operate_advertisement
            || self.amplifier_status_profile == "aethersdr_pgxl_direct_lab"
    }

    fn is_lab_mode(&self) -> bool {
        self.amplifier_status_profile == "aethersdr_pgxl_direct_lab"
            || self.pgxl_force_operate_advertisement
            || self.flex_force_operate_via_radio
    }
}

pub async fn run(settings: FlexInjectionSettings, state: SharedState) {
    let mut backoff = settings.reconnect_initial.max(Duration::from_millis(100));
    {
        let mut guard = state.write().await;
        guard.flex_injection.enabled = true;
        guard.flex_injection.connection_state = ConnectionState::Connecting;
        guard.flex_injection.radio_addr = Some(settings.radio_addr.to_string());
        guard.flex_injection.pgxl_connect_assist_enabled = settings.pgxl_connect_assist;
        guard.flex_injection.flex_force_operate_via_radio = settings.flex_force_operate_via_radio;
        guard
            .lifecycle
            .flex_session
            .transition(LifecycleState::Connecting, "Flex injection task started");
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
                let mut guard = state.write().await;
                guard.flex_injection.last_error = Some(err.to_string());
            }
        }
        let amplifier_removed = {
            let guard = state.read().await;
            guard.flex_injection.amplifier_removed_count > 0
        };
        {
            let mut guard = state.write().await;
            if guard.flex_injection.connection_state == ConnectionState::Connected {
                backoff = settings.reconnect_initial.max(Duration::from_millis(100));
            }
            if amplifier_removed && !settings.is_lab_mode() {
                guard.flex_injection.connection_state = ConnectionState::Degraded;
                guard.flex_injection.degraded_reason = Some(
                    "Flex removed the amplifier object; registration halted until bridge restart"
                        .to_string(),
                );
                guard.flex_injection.amplifier_recreate_reason =
                    Some("halted after Flex amplifier removed event".to_string());
                guard.lifecycle.flex_session.transition(
                    LifecycleState::Degraded,
                    "Flex amplifier removed; reconnect disabled until restart",
                );
                guard.lifecycle.amplifier.transition(
                    LifecycleState::Removed,
                    "Flex removed amplifier object; restart required",
                );
                warn!(
                    event_id = "flex_amplifier_registration_halted",
                    "Flex removed amplifier object; EGB will not recreate it until restart"
                );
                break;
            }
            guard.flex_injection.connection_state = ConnectionState::Degraded;
            guard.flex_injection.degraded_reason =
                Some("session ended; reconnect pending".to_string());
            guard.flex_injection.amplifier_handle = None;
            guard.lifecycle.flex_session.transition(
                LifecycleState::Reconnecting,
                "Flex session ended; reconnect pending",
            );
            guard
                .lifecycle
                .amplifier
                .transition(LifecycleState::Degraded, "Flex session ended");
        }
        sleep(backoff).await;
        backoff = (backoff * 2).min(settings.reconnect_max.max(settings.reconnect_initial));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PgxlStartupTriggerStrategy {
    Current,
    RapidSubOnly,
    ReannounceStatusOnly,
    ReannounceCreateStyleStatus,
    NoBurst,
}

impl PgxlStartupTriggerStrategy {
    fn from_config(value: &str) -> Self {
        match value {
            "rapid_sub_only" => Self::RapidSubOnly,
            "reannounce_status_only" => Self::ReannounceStatusOnly,
            "reannounce_create_style_status" => Self::ReannounceCreateStyleStatus,
            "no_burst" => Self::NoBurst,
            _ => Self::Current,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::RapidSubOnly => "rapid_sub_only",
            Self::ReannounceStatusOnly => "reannounce_status_only",
            Self::ReannounceCreateStyleStatus => "reannounce_create_style_status",
            Self::NoBurst => "no_burst",
        }
    }

    fn max_count(self) -> u8 {
        match self {
            Self::NoBurst => 0,
            Self::RapidSubOnly => 20,
            _ => 10,
        }
    }

    fn interval(self) -> Duration {
        match self {
            Self::RapidSubOnly => Duration::from_millis(250),
            _ => Duration::from_secs(1),
        }
    }

    fn sends_sub_amplifier_all(self) -> bool {
        matches!(self, Self::Current | Self::RapidSubOnly)
    }

    fn logs_status_reannounce(self) -> bool {
        !matches!(self, Self::NoBurst | Self::RapidSubOnly)
    }
}

fn arm_startup_burst(
    timer: &mut std::pin::Pin<Box<tokio::time::Sleep>>,
    strategy: PgxlStartupTriggerStrategy,
) -> bool {
    if strategy == PgxlStartupTriggerStrategy::NoBurst {
        timer
            .as_mut()
            .reset(tokio::time::Instant::now() + Duration::from_secs(86_400));
        return false;
    }
    timer
        .as_mut()
        .reset(tokio::time::Instant::now() + Duration::from_millis(250));
    true
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
        guard.flex_injection.last_error = None;
        guard.flex_injection.tcp_connect_success_count = guard
            .flex_injection
            .tcp_connect_success_count
            .saturating_add(1);
        guard.flex_injection.last_command = None;
        guard.flex_injection.last_response = None;
        guard.flex_injection.client_handle_received = false;
        guard.flex_injection.amplifier_create_sent = false;
        guard.flex_injection.amplifier_create_accepted = false;
        guard.flex_injection.post_amplifier_registration_sent = false;
        guard.flex_injection.registration_continued_without_handle = false;
        guard.flex_injection.keepalive_enable_accepted = false;
        guard.flex_injection.sub_amplifier_all_accepted = false;
        guard.flex_injection.interlock_disabled_for_test = settings.disable_amp_interlock;
        if settings.disable_amp_interlock {
            guard.flex_injection.last_interlock_reason =
                Some("INTERLOCK_DISABLED_FOR_TEST".to_string());
        }
        guard.flex_injection.meter_publish_supported = Some(false);
        guard.flex_injection.meter_publish_last_result = Some(
            "Flex AMP meter objects are created, but no verified client-side meter value publication command is implemented".to_string(),
        );
        guard.lifecycle.flex_session.transition(
            LifecycleState::Connecting,
            "Flex TCP connected; waiting for handle",
        );
    }

    let mut amplifier_create: Option<PendingCommand> = None;
    let mut amplifier_create_sent = false;
    let mut post_amplifier_registration_sent = false;
    let mut next_seq = 1_u32;
    let mut ping_timer = Box::pin(sleep(settings.ping_interval.min(Duration::from_secs(2))));
    let mut tuner_refresh_timer = Box::pin(sleep(settings.tuner_refresh_interval));
    let mut amplifier_reannounce_timer = Box::pin(sleep(settings.amplifier_reannounce_interval));
    let mut amplifier_startup_burst_timer = Box::pin(sleep(Duration::from_secs(86_400)));
    let mut amplifier_startup_burst_count = 0_u8;
    let mut amplifier_startup_burst_active = false;
    let startup_trigger =
        PgxlStartupTriggerStrategy::from_config(settings.pgxl_startup_trigger_strategy.as_str());
    let mut pgxl_connect_assist_retry_timer = Box::pin(sleep(Duration::from_secs(30)));
    let mut post_registration_fallback_timer = Box::pin(sleep(Duration::from_secs(86_400)));

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
            session.remember_recent_line(format!("RX {line}"));
            if let Some(handle) = session.handle.clone() {
                let mut guard = state.write().await;
                guard.flex_injection.client_handle = Some(handle);
                guard.flex_injection.client_handle_received = true;
                guard
                    .lifecycle
                    .flex_session
                    .transition(LifecycleState::Active, "Flex client handle received");
            }
            trace_flex_rx(&line);
            {
                let mut guard = state.write().await;
                guard.flex_injection.last_rx_line = Some(line.clone());
            }
            if let Some(status) = parse_slice_status(&line) {
                update_radio_context_from_slice(&state, &status).await;
            }
            if let Some(status) = parse_transmit_status(&line) {
                update_radio_context_from_transmit(&state, &status).await;
            }
            if let Some(status) = parse_radio_status(&line) {
                update_radio_context_from_radio(&state, &status).await;
            }
            if let Some(status) = parse_interlock_status(&line) {
                observe_interlock_status(&state, &status).await;
            }

            if session.has_handle && !amplifier_create_sent {
                wait_for_kpa_first_poll_if_needed(settings, &state).await;
                let item = amplifier_create
                    .get_or_insert(pending_amplifier_create_command_with_state(settings, &state).await);
                {
                    let mut guard = state.write().await;
                    guard
                        .lifecycle
                        .amplifier
                        .transition(LifecycleState::ObjectCreated, "sending one-shot Flex amplifier registration");
                }
                validate_amplifier_create_for_profile(
                    &settings.amplifier_status_profile,
                    &item.command,
                )
                .map_err(anyhow::Error::msg)?;
                info!(
                    profile = %settings.amplifier_status_profile,
                    create_line = %item.command,
                    "Flex amplifier create profile selected"
                );
                trace_amplifier_advertisement(
                    settings,
                    &state,
                    "amplifier_create",
                    "registration",
                    &item.command,
                )
                .await;
                send_tracked_command(
                    &mut writer,
                    &mut session,
                    &state,
                    &mut next_seq,
                    item.clone(),
                )
                .await?;
                amplifier_create_sent = true;
                {
                    let mut guard = state.write().await;
                    guard
                        .lifecycle
                        .flex_session
                        .transition(LifecycleState::ObjectCreated, "amplifier create sent; waiting for amplifier status");
                    guard
                        .lifecycle
                        .amplifier
                        .transition(LifecycleState::ObjectAdvertised, "amplifier create sent");
                }
            }

            if let Some((seq, code, body)) = parse_response(&line) {
                if settings.flex_force_operate_via_radio {
                    append_flex_operate_lab_line(format!("RX_RESPONSE R{seq}|{code}|{body}"));
                }
                let observed_response = session
                    .observe_response(settings, &state, seq, &code, &body)
                    .await;
                if matches!(
                    observed_response,
                    Some((PendingKind::AmplifierCreate, true))
                ) && !post_amplifier_registration_sent
                    && session.amplifier_handle.is_none()
                {
                    post_registration_fallback_timer.as_mut().reset(
                        tokio::time::Instant::now() + Duration::from_millis(2_000),
                    );
                    append_evidence_line(
                        "flex-registration-health.md",
                        "Amplifier create accepted; waiting briefly for amplifier status/handle before post-registration fallback.",
                    );
                }
                if amplifier_create_sent
                    && !post_amplifier_registration_sent
                    && session.amplifier_handle.is_some()
                {
                    send_post_amplifier_registration_commands(
                        settings,
                        &mut writer,
                        &mut session,
                        &state,
                        &mut next_seq,
                    )
                    .await?;
                    post_amplifier_registration_sent = true;
                    amplifier_startup_burst_count = 0;
                    amplifier_startup_burst_active =
                        arm_startup_burst(&mut amplifier_startup_burst_timer, startup_trigger);
                    amplifier_reannounce_timer.as_mut().reset(
                        tokio::time::Instant::now() + settings.amplifier_reannounce_interval,
                    );
                }
            }

            if let Some(status) = parse_amplifier_status(&line) {
                observe_tuner_presence(&state, &status).await;
                if session.observe_amplifier_status(settings, &status) {
                    record_amplifier_pairing_status(
                        &state,
                        status.raw.clone(),
                        status.kvs.iter().map(|(key, _)| key.clone()).collect(),
                    )
                    .await;
                    if status.is_removed() {
                        record_amplifier_removed(&state, &status.handle, &session).await;
                        anyhow::bail!(
                            "Flex removed amplifier object {}; registration halted",
                            status.handle
                        );
                    }
                    set_amplifier_handle(&state, &status.handle).await;
                    if !post_amplifier_registration_sent {
                        send_post_amplifier_registration_commands(
                            settings,
                            &mut writer,
                            &mut session,
                            &state,
                            &mut next_seq,
                        )
                        .await?;
                        post_amplifier_registration_sent = true;
                        amplifier_startup_burst_count = 0;
                        amplifier_startup_burst_active =
                            arm_startup_burst(&mut amplifier_startup_burst_timer, startup_trigger);
                        amplifier_reannounce_timer.as_mut().reset(
                            tokio::time::Instant::now() + settings.amplifier_reannounce_interval,
                        );
                    }
                    if settings.flex_force_operate_via_radio {
                        append_flex_operate_lab_line(format!("RX_STATUS {}", status.raw));
                        send_flex_operate_lab_command(
                            settings,
                            &mut writer,
                            &mut session,
                            &state,
                            &mut next_seq,
                            &status.handle,
                        )
                        .await?;
                    }
                    if settings.pgxl_connect_assist {
                        append_flex_operate_lab_line(format!(
                            "CONNECT_ASSIST_RX_STATUS {}",
                            status.raw
                        ));
                        send_pgxl_connect_assist_command(
                            settings,
                            &mut writer,
                            &mut session,
                            &state,
                            &mut next_seq,
                            &status.handle,
                        )
                        .await?;
                    }
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
            if post_amplifier_registration_sent {
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
        () = &mut post_registration_fallback_timer => {
            if amplifier_create_sent
                && !post_amplifier_registration_sent
                && session.amplifier_handle.is_none()
            {
                warn!(
                    event_id = "flex_registration_serial_fallback",
                    "amplifier create accepted but no amplifier status handle received; continuing registration using serial/name fallback"
                );
                append_evidence_line(
                    "flex-registration-health.md",
                    "amplifier create accepted but no amplifier status handle received; continuing registration using serial/name fallback",
                );
                {
                    let mut guard = state.write().await;
                    guard.flex_injection.registration_continued_without_handle = true;
                    guard.flex_injection.degraded_reason = None;
                    guard
                        .lifecycle
                        .flex_session
                        .transition(LifecycleState::Subscribed, "post-amplifier registration fallback after create accepted");
                }
                send_post_amplifier_registration_commands(
                    settings,
                    &mut writer,
                    &mut session,
                    &state,
                    &mut next_seq,
                )
                .await?;
                post_amplifier_registration_sent = true;
                amplifier_startup_burst_count = 0;
                amplifier_startup_burst_active =
                    arm_startup_burst(&mut amplifier_startup_burst_timer, startup_trigger);
                amplifier_reannounce_timer.as_mut().reset(
                    tokio::time::Instant::now() + settings.amplifier_reannounce_interval,
                );
            }
            post_registration_fallback_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(86_400));
        }
        () = &mut amplifier_startup_burst_timer => {
            let pgxl_connected = {
                let guard = state.read().await;
                guard.clients.pgxl_session_started_count > 0
            };
            if post_amplifier_registration_sent
                && !pgxl_connected
                && amplifier_startup_burst_count < startup_trigger.max_count()
            {
                amplifier_startup_burst_count = amplifier_startup_burst_count.saturating_add(1);
                if startup_trigger.sends_sub_amplifier_all() {
                    send_tracked_command(
                        &mut writer,
                        &mut session,
                        &state,
                        &mut next_seq,
                        PendingCommand::new(
                            "amplifier_startup_burst_refresh",
                            "sub amplifier all",
                            PendingKind::AmplifierReannounce,
                        ),
                    )
                    .await?;
                }
                if startup_trigger.logs_status_reannounce() {
                    let line = if startup_trigger == PgxlStartupTriggerStrategy::ReannounceCreateStyleStatus {
                        settings.amplifier_create_command()
                    } else {
                        synthetic_amplifier_status_line(
                            settings,
                            &state,
                            session.amplifier_handle.as_deref(),
                        )
                        .await
                    };
                    trace_amplifier_advertisement(
                        settings,
                        &state,
                        "amplifier_status",
                        startup_trigger.as_str(),
                        &line,
                    )
                    .await;
                    append_flex_log_line("amplifier-status-lines.log", &line);
                    append_evidence_line(
                        "amplifier-reannounce.log",
                        format!(
                            "startup_burst#{} strategy={} {line}",
                            amplifier_startup_burst_count,
                            startup_trigger.as_str()
                        ),
                    );
                    append_evidence_line("amplifier-status-lines.log", line);
                }
                {
                    let mut guard = state.write().await;
                    guard.flex_injection.amplifier_reannounce_count =
                        guard.flex_injection.amplifier_reannounce_count.saturating_add(1);
                    guard.flex_injection.amplifier_direct_connect_expected =
                        Some(!settings.amplifier_ip.is_loopback());
                    guard.flex_injection.last_amplifier_reannounce_reason =
                        Some(format!(
                            "startup_burst_{}_{}",
                            startup_trigger.as_str(),
                            amplifier_startup_burst_count
                        ));
                    guard.flex_injection.amplifier_pgxl_tcp_attempted_after_status =
                        guard.clients.pgxl_session_started_count > 0;
                }
                info!(
                    event_id = "amplifier_startup_burst_refresh",
                    burst_count = amplifier_startup_burst_count,
                    strategy = startup_trigger.as_str(),
                    "Flex amplifier startup burst refresh query sent"
                );
                amplifier_startup_burst_timer
                    .as_mut()
                    .reset(tokio::time::Instant::now() + startup_trigger.interval());
            } else {
                amplifier_startup_burst_active = false;
                amplifier_startup_burst_timer
                    .as_mut()
                    .reset(tokio::time::Instant::now() + Duration::from_secs(86_400));
                amplifier_reannounce_timer.as_mut().reset(
                    tokio::time::Instant::now() + settings.amplifier_reannounce_interval,
                );
            }
        }
        () = &mut amplifier_reannounce_timer => {
            if post_amplifier_registration_sent && !amplifier_startup_burst_active {
                send_tracked_command(
                    &mut writer,
                    &mut session,
                    &state,
                    &mut next_seq,
                    PendingCommand::new(
                        "amplifier_reannounce_refresh",
                        "sub amplifier all",
                        PendingKind::AmplifierReannounce,
                    ),
                )
                .await?;
                let line = synthetic_amplifier_status_line(
                    settings,
                    &state,
                    session.amplifier_handle.as_deref(),
                )
                .await;
                trace_amplifier_advertisement(
                    settings,
                    &state,
                    "amplifier_status",
                    "periodic_reannounce",
                    &line,
                )
                .await;
                append_flex_log_line("amplifier-status-lines.log", &line);
                append_evidence_line("amplifier-reannounce.log", line.clone());
                append_evidence_line("amplifier-status-lines.log", line);
                {
                    let mut guard = state.write().await;
                    guard.flex_injection.amplifier_reannounce_count =
                        guard.flex_injection.amplifier_reannounce_count.saturating_add(1);
                    guard.flex_injection.amplifier_direct_connect_expected =
                        Some(!settings.amplifier_ip.is_loopback());
                    guard.flex_injection.last_amplifier_reannounce_reason =
                        Some("periodic_sub_amplifier_all".to_string());
                    guard.flex_injection.amplifier_pgxl_tcp_attempted_after_status =
                        guard.clients.pgxl_session_started_count > 0;
                }
                info!(
                    event_id = "amplifier_presence_refreshed",
                    profile = %settings.amplifier_status_profile,
                    "Flex amplifier presence refresh query sent"
                );
            }
            amplifier_reannounce_timer.as_mut().reset(tokio::time::Instant::now() + settings.amplifier_reannounce_interval);
        }
        () = &mut tuner_refresh_timer, if settings.tuner_presence_refresh => {
            if post_amplifier_registration_sent && !amplifier_startup_burst_active {
                send_tracked_command(
                    &mut writer,
                    &mut session,
                    &state,
                    &mut next_seq,
                    PendingCommand::new(
                        "tuner_presence_refresh",
                        "sub amplifier all",
                        PendingKind::TunerPresenceRefresh,
                    ),
                )
                .await?;
                {
                    let mut guard = state.write().await;
                    guard.flex_injection.tuner_registration_refresh_count =
                        guard.flex_injection.tuner_registration_refresh_count.saturating_add(1);
                    guard.flex_injection.tuner_reannounce_count =
                        guard.flex_injection.tuner_reannounce_count.saturating_add(1);
                }
                append_evidence_json(
                    "disconnect-events.jsonl",
                    &serde_json::json!({
                        "event": "tuner_presence_refreshed",
                        "source": "flex_injection",
                        "command": "sub amplifier all",
                    }),
                );
                info!(
                    event_id = "tuner_presence_refreshed",
                    "Flex tuner presence refresh query sent"
                );
            }
            tuner_refresh_timer.as_mut().reset(tokio::time::Instant::now() + settings.tuner_refresh_interval);
        }
        () = &mut pgxl_connect_assist_retry_timer, if settings.pgxl_connect_assist => {
            let should_retry = {
                let guard = state.read().await;
                post_amplifier_registration_sent
                    && session.amplifier_handle.is_some()
                    && guard.clients.pgxl_session_started_count == 0
            };
            if should_retry {
                session.assist_sent_handle = None;
                {
                    let mut guard = state.write().await;
                    guard.flex_injection.pgxl_connect_assist_retry_count =
                        guard.flex_injection.pgxl_connect_assist_retry_count.saturating_add(1);
                }
                send_tracked_command(
                    &mut writer,
                    &mut session,
                    &state,
                    &mut next_seq,
                    PendingCommand::new(
                        "pgxl_connect_assist_retry",
                        "sub amplifier all",
                        PendingKind::AmplifierReannounce,
                    ),
                )
                .await?;
                info!(
                    event_id = "pgxl_connect_assist_retry",
                    "PGXL connect-assist retry: reset assist guard and re-sent sub amplifier all"
                );
            }
            pgxl_connect_assist_retry_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(30));
                    }
                }
    }

    #[allow(unreachable_code)]
    Ok(())
}

async fn wait_for_kpa_first_poll_if_needed(settings: &FlexInjectionSettings, state: &SharedState) {
    if settings.amplifier_startup_state_policy != "wait_for_first_kpa_poll" {
        return;
    }
    let started = Instant::now();
    {
        let mut guard = state.write().await;
        guard.amp.advertisement_waiting_for_first_poll = true;
    }
    loop {
        let (ready, error) = {
            let guard = state.read().await;
            (
                guard.amp.first_poll_completed,
                guard
                    .amp
                    .first_poll_error
                    .clone()
                    .or_else(|| guard.amp.serial_port_open_error.clone()),
            )
        };
        if ready {
            let mut guard = state.write().await;
            guard.amp.advertisement_waiting_for_first_poll = false;
            append_evidence_line(
                "first-poll-sequence.log",
                "KPA500 first poll completed before amplifier advertisement",
            );
            return;
        }
        if started.elapsed() >= settings.wait_first_kpa_poll_timeout {
            let warning = format!(
                "kpa500_not_polling: no successful KPA500 first poll after {} ms; last_error={}",
                settings.wait_first_kpa_poll_timeout.as_millis(),
                error.as_deref().unwrap_or("none")
            );
            {
                let mut guard = state.write().await;
                guard.amp.advertisement_waiting_for_first_poll = false;
                guard.amp.first_poll_error = Some(warning.clone());
                guard.flex_injection.degraded_reason = Some(warning.clone());
            }
            append_evidence_line("warnings-errors.log", warning.clone());
            append_evidence_line("first-poll-sequence.log", warning.clone());
            append_evidence_json(
                "disconnect-events.jsonl",
                &serde_json::json!({
                    "event": "kpa500_not_polling",
                    "timeout_ms": settings.wait_first_kpa_poll_timeout.as_millis(),
                    "last_error": error,
                }),
            );
            warn!(
                event_id = "kpa500_not_polling",
                "KPA500 did not complete first poll before amplifier advertisement"
            );
            return;
        }
        sleep(Duration::from_millis(100)).await;
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
    let duplicate_label = !session.sent_labels.insert(pending.label.clone());
    if duplicate_label {
        let mut guard = state.write().await;
        match &pending.kind {
            PendingKind::AmplifierCreate => {
                guard.flex_injection.duplicate_amplifier_create_count = guard
                    .flex_injection
                    .duplicate_amplifier_create_count
                    .saturating_add(1);
                guard.flex_injection.amplifier_recreate_reason =
                    Some("duplicate amplifier create attempted in one Flex session".to_string());
            }
            PendingKind::MeterCreate { .. } => {
                guard.flex_injection.duplicate_meter_create_count = guard
                    .flex_injection
                    .duplicate_meter_create_count
                    .saturating_add(1);
            }
            PendingKind::InterlockCreate => {
                guard.flex_injection.duplicate_interlock_create_count = guard
                    .flex_injection
                    .duplicate_interlock_create_count
                    .saturating_add(1);
            }
            PendingKind::Subscription => {
                guard.flex_injection.duplicate_subscription_count = guard
                    .flex_injection
                    .duplicate_subscription_count
                    .saturating_add(1);
            }
            _ => {}
        }
        append_evidence_json(
            "lifecycle-events.jsonl",
            &serde_json::json!({
                "event": "duplicate_flex_command_label",
                "label": pending.label,
                "command": pending.command,
            }),
        );
    }
    send_command(writer, seq, &pending.command).await?;
    session.remember_recent_line(format!("TX C{seq}|{}", pending.command));
    if pending.kind == PendingKind::AmplifierCreate {
        append_flex_log_line(
            "amplifier-status-lines.log",
            &format!("TX C{seq}|{}", pending.command),
        );
        append_evidence_line(
            "amplifier-status-lines.log",
            format!("TX C{seq}|{}", pending.command),
        );
    }
    info!(
        seq,
        label = %pending.label,
        command = %pending.command,
        "Flex PGXL registration command sent"
    );
    session.pending.insert(seq, pending.clone());
    {
        let mut guard = state.write().await;
        guard.flex_injection.last_command = Some(pending.command.clone());
        guard.flex_injection.last_tx_line = Some(format!("C{seq}|{}", pending.command));
        if pending.command == "sub amplifier all" {
            guard.flex_injection.sub_amplifier_all_command_count = guard
                .flex_injection
                .sub_amplifier_all_command_count
                .saturating_add(1);
        }
        if pending.kind == PendingKind::AmplifierCreate {
            guard.flex_injection.amplifier_create_sent = true;
            guard.flex_injection.amplifier_create_count = guard
                .flex_injection
                .amplifier_create_count
                .saturating_add(1);
        }
        guard.flex_injection.pending_count = session.pending.len();
    }
    *next_seq = next_seq.saturating_add(1);
    Ok(())
}

async fn send_post_amplifier_registration_commands(
    settings: &FlexInjectionSettings,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    session: &mut FlexSession,
    state: &SharedState,
    next_seq: &mut u32,
) -> Result<()> {
    info!(
        amplifier_handle = session.amplifier_handle.as_deref().unwrap_or("unknown"),
        "Flex amplifier handle observed; sending meters, interlock, keepalive, and subscriptions"
    );
    append_evidence_line(
        "flex-registration-health.md",
        format!(
            "Post-amplifier registration started after amplifier_handle={}",
            session.amplifier_handle.as_deref().unwrap_or("unknown")
        ),
    );
    for item in post_amplifier_registration_commands(settings) {
        send_tracked_command(writer, session, state, next_seq, item).await?;
    }
    {
        let mut guard = state.write().await;
        guard.flex_injection.post_amplifier_registration_sent = true;
        let reason = if session.amplifier_handle.is_some() {
            "post-amplifier registration commands sent after amplifier handle observed"
        } else {
            "post-amplifier registration commands sent after create-accepted fallback"
        };
        guard
            .lifecycle
            .flex_session
            .transition(LifecycleState::Subscribed, reason);
    }
    Ok(())
}

async fn send_flex_operate_lab_command(
    settings: &FlexInjectionSettings,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    session: &mut FlexSession,
    state: &SharedState,
    next_seq: &mut u32,
    handle: &str,
) -> Result<()> {
    if !settings.flex_force_operate_via_radio || session.operate_lab_sent {
        return Ok(());
    }
    session.operate_lab_sent = true;
    let command = format!("amplifier set {handle} operate=1");
    append_flex_operate_lab_line(format!("TX_PENDING {command}"));
    {
        let mut guard = state.write().await;
        guard.flex_injection.flex_force_operate_via_radio = true;
        guard.flex_injection.flex_desired_amp_state = Some("OPERATE".to_string());
        guard.flex_injection.flex_operate_lab_command_count = guard
            .flex_injection
            .flex_operate_lab_command_count
            .saturating_add(1);
    }
    send_tracked_command(
        writer,
        session,
        state,
        next_seq,
        PendingCommand::new(
            "amplifier_operate_lab",
            command,
            PendingKind::AmplifierOperateLab,
        ),
    )
    .await
}

async fn send_pgxl_connect_assist_command(
    settings: &FlexInjectionSettings,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    session: &mut FlexSession,
    state: &SharedState,
    next_seq: &mut u32,
    handle: &str,
) -> Result<()> {
    if !settings.pgxl_connect_assist || session.assist_sent_handle.as_deref() == Some(handle) {
        return Ok(());
    }
    session.assist_sent_handle = Some(handle.to_string());
    let command = format!("amplifier set {handle} operate=1");
    append_flex_operate_lab_line(format!("CONNECT_ASSIST_TX_PENDING {command}"));
    {
        let mut guard = state.write().await;
        guard.flex_injection.pgxl_connect_assist_enabled = true;
        guard.flex_injection.flex_desired_amp_state =
            Some("OPERATE_FOR_PGXL_CONNECT_ASSIST".to_string());
        guard.flex_injection.pgxl_connect_assist_sent_count = guard
            .flex_injection
            .pgxl_connect_assist_sent_count
            .saturating_add(1);
        guard.flex_injection.pgxl_connect_assist_last_result = Some("sent".to_string());
    }
    send_tracked_command(
        writer,
        session,
        state,
        next_seq,
        PendingCommand::new(
            "pgxl_connect_assist",
            command,
            PendingKind::PgxlConnectAssist,
        ),
    )
    .await
}

#[derive(Debug, Default)]
struct FlexSession {
    has_handle: bool,
    handle: Option<String>,
    version: Option<String>,
    amplifier_handle: Option<String>,
    operate_lab_sent: bool,
    assist_sent_handle: Option<String>,
    pending: HashMap<u32, PendingCommand>,
    sent_labels: HashSet<String>,
    recent_lines: VecDeque<String>,
}

impl FlexSession {
    fn remember_recent_line(&mut self, line: impl Into<String>) {
        if self.recent_lines.len() >= 50 {
            self.recent_lines.pop_front();
        }
        self.recent_lines.push_back(line.into());
    }

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
    ) -> Option<(PendingKind, bool)> {
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

        let pending = pending?;
        let observed = Some((pending.kind.clone(), code == "0"));
        if code != "0" {
            if matches!(pending.kind, PendingKind::PgxlConnectAssist) {
                let mut guard = state.write().await;
                guard.flex_injection.pgxl_connect_assist_last_result =
                    Some(format!("rejected:{code}:{body}"));
            }
            if matches!(pending.kind, PendingKind::Ping) {
                let mut guard = state.write().await;
                guard.flex_injection.ping_failure_count =
                    guard.flex_injection.ping_failure_count.saturating_add(1);
                guard.flex_injection.degraded_reason =
                    Some(format!("Flex ping rejected: {code} {body}"));
            }
            return observed;
        }
        match pending.kind {
            PendingKind::AmplifierCreate => {
                {
                    let mut guard = state.write().await;
                    guard.flex_injection.amplifier_create_accepted = true;
                    guard.lifecycle.amplifier.transition(
                        LifecycleState::ObjectAccepted,
                        "Flex accepted amplifier create",
                    );
                }
                if let Some(handle) = response_object_id(body) {
                    self.amplifier_handle = Some(handle.to_string());
                    set_amplifier_handle(state, handle).await;
                }
            }
            PendingKind::AmplifierOperateLab => {
                let mut guard = state.write().await;
                guard.flex_injection.flex_operate_lab_accept_count = guard
                    .flex_injection
                    .flex_operate_lab_accept_count
                    .saturating_add(1);
            }
            PendingKind::PgxlConnectAssist => {
                let mut guard = state.write().await;
                guard.flex_injection.pgxl_connect_assist_last_result = Some("accepted".to_string());
                guard.flex_injection.pgxl_connect_assist_triggered_tcp =
                    guard.clients.pgxl_session_started_count > 0;
            }
            PendingKind::MeterCreate { name } => {
                if let Some(handle) = response_object_id(body) {
                    upsert_meter_handle(state, name, handle).await;
                }
                let mut guard = state.write().await;
                guard.flex_injection.meter_publish_supported = Some(false);
                guard.flex_injection.meter_publish_last_result = Some(
                    "meter create accepted; live value publication command remains unverified"
                        .to_string(),
                );
            }
            PendingKind::InterlockCreate => {
                if let Some(handle) = response_object_id(body) {
                    let mut guard = state.write().await;
                    guard.flex_injection.interlock_handle = Some(handle.to_string());
                }
            }
            PendingKind::Subscription => {
                if pending.command.contains("sub amplifier all") {
                    let mut guard = state.write().await;
                    guard.flex_injection.sub_amplifier_all_accepted = true;
                    guard
                        .lifecycle
                        .flex_session
                        .transition(LifecycleState::Subscribed, "sub amplifier all accepted");
                }
            }
            PendingKind::KeepaliveEnable => {
                let mut guard = state.write().await;
                guard.flex_injection.keepalive_enable_accepted = true;
            }
            PendingKind::AmplifierReannounce | PendingKind::TunerPresenceRefresh => {}
            PendingKind::Ping => {
                let mut guard = state.write().await;
                if code == "0" {
                    guard.flex_injection.degraded_reason = None;
                    guard.flex_injection.connection_state = ConnectionState::Connected;
                    guard.flex_injection.ping_ack_count =
                        guard.flex_injection.ping_ack_count.saturating_add(1);
                    guard.flex_injection.last_ping_latency_ms =
                        Some(duration_millis_u64(pending.created_at.elapsed()));
                } else {
                    guard.flex_injection.ping_failure_count =
                        guard.flex_injection.ping_failure_count.saturating_add(1);
                    guard.flex_injection.degraded_reason =
                        Some(format!("Flex ping rejected: {code} {body}"));
                }
            }
        }
        observed
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
            if status.is_removed() {
                append_flex_log_line(
                    "amplifier-status-lines.log",
                    &format!("REMOVED {}", status.raw),
                );
                if self.amplifier_handle.as_deref() == Some(status.handle.as_str()) {
                    self.amplifier_handle = None;
                    info!(
                        amplifier_handle = %status.handle,
                        "Flex amplifier object removal observed"
                    );
                }
                return true;
            }
            if self.amplifier_handle.as_deref() != Some(status.handle.as_str()) {
                self.amplifier_handle = Some(status.handle.clone());
                info!(
                    amplifier_handle = %status.handle,
                    model = status.value("model").unwrap_or(""),
                    serial = status.value("serial_num").unwrap_or(""),
                    "Flex amplifier object handle observed"
                );
            }
            append_flex_log_line("amplifier-status-lines.log", &status.raw);
            true
        } else {
            false
        }
    }
}

async fn set_amplifier_handle(state: &SharedState, handle: &str) {
    let mut guard = state.write().await;
    if guard.flex_injection.amplifier_handle.as_deref() != Some(handle) {
        guard.flex_injection.amplifier_handle_change_count = guard
            .flex_injection
            .amplifier_handle_change_count
            .saturating_add(1);
    }
    guard.flex_injection.amplifier_handle = Some(handle.to_string());
    guard.flex_injection.amplifier_last_seen_at = Some(SystemTime::now());
    guard.flex_injection.amplifier_object_seen_at_ms = Some(timestamp_millis());
    guard.lifecycle.amplifier.transition(
        LifecycleState::Active,
        format!("Flex amplifier handle observed: {handle}"),
    );
    append_evidence_json(
        "lifecycle-events.jsonl",
        &serde_json::json!({
            "event": "amplifier_handle_observed",
            "handle": handle,
        }),
    );
}

async fn record_amplifier_pairing_status(
    state: &SharedState,
    line: String,
    candidate_fields: Vec<String>,
) {
    let mut guard = state.write().await;
    let observed_state = line
        .split_whitespace()
        .find_map(|token| token.strip_prefix("state="))
        .map(str::to_string);
    guard.flex_injection.last_amplifier_status_line = Some(line);
    guard
        .flex_injection
        .amplifier_object_seen_at_ms
        .get_or_insert_with(timestamp_millis);
    if let Some(observed_state) = observed_state {
        if guard.flex_injection.flex_desired_amp_state.as_deref() == Some("OPERATE")
            && observed_state == "STANDBY"
        {
            guard.flex_injection.radio_rewritten_amp_state = Some("STANDBY".to_string());
        }
    }
    guard.flex_injection.amplifier_pairing_candidate_fields = candidate_fields;
    guard
        .flex_injection
        .amplifier_pgxl_tcp_attempted_after_status = guard.clients.pgxl_session_started_count > 0;
    if guard.flex_injection.pgxl_connect_assist_enabled {
        guard.flex_injection.pgxl_connect_assist_triggered_tcp =
            guard.clients.pgxl_session_started_count > 0;
    }
}

async fn record_amplifier_candidate_fields(state: &SharedState, candidate_fields: Vec<String>) {
    let mut guard = state.write().await;
    guard.flex_injection.amplifier_pairing_candidate_fields = candidate_fields;
    guard
        .flex_injection
        .amplifier_pgxl_tcp_attempted_after_status = guard.clients.pgxl_session_started_count > 0;
}

async fn observe_interlock_status(state: &SharedState, status: &KeyValueStatus) {
    let reason = status.value("reason").unwrap_or_default();
    let amplifier = status.value("amplifier").unwrap_or_default();
    let interlock_state = status.value("state").unwrap_or_default();
    let tx_allowed = status
        .value("tx_allowed")
        .map(|value| matches!(value, "1" | "true" | "True" | "TRUE"));
    let mut guard = state.write().await;
    guard.flex_injection.last_interlock_status_line = Some(status.raw.clone());
    guard.flex_injection.last_interlock_state = if interlock_state.is_empty() {
        None
    } else {
        Some(interlock_state.to_string())
    };
    guard.flex_injection.last_interlock_reason = if reason.is_empty() {
        None
    } else {
        Some(reason.to_string())
    };
    guard.flex_injection.last_interlock_tx_allowed = tx_allowed;
    if reason == "AMP:PG-XL" && amplifier.is_empty() {
        guard.flex_injection.interlock_amplifier_field_empty = true;
        guard.flex_injection.interlock_empty_amplifier_count = guard
            .flex_injection
            .interlock_empty_amplifier_count
            .saturating_add(1);
        if tx_allowed == Some(false) {
            guard.flex_injection.interlock_blocked_count = guard
                .flex_injection
                .interlock_blocked_count
                .saturating_add(1);
            guard.flex_injection.degraded_reason = Some(
                "INTERLOCK_BLOCKED: Flex interlock reports AMP:PG-XL with tx_allowed=0".to_string(),
            );
            warn!(
                event_id = "flex_interlock_blocked",
                raw = %status.raw,
                "Flex interlock blocks TX for AMP:PG-XL"
            );
        } else {
            if guard
                .flex_injection
                .degraded_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("INTERLOCK_BLOCKED"))
            {
                guard.flex_injection.degraded_reason = None;
            }
            warn!(
                event_id = "flex_interlock_empty_amplifier_warn",
                raw = %status.raw,
                "Flex interlock has empty amplifier field but TX is allowed; treating as warning"
            );
        }
        append_evidence_json(
            "lifecycle-events.jsonl",
            &serde_json::json!({
                "event": "flex_interlock_empty_amplifier",
                "raw": status.raw,
                "state": interlock_state,
                "reason": reason,
                "tx_allowed": tx_allowed,
            }),
        );
    } else if !amplifier.is_empty() {
        guard.flex_injection.interlock_amplifier_field_empty = false;
    }
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

async fn record_amplifier_removed(state: &SharedState, handle: &str, session: &FlexSession) {
    let mut guard = state.write().await;
    let previous_state = guard.flex_injection.last_advertised_flex_amp_state.clone();
    let pgxl_client_count = guard.clients.pgxl_client_count;
    let pgxl_started = guard.clients.pgxl_session_started_count;
    let last_command = guard.flex_injection.last_command.clone();
    let last_response = guard.flex_injection.last_response.clone();
    let last_rx = guard.flex_injection.last_rx_line.clone();
    let last_tx = guard.flex_injection.last_tx_line.clone();
    let last_emitted = guard
        .flex_injection
        .last_emitted_amplifier_advertisement_line
        .clone();
    let last_status = guard.flex_injection.last_amplifier_status_line.clone();
    let last_pgxl_state = guard.flex_injection.last_advertised_pgxl_state.clone();
    let kpa_snapshot = serde_json::json!({
        "connection_state": guard.amp.connection_state.as_str(),
        "poll_success_count": guard.amp.runtime.poll_success_count,
        "first_poll_completed": guard.amp.first_poll_completed,
        "state": guard.amp.state.pgxl_state(),
        "operate": guard.amp.operate,
        "temperature_c": guard.amp.temperature_c,
        "pa_voltage_volts": guard.amp.pa_voltage_volts,
        "pa_current_amps": guard.amp.pa_current_amps,
        "forward_power_watts": guard.amp.forward_power_watts,
        "swr": guard.amp.swr,
        "fault": guard.amp.fault,
        "last_raw_response": guard.amp.last_raw_response,
        "last_successful_command": guard.amp.last_successful_command,
    });
    let object_snapshot = serde_json::json!({
        "amplifier_create_count": guard.flex_injection.amplifier_create_count,
        "duplicate_amplifier_create_count": guard.flex_injection.duplicate_amplifier_create_count,
        "duplicate_meter_create_count": guard.flex_injection.duplicate_meter_create_count,
        "duplicate_interlock_create_count": guard.flex_injection.duplicate_interlock_create_count,
        "duplicate_subscription_count": guard.flex_injection.duplicate_subscription_count,
        "amplifier_create_accepted": guard.flex_injection.amplifier_create_accepted,
        "sub_amplifier_all_accepted": guard.flex_injection.sub_amplifier_all_accepted,
        "meter_handles": guard.flex_injection.meter_handles,
        "interlock_handle": guard.flex_injection.interlock_handle,
    });
    let recent_lines = session.recent_lines.iter().cloned().collect::<Vec<_>>();
    guard.flex_injection.amplifier_removed_count = guard
        .flex_injection
        .amplifier_removed_count
        .saturating_add(1);
    guard.flex_injection.last_amplifier_removed_reason =
        Some(format!("Flex status reported amplifier {handle} removed"));
    if guard.flex_injection.amplifier_handle.as_deref() == Some(handle) {
        guard.flex_injection.amplifier_handle = None;
    }
    guard.flex_injection.amp_widget_visibility_risk =
        Some(format!("Flex removed amplifier handle {handle}"));
    guard.lifecycle.amplifier.transition(
        LifecycleState::Removed,
        format!("Flex reported amplifier {handle} removed"),
    );
    guard.lifecycle.pgxl.transition(
        LifecycleState::Degraded,
        "Flex amplifier object was removed",
    );
    append_evidence_json(
        "disconnect-events.jsonl",
        &serde_json::json!({
            "event": "flex_amplifier_removed",
            "handle": handle,
            "count": guard.flex_injection.amplifier_removed_count,
            "previous_advertised_state": previous_state,
            "pgxl_client_count": pgxl_client_count,
            "pgxl_session_started_count": pgxl_started,
            "last_flex_command": last_command,
            "last_flex_response": last_response,
            "last_flex_rx": last_rx,
            "last_flex_tx": last_tx,
            "last_emitted_amplifier_line": last_emitted,
            "last_radio_amplifier_status_line": last_status,
            "last_pgxl_advertised_state": last_pgxl_state,
            "kpa_snapshot": kpa_snapshot,
            "flex_object_snapshot": object_snapshot,
            "recent_flex_lines": recent_lines,
        }),
    );
    append_evidence_line(
        "amplifier-removal-timeline.md",
        format!(
            "- Flex reported amplifier `{handle}` removed. previous_state={:?} pgxl_clients={} pgxl_sessions={} last_command={:?} last_response={:?}",
            previous_state, pgxl_client_count, pgxl_started, last_command, last_response
        ),
    );
    append_evidence_line(
        "amplifier-removed-live-root-cause.md",
        format!(
            "# Amplifier Removed Live Root Cause\n\nObserved `amplifier {handle} removed` from Flex.\n\n\
## Last Flex Lines\n\n{}\n\n\
## Last Advertisements\n\n- Last emitted amplifier line: {:?}\n- Last radio amplifier status line: {:?}\n- Last PGXL advertised state: {:?}\n\n\
## KPA Snapshot\n\n```json\n{}\n```\n\n\
## Flex Object Snapshot\n\n```json\n{}\n```\n\n\
## Interpretation\n\nThis is a Flex-side object lifecycle failure. The run is not operationally ready until the preceding Flex RX/TX sequence shows whether the radio removed the object because of duplicate registration, rejected lifecycle commands, keepalive loss, interlock/meter rejection, or invalid amplifier identity fields.\n",
            recent_lines.join("\n"),
            last_emitted,
            last_status,
            last_pgxl_state,
            serde_json::to_string_pretty(&kpa_snapshot).unwrap_or_else(|_| "{}".to_string()),
            serde_json::to_string_pretty(&object_snapshot).unwrap_or_else(|_| "{}".to_string()),
        ),
    );
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
    if settings.flex_force_operate_via_radio {
        return Ok(());
    }
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
        {
            let mut guard = state.write().await;
            guard.controls.aethersdr_button_command_seen = true;
            guard.controls.control_requested_count =
                guard.controls.control_requested_count.saturating_add(1);
            guard.controls.last_flex_amp_set_command = Some(status.raw.clone());
            guard.controls.last_mapped_elecraft_action = Some("KPA500 ^OS1;".to_string());
            guard.controls.last_safety_decision =
                Some("blocked_by_rf_risk: allow_rf_risk=false".to_string());
            guard.controls.blocked_by_rf_risk_count =
                guard.controls.blocked_by_rf_risk_count.saturating_add(1);
        }
        append_evidence_json(
            "control-events.jsonl",
            &serde_json::json!({
                "protocol": "Flex",
                "raw": status.raw,
                "mapped_action": "KPA500 ^OS1;",
                "safety_decision": "blocked_by_rf_risk",
            }),
        );
        append_evidence_line(
            "flex-control-commands.log",
            format!("RX {} -> KPA500 ^OS1; blocked_by_rf_risk", status.raw),
        );
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
        guard.controls.aethersdr_button_command_seen = true;
        guard.controls.control_requested_count =
            guard.controls.control_requested_count.saturating_add(1);
        guard.controls.last_flex_amp_set_command = Some(status.raw.clone());
        guard.controls.last_mapped_elecraft_action = Some(if operate {
            "KPA500 ^OS1;".to_string()
        } else {
            "KPA500 ^OS0;".to_string()
        });
        guard.controls.last_safety_decision = Some("accepted_desired_state".to_string());
        if guard.desired.amp_operate != Some(operate) {
            guard.desired.amp_operate = Some(operate);
        }
    }
    append_evidence_json(
        "control-events.jsonl",
        &serde_json::json!({
            "protocol": "Flex",
            "raw": status.raw,
            "requested_operate": operate,
            "mapped_action": if operate { "KPA500 ^OS1;" } else { "KPA500 ^OS0;" },
            "safety_decision": "accepted_desired_state",
        }),
    );
    append_evidence_line(
        "flex-control-commands.log",
        format!(
            "RX {} -> {} accepted_desired_state",
            status.raw,
            if operate {
                "KPA500 ^OS1;"
            } else {
                "KPA500 ^OS0;"
            }
        ),
    );
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingKind {
    AmplifierCreate,
    MeterCreate { name: &'static str },
    InterlockCreate,
    KeepaliveEnable,
    Subscription,
    AmplifierReannounce,
    AmplifierOperateLab,
    PgxlConnectAssist,
    TunerPresenceRefresh,
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

    fn is_removed(&self) -> bool {
        self.raw
            .split('|')
            .nth(1)
            .is_some_and(|body| body.split_whitespace().any(|token| token == "removed"))
    }

    fn requested_operate(&self) -> Option<bool> {
        if let Some(value) = self.value("operate") {
            return match value {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            };
        }
        None
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
    let body = status_body(line)?;
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

fn status_body(line: &str) -> Option<&str> {
    if !line.starts_with('S') {
        return None;
    }
    Some(line.split_once('|')?.1)
}

#[derive(Debug, Clone)]
struct KeyValueStatus {
    raw: String,
    handle: Option<String>,
    kvs: Vec<(String, String)>,
}

impl KeyValueStatus {
    fn value(&self, key: &str) -> Option<&str> {
        self.kvs
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }
}

fn parse_kv_status(line: &str, object: &str) -> Option<KeyValueStatus> {
    let body = status_body(line)?;
    let rest = body.strip_prefix(object)?.trim_start();
    let mut parts = rest.split_whitespace();
    let first = parts.next();
    let (handle, tokens): (Option<String>, Vec<&str>) =
        if first.is_some_and(|part| part.contains('=')) {
            (None, std::iter::once(first.unwrap()).chain(parts).collect())
        } else {
            (first.map(str::to_string), parts.collect())
        };
    let kvs = tokens
        .into_iter()
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect::<Vec<_>>();
    Some(KeyValueStatus {
        raw: line.to_string(),
        handle,
        kvs,
    })
}

fn parse_slice_status(line: &str) -> Option<KeyValueStatus> {
    parse_kv_status(line, "slice ")
}

fn parse_transmit_status(line: &str) -> Option<KeyValueStatus> {
    parse_kv_status(line, "transmit ")
}

fn parse_radio_status(line: &str) -> Option<KeyValueStatus> {
    parse_kv_status(line, "radio ")
}

fn parse_interlock_status(line: &str) -> Option<KeyValueStatus> {
    parse_kv_status(line, "interlock ")
}

async fn update_radio_context_from_slice(state: &SharedState, status: &KeyValueStatus) {
    let frequency_hz = status
        .value("RF_frequency")
        .or_else(|| status.value("frequency"))
        .or_else(|| status.value("freq"))
        .and_then(parse_flex_frequency_hz);
    let is_tx = status
        .value("tx")
        .is_some_and(|value| matches!(value, "1" | "true" | "True" | "TRUE"));
    let slice_id = status
        .handle
        .as_deref()
        .and_then(parse_flex_handle_or_decimal);
    let mut guard = state.write().await;
    let should_update = is_tx
        || guard.radio_context.active_tx_slice.is_none()
        || guard.radio_context.frequency_hz.is_none();
    if !should_update {
        return;
    }
    if let Some(slice_id) = slice_id {
        guard.radio_context.active_tx_slice = Some(slice_id);
    }
    if let Some(frequency_hz) = frequency_hz {
        let band = Band::from_frequency_hz(frequency_hz);
        guard.frequency_hz = frequency_hz;
        guard.band = band;
        guard.radio_context.frequency_hz = Some(frequency_hz);
        guard.radio_context.band = band;
        guard.lifecycle.tgxl.transition(
            LifecycleState::Active,
            format!("Flex TX slice frequency updated to {frequency_hz} Hz"),
        );
    }
    if let Some(mode) = status.value("mode") {
        guard.radio_context.mode = Some(mode.to_string());
    }
    if let Some(tx_ant) = status.value("txant").or_else(|| status.value("tx_ant")) {
        guard.radio_context.tx_antenna = Some(tx_ant.to_string());
    }
    if let Some(rx_ant) = status.value("rxant").or_else(|| status.value("rx_ant")) {
        guard.radio_context.rx_antenna = Some(rx_ant.to_string());
    }
    guard.radio_context.source = Some("flex_slice".to_string());
    guard.radio_context.updated_at = Some(SystemTime::now());
    let record = serde_json::json!({
        "event": "radio_context_updated",
        "source": "flex_slice",
        "active_tx_slice": guard.radio_context.active_tx_slice,
        "frequency_hz": guard.radio_context.frequency_hz,
        "band": guard.radio_context.band.as_str(),
        "mode": guard.radio_context.mode,
        "tx_antenna": guard.radio_context.tx_antenna,
        "rx_antenna": guard.radio_context.rx_antenna,
        "raw": status.raw,
    });
    drop(guard);
    append_evidence_json("radio-context.json", &record);
    append_evidence_line(
        "tgxl_state_transition.log",
        format!(
            "Flex slice update propagated to TGXL context: frequency_hz={:?} band={} mode={:?}",
            record["frequency_hz"], record["band"], record["mode"]
        ),
    );
}

async fn update_radio_context_from_transmit(state: &SharedState, status: &KeyValueStatus) {
    let mut guard = state.write().await;
    let mut changed = false;
    if let Some(tx_ant) = status.value("tx_ant").or_else(|| status.value("txant")) {
        guard.radio_context.tx_antenna = Some(tx_ant.to_string());
        changed = true;
    }
    if let Some(rx_ant) = status.value("rx_ant").or_else(|| status.value("rxant")) {
        guard.radio_context.rx_antenna = Some(rx_ant.to_string());
        changed = true;
    }
    if changed {
        guard.radio_context.source = Some("flex_transmit".to_string());
        guard.radio_context.updated_at = Some(SystemTime::now());
        let record = serde_json::json!({
            "event": "radio_context_updated",
            "source": "flex_transmit",
            "frequency_hz": guard.radio_context.frequency_hz,
            "band": guard.radio_context.band.as_str(),
            "tx_antenna": guard.radio_context.tx_antenna,
            "rx_antenna": guard.radio_context.rx_antenna,
            "raw": status.raw,
        });
        drop(guard);
        append_evidence_json("radio-context.json", &record);
    }
}

async fn update_radio_context_from_radio(state: &SharedState, status: &KeyValueStatus) {
    let mut guard = state.write().await;
    let mut changed = false;
    if let Some(serial) = status
        .value("serial")
        .or_else(|| status.value("serial_num"))
        .or_else(|| status.value("serial_number"))
    {
        guard.radio_context.radio_serial = Some(serial.to_string());
        changed = true;
    }
    if let Some(nickname) = status.value("nickname").or_else(|| status.value("name")) {
        guard.radio_context.radio_nickname = Some(nickname.to_string());
        changed = true;
    }
    if let Some(callsign) = status.value("callsign").or_else(|| status.value("call")) {
        guard.radio_context.radio_callsign = Some(callsign.to_string());
        changed = true;
    }
    if changed {
        guard.radio_context.source = Some("flex_radio".to_string());
        guard.radio_context.updated_at = Some(SystemTime::now());
        let record = serde_json::json!({
            "event": "radio_context_updated",
            "source": "flex_radio",
            "radio_serial": guard.radio_context.radio_serial,
            "radio_nickname": guard.radio_context.radio_nickname,
            "radio_callsign": guard.radio_context.radio_callsign,
            "raw": status.raw,
        });
        drop(guard);
        append_evidence_json("radio-context.json", &record);
    }
}

fn parse_flex_frequency_hz(value: &str) -> Option<u64> {
    let parsed = value.parse::<f64>().ok()?;
    if parsed <= 0.0 {
        return None;
    }
    let hz = if parsed < 1000.0 {
        parsed * 1_000_000.0
    } else if parsed < 1_000_000.0 {
        parsed * 1000.0
    } else {
        parsed
    };
    Some(hz.round() as u64)
}

fn parse_flex_handle_or_decimal(value: &str) -> Option<u32> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<u32>().ok()
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
    append_evidence_line("flex-rx.log", line);
}

fn trace_flex_tx(line: &str) {
    debug!(line = %line, "FLEX TX >");
    append_evidence_line("flex-tx.log", line);
}

fn append_flex_log_line(file_name: &str, line: &str) {
    let dir = std::path::Path::new("logs").join("flex");
    let _ = create_dir_all(&dir);
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(file_name))
    {
        let _ = writeln!(file, "{line}");
    }
}

fn append_flex_operate_lab_line(line: impl AsRef<str>) {
    let line = format!(
        "{} {}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        line.as_ref()
    );
    append_flex_log_line("flex-amplifier-operate-sequence.log", &line);
    append_evidence_line("flex-amplifier-operate-sequence.log", line);
}

async fn trace_amplifier_advertisement(
    settings: &FlexInjectionSettings,
    state: &SharedState,
    kind: &str,
    reason: &str,
    line: &str,
) {
    let (source_kpa_state, source_kpa_fault, advertised_state) = {
        let guard = state.read().await;
        (
            guard.amp.state.pgxl_state().to_string(),
            guard.amp.fault.clone(),
            advertised_amp_state_for_settings(settings, &guard.amp).to_string(),
        )
    };
    let record = serde_json::json!({
        "timestamp_ms": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis(),
        "kind": kind,
        "reason": reason,
        "profile": settings.amplifier_status_profile,
        "source_kpa_state": source_kpa_state,
        "source_kpa_fault": source_kpa_fault,
        "advertised_state": advertised_state,
        "line": line,
    });
    if let Ok(json) = serde_json::to_string(&record) {
        if settings.trace_amplifier_advertisements {
            append_flex_log_line("amplifier-advertisements.jsonl", &json);
        }
        append_evidence_line("amplifier-advertisements.jsonl", json);
    }
    if matches!(kind, "amplifier_create" | "amplifier_status") {
        let mut guard = state.write().await;
        guard.flex_injection.last_advertised_flex_amp_state = Some(advertised_state);
        guard
            .flex_injection
            .last_emitted_amplifier_advertisement_line = Some(line.to_string());
        guard
            .flex_injection
            .last_emitted_amplifier_advertisement_kind = Some(kind.to_string());
    }
}

pub fn amplifier_create_command(
    amplifier_ip: IpAddr,
    amplifier_port: u16,
    model: &str,
    serial: &str,
    ant_map: &str,
    profile: &str,
    open_trigger_variant: &str,
) -> String {
    amplifier_create_command_with_state_for_variant(
        amplifier_ip,
        amplifier_port,
        model,
        serial,
        ant_map,
        profile,
        open_trigger_variant,
        None,
    )
}

pub fn amplifier_create_command_with_state(
    amplifier_ip: IpAddr,
    amplifier_port: u16,
    model: &str,
    serial: &str,
    ant_map: &str,
    profile: &str,
    state_value: Option<&str>,
) -> String {
    amplifier_create_command_with_state_for_variant(
        amplifier_ip,
        amplifier_port,
        model,
        serial,
        ant_map,
        profile,
        "current",
        state_value,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn amplifier_create_command_with_state_for_variant(
    amplifier_ip: IpAddr,
    amplifier_port: u16,
    model: &str,
    serial: &str,
    ant_map: &str,
    profile: &str,
    open_trigger_variant: &str,
    state_value: Option<&str>,
) -> String {
    let mut command = format!(
        "amplifier create ip={amplifier_ip} port={amplifier_port} model={} serial_num={} ant={}",
        sanitize_token(model),
        sanitize_token(serial),
        sanitize_token(ant_map)
    );
    if open_trigger_variant != "current" {
        match open_trigger_variant {
            "state_only" | "state_ip_port" | "state_model_ip_port_serial" => {
                if let Some(state_value) = state_value {
                    command.push_str(&format!(" state={}", sanitize_token(state_value)));
                }
            }
            "state_connected" => {
                if let Some(state_value) = state_value {
                    command.push_str(&format!(" state={}", sanitize_token(state_value)));
                }
                command.push_str(" connected=1");
            }
            "availability_fields" => {
                if let Some(state_value) = state_value {
                    command.push_str(&format!(" state={}", sanitize_token(state_value)));
                }
                command.push_str(" available=1 tx_ready=1 control=1");
            }
            _ => {}
        }
        return command;
    }
    match profile {
        "pgxl_verbose" | "old_good_pgxl" => {
            if let Some(state_value) = state_value {
                command.push_str(&format!(" state={}", sanitize_token(state_value)));
            }
            command.push_str(" connected=1 configured=1 enabled=1");
        }
        "aethersdr_minimal" | "aethersdr_operational" => {
            if let Some(state_value) = state_value {
                command.push_str(&format!(" state={}", sanitize_token(state_value)));
            }
        }
        "aethersdr_force_direct" | "aethersdr_pgxl_direct_lab" => {
            if let Some(state_value) = state_value {
                command.push_str(&format!(" state={}", sanitize_token(state_value)));
            }
            command.push_str(" connected=1 configured=1 enabled=1 direct=1 lan=1");
        }
        "strict_real_pgxl" | "official_pgxl" => {}
        _ => {}
    }
    command
}

pub fn amplifier_create_has_nonstandard_fields(command: &str) -> bool {
    [
        " state=",
        " connected=",
        " configured=",
        " enabled=",
        " direct=",
        " lan=",
    ]
    .iter()
    .any(|field| command.contains(field))
}

pub fn validate_amplifier_create_for_profile(profile: &str, command: &str) -> Result<(), String> {
    let has_state = command.contains(" state=");
    let forbidden = [
        " connected=",
        " configured=",
        " enabled=",
        " direct=",
        " lan=",
    ]
    .iter()
    .filter(|field| command.contains(**field))
    .copied()
    .collect::<Vec<_>>();
    match profile {
        "official_pgxl" => {
            if has_state || !forbidden.is_empty() {
                return Err(format!(
                    "official_pgxl create line contains non-official fields: {command}"
                ));
            }
        }
        "aethersdr_minimal" | "aethersdr_operational" => {
            if !has_state {
                return Err(format!(
                    "{profile} create line must include only live state as the compatibility field: {command}"
                ));
            }
            if !forbidden.is_empty() {
                return Err(format!(
                    "{profile} create line contains stripped/noisy fields {forbidden:?}: {command}"
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

async fn synthetic_amplifier_status_line(
    settings: &FlexInjectionSettings,
    state: &SharedState,
    handle: Option<&str>,
) -> String {
    let handle = handle.unwrap_or("unknown");
    let amp = {
        let guard = state.read().await;
        guard.amp.clone()
    };
    let state_value = advertised_amp_state_for_settings(settings, &amp);
    let fault = amp.fault.as_deref().unwrap_or("");
    let mut candidate_fields = match settings.aethersdr_open_trigger_variant.as_str() {
        "state_only" => vec!["state".to_string()],
        "state_connected" => vec!["state".to_string(), "connected".to_string()],
        "state_ip_port" => vec!["state".to_string(), "ip".to_string(), "port".to_string()],
        "state_model_ip_port_serial" => vec![
            "state".to_string(),
            "model".to_string(),
            "ip".to_string(),
            "port".to_string(),
            "serial_num".to_string(),
        ],
        "availability_fields" => vec![
            "state".to_string(),
            "available".to_string(),
            "tx_ready".to_string(),
            "control".to_string(),
        ],
        _ => vec![
            "model".to_string(),
            "ip".to_string(),
            "port".to_string(),
            "serial_num".to_string(),
            "ant".to_string(),
            "state".to_string(),
        ],
    };
    let mut line = if settings.amplifier_status_profile == "strict_real_pgxl" {
        format!(
            "amplifier {handle} model={} ip={} port={} serial_num={} ant={} state={}",
            sanitize_token(&settings.amplifier_model),
            settings.amplifier_ip,
            settings.amplifier_port,
            sanitize_token(&settings.serial),
            sanitize_token(&settings.ant_map),
            state_value
        )
    } else {
        candidate_fields.extend(["temp".to_string(), "id".to_string(), "fault".to_string()]);
        format!(
            "amplifier {handle} model={} ip={} port={} serial_num={} ant={} state={} temp={:.1} id={:.1} fault={}",
            sanitize_token(&settings.amplifier_model),
            settings.amplifier_ip,
            settings.amplifier_port,
            sanitize_token(&settings.serial),
            sanitize_token(&settings.ant_map),
            state_value,
            amp.temperature_c,
            amp.pa_current_amps,
            sanitize_token(fault)
        )
    };
    if settings.aethersdr_open_trigger_variant == "state_connected" {
        line.push_str(" connected=1");
    } else if settings.aethersdr_open_trigger_variant == "availability_fields" {
        line.push_str(" available=1 tx_ready=1 control=1");
    }
    match settings.amplifier_status_profile.as_str() {
        "pgxl_verbose" | "old_good_pgxl" => {
            candidate_fields.extend([
                "connected".to_string(),
                "configured".to_string(),
                "enabled".to_string(),
            ]);
            line.push_str(" connected=1 configured=1 enabled=1");
        }
        "aethersdr_force_direct" | "aethersdr_pgxl_direct_lab" => {
            candidate_fields.extend([
                "connected".to_string(),
                "configured".to_string(),
                "enabled".to_string(),
                "direct".to_string(),
                "lan".to_string(),
            ]);
            line.push_str(" connected=1 configured=1 enabled=1 direct=1 lan=1")
        }
        _ => {}
    }
    record_amplifier_candidate_fields(state, candidate_fields).await;
    line
}

fn advertised_amp_state(amp: &bridge_core::AmpState) -> &'static str {
    if !amp.first_poll_completed
        && amp.startup_state_policy.as_deref() == Some("wait_for_first_kpa_poll")
        && !amp_has_recent_poll(amp)
    {
        return "UNKNOWN";
    }
    if amp.fault.is_some() || amp.state == bridge_core::AmpOperatingState::Fault {
        "FAULT"
    } else {
        amp.state.pgxl_state()
    }
}

fn amp_has_recent_poll(amp: &bridge_core::AmpState) -> bool {
    if amp.runtime.poll_success_count == 0 {
        return false;
    }
    amp.last_successful_poll_at
        .and_then(|poll| SystemTime::now().duration_since(poll).ok())
        .is_some_and(|elapsed| elapsed <= Duration::from_secs(10))
}

fn advertised_amp_state_for_settings(
    settings: &FlexInjectionSettings,
    amp: &bridge_core::AmpState,
) -> &'static str {
    if settings.lab_forces_operate_advertisement() {
        "OPERATE"
    } else {
        advertised_amp_state(amp)
    }
}

fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn registration_command_lines(settings: &FlexInjectionSettings) -> Vec<String> {
    registration_commands(settings)
        .into_iter()
        .map(|pending| pending.command)
        .collect()
}

#[cfg(test)]
async fn registration_commands_with_state(
    settings: &FlexInjectionSettings,
    state: &SharedState,
) -> Vec<PendingCommand> {
    let mut commands = Vec::new();
    commands.push(pending_amplifier_create_command_with_state(settings, state).await);
    commands.extend(post_amplifier_registration_commands(settings));
    commands
}

async fn pending_amplifier_create_command_with_state(
    settings: &FlexInjectionSettings,
    state: &SharedState,
) -> PendingCommand {
    let state_value = {
        let guard = state.read().await;
        advertised_amp_state_for_settings(settings, &guard.amp).to_string()
    };
    PendingCommand::new(
        "amplifier_create",
        settings.amplifier_create_command_with_state(&state_value),
        PendingKind::AmplifierCreate,
    )
}

fn registration_commands(settings: &FlexInjectionSettings) -> Vec<PendingCommand> {
    registration_commands_inner(settings, None)
}

fn registration_commands_inner(
    settings: &FlexInjectionSettings,
    state_value: Option<&str>,
) -> Vec<PendingCommand> {
    let mut commands = Vec::new();
    commands.push(PendingCommand::new(
        "amplifier_create",
        state_value
            .map(|value| settings.amplifier_create_command_with_state(value))
            .unwrap_or_else(|| settings.amplifier_create_command()),
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
    if settings.full_pgxl_registration
        && settings.create_interlock
        && !settings.disable_amp_interlock
    {
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
    commands.push(PendingCommand::new(
        "sub_slice_all",
        "sub slice all",
        PendingKind::Subscription,
    ));
    commands.push(PendingCommand::new(
        "sub_tx_all",
        "sub tx all",
        PendingKind::Subscription,
    ));
    commands
}

fn post_amplifier_registration_commands(settings: &FlexInjectionSettings) -> Vec<PendingCommand> {
    let mut commands = Vec::new();
    if settings.full_pgxl_registration && settings.create_meters {
        for meter in amp_meter_create_commands() {
            commands.push(PendingCommand::new(
                format!("meter_create_{}", meter.name),
                meter.command,
                PendingKind::MeterCreate { name: meter.name },
            ));
        }
    }
    if settings.full_pgxl_registration
        && settings.create_interlock
        && !settings.disable_amp_interlock
    {
        commands.push(PendingCommand::new(
            "interlock_create",
            interlock_create_command(&settings.serial),
            PendingKind::InterlockCreate,
        ));
    } else if settings.full_pgxl_registration && settings.disable_amp_interlock {
        append_evidence_line(
            "flex-registration-health.md",
            "AMP interlock creation skipped because flex_injection.disable_amp_interlock=true (TEST ONLY).",
        );
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
    commands.push(PendingCommand::new(
        "sub_slice_all",
        "sub slice all",
        PendingKind::Subscription,
    ));
    commands.push(PendingCommand::new(
        "sub_tx_all",
        "sub tx all",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgxlTriggerVariant {
    pub name: String,
    pub state: String,
    pub line: String,
    pub notes: String,
}

pub fn pgxl_direct_trigger_variants(settings: &FlexInjectionSettings) -> Vec<PgxlTriggerVariant> {
    let handle = "0x42000001";
    let model = sanitize_token(&settings.amplifier_model);
    let ip = settings.amplifier_ip;
    let port = settings.amplifier_port;
    let serial = sanitize_token(&settings.serial);
    let serial_num = format!("serial_num={serial}");
    let serial_short = format!("serial={serial}");
    let mut variants = Vec::new();
    for state in ["STANDBY", "OPERATE"] {
        variants.push(PgxlTriggerVariant {
            name: format!("{state}-A-model-ip"),
            state: state.to_string(),
            line: format!("amplifier {handle} model={model} ip={ip} state={state}"),
            notes: "AetherSDR source only requires non-TGXL model plus non-empty ip for amplifierChanged(true).".to_string(),
        });
        variants.push(PgxlTriggerVariant {
            name: format!("{state}-B-model-ip-port"),
            state: state.to_string(),
            line: format!("amplifier {handle} model={model} ip={ip} port={port} state={state}"),
            notes: "Adds explicit PGXL TCP port; AetherSDR direct auto-connect currently uses default 9008 when only an IP is supplied.".to_string(),
        });
        variants.push(PgxlTriggerVariant {
            name: format!("{state}-C-ip-model-port-connected"),
            state: state.to_string(),
            line: format!(
                "amplifier {handle} ip={ip} model={model} port={port} state={state} connected=1"
            ),
            notes: "Tests field ordering and connected=1 without extra direct/lan flags."
                .to_string(),
        });
        variants.push(PgxlTriggerVariant {
            name: format!("{state}-D-serial-field"),
            state: state.to_string(),
            line: format!(
                "amplifier {handle} ip={ip} model={model} port={port} {serial_short} state={state} connected=1"
            ),
            notes: "Tests serial= spelling in case client code or radio firmware rewrites serial_num.".to_string(),
        });
        variants.push(PgxlTriggerVariant {
            name: format!("{state}-E-serial-num-direct-lan"),
            state: state.to_string(),
            line: format!(
                "amplifier {handle} ip={ip} model={model} port={port} {serial_num} state={state} connected=1 configured=1 enabled=1 direct=1 lan=1"
            ),
            notes: "Current EGB direct-trigger candidate with full AetherSDR lab flags.".to_string(),
        });
    }
    variants
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
            "minimal",
            "current",
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
            "minimal",
            "current",
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
        assert_eq!(standby.requested_operate(), None);
    }

    #[test]
    fn ignores_response_lines_when_parsing_amplifier_status() {
        assert!(parse_amplifier_status("R44|0|amplifier 0x42000001 model=PowerGeniusXL").is_none());
    }

    #[test]
    fn parses_flex_frequency_units() {
        assert_eq!(parse_flex_frequency_hz("14.200000"), Some(14_200_000));
        assert_eq!(parse_flex_frequency_hz("14200.000"), Some(14_200_000));
        assert_eq!(parse_flex_frequency_hz("14200000"), Some(14_200_000));
        assert_eq!(parse_flex_frequency_hz("0.0"), None);
    }

    #[test]
    fn parses_slice_status_with_tx_context() {
        let status = parse_slice_status(
            "S1A2B|slice 0 RF_frequency=14.200000 tx=1 mode=USB txant=ANT1 rxant=ANT1",
        )
        .unwrap();
        assert_eq!(status.handle.as_deref(), Some("0"));
        assert_eq!(status.value("RF_frequency"), Some("14.200000"));
        assert_eq!(status.value("tx"), Some("1"));
        assert_eq!(status.value("mode"), Some("USB"));
    }

    #[tokio::test]
    async fn flex_slice_updates_shared_radio_context() {
        let state = bridge_core::state::shared_default_state();
        let status = parse_slice_status(
            "S1A2B|slice 1 RF_frequency=7.100000 tx=1 mode=LSB txant=ANT2 rxant=ANT1",
        )
        .unwrap();
        update_radio_context_from_slice(&state, &status).await;
        let guard = state.read().await;
        assert_eq!(guard.frequency_hz, 7_100_000);
        assert_eq!(guard.band, bridge_core::Band::M40);
        assert_eq!(guard.radio_context.active_tx_slice, Some(1));
        assert_eq!(guard.radio_context.frequency_hz, Some(7_100_000));
        assert_eq!(guard.radio_context.band, bridge_core::Band::M40);
        assert_eq!(guard.radio_context.mode.as_deref(), Some("LSB"));
        assert_eq!(guard.radio_context.tx_antenna.as_deref(), Some("ANT2"));
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

    #[tokio::test]
    async fn strict_real_pgxl_status_uses_conservative_fields() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "2-50/18-0005".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "strict_real_pgxl".to_string(),
            trace_amplifier_advertisements: false,
            pgxl_force_operate_advertisement: false,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "advertise_standby_immediately".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        let state = bridge_core::state::shared_mock_state();
        let line = synthetic_amplifier_status_line(&settings, &state, Some("0x42000001")).await;
        assert_eq!(
            line,
            "amplifier 0x42000001 model=PowerGeniusXL ip=192.168.1.50 port=9008 serial_num=2-50/18-0005 ant=ANT1:PORTA,ANT2:PORTB state=STANDBY"
        );
        assert!(!line.contains("connected="));
        assert!(!line.contains("direct="));
        let guard = state.read().await;
        assert_eq!(
            guard.flex_injection.amplifier_pairing_candidate_fields,
            vec![
                "model".to_string(),
                "ip".to_string(),
                "port".to_string(),
                "serial_num".to_string(),
                "ant".to_string(),
                "state".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn amplifier_status_uses_live_operate_state() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "aethersdr_operational".to_string(),
            trace_amplifier_advertisements: false,
            pgxl_force_operate_advertisement: false,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "advertise_standby_immediately".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        let state = bridge_core::state::shared_default_state();
        {
            let mut guard = state.write().await;
            guard.amp.connection_state = bridge_core::ConnectionState::Connected;
            guard.amp.connected = true;
            guard.amp.operate = true;
            guard.amp.state = bridge_core::AmpOperatingState::Operate;
        }
        let line = synthetic_amplifier_status_line(&settings, &state, Some("0x42000001")).await;
        assert!(line.contains("state=OPERATE"));
        {
            let mut guard = state.write().await;
            guard.amp.operate = false;
            guard.amp.state = bridge_core::AmpOperatingState::Standby;
        }
        let line = synthetic_amplifier_status_line(&settings, &state, Some("0x42000001")).await;
        assert!(line.contains("state=STANDBY"));
    }

    #[tokio::test]
    async fn amplifier_status_does_not_report_fault_without_kpa_fault() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "aethersdr_force_direct".to_string(),
            trace_amplifier_advertisements: false,
            pgxl_force_operate_advertisement: false,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "advertise_standby_immediately".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        let state = bridge_core::state::shared_default_state();
        {
            let mut guard = state.write().await;
            guard.amp.connection_state = bridge_core::ConnectionState::Degraded;
            guard.amp.connected = false;
            guard.amp.operate = true;
            guard.amp.state = bridge_core::AmpOperatingState::Operate;
            guard.amp.fault = None;
        }
        let line = synthetic_amplifier_status_line(&settings, &state, Some("0x42000001")).await;
        assert!(line.contains("state=OPERATE"));
        assert!(!line.contains("state=FAULT"));
    }

    #[test]
    fn direct_profiles_do_not_hardcode_standby_on_create() {
        for profile in ["pgxl_verbose", "old_good_pgxl", "aethersdr_force_direct"] {
            let cmd = amplifier_create_command(
                "192.168.1.50".parse().unwrap(),
                9008,
                "PowerGeniusXL",
                "EGB-KPA500",
                "ANT1:PORTA,ANT2:PORTB",
                profile,
                "current",
            );
            assert!(
                !cmd.contains("state=STANDBY"),
                "{profile} must not hard-code STANDBY in amplifier create"
            );
        }
    }

    #[tokio::test]
    async fn registration_create_uses_live_state_for_direct_profile() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "aethersdr_force_direct".to_string(),
            trace_amplifier_advertisements: false,
            pgxl_force_operate_advertisement: false,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "advertise_standby_immediately".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        let state = bridge_core::state::shared_default_state();
        {
            let mut guard = state.write().await;
            guard.amp.connection_state = bridge_core::ConnectionState::Connected;
            guard.amp.connected = true;
            guard.amp.operate = true;
            guard.amp.state = bridge_core::AmpOperatingState::Operate;
        }
        let commands = registration_commands_with_state(&settings, &state).await;
        assert!(commands[0].command.contains("state=OPERATE"));
        assert!(commands[0].command.contains("direct=1"));
    }

    #[tokio::test]
    async fn wait_policy_uses_unknown_before_first_poll() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "aethersdr_force_direct".to_string(),
            trace_amplifier_advertisements: false,
            pgxl_force_operate_advertisement: false,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "wait_for_first_kpa_poll".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        let state = bridge_core::state::shared_default_state();
        {
            let mut guard = state.write().await;
            guard.amp.startup_state_policy = Some("wait_for_first_kpa_poll".to_string());
        }
        let commands = registration_commands_with_state(&settings, &state).await;
        assert!(commands[0].command.contains("state=UNKNOWN"));
    }

    #[tokio::test]
    async fn wait_policy_uses_live_state_after_recent_poll() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "aethersdr_force_direct".to_string(),
            trace_amplifier_advertisements: false,
            pgxl_force_operate_advertisement: false,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "wait_for_first_kpa_poll".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        let state = bridge_core::state::shared_default_state();
        {
            let mut guard = state.write().await;
            guard.amp.startup_state_policy = Some("wait_for_first_kpa_poll".to_string());
            guard.amp.connection_state = bridge_core::ConnectionState::Connected;
            guard.amp.connected = true;
            guard.amp.operate = true;
            guard.amp.state = bridge_core::AmpOperatingState::Operate;
            guard.amp.temperature_c = 38.0;
            guard.amp.last_successful_poll_at = Some(SystemTime::now());
            guard.amp.runtime.record_poll_success(20);
        }
        let commands = registration_commands_with_state(&settings, &state).await;
        assert!(commands[0].command.contains("state=OPERATE"));
        assert!(!commands[0].command.contains("state=UNKNOWN"));

        let status = synthetic_amplifier_status_line(&settings, &state, Some("0x42000001")).await;
        assert!(status.contains("state=OPERATE"));
        assert!(status.contains("temp=38.0"));
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
            amplifier_status_profile: "pgxl_paired".to_string(),
            trace_amplifier_advertisements: false,
            pgxl_force_operate_advertisement: false,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "advertise_standby_immediately".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
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
        assert!(commands.contains(&"sub amplifier all".to_string()));
        assert!(commands.contains(&"sub slice all".to_string()));
        assert!(commands.contains(&"sub tx all".to_string()));
        assert_eq!(commands.last().map(String::as_str), Some("sub tx all"));
    }

    #[tokio::test]
    async fn lab_force_operate_advertises_operate_without_live_state() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "aethersdr_pgxl_direct_lab".to_string(),
            trace_amplifier_advertisements: false,
            pgxl_force_operate_advertisement: true,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "wait_for_first_kpa_poll".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        let state = bridge_core::state::shared_default_state();
        let commands = registration_commands_with_state(&settings, &state).await;
        assert!(commands[0].command.contains("state=OPERATE"));
        assert!(commands[0].command.contains("direct=1"));

        let variants = pgxl_direct_trigger_variants(&settings);
        assert!(variants.iter().any(|variant| {
            variant.name == "OPERATE-E-serial-num-direct-lan" && variant.line.contains("direct=1")
        }));
        assert!(variants.iter().any(|variant| {
            variant.name == "STANDBY-A-model-ip" && variant.line.contains("state=STANDBY")
        }));
    }

    #[tokio::test]
    async fn real_operational_profile_emits_official_create_only() {
        let settings = FlexInjectionSettings {
            radio_addr: "192.168.0.199:4992".parse().unwrap(),
            amplifier_ip: "192.168.0.189".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "official_pgxl".to_string(),
            trace_amplifier_advertisements: true,
            pgxl_force_operate_advertisement: false,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "wait_for_first_kpa_poll".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(30000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        let state = bridge_core::state::shared_default_state();
        {
            let mut guard = state.write().await;
            guard.amp.first_poll_completed = true;
            guard.amp.last_successful_poll_at = Some(SystemTime::now());
            guard.amp.runtime.record_poll_success(10);
        }
        let commands = registration_commands_with_state(&settings, &state).await;
        assert_eq!(
            commands[0].command,
            "amplifier create ip=192.168.0.189 port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB"
        );
        assert!(!amplifier_create_has_nonstandard_fields(
            &commands[0].command
        ));
    }

    #[test]
    fn force_direct_profile_emits_nonstandard_create_fields_for_lab_only() {
        let command = amplifier_create_command_with_state(
            "192.168.0.189".parse().unwrap(),
            9008,
            "PowerGeniusXL",
            "EGB-KPA500",
            "ANT1:PORTA,ANT2:PORTB",
            "aethersdr_force_direct",
            Some("STANDBY"),
        );
        assert!(amplifier_create_has_nonstandard_fields(&command));
        assert!(command.contains("direct=1"));
    }

    #[test]
    fn aethersdr_minimal_profile_emits_only_state_create_field() {
        let command = amplifier_create_command_with_state(
            "192.168.0.189".parse().unwrap(),
            9008,
            "PowerGeniusXL",
            "EGB-KPA500",
            "ANT1:PORTA,ANT2:PORTB",
            "aethersdr_minimal",
            Some("STANDBY"),
        );
        assert_eq!(
            command,
            "amplifier create ip=192.168.0.189 port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB state=STANDBY"
        );
        validate_amplifier_create_for_profile("aethersdr_minimal", &command).unwrap();
        assert!(!command.contains("connected="));
        assert!(!command.contains("configured="));
        assert!(!command.contains("enabled="));
        assert!(!command.contains("direct="));
        assert!(!command.contains("lan="));
    }

    #[test]
    fn aethersdr_operational_alias_emits_minimal_state_only() {
        let command = amplifier_create_command_with_state(
            "192.168.0.189".parse().unwrap(),
            9008,
            "PowerGeniusXL",
            "EGB-KPA500",
            "ANT1:PORTA,ANT2:PORTB",
            "aethersdr_operational",
            Some("STANDBY"),
        );
        assert_eq!(
            command,
            "amplifier create ip=192.168.0.189 port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB state=STANDBY"
        );
        validate_amplifier_create_for_profile("aethersdr_operational", &command).unwrap();
    }

    #[test]
    fn create_profile_validation_rejects_extra_fields() {
        let official_with_state =
            "amplifier create ip=192.168.0.189 port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB state=STANDBY";
        assert!(
            validate_amplifier_create_for_profile("official_pgxl", official_with_state)
                .unwrap_err()
                .contains("non-official")
        );

        let minimal_with_noise =
            "amplifier create ip=192.168.0.189 port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB state=STANDBY connected=1";
        assert!(
            validate_amplifier_create_for_profile("aethersdr_minimal", minimal_with_noise)
                .unwrap_err()
                .contains("stripped/noisy")
        );
    }

    #[test]
    fn parses_empty_interlock_amplifier_status() {
        let status = parse_interlock_status(
            "S0|interlock tx_client_handle=0x00000000 state=READY reason=AMP:PG-XL source= tx_allowed=1 amplifier=",
        )
        .unwrap();
        assert_eq!(status.value("reason"), Some("AMP:PG-XL"));
        assert_eq!(status.value("amplifier"), Some(""));
    }

    #[test]
    fn post_registration_commands_wait_until_after_amplifier_create() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "aethersdr_minimal".to_string(),
            trace_amplifier_advertisements: false,
            pgxl_force_operate_advertisement: false,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "wait_for_first_kpa_poll".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        let post = post_amplifier_registration_commands(&settings)
            .into_iter()
            .map(|pending| pending.command)
            .collect::<Vec<_>>();
        assert!(!post
            .iter()
            .any(|command| command.starts_with("amplifier create")));
        assert!(post
            .iter()
            .any(|command| command.starts_with("meter create name=FWD")));
        assert!(post.iter().any(|command| command
            == "interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=EGB-KPA500"));
    }

    #[test]
    fn lab_force_radio_operate_setting_is_separate_from_rf_risk() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "aethersdr_pgxl_direct_lab".to_string(),
            trace_amplifier_advertisements: true,
            pgxl_force_operate_advertisement: true,
            flex_force_operate_via_radio: true,
            pgxl_connect_assist: false,
            amplifier_startup_state_policy: "wait_for_first_kpa_poll".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        assert!(settings.flex_force_operate_via_radio);
        assert!(!settings.allow_rf_risk);
        assert!(settings.lab_forces_operate_advertisement());
    }

    #[test]
    fn pgxl_connect_assist_is_not_rf_risk_control() {
        let settings = FlexInjectionSettings {
            radio_addr: "127.0.0.1:4992".parse().unwrap(),
            amplifier_ip: "192.168.1.50".parse().unwrap(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle_label: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            amplifier_status_profile: "aethersdr_force_direct".to_string(),
            trace_amplifier_advertisements: true,
            pgxl_force_operate_advertisement: false,
            flex_force_operate_via_radio: false,
            pgxl_connect_assist: true,
            amplifier_startup_state_policy: "wait_for_first_kpa_poll".to_string(),
            wait_first_kpa_poll_timeout: Duration::from_millis(10000),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            disable_amp_interlock: false,
            allow_rf_risk: false,
            reconnect_initial: Duration::from_millis(1000),
            reconnect_max: Duration::from_millis(30000),
            ping_interval: Duration::from_millis(30000),
            tuner_presence_refresh: false,
            tuner_refresh_interval: Duration::from_millis(5000),
            amplifier_reannounce_interval: Duration::from_millis(5000),
            pgxl_startup_trigger_strategy: "current".to_string(),
            aethersdr_open_trigger_variant: "current".to_string(),
        };
        assert!(settings.pgxl_connect_assist);
        assert!(!settings.flex_force_operate_via_radio);
        assert!(!settings.allow_rf_risk);
        assert!(!settings.lab_forces_operate_advertisement());
    }
}
