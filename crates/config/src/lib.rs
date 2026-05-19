use serde::{Deserialize, Serialize};
use std::fs;
use std::net::IpAddr;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse YAML config {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("invalid config: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BridgeConfig {
    pub server: ServerConfig,
    pub pgxl: PgxlConfig,
    pub tgxl: TgxlConfig,
    pub kpa500: SerialDeviceConfig,
    pub kat500: SerialDeviceConfig,
    pub security: SecurityConfig,
    pub logging: LoggingConfig,
    pub metrics: MetricsConfig,
    pub control: ControlConfig,
    pub flex_injection: FlexInjectionConfig,
    pub mock: MockConfig,
}

impl BridgeConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path_ref = path.as_ref();
        let path_str = path_ref.display().to_string();
        let text = fs::read_to_string(path_ref).map_err(|source| ConfigError::Read {
            path: path_str.clone(),
            source,
        })?;
        let cfg: Self = serde_yaml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path_str,
            source,
        })?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.server.bind_ip.parse::<IpAddr>().map_err(|_| {
            ConfigError::Invalid(format!(
                "server.bind_ip is not an IP address: {}",
                self.server.bind_ip
            ))
        })?;
        validate_port("pgxl.port", self.pgxl.port)?;
        validate_port("tgxl.port", self.tgxl.port)?;
        self.pgxl.validate()?;
        self.tgxl.validate()?;
        if self.metrics.enabled {
            self.metrics.bind_ip.parse::<IpAddr>().map_err(|_| {
                ConfigError::Invalid(format!(
                    "metrics.bind_ip is not an IP address: {}",
                    self.metrics.bind_ip
                ))
            })?;
            validate_port("metrics.port", self.metrics.port)?;
        }
        self.flex_injection.validate()?;
        self.kpa500.validate("kpa500")?;
        self.kat500.validate("kat500")?;
        if self.control.verify_delay_ms == 0 {
            return Err(ConfigError::Invalid(
                "control.verify_delay_ms must be > 0".to_string(),
            ));
        }
        if self.logging.transcript_rotate_bytes == 0 {
            return Err(ConfigError::Invalid(
                "logging.transcript_rotate_bytes must be > 0".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            pgxl: PgxlConfig::default(),
            tgxl: TgxlConfig::default(),
            kpa500: SerialDeviceConfig {
                enabled: true,
                com_port: "COM21".to_string(),
                baud: 38_400,
                polling_interval_ms: 1000,
                mock: true,
                dry_run: true,
                allow_rf_risk: false,
            },
            kat500: SerialDeviceConfig {
                enabled: true,
                com_port: "COM8".to_string(),
                baud: 38_400,
                polling_interval_ms: 1000,
                mock: true,
                dry_run: true,
                allow_rf_risk: false,
            },
            security: SecurityConfig::default(),
            logging: LoggingConfig::default(),
            metrics: MetricsConfig::default(),
            control: ControlConfig::default(),
            flex_injection: FlexInjectionConfig::default(),
            mock: MockConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind_ip: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_ip: "127.0.0.1".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PgxlConfig {
    pub enabled: bool,
    pub port: u16,
    pub aethersdr_compat: bool,
    pub compat_profile: String,
    pub strict_emulation: bool,
    pub startup_delay_ms: u64,
    pub force_direct_connected_test: bool,
}

impl PgxlConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        match self.compat_profile.as_str() {
            "strict" | "aethersdr" | "smartsdr" | "permissive" => Ok(()),
            other => Err(ConfigError::Invalid(format!(
                "pgxl.compat_profile must be one of strict, aethersdr, smartsdr, permissive; got {other}"
            ))),
        }
    }
}

impl Default for PgxlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 9008,
            aethersdr_compat: false,
            compat_profile: "aethersdr".to_string(),
            strict_emulation: false,
            startup_delay_ms: 0,
            force_direct_connected_test: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TgxlConfig {
    pub enabled: bool,
    pub port: u16,
    pub aethersdr_compat: bool,
    pub smartsdr_compat: bool,
    pub control_profile: String,
    pub strict_emulation: bool,
    pub startup_delay_ms: u64,
    pub force_presence_test: bool,
    pub experimental_presence_refresh: bool,
}

impl TgxlConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        match self.control_profile.as_str() {
            "readonly" | "tgxl_control_ready" | "tgxl_verbose_control" => Ok(()),
            other => Err(ConfigError::Invalid(format!(
                "tgxl.control_profile must be one of readonly, tgxl_control_ready, tgxl_verbose_control; got {other}"
            ))),
        }
    }
}

