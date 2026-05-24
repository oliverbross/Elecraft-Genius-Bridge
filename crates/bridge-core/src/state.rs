use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

pub type SharedState = Arc<RwLock<BridgeState>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Band {
    Unknown,
    M160,
    M80,
    M60,
    M40,
    M30,
    M20,
    M17,
    M15,
    M12,
    M10,
    M6,
}

impl Default for Band {
    fn default() -> Self {
        Self::Unknown
    }
}

impl Band {
    pub fn from_frequency_hz(frequency_hz: u64) -> Self {
        match frequency_hz {
            1_800_000..=2_000_000 => Self::M160,
            3_500_000..=4_000_000 => Self::M80,
            5_330_500..=5_406_500 => Self::M60,
            7_000_000..=7_300_000 => Self::M40,
            10_100_000..=10_150_000 => Self::M30,
            14_000_000..=14_350_000 => Self::M20,
            18_068_000..=18_168_000 => Self::M17,
            21_000_000..=21_450_000 => Self::M15,
            24_890_000..=24_990_000 => Self::M12,
            28_000_000..=29_700_000 => Self::M10,
            50_000_000..=54_000_000 => Self::M6,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::M160 => "160m",
            Self::M80 => "80m",
            Self::M60 => "60m",
            Self::M40 => "40m",
            Self::M30 => "30m",
            Self::M20 => "20m",
            Self::M17 => "17m",
            Self::M15 => "15m",
            Self::M12 => "12m",
            Self::M10 => "10m",
            Self::M6 => "6m",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmpOperatingState {
    Standby,
    Idle,
    Operate,
    TransmitA,
    TransmitB,
    PowerUp,
    Fault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Degraded,
    Error,
}

impl ConnectionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Degraded => "degraded",
            Self::Error => "error",
        }
    }

    pub fn is_healthy(self) -> bool {
        matches!(self, Self::Connected)
    }
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::Disconnected
    }
}

impl AmpOperatingState {
    pub fn pgxl_state(self) -> &'static str {
        match self {
            Self::Standby => "STANDBY",
            Self::Idle => "IDLE",
            Self::Operate => "OPERATE",
            Self::TransmitA => "TRANSMIT_A",
            Self::TransmitB => "TRANSMIT_B",
            Self::PowerUp => "POWERUP",
            Self::Fault => "FAULT",
        }
    }
}

