use anyhow::{Context, Result};
use bridge_core::state::{shared_default_state, shared_mock_state};
use bridge_core::{ConnectionState, SharedState};
use clap::{Parser, Subcommand};
use egb_config::BridgeConfig;
use elecraft_kat500::{
    command_map as kat_command_map, CommandSafety as KatCommandSafety, Kat500Driver, Kat500Settings,
};
use elecraft_kpa500::{
    command_map as kpa_command_map, CommandSafety as KpaCommandSafety, Kpa500Driver, Kpa500Settings,
};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_serial::SerialPortType;
use tracing::{error, info, warn};
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
        #[arg(long)]
        allow_control: bool,
        #[arg(long)]
        allow_rf_risk: bool,
    },
    TestKat {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long)]
        allow_control: bool,
        #[arg(long)]
        allow_rf_risk: bool,
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
        Commands::TestKpa {
            config,
            allow_control,
            allow_rf_risk,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            test_kpa(&cfg, allow_control, allow_rf_risk).await
        }
        Commands::TestKat {
            config,
            allow_control,
            allow_rf_risk,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            test_kat(&cfg, allow_control, allow_rf_risk).await
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
                dry_run: cfg.kpa500.dry_run,
                transcript_dir: cfg
                    .logging
                    .serial_transcript_dir
                    .as_ref()
                    .map(PathBuf::from),
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
                dry_run: cfg.kat500.dry_run,
                transcript_dir: cfg
                    .logging
                    .serial_transcript_dir
                    .as_ref()
                    .map(PathBuf::from),
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

    if cfg.metrics.enabled {
        let bind_ip: IpAddr = cfg
            .metrics
            .bind_ip
            .parse()
            .context("metrics.bind_ip passed validation but failed to parse")?;
        let addr = SocketAddr::new(bind_ip, cfg.metrics.port);
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(err) = run_metrics_endpoint(addr, state).await {
                error!(error = %err, "metrics endpoint stopped");
            }
        });
    }

    {
        let state = state.clone();
        let amp_after = Duration::from_millis(cfg.kpa500.polling_interval_ms.saturating_mul(3));
        let tuner_after = Duration::from_millis(cfg.kat500.polling_interval_ms.saturating_mul(3));
        tokio::spawn(async move {
            stale_state_watchdog(
                state,
                amp_after.max(Duration::from_secs(3)),
                tuner_after.max(Duration::from_secs(3)),
            )
            .await;
        });
    }

    info!("Elecraft Genius Bridge running; press Ctrl+C to stop");
    tokio::signal::ctrl_c()
        .await
        .context("failed waiting for Ctrl+C")?;
    info!("shutdown requested");
    Ok(())
}

async fn stale_state_watchdog(
    state: SharedState,
    amp_stale_after: Duration,
    tuner_stale_after: Duration,
) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let now = SystemTime::now();
        let mut guard = state.write().await;
        if guard.amp.connection_state == ConnectionState::Connected
            && is_stale(guard.amp.last_successful_poll_at, now, amp_stale_after)
        {
            guard.amp.connection_state = ConnectionState::Degraded;
            guard.amp.connected = false;
            warn!(
                event_id = "stale_device_state",
                device = "KPA500",
                stale_after_ms = amp_stale_after.as_millis(),
                "KPA500 state degraded because polling timestamp is stale"
            );
        }
        if guard.tuner.connection_state == ConnectionState::Connected
            && is_stale(guard.tuner.last_successful_poll_at, now, tuner_stale_after)
        {
            guard.tuner.connection_state = ConnectionState::Degraded;
            guard.tuner.connected = false;
            warn!(
                event_id = "stale_device_state",
                device = "KAT500",
                stale_after_ms = tuner_stale_after.as_millis(),
                "KAT500 state degraded because polling timestamp is stale"
            );
        }
    }
}

fn is_stale(last_poll: Option<SystemTime>, now: SystemTime, stale_after: Duration) -> bool {
    last_poll
        .and_then(|last| now.duration_since(last).ok())
        .is_some_and(|elapsed| elapsed > stale_after)
}

async fn run_metrics_endpoint(addr: SocketAddr, state: SharedState) -> Result<()> {
    if !addr.ip().is_loopback() {
        anyhow::bail!("metrics endpoint must bind to a loopback address");
    }
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind metrics endpoint on {addr}"))?;
    info!(%addr, "metrics endpoint listening");
    loop {
        let (mut socket, peer) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            let mut request = [0_u8; 1024];
            let read = socket.read(&mut request).await.unwrap_or_default();
            let request = String::from_utf8_lossy(&request[..read]);
            let status = if request.starts_with("GET /status ") {
                let body = status_json(&state).await;
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
            } else {
                let body = "{\"error\":\"not_found\"}";
                format!(
                    "HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
            };
            if let Err(err) = socket.write_all(status.as_bytes()).await {
                warn!(%peer, error = %err, "failed to write metrics response");
            }
        });
    }
}

async fn status_json(state: &SharedState) -> String {
    let guard = state.read().await;
    serde_json::json!({
        "amp": {
            "connection_state": guard.amp.connection_state.as_str(),
            "connected": guard.amp.connected,
            "firmware_version": guard.amp.firmware_version,
            "capabilities": guard.amp.capabilities,
            "last_successful_poll_ms": system_time_ms(guard.amp.last_successful_poll_at),
        },
        "tuner": {
            "connection_state": guard.tuner.connection_state.as_str(),
            "connected": guard.tuner.connected,
            "firmware_version": guard.tuner.firmware_version,
            "capabilities": guard.tuner.capabilities,
            "last_successful_poll_ms": system_time_ms(guard.tuner.last_successful_poll_at),
        },
        "clients": {
            "pgxl_client_count": guard.clients.pgxl_client_count,
            "tgxl_client_count": guard.clients.tgxl_client_count,
        },
        "protocol": guard.protocol,
    })
    .to_string()
}

