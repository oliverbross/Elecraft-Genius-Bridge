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
        self.kpa500.validate("kpa500")?;
        self.kat500.validate("kat500")?;
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
            },
            kat500: SerialDeviceConfig {
                enabled: true,
                com_port: "COM8".to_string(),
                baud: 38_400,
                polling_interval_ms: 1000,
                mock: true,
            },
            security: SecurityConfig::default(),
            logging: LoggingConfig::default(),
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
}

impl Default for PgxlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 9008,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TgxlConfig {
    pub enabled: bool,
    pub port: u16,
}

impl Default for TgxlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 9010,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SerialDeviceConfig {
    pub enabled: bool,
    pub com_port: String,
    pub baud: u32,
    pub polling_interval_ms: u64,
    pub mock: bool,
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
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            protocol_trace: false,
            protocol_transcript_dir: None,
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
}