impl Default for AmpOperatingState {
    fn default() -> Self {
        Self::Standby
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeState {
    pub frequency_hz: u64,
    pub band: Band,
    pub radio_context: RadioContext,
    pub amp: AmpState,
    pub tuner: TunerState,
    pub flex_injection: FlexInjectionState,
    pub clients: ClientState,
    pub desired: DesiredState,
    pub protocol: ProtocolCounters,
    pub controls: ControlDiagnostics,
    pub lifecycle: LifecycleDiagnostics,
    pub effective_controls: EffectiveControlPolicy,
    pub config_identity: RuntimeConfigIdentity,
}

impl Default for BridgeState {
    fn default() -> Self {
        Self {
            frequency_hz: 14_200_000,
            band: Band::M20,
            radio_context: RadioContext {
                frequency_hz: Some(14_200_000),
                band: Band::M20,
                source: Some("default".to_string()),
                ..RadioContext::default()
            },
            amp: AmpState::default(),
            tuner: TunerState::default(),
            flex_injection: FlexInjectionState::default(),
            clients: ClientState::default(),
            desired: DesiredState::default(),
            protocol: ProtocolCounters::default(),
            controls: ControlDiagnostics::default(),
            lifecycle: LifecycleDiagnostics::default(),
            effective_controls: EffectiveControlPolicy::default(),
            config_identity: RuntimeConfigIdentity::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleState {
    Disconnected,
    Connecting,
    Subscribed,
    ObjectCreated,
    ObjectAdvertised,
    ObjectAccepted,
    TcpConnected,
    Active,
    Degraded,
    Removed,
    Reconnecting,
    NotStarted,
}

impl LifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Subscribed => "subscribed",
            Self::ObjectCreated => "object-created",
            Self::ObjectAdvertised => "object-advertised",
            Self::ObjectAccepted => "object-accepted",
            Self::TcpConnected => "tcp-connected",
            Self::Active => "active",
            Self::Degraded => "degraded",
            Self::Removed => "removed",
            Self::Reconnecting => "reconnecting",
            Self::NotStarted => "not-started",
        }
    }
}

impl Default for LifecycleState {
    fn default() -> Self {
        Self::NotStarted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TuneLifecycleState {
    Idle,
    TuneRequested,
    Tuning,
    TuneComplete,
    TuneFailed,
    Cooldown,
}

impl TuneLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::TuneRequested => "tune-requested",
            Self::Tuning => "tuning",
            Self::TuneComplete => "tune-complete",
            Self::TuneFailed => "tune-failed",
            Self::Cooldown => "cooldown",
        }
    }
}

impl Default for TuneLifecycleState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleStatus {
    pub state: LifecycleState,
    pub transition_count: u64,
    pub last_transition_reason: Option<String>,
    pub entered_at_ms: Option<u128>,
}

impl Default for LifecycleStatus {
    fn default() -> Self {
        Self {
            state: LifecycleState::NotStarted,
            transition_count: 0,
            last_transition_reason: None,
            entered_at_ms: None,
        }
    }
}

impl LifecycleStatus {
    pub fn transition(&mut self, state: LifecycleState, reason: impl Into<String>) {
        let reason = reason.into();
        if self.state != state || self.last_transition_reason.as_deref() != Some(reason.as_str()) {
            self.state = state;
            self.transition_count = self.transition_count.saturating_add(1);
            self.last_transition_reason = Some(reason);
            self.entered_at_ms = Some(timestamp_millis());
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuneLifecycleStatus {
    pub state: TuneLifecycleState,
    pub transition_count: u64,
    pub last_transition_reason: Option<String>,
    pub entered_at_ms: Option<u128>,
    pub last_successful_tune_ms: Option<u128>,
    pub last_failed_tune_ms: Option<u128>,
}

impl Default for TuneLifecycleStatus {
    fn default() -> Self {
        Self {
            state: TuneLifecycleState::Idle,
            transition_count: 0,
            last_transition_reason: None,
            entered_at_ms: None,
            last_successful_tune_ms: None,
            last_failed_tune_ms: None,
        }
    }
}

impl TuneLifecycleStatus {
    pub fn transition(&mut self, state: TuneLifecycleState, reason: impl Into<String>) {
        let reason = reason.into();
        if self.state != state || self.last_transition_reason.as_deref() != Some(reason.as_str()) {
            self.state = state;
            self.transition_count = self.transition_count.saturating_add(1);
            self.last_transition_reason = Some(reason);
            self.entered_at_ms = Some(timestamp_millis());
        }
    }

    pub fn mark_success(&mut self, reason: impl Into<String>) {
        self.last_successful_tune_ms = Some(timestamp_millis());
        self.transition(TuneLifecycleState::TuneComplete, reason);
    }

    pub fn mark_failure(&mut self, reason: impl Into<String>) {
        self.last_failed_tune_ms = Some(timestamp_millis());
        self.transition(TuneLifecycleState::TuneFailed, reason);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LifecycleDiagnostics {
    pub flex_session: LifecycleStatus,
    pub amplifier: LifecycleStatus,
    pub tgxl: LifecycleStatus,
    pub pgxl: LifecycleStatus,
    pub aether_client: LifecycleStatus,
    pub tune: TuneLifecycleStatus,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RadioContext {
    pub active_tx_slice: Option<u32>,
    pub frequency_hz: Option<u64>,
    pub band: Band,
    pub mode: Option<String>,
    pub tx_antenna: Option<String>,
    pub rx_antenna: Option<String>,
    pub radio_serial: Option<String>,
    pub radio_nickname: Option<String>,
    pub radio_callsign: Option<String>,
    pub source: Option<String>,
    pub last_tune_frequency_hz: Option<u64>,
    pub last_tune_band: Option<Band>,
    pub last_kat500_follow_frequency_hz: Option<u64>,
    pub last_kat500_follow_wire: Option<String>,
    pub last_kat500_follow_requested_khz: Option<u64>,
    pub last_kat500_follow_confirmed_khz: Option<u64>,
    pub last_kat500_follow_confirmation_match: Option<bool>,
    pub kat500_follow_sent_count: u64,
    pub kat500_follow_skipped_count: u64,
    pub kat500_follow_stale_response_count: u64,
    pub kat500_follow_retry_count: u64,
    pub last_kpa500_follow_band: Option<Band>,
    pub last_kpa500_follow_wire: Option<String>,
    pub last_kpa500_follow_result: Option<String>,
    pub last_kpa500_follow_requested_bn: Option<u8>,
    pub last_kpa500_follow_confirmed_bn: Option<u8>,
    pub last_kpa500_follow_confirmation_match: Option<bool>,
    pub kpa500_follow_stale_response_count: u64,
    pub kpa500_follow_retry_count: u64,
    pub kpa500_follow_sent_count: u64,
    pub kpa500_follow_skipped_count: u64,
    #[serde(skip)]
    pub updated_at: Option<SystemTime>,
    #[serde(skip)]
    pub last_tune_at: Option<SystemTime>,
}

impl BridgeState {
    pub fn mock() -> Self {
        Self {
            amp: AmpState {
                connected: true,
                connection_state: ConnectionState::Connected,
                operate: false,
                state: AmpOperatingState::Standby,
                forward_power_watts: 0.0,
                reflected_power_watts: 0.0,
                swr: 1.0,
                pa_current_amps: 0.0,
                pa_voltage_volts: 0.0,
                temperature_c: 32.0,
                meffa: "OK".to_string(),
                fault: None,
                warning: None,
                firmware_version: Some("MOCK".to_string()),
                serial_number: Some("MOCK-KPA500".to_string()),
                capabilities: vec!["mock-status".to_string()],
                first_poll_completed: true,
                first_poll_error: None,
                serial_port_open_error: None,
                last_raw_response: None,
                last_successful_command: Some("mock_poll".to_string()),
                startup_state_policy: None,
                advertisement_waiting_for_first_poll: false,
                last_serial_command_at: None,
                last_serial_response_at: None,
                last_successful_poll_at: Some(SystemTime::now()),
                runtime: DeviceRuntimeStats::default(),
            },
            tuner: TunerState {
                connected: true,
                connection_state: ConnectionState::Connected,
                operate: false,
                bypass: false,
                tuning: false,
                selected_antenna: Some(0),
                relay_c1: 20,
                relay_l: 35,
                relay_c2: 20,
                forward_power_watts: 0.0,
                swr: 1.0,
                fault: None,
                firmware_version: Some("MOCK".to_string()),
                serial_number: Some("MOCK-KAT500".to_string()),
                capabilities: vec!["mock-status".to_string()],
                last_serial_command_at: None,
                last_serial_response_at: None,
                last_successful_poll_at: Some(SystemTime::now()),
                runtime: DeviceRuntimeStats::default(),
            },
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmpState {
    pub connected: bool,
    pub connection_state: ConnectionState,
    pub operate: bool,
    pub state: AmpOperatingState,
    pub forward_power_watts: f32,
    pub reflected_power_watts: f32,
    pub swr: f32,
    pub pa_current_amps: f32,
    pub pa_voltage_volts: f32,
    pub temperature_c: f32,
    pub meffa: String,
    pub fault: Option<String>,
    pub warning: Option<String>,
    pub firmware_version: Option<String>,
    pub serial_number: Option<String>,
    pub capabilities: Vec<String>,
    pub first_poll_completed: bool,
    pub first_poll_error: Option<String>,
    pub serial_port_open_error: Option<String>,
    pub last_raw_response: Option<String>,
    pub last_successful_command: Option<String>,
    pub startup_state_policy: Option<String>,
    pub advertisement_waiting_for_first_poll: bool,
    #[serde(skip)]
    pub last_serial_command_at: Option<SystemTime>,
    #[serde(skip)]
    pub last_serial_response_at: Option<SystemTime>,
    #[serde(skip)]
    pub last_successful_poll_at: Option<SystemTime>,
    pub runtime: DeviceRuntimeStats,
}

impl Default for AmpState {
    fn default() -> Self {
        Self {
            connected: false,
            connection_state: ConnectionState::Disconnected,
            operate: false,
            state: AmpOperatingState::Standby,
            forward_power_watts: 0.0,
            reflected_power_watts: 0.0,
            swr: 1.0,
            pa_current_amps: 0.0,
            pa_voltage_volts: 0.0,
            temperature_c: 0.0,
            meffa: "UNKNOWN".to_string(),
            fault: None,
            warning: None,
            firmware_version: None,
            serial_number: None,
            capabilities: Vec::new(),
            first_poll_completed: false,
            first_poll_error: None,
            serial_port_open_error: None,
            last_raw_response: None,
            last_successful_command: None,
            startup_state_policy: None,
            advertisement_waiting_for_first_poll: false,
            last_serial_command_at: None,
            last_serial_response_at: None,
            last_successful_poll_at: None,
            runtime: DeviceRuntimeStats::default(),
        }
    }
}

impl AmpState {
    pub fn is_connected(&self) -> bool {
        self.connection_state.is_healthy()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunerState {
    pub connected: bool,
    pub connection_state: ConnectionState,
    pub operate: bool,
    pub bypass: bool,
    pub tuning: bool,
    pub selected_antenna: Option<u8>,
    pub relay_c1: i32,
    pub relay_l: i32,
    pub relay_c2: i32,
    pub forward_power_watts: f32,
    pub swr: f32,
    pub fault: Option<String>,
    pub firmware_version: Option<String>,
    pub serial_number: Option<String>,
    pub capabilities: Vec<String>,
    #[serde(skip)]
    pub last_serial_command_at: Option<SystemTime>,
    #[serde(skip)]
    pub last_serial_response_at: Option<SystemTime>,
    #[serde(skip)]
    pub last_successful_poll_at: Option<SystemTime>,
    pub runtime: DeviceRuntimeStats,
}

impl Default for TunerState {
    fn default() -> Self {
        Self {
            connected: false,
            connection_state: ConnectionState::Disconnected,
            operate: false,
            bypass: false,
            tuning: false,
            selected_antenna: None,
            relay_c1: 0,
            relay_l: 0,
            relay_c2: 0,
            forward_power_watts: 0.0,
            swr: 1.0,
            fault: None,
            firmware_version: None,
            serial_number: None,
            capabilities: Vec::new(),
            last_serial_command_at: None,
            last_serial_response_at: None,
            last_successful_poll_at: None,
            runtime: DeviceRuntimeStats::default(),
        }
    }
}

impl TunerState {
    pub fn is_connected(&self) -> bool {
        self.connection_state.is_healthy()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceRuntimeStats {
    pub reconnect_count: u64,
    pub poll_success_count: u64,
    pub poll_failure_count: u64,
    pub stale_transition_count: u64,
    pub last_poll_latency_ms: u64,
    pub max_poll_latency_ms: u64,
    pub total_poll_latency_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlexInjectionState {
    pub enabled: bool,
    pub connection_state: ConnectionState,
    pub active_amplifier_status_profile: Option<String>,
    pub active_tgxl_control_profile: Option<String>,
    pub client_handle: Option<String>,
    pub amplifier_handle: Option<String>,
    pub meter_handles: Vec<FlexMeterHandle>,
    pub meter_publish_count: u64,
    pub meter_publish_supported: Option<bool>,
    pub meter_publish_last_result: Option<String>,
    pub interlock_handle: Option<String>,
    pub last_interlock_status_line: Option<String>,
    pub last_interlock_state: Option<String>,
    pub last_interlock_reason: Option<String>,
    pub last_interlock_tx_allowed: Option<bool>,
    pub last_interlock_transition: Option<String>,
    pub last_interlock_transition_at_ms: Option<u128>,
    pub last_tx_allowed_transition: Option<String>,
    pub last_tx_allowed_transition_at_ms: Option<u128>,
    pub interlock_blocked_count: u64,
    pub interlock_disabled_for_test: bool,
    pub interlock_amplifier_field_empty: bool,
    pub interlock_empty_amplifier_count: u64,
    pub amplifier_operable_eligibility: Option<String>,
    pub meter_availability: Option<String>,
    pub external_control_capable_state: Option<String>,
    pub last_command: Option<String>,
    pub last_response: Option<String>,
    pub radio_addr: Option<String>,
    pub tcp_connect_success_count: u64,
    pub last_error: Option<String>,
    pub last_rx_line: Option<String>,
    pub last_tx_line: Option<String>,
    pub client_handle_received: bool,
    pub amplifier_create_sent: bool,
    pub amplifier_create_accepted: bool,
    pub post_amplifier_registration_sent: bool,
    pub registration_continued_without_handle: bool,
    pub keepalive_enable_accepted: bool,
    pub sub_amplifier_all_accepted: bool,
    pub sub_amplifier_all_command_count: u64,
    pub command_success_count: u64,
    pub command_failure_count: u64,
    pub ping_count: u64,
    pub ping_failure_count: u64,
    pub pending_count: usize,
    pub expired_pending_count: u64,
    pub degraded_reason: Option<String>,
    pub tuner_handle: Option<String>,
    pub tuner_appeared_count: u64,
    pub tuner_disappeared_count: u64,
    pub tuner_registration_refresh_count: u64,
    pub tuner_presence_expired_count: u64,
    pub tuner_reannounce_count: u64,
    pub amplifier_reannounce_count: u64,
    pub amplifier_handle_change_count: u64,
    pub amplifier_removed_count: u64,
    pub amplifier_create_count: u64,
    pub duplicate_amplifier_create_count: u64,
    pub duplicate_subscription_count: u64,
    pub duplicate_meter_create_count: u64,
    pub duplicate_interlock_create_count: u64,
    pub amp_widget_visibility_risk: Option<String>,
    pub amplifier_direct_connect_expected: Option<bool>,
    pub last_amplifier_status_line: Option<String>,
    pub amplifier_object_seen_at_ms: Option<u128>,
    pub last_emitted_amplifier_advertisement_line: Option<String>,
    pub last_emitted_amplifier_advertisement_kind: Option<String>,
    pub amplifier_pairing_candidate_fields: Vec<String>,
    pub amplifier_pgxl_tcp_attempted_after_status: bool,
    pub amplifier_reannounce_requested: bool,
    pub amplifier_reannounce_request_count: u64,
    pub last_amplifier_reannounce_request_reason: Option<String>,
    pub last_amplifier_reannounce_reason: Option<String>,
    pub last_amplifier_removed_reason: Option<String>,
    pub amplifier_recreate_reason: Option<String>,
    pub last_lifecycle_event: Option<String>,
    pub last_lifecycle_event_at_ms: Option<u128>,
    pub last_advertised_flex_amp_state: Option<String>,
    pub last_advertised_pgxl_state: Option<String>,
    pub last_advertised_tgxl_operate: Option<bool>,
    pub last_kpa_state_change_detected_at_ms: Option<u128>,
    pub last_kpa_state_change_state: Option<String>,
    pub last_pgxl_status_state_at_ms: Option<u128>,
    pub last_pgxl_status_state: Option<String>,
    pub last_flex_reannounce_sent_at_ms: Option<u128>,
    pub last_flex_reannounce_state: Option<String>,
    pub state_advertisement_mismatch: Option<String>,
    pub state_advertisement_mismatch_count: u64,
    pub flex_desired_amp_state: Option<String>,
    pub radio_rewritten_amp_state: Option<String>,
    pub flex_force_operate_via_radio: bool,
    pub flex_operate_lab_command_count: u64,
    pub flex_operate_lab_accept_count: u64,
    pub pgxl_connect_assist_enabled: bool,
    pub pgxl_connect_assist_sent_count: u64,
    pub pgxl_connect_assist_last_result: Option<String>,
    pub pgxl_connect_assist_triggered_tcp: bool,
    pub pgxl_connect_assist_retry_count: u64,
    pub ping_ack_count: u64,
    pub last_ping_latency_ms: Option<u64>,
    pub last_tuner_disappearance_reason: Option<String>,
    #[serde(skip)]
    pub tuner_last_seen_at: Option<SystemTime>,
    #[serde(skip)]
    pub amplifier_last_seen_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlexMeterHandle {
    pub name: String,
    pub handle: String,
}

impl DeviceRuntimeStats {
    pub fn record_poll_success(&mut self, latency_ms: u64) {
        self.poll_success_count = self.poll_success_count.saturating_add(1);
        self.last_poll_latency_ms = latency_ms;
        self.max_poll_latency_ms = self.max_poll_latency_ms.max(latency_ms);
        self.total_poll_latency_ms = self.total_poll_latency_ms.saturating_add(latency_ms);
    }

    pub fn average_poll_latency_ms(&self) -> Option<u64> {
        if self.poll_success_count == 0 {
            None
        } else {
            Some(self.total_poll_latency_ms / self.poll_success_count)
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientState {
    pub pgxl_connected: bool,
    pub tgxl_connected: bool,
    pub pgxl_client_count: usize,
    pub tgxl_client_count: usize,
    pub pgxl_listener_ready_at_ms: Option<u128>,
    pub tgxl_listener_ready_at_ms: Option<u128>,
    pub pgxl_first_accept_at_ms: Option<u128>,
    pub tgxl_first_accept_at_ms: Option<u128>,
    pub pgxl_session_started_count: u64,
    pub tgxl_session_started_count: u64,
    pub next_session_id: u64,
    pub pgxl_sessions: Vec<ProtocolClientSession>,
    pub tgxl_sessions: Vec<ProtocolClientSession>,
    pub pgxl_last_disconnect_reason: Option<String>,
    pub tgxl_last_disconnect_reason: Option<String>,
    pub pgxl_manual_connect_no_socket_attempt_count: u64,
    pub pgxl_last_no_socket_attempt_warning: Option<String>,
    pub pgxl_last_no_socket_warning_at_ms: Option<u128>,
    pub pgxl_reannounce_count_at_first_accept: Option<u64>,
    pub pgxl_sub_amp_all_count_at_first_accept: Option<u64>,
    pub pgxl_last_amp_status_before_accept: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolClientSession {
    pub id: u64,
    pub protocol: String,
    pub peer: String,
    pub connected_at_ms: u128,
    pub last_command: Option<String>,
    pub commands_received: u64,
    pub responses_sent: u64,
    pub parse_failures: u64,
    pub unknown_commands: u64,
    pub last_response_latency_ms: u64,
    pub max_response_latency_ms: u64,
}

impl ProtocolClientSession {
    pub fn new(id: u64, protocol: &str, peer: impl ToString, connected_at_ms: u128) -> Self {
        Self {
            id,
            protocol: protocol.to_string(),
            peer: peer.to_string(),
            connected_at_ms,
            last_command: None,
            commands_received: 0,
            responses_sent: 0,
            parse_failures: 0,
            unknown_commands: 0,
            last_response_latency_ms: 0,
            max_response_latency_ms: 0,
        }
    }

    pub fn record_command(&mut self, command: &str) {
        self.commands_received = self.commands_received.saturating_add(1);
        self.last_command = Some(command.to_string());
    }

    pub fn record_response(&mut self, latency_ms: u64) {
        self.responses_sent = self.responses_sent.saturating_add(1);
        self.last_response_latency_ms = latency_ms;
        self.max_response_latency_ms = self.max_response_latency_ms.max(latency_ms);
    }
}

pub fn push_capability(capabilities: &mut Vec<String>, capability: &str) {
    if !capabilities.iter().any(|existing| existing == capability) {
        capabilities.push(capability.to_string());
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DesiredState {
    pub amp_operate: Option<bool>,
    pub tuner_autotune_requested: bool,
    pub tuner_selected_antenna: Option<u8>,
    pub tuner_bypass: Option<bool>,
    pub tuner_manual_tune: Option<ManualTuneRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualTuneRequest {
    pub relay: u8,
    pub movement: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtocolCounters {
    pub pgxl: ProtocolCounterSet,
    pub tgxl: ProtocolCounterSet,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlDiagnostics {
    pub aethersdr_button_command_seen: bool,
    pub last_tgxl_control_command: Option<String>,
    pub last_pgxl_control_command: Option<String>,
    pub last_flex_amp_set_command: Option<String>,
    pub last_mapped_elecraft_action: Option<String>,
    pub last_executed_elecraft_command: Option<String>,
    pub last_safety_decision: Option<String>,
    pub blocked_by_dry_run_count: u64,
    pub blocked_by_rf_risk_count: u64,
    pub control_requested_count: u64,
    pub duplicate_autotune_suppressed_count: u64,
    pub last_tune_frequency_hz: Option<u64>,
    pub last_tune_band: Option<String>,
    pub tune_requested_count: u64,
    pub tune_executed_count: u64,
    pub tune_failed_count: u64,
    pub last_tune_result: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EffectiveControlPolicy {
    pub raw_kpa_dry_run: bool,
    pub raw_kpa_allow_control: bool,
    pub raw_kpa_allow_rf_risk: bool,
    pub raw_kat_dry_run: bool,
    pub raw_kat_allow_control: bool,
    pub raw_kat_allow_rf_risk: bool,
    pub operational_enabled: bool,
    pub operational_confirmation_valid: bool,
    pub operational_override_active: bool,
    pub effective_kat_tune_enabled: bool,
    pub effective_kat_bypass_enabled: bool,
    pub effective_kat_antenna_enabled: bool,
    pub effective_kpa_standby_enabled: bool,
    pub effective_kpa_operate_enabled: bool,
    pub effective_clear_fault_enabled: bool,
    pub effective_kpa_dry_run: bool,
    pub effective_kpa_allow_control: bool,
    pub effective_kpa_allow_rf_risk: bool,
    pub effective_kat_dry_run: bool,
    pub effective_kat_allow_control: bool,
    pub effective_kat_allow_rf_risk: bool,
    pub kat_tune_reason: String,
    pub kat_bypass_reason: String,
    pub kat_antenna_reason: String,
    pub kpa_standby_reason: String,
    pub kpa_operate_reason: String,
    pub clear_fault_reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeConfigIdentity {
    pub config_path: Option<String>,
    pub config_hash: Option<String>,
    pub config_loaded_at_ms: Option<u128>,
    pub config_source_hash: Option<String>,
    pub config_effective_hash: Option<String>,
    pub config_hash_match: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtocolCounterSet {
    pub commands_received: u64,
    pub responses_sent: u64,
    pub parse_failures: u64,
    pub unknown_commands: u64,
    pub unsupported_features: u64,
    pub unexpected_sequences: u64,
}

pub fn shared_mock_state() -> SharedState {
    Arc::new(RwLock::new(BridgeState::mock()))
}

pub fn shared_default_state() -> SharedState {
    Arc::new(RwLock::new(BridgeState::default()))
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_stats_track_latency_and_counts() {
        let mut stats = DeviceRuntimeStats::default();
        assert_eq!(stats.average_poll_latency_ms(), None);

        stats.record_poll_success(100);
        stats.record_poll_success(300);

        assert_eq!(stats.poll_success_count, 2);
        assert_eq!(stats.last_poll_latency_ms, 300);
        assert_eq!(stats.max_poll_latency_ms, 300);
        assert_eq!(stats.average_poll_latency_ms(), Some(200));
    }

    #[test]
    fn derives_ham_band_from_frequency() {
        assert_eq!(Band::from_frequency_hz(1_900_000), Band::M160);
        assert_eq!(Band::from_frequency_hz(3_800_000), Band::M80);
        assert_eq!(Band::from_frequency_hz(7_100_000), Band::M40);
        assert_eq!(Band::from_frequency_hz(14_200_000), Band::M20);
        assert_eq!(Band::from_frequency_hz(18_100_000), Band::M17);
        assert_eq!(Band::from_frequency_hz(21_200_000), Band::M15);
        assert_eq!(Band::from_frequency_hz(24_930_000), Band::M12);
        assert_eq!(Band::from_frequency_hz(28_500_000), Band::M10);
        assert_eq!(Band::from_frequency_hz(50_300_000), Band::M6);
        assert_eq!(Band::from_frequency_hz(11_000_000), Band::Unknown);
    }
}