impl Default for TgxlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 9010,
            aethersdr_compat: false,
            smartsdr_compat: false,
            control_profile: "readonly".to_string(),
            strict_emulation: false,
            startup_delay_ms: 0,
            force_presence_test: false,
            experimental_presence_refresh: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MockConfig {
    pub pgxl_fault: bool,
    pub tgxl_fault: bool,
    pub high_swr: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SerialDeviceConfig {
    pub enabled: bool,
    pub com_port: String,
    pub baud: u32,
    pub polling_interval_ms: u64,
    pub mock: bool,
    pub dry_run: bool,
    pub allow_rf_risk: bool,
}

impl SerialDeviceConfig {
    fn validate(&self, prefix: &str) -> Result<(), ConfigError> {
        if self.enabled && !self.mock && self.com_port.trim().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "{prefix}.com_port is required when enabled and mock=false"
            )));
        }
        if self.baud == 0 {
            return Err(ConfigError::Invalid(format!("{prefix}.baud must be > 0")));
        }
        if self.polling_interval_ms == 0 {
            return Err(ConfigError::Invalid(format!(
                "{prefix}.polling_interval_ms must be > 0"
            )));
        }
        Ok(())
    }
}

impl Default for SerialDeviceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            com_port: String::new(),
            baud: 38_400,
            polling_interval_ms: 1000,
            mock: true,
            dry_run: true,
            allow_rf_risk: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityConfig {
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
    pub protocol_trace: bool,
    pub protocol_transcript_dir: Option<String>,
    pub serial_transcript_dir: Option<String>,
    pub transcript_rotate_bytes: u64,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            protocol_trace: false,
            protocol_transcript_dir: None,
            serial_transcript_dir: None,
            transcript_rotate_bytes: 1_048_576,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub bind_ip: String,
    pub port: u16,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_ip: "127.0.0.1".to_string(),
            port: 9160,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlConfig {
    pub verify_delay_ms: u64,
}

impl Default for ControlConfig {
    fn default() -> Self {
        Self {
            verify_delay_ms: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FlexInjectionConfig {
    pub enabled: bool,
    pub radio_ip: String,
    pub radio_port: u16,
    pub amplifier_ip: String,
    pub amplifier_port: u16,
    pub amplifier_model: String,
    pub serial: String,
    pub handle: String,
    pub ant_map: String,
    pub full_pgxl_registration: bool,
    pub create_meters: bool,
    pub create_interlock: bool,
    pub amplifier_status_profile: String,
    pub amplifier_reannounce_interval_ms: u64,
    pub reconnect_initial_ms: u64,
    pub reconnect_max_ms: u64,
    pub ping_interval_ms: u64,
    pub tuner_refresh_interval_ms: u64,
}

impl FlexInjectionConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if !self.enabled {
            return Ok(());
        }
        let radio_ip = parse_ip("flex_injection.radio_ip", &self.radio_ip)?;
        let amplifier_ip = parse_ip("flex_injection.amplifier_ip", &self.amplifier_ip)?;
        validate_lan_or_loopback("flex_injection.radio_ip", radio_ip)?;
        validate_lan_or_loopback("flex_injection.amplifier_ip", amplifier_ip)?;
        validate_port("flex_injection.radio_port", self.radio_port)?;
        validate_port("flex_injection.amplifier_port", self.amplifier_port)?;
        validate_nonempty_token("flex_injection.amplifier_model", &self.amplifier_model)?;
        validate_nonempty_token("flex_injection.serial", &self.serial)?;
        validate_nonempty_token("flex_injection.handle", &self.handle)?;
        validate_nonempty_token("flex_injection.ant_map", &self.ant_map)?;
        self.validate_status_profile()?;
        if self.amplifier_reannounce_interval_ms == 0 {
            return Err(ConfigError::Invalid(
                "flex_injection.amplifier_reannounce_interval_ms must be > 0".to_string(),
            ));
        }
        if self.reconnect_initial_ms == 0 {
            return Err(ConfigError::Invalid(
                "flex_injection.reconnect_initial_ms must be > 0".to_string(),
            ));
        }
        if self.reconnect_max_ms < self.reconnect_initial_ms {
            return Err(ConfigError::Invalid(
                "flex_injection.reconnect_max_ms must be >= reconnect_initial_ms".to_string(),
            ));
        }
        if self.ping_interval_ms == 0 {
            return Err(ConfigError::Invalid(
                "flex_injection.ping_interval_ms must be > 0".to_string(),
            ));
        }
        if self.tuner_refresh_interval_ms == 0 {
            return Err(ConfigError::Invalid(
                "flex_injection.tuner_refresh_interval_ms must be > 0".to_string(),
            ));
        }
        Ok(())
    }

    fn validate_status_profile(&self) -> Result<(), ConfigError> {
        match self.amplifier_status_profile.as_str() {
            "minimal" | "pgxl_paired" | "pgxl_verbose" | "aethersdr_force_direct" => Ok(()),
            other => Err(ConfigError::Invalid(format!(
                "flex_injection.amplifier_status_profile must be one of minimal, pgxl_paired, pgxl_verbose, aethersdr_force_direct; got {other}"
            ))),
        }
    }
}

impl Default for FlexInjectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            radio_ip: "127.0.0.1".to_string(),
            radio_port: 4992,
            amplifier_ip: "127.0.0.1".to_string(),
            amplifier_port: 9008,
            amplifier_model: "PowerGeniusXL".to_string(),
            serial: "EGB-KPA500".to_string(),
            handle: "amp_1".to_string(),
            ant_map: "ANT1:PORTA,ANT2:PORTB".to_string(),
            full_pgxl_registration: true,
            create_meters: true,
            create_interlock: true,
            amplifier_status_profile: "pgxl_paired".to_string(),
            amplifier_reannounce_interval_ms: 5000,
            reconnect_initial_ms: 1000,
            reconnect_max_ms: 30000,
            ping_interval_ms: 30000,
            tuner_refresh_interval_ms: 5000,
        }
    }
}

