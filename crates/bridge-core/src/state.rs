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
        }
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
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientState {
    pub pgxl_connected: bool,
    pub tgxl_connected: bool,
    pub pgxl_client_count: usize,
    pub tgxl_client_count: usize,
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
