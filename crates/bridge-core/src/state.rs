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
    pub amp: AmpState,
    pub tuner: TunerState,
    pub flex_injection: FlexInjectionState,
    pub clients: ClientState,
    pub desired: DesiredState,
    pub protocol: ProtocolCounters,
}

impl Default for BridgeState {
    fn default() -> Self {
        Self {
            frequency_hz: 14_200_000,
            band: Band::M20,
            amp: AmpState::default(),
            tuner: TunerState::default(),
            flex_injection: FlexInjectionState::default(),
            clients: ClientState::default(),
            desired: DesiredState::default(),
            protocol: ProtocolCounters::default(),
        }
    }
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
    pub client_handle: Option<String>,
    pub amplifier_handle: Option<String>,
    pub meter_handles: Vec<FlexMeterHandle>,
    pub interlock_handle: Option<String>,
    pub last_command: Option<String>,
    pub last_response: Option<String>,
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
    pub amplifier_direct_connect_expected: Option<bool>,
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
    pub pgxl_session_started_count: u64,
    pub tgxl_session_started_count: u64,
    pub next_session_id: u64,
    pub pgxl_sessions: Vec<ProtocolClientSession>,
    pub tgxl_sessions: Vec<ProtocolClientSession>,
    pub pgxl_last_disconnect_reason: Option<String>,
    pub tgxl_last_disconnect_reason: Option<String>,
    pub pgxl_manual_connect_no_socket_attempt_count: u64,
    pub pgxl_last_no_socket_attempt_warning: Option<String>,
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
}