fn validate_port(name: &str, port: u16) -> Result<(), ConfigError> {
    if port == 0 {
        return Err(ConfigError::Invalid(format!(
            "{name} must be between 1 and 65535"
        )));
    }
    Ok(())
}

fn parse_ip(name: &str, value: &str) -> Result<IpAddr, ConfigError> {
    value
        .parse::<IpAddr>()
        .map_err(|_| ConfigError::Invalid(format!("{name} is not an IP address: {value}")))
}

pub fn is_lan_or_loopback_or_cgnat(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
        }
        IpAddr::V6(ip) => {
            let first = ip.segments()[0];
            ip.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

fn validate_lan_or_loopback(name: &str, ip: IpAddr) -> Result<(), ConfigError> {
    if is_lan_or_loopback_or_cgnat(ip) {
        Ok(())
    } else {
        Err(ConfigError::Invalid(format!(
            "{name} must be loopback, private LAN, link-local, or CGNAT for Phase 17 injection"
        )))
    }
}

fn validate_nonempty_token(name: &str, value: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::Invalid(format!("{name} must not be empty")));
    }
    if value.contains('|') || value.chars().any(char::is_whitespace) {
        return Err(ConfigError::Invalid(format!(
            "{name} must not contain whitespace or pipe characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_validates() {
        BridgeConfig::default().validate().unwrap();
    }

    #[test]
    fn rejects_bad_bind_ip() {
        let mut cfg = BridgeConfig::default();
        cfg.server.bind_ip = "not-an-ip".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn parses_yaml() {
        let cfg: BridgeConfig = serde_yaml::from_str(
            r#"
server:
  bind_ip: 127.0.0.1
pgxl:
  port: 9008
"#,
        )
        .unwrap();
        assert_eq!(cfg.pgxl.port, 9008);
        assert_eq!(cfg.tgxl.port, 9010);
    }

    #[test]
    fn validates_pgxl_compat_profile() {
        let mut cfg = BridgeConfig::default();
        cfg.pgxl.compat_profile = "smartsdr".to_string();
        cfg.validate().unwrap();

        cfg.pgxl.compat_profile = "unknown".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validates_lan_only_flex_injection() {
        let mut cfg = BridgeConfig::default();
        cfg.flex_injection.enabled = true;
        cfg.flex_injection.radio_ip = "8.8.8.8".to_string();
        assert!(cfg.validate().is_err());

        cfg.flex_injection.radio_ip = "192.168.1.100".to_string();
        cfg.flex_injection.amplifier_ip = "192.168.1.50".to_string();
        cfg.validate().unwrap();
    }

    #[test]
    fn lan_scope_helper_covers_loopback_private_link_local_and_cgnat() {
        assert!(is_lan_or_loopback_or_cgnat("127.0.0.1".parse().unwrap()));
        assert!(is_lan_or_loopback_or_cgnat("192.168.1.10".parse().unwrap()));
        assert!(is_lan_or_loopback_or_cgnat(
            "169.254.10.20".parse().unwrap()
        ));
        assert!(is_lan_or_loopback_or_cgnat("100.64.1.1".parse().unwrap()));
        assert!(is_lan_or_loopback_or_cgnat("::1".parse().unwrap()));
        assert!(is_lan_or_loopback_or_cgnat("fd00::1".parse().unwrap()));
        assert!(is_lan_or_loopback_or_cgnat("fe80::1".parse().unwrap()));
        assert!(!is_lan_or_loopback_or_cgnat("8.8.8.8".parse().unwrap()));
        assert!(!is_lan_or_loopback_or_cgnat(
            "2001:4860:4860::8888".parse().unwrap()
        ));
    }
}