fn system_time_ms(value: Option<SystemTime>) -> Option<u128> {
    value.and_then(|time| {
        time.duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_millis())
    })
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

async fn test_kpa(cfg: &BridgeConfig, allow_control: bool, allow_rf_risk: bool) -> Result<()> {
    println!("KPA500 safety summary:");
    println!(
        "  port={} baud={} mock={} dry_run={}",
        cfg.kpa500.com_port, cfg.kpa500.baud, cfg.kpa500.mock, cfg.kpa500.dry_run
    );
    print_kpa_command_summary(
        kpa_command_map(),
        allow_control,
        allow_rf_risk,
        cfg.kpa500.dry_run,
    );

    let driver = Kpa500Driver::new(
        Kpa500Settings {
            com_port: cfg.kpa500.com_port.clone(),
            baud: cfg.kpa500.baud,
            polling_interval: Duration::from_millis(cfg.kpa500.polling_interval_ms),
            mock: cfg.kpa500.mock,
            dry_run: cfg.kpa500.dry_run,
            transcript_dir: cfg
                .logging
                .serial_transcript_dir
                .as_ref()
                .map(PathBuf::from),
        },
        shared_default_state(),
    );
    driver.connect().await?;
    driver.poll_status().await?;
    if allow_control {
        println!("KPA500 control test: set_standby is allowed by flag");
        driver.set_standby().await?;
    }
    if allow_rf_risk {
        println!("KPA500 RF-risk test: set_operate requested");
        driver.set_operate().await?;
    }
    Ok(())
}

async fn test_kat(cfg: &BridgeConfig, allow_control: bool, allow_rf_risk: bool) -> Result<()> {
    println!("KAT500 safety summary:");
    println!(
        "  port={} baud={} mock={} dry_run={}",
        cfg.kat500.com_port, cfg.kat500.baud, cfg.kat500.mock, cfg.kat500.dry_run
    );
    print_kat_command_summary(
        kat_command_map(),
        allow_control,
        allow_rf_risk,
        cfg.kat500.dry_run,
    );

    let driver = Kat500Driver::new(
        Kat500Settings {
            com_port: cfg.kat500.com_port.clone(),
            baud: cfg.kat500.baud,
            polling_interval: Duration::from_millis(cfg.kat500.polling_interval_ms),
            mock: cfg.kat500.mock,
            dry_run: cfg.kat500.dry_run,
            transcript_dir: cfg
                .logging
                .serial_transcript_dir
                .as_ref()
                .map(PathBuf::from),
        },
        shared_default_state(),
    );
    driver.connect().await?;
    driver.poll_status().await?;
    if allow_control {
        println!("KAT500 control test: set_bypass(true) is allowed by flag");
        driver.set_bypass(true).await?;
    }
    if allow_rf_risk {
        println!("KAT500 RF-risk test: autotune requested");
        driver.autotune().await?;
    }
    Ok(())
}

fn print_kpa_command_summary(
    commands: &[elecraft_kpa500::ElecraftCommand],
    allow_control: bool,
    allow_rf_risk: bool,
    dry_run: bool,
) {
    println!("  allow_control={allow_control} allow_rf_risk={allow_rf_risk}");
    if dry_run {
        println!("  dry_run=true: only read-only commands may be sent");
    }
    for command in commands {
        let allowed_by_flags = match command.safety {
            KpaCommandSafety::ReadOnly => true,
            KpaCommandSafety::StateChangeSafe => allow_control,
            KpaCommandSafety::RfRisk => allow_rf_risk,
            KpaCommandSafety::DestructiveOrUnknown => false,
        };
        let dry_run_blocks = dry_run && command.safety != KpaCommandSafety::ReadOnly;
        let disposition = if dry_run_blocks {
            "BLOCKED by dry_run"
        } else if allowed_by_flags {
            "WILL SEND if reached"
        } else {
            "SKIPPED by CLI flags"
        };
        println!(
            "  {disposition}: {} wire={} safety={:?} verified={}",
            command.label, command.wire, command.safety, command.verified
        );
    }
}

fn print_kat_command_summary(
    commands: &[elecraft_kat500::ElecraftCommand],
    allow_control: bool,
    allow_rf_risk: bool,
    dry_run: bool,
) {
    println!("  allow_control={allow_control} allow_rf_risk={allow_rf_risk}");
    if dry_run {
        println!("  dry_run=true: only read-only commands may be sent");
    }
    for command in commands {
        let allowed_by_flags = match command.safety {
            KatCommandSafety::ReadOnly => true,
            KatCommandSafety::StateChangeSafe => allow_control,
            KatCommandSafety::RfRisk => allow_rf_risk,
            KatCommandSafety::DestructiveOrUnknown => false,
        };
        let dry_run_blocks = dry_run && command.safety != KatCommandSafety::ReadOnly;
        let disposition = if dry_run_blocks {
            "BLOCKED by dry_run"
        } else if allowed_by_flags {
            "WILL SEND if reached"
        } else {
            "SKIPPED by CLI flags"
        };
        println!(
            "  {disposition}: {} wire={} safety={:?} verified={}",
            command.label, command.wire, command.safety, command.verified
        );
    }
}
