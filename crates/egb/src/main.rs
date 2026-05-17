use anyhow::{Context, Result};
use bridge_core::state::{shared_default_state, shared_mock_state};
use clap::{Parser, Subcommand};
use egb_config::BridgeConfig;
use elecraft_kat500::{Kat500Driver, Kat500Settings};
use elecraft_kpa500::{Kpa500Driver, Kpa500Settings};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;
use tokio_serial::{SerialPortBuilderExt, SerialPortType};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "egb", version, about = "Elecraft Genius Bridge")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Run {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    CheckConfig {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    ListSerial,
    TestKpa {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    TestKat {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    ReplayPgxl {
        #[arg(long, default_value = "127.0.0.1:9008")]
        bind: SocketAddr,
    },
    ReplayTgxl {
        #[arg(long, default_value = "127.0.0.1:9010")]
        bind: SocketAddr,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run { config } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_bridge(cfg).await
        }
        Commands::CheckConfig { config } => {
            let cfg = BridgeConfig::load(&config)?;
            println!("config OK: {}", config.display());
            println!(
                "bind={} pgxl={} tgxl={} kpa_mock={} kat_mock={}",
                cfg.server.bind_ip, cfg.pgxl.port, cfg.tgxl.port, cfg.kpa500.mock, cfg.kat500.mock
            );
            Ok(())
        }
        Commands::ListSerial => {
            list_serial_ports()?;
            Ok(())
        }
        Commands::TestKpa { config } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            test_serial_device(
                "KPA500",
                &cfg.kpa500.com_port,
                cfg.kpa500.baud,
                cfg.kpa500.mock,
            )
            .await
        }
        Commands::TestKat { config } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            test_serial_device(
                "KAT500",
                &cfg.kat500.com_port,
                cfg.kat500.baud,
                cfg.kat500.mock,
            )
            .await
        }
        Commands::ReplayPgxl { bind } => {
            init_logging("debug");
            test_harness::replay_pgxl(bind).await
        }
        Commands::ReplayTgxl { bind } => {
            init_logging("debug");
            test_harness::replay_tgxl(bind).await
        }
    }
}

async fn run_bridge(cfg: BridgeConfig) -> Result<()> {
    let all_mock = cfg.kpa500.mock && cfg.kat500.mock;
    let state = if all_mock {
        shared_mock_state()
    } else {
        shared_default_state()
    };

    if cfg.kpa500.enabled {
        let driver = Kpa500Driver::new(
            Kpa500Settings {
                com_port: cfg.kpa500.com_port.clone(),
                baud: cfg.kpa500.baud,
                polling_interval: Duration::from_millis(cfg.kpa500.polling_interval_ms),
                mock: cfg.kpa500.mock,
            },
            state.clone(),
        );
        tokio::spawn(driver.run());
    }

    if cfg.kat500.enabled {
        let driver = Kat500Driver::new(
            Kat500Settings {
                com_port: cfg.kat500.com_port.clone(),
                baud: cfg.kat500.baud,
                polling_interval: Duration::from_millis(cfg.kat500.polling_interval_ms),
                mock: cfg.kat500.mock,
            },
            state.clone(),
        );
        tokio::spawn(driver.run());
    }

    let bind_ip: IpAddr = cfg
        .server
        .bind_ip
        .parse()
        .context("server.bind_ip passed validation but failed to parse")?;

    if cfg.pgxl.enabled {
        let addr = SocketAddr::new(bind_ip, cfg.pgxl.port);
        let state = state.clone();
        let options = pgxl_emulator::EmulatorOptions {
            protocol_trace: cfg.logging.protocol_trace,
            transcript_dir: cfg
                .logging
                .protocol_transcript_dir
                .as_ref()
                .map(PathBuf::from),
        };
        tokio::spawn(async move {
            if let Err(err) = pgxl_emulator::run_with_options(addr, state, options).await {
                error!(error = %err, "PGXL emulator stopped");
            }
        });
    }

    if cfg.tgxl.enabled {
        let addr = SocketAddr::new(bind_ip, cfg.tgxl.port);
        let state = state.clone();
        let options = tgxl_emulator::EmulatorOptions {
            protocol_trace: cfg.logging.protocol_trace,
            transcript_dir: cfg
                .logging
                .protocol_transcript_dir
                .as_ref()
                .map(PathBuf::from),
        };
        tokio::spawn(async move {
            if let Err(err) = tgxl_emulator::run_with_options(addr, state, options).await {
                error!(error = %err, "TGXL emulator stopped");
            }
        });
    }

    info!("Elecraft Genius Bridge running; press Ctrl+C to stop");
    tokio::signal::ctrl_c()
        .await
        .context("failed waiting for Ctrl+C")?;
    info!("shutdown requested");
    Ok(())
}

fn init_logging(level: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn list_serial_ports() -> Result<()> {
    let ports = tokio_serial::available_ports().context("failed to enumerate serial ports")?;
    if ports.is_empty() {
        println!("no serial ports found");
        return Ok(());
    }
    for port in ports {
        let kind = match port.port_type {
            SerialPortType::UsbPort(info) => {
                format!("USB vid={:04x} pid={:04x}", info.vid, info.pid)
            }
            SerialPortType::BluetoothPort => "Bluetooth".to_string(),
            SerialPortType::PciPort => "PCI".to_string(),
            SerialPortType::Unknown => "Unknown".to_string(),
        };
        println!("{} {}", port.port_name, kind);
    }
    Ok(())
}

async fn test_serial_device(name: &str, port: &str, baud: u32, mock: bool) -> Result<()> {
    if mock {
        println!("{name} mock mode OK");
        return Ok(());
    }
    let _port = tokio_serial::new(port.to_string(), baud)
        .open_native_async()
        .with_context(|| format!("{name} failed to open {port} at {baud} baud"))?;
    println!("{name} opened {port} at {baud} baud");
    Ok(())
}
