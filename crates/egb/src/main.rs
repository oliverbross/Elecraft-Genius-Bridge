use anyhow::{Context, Result};
use bridge_core::state::{shared_default_state, shared_mock_state};
use bridge_core::{
    append_evidence_json, append_evidence_line, set_evidence_dir, AmpOperatingState,
    ConnectionState, SharedState,
};
use clap::{Parser, Subcommand, ValueEnum};
use egb_config::{is_lan_or_loopback_or_cgnat, BridgeConfig};
use elecraft_kat500::{
    command_map as kat_command_map, read_only_discovery_commands as kat_discovery_commands,
    read_only_poll_commands as kat_poll_commands, CommandOutcome as KatCommandOutcome,
    CommandSafety as KatCommandSafety, Kat500Driver, Kat500Settings,
};
use elecraft_kpa500::{
    command_map as kpa_command_map, read_only_discovery_commands as kpa_discovery_commands,
    read_only_poll_commands as kpa_poll_commands, CommandOutcome as KpaCommandOutcome,
    CommandResultState as KpaCommandResultState, CommandSafety as KpaCommandSafety,
    ControlCommandResult as KpaControlCommandResult, Kpa500Driver, Kpa500Settings,
};
use flex_injection::FlexInjectionSettings;
use std::fs::{self, File};
use std::io::Seek;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_serial::{SerialPortBuilderExt, SerialPortType, SerialStream};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

static BRIDGE_STARTED_AT: OnceLock<SystemTime> = OnceLock::new();

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
    SoakTest {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long)]
        duration_hours: f64,
    },
    StabilityTest {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long)]
        duration_minutes: f64,
    },
    EvidenceTest {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long)]
        duration_minutes: f64,
    },
    ControlLab {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long)]
        duration_minutes: f64,
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
    TestKpaLive {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long)]
        allow_control: bool,
        #[arg(long)]
        allow_rf_risk: bool,
        #[arg(long)]
        confirm_rf_risk: Option<String>,
    },
    TestKpaOperate {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long)]
        allow_rf_risk: bool,
        #[arg(long)]
        confirm_rf_risk: Option<String>,
    },
    TestKat {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long)]
        allow_control: bool,
        #[arg(long)]
        allow_rf_risk: bool,
    },
    TestPgxlDirect {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 9008)]
        port: u16,
    },
    PgxlTriggerLab {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long, default_value = "pgxl_paired")]
        profile: String,
        #[arg(long)]
        duration_minutes: f64,
    },
    SerialProbeBatch {
        #[arg(long)]
        port: String,
        #[arg(long)]
        baud: u32,
        #[arg(long, value_delimiter = ',')]
        send: Vec<String>,
        #[arg(long, default_value_t = 1000)]
        timeout_ms: u64,
        #[arg(long, value_enum, default_value_t = SerialTerminator::None)]
        terminator: SerialTerminator,
        #[arg(long, default_value = "logs/serial")]
        transcript_dir: PathBuf,
    },
    SerialProbe {
        #[arg(long)]
        port: String,
        #[arg(long)]
        baud: u32,
        #[arg(long)]
        send: String,
        #[arg(long, default_value_t = 1000)]
        timeout_ms: u64,
        #[arg(long, value_enum, default_value_t = SerialTerminator::None)]
        terminator: SerialTerminator,
        #[arg(long, default_value = "logs/serial")]
        transcript_dir: PathBuf,
    },
    BaudScan {
        #[arg(long)]
        port: String,
        #[arg(long, default_value_t = 1000)]
        timeout_ms: u64,
        #[arg(long)]
        version_query: Option<String>,
        #[arg(long, default_value = "logs/serial")]
        transcript_dir: PathBuf,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SerialTerminator {
    None,
    Cr,
    Lf,
    Crlf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run { config } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_bridge(cfg, config).await
        }
        Commands::SoakTest {
            config,
            duration_hours,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_soak_test(cfg, config, duration_hours).await
        }
        Commands::StabilityTest {
            config,
            duration_minutes,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_evidence_test("stability-test", cfg, config, duration_minutes).await
        }
        Commands::EvidenceTest {
            config,
            duration_minutes,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_evidence_test("evidence-test", cfg, config, duration_minutes).await
        }
        Commands::ControlLab {
            config,
            duration_minutes,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_evidence_test("control-lab", cfg, config, duration_minutes).await
        }
        Commands::CheckConfig { config } => {
            let cfg = BridgeConfig::load(&config)?;
            println!("config OK: {}", config.display());
            println!(
                "bind={} pgxl={} tgxl={} kpa_mock={} kat_mock={} flex_injection={}",
                cfg.server.bind_ip,
                cfg.pgxl.port,
                cfg.tgxl.port,
                cfg.kpa500.mock,
                cfg.kat500.mock,
                cfg.flex_injection.enabled
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
            run_test_with_evidence("test-kpa", &config, &cfg, async {
                test_kpa(&cfg, allow_control, allow_rf_risk).await
            })
            .await
        }
        Commands::TestKpaLive {
            config,
            allow_control,
            allow_rf_risk,
            confirm_rf_risk,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            require_rf_risk_confirmation(allow_rf_risk, confirm_rf_risk.as_deref())?;
            run_test_with_evidence("test-kpa-live", &config, &cfg, async {
                test_kpa(&cfg, allow_control, allow_rf_risk).await
            })
            .await
        }
        Commands::TestKpaOperate {
            config,
            allow_rf_risk,
            confirm_rf_risk,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            require_rf_risk_confirmation(allow_rf_risk, confirm_rf_risk.as_deref())?;
            run_test_with_evidence("test-kpa-operate", &config, &cfg, async {
                test_kpa_operate(&cfg, allow_rf_risk).await
            })
            .await
        }
        Commands::TestKat {
            config,
            allow_control,
            allow_rf_risk,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_test_with_evidence("test-kat", &config, &cfg, async {
                test_kat(&cfg, allow_control, allow_rf_risk).await
            })
            .await
        }
        Commands::TestPgxlDirect { host, port } => {
            init_logging("debug");
            test_pgxl_direct(&host, port).await
        }
        Commands::PgxlTriggerLab {
            config,
            profile,
            duration_minutes,
        } => {
            let mut cfg = BridgeConfig::load(&config)?;
            cfg.flex_injection.amplifier_status_profile = profile;
            init_logging(&cfg.logging.level);
            run_pgxl_trigger_lab(cfg, config, duration_minutes).await
        }
        Commands::SerialProbe {
            port,
            baud,
            send,
            timeout_ms,
            terminator,
            transcript_dir,
        } => {
            init_logging("info");
            serial_probe(
                &port,
                baud,
                &send,
                terminator,
                Duration::from_millis(timeout_ms),
                Some(transcript_dir),
            )
            .await
        }
        Commands::SerialProbeBatch {
            port,
            baud,
            send,
            timeout_ms,
            terminator,
            transcript_dir,
        } => {
            init_logging("info");
            serial_probe_batch(
                &port,
                baud,
                &send,
                terminator,
                Duration::from_millis(timeout_ms),
                Some(transcript_dir),
            )
            .await
        }
        Commands::BaudScan {
            port,
            timeout_ms,
            version_query,
            transcript_dir,
        } => {
            init_logging("info");
            baud_scan(
                &port,
                Duration::from_millis(timeout_ms),
                version_query.as_deref(),
                Some(transcript_dir),
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

async fn start_bridge(cfg: &BridgeConfig) -> Result<SharedState> {
    let _ = BRIDGE_STARTED_AT.set(SystemTime::now());
    let all_mock = cfg.kpa500.mock && cfg.kat500.mock;
    let state = if all_mock {
        shared_mock_state()
    } else {
        shared_default_state()
    };
    apply_mock_config(cfg, &state).await;

    if cfg.kpa500.enabled {
        let driver = Kpa500Driver::new(
            Kpa500Settings {
                com_port: cfg.kpa500.com_port.clone(),
                baud: cfg.kpa500.baud,
                polling_interval: Duration::from_millis(cfg.kpa500.polling_interval_ms),
                mock: cfg.kpa500.mock,
                dry_run: cfg.kpa500.dry_run,
                allow_rf_risk: cfg.kpa500.allow_rf_risk,
                control_verify_delay: Duration::from_millis(cfg.control.verify_delay_ms),
                transcript_rotate_bytes: cfg.logging.transcript_rotate_bytes,
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
                allow_rf_risk: cfg.kat500.allow_rf_risk,
                transcript_rotate_bytes: cfg.logging.transcript_rotate_bytes,
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
            transcript_rotate_bytes: cfg.logging.transcript_rotate_bytes,
            aethersdr_compat: cfg.pgxl.aethersdr_compat
                || matches!(
                    cfg.pgxl.compat_profile.as_str(),
                    "aethersdr" | "smartsdr" | "permissive"
                ),
            compat_profile: cfg.pgxl.compat_profile.clone(),
            strict_emulation: cfg.pgxl.strict_emulation,
            startup_delay: Duration::from_millis(cfg.pgxl.startup_delay_ms),
            force_direct_connected_test: cfg.pgxl.force_direct_connected_test,
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
            transcript_rotate_bytes: cfg.logging.transcript_rotate_bytes,
            aethersdr_compat: cfg.tgxl.aethersdr_compat || cfg.tgxl.smartsdr_compat,
            control_profile: cfg.tgxl.control_profile.clone(),
            strict_emulation: cfg.tgxl.strict_emulation,
            startup_delay: Duration::from_millis(cfg.tgxl.startup_delay_ms),
            force_presence_test: cfg.tgxl.force_presence_test,
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

    if cfg.flex_injection.enabled {
        let radio_ip: IpAddr = cfg
            .flex_injection
            .radio_ip
            .parse()
            .context("flex_injection.radio_ip passed validation but failed to parse")?;
        let amplifier_ip: IpAddr = cfg
            .flex_injection
            .amplifier_ip
            .parse()
            .context("flex_injection.amplifier_ip passed validation but failed to parse")?;
        let settings = FlexInjectionSettings {
            radio_addr: SocketAddr::new(radio_ip, cfg.flex_injection.radio_port),
            amplifier_ip,
            amplifier_port: cfg.flex_injection.amplifier_port,
            amplifier_model: cfg.flex_injection.amplifier_model.clone(),
            serial: cfg.flex_injection.serial.clone(),
            handle_label: cfg.flex_injection.handle.clone(),
            ant_map: cfg.flex_injection.ant_map.clone(),
            amplifier_status_profile: cfg.flex_injection.amplifier_status_profile.clone(),
            full_pgxl_registration: cfg.flex_injection.full_pgxl_registration,
            create_meters: cfg.flex_injection.create_meters,
            create_interlock: cfg.flex_injection.create_interlock,
            allow_rf_risk: cfg.kpa500.allow_rf_risk,
            reconnect_initial: Duration::from_millis(cfg.flex_injection.reconnect_initial_ms),
            reconnect_max: Duration::from_millis(cfg.flex_injection.reconnect_max_ms),
            ping_interval: Duration::from_millis(cfg.flex_injection.ping_interval_ms),
            tuner_presence_refresh: cfg.tgxl.experimental_presence_refresh,
            tuner_refresh_interval: Duration::from_millis(
                cfg.flex_injection.tuner_refresh_interval_ms,
            ),
            amplifier_reannounce_interval: Duration::from_millis(
                cfg.flex_injection.amplifier_reannounce_interval_ms,
            ),
        };
        let state = state.clone();
        tokio::spawn(async move {
            flex_injection::run(settings, state).await;
        });
    }

    if cfg.pgxl.enabled && cfg.flex_injection.enabled {
        let state = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let mut guard = state.write().await;
            if guard.flex_injection.amplifier_handle.is_some()
                && guard.clients.pgxl_session_started_count == 0
            {
                guard.clients.pgxl_manual_connect_no_socket_attempt_count = guard
                    .clients
                    .pgxl_manual_connect_no_socket_attempt_count
                    .saturating_add(1);
                let warning = "pgxl_manual_connect_no_socket_attempt: amplifier is present but no TCP 9008 PGXL session has started".to_string();
                guard.clients.pgxl_last_no_socket_attempt_warning = Some(warning.clone());
                drop(guard);
                append_evidence_line("warnings-errors.log", warning.clone());
                append_evidence_json(
                    "disconnect-events.jsonl",
                    &serde_json::json!({
                        "event": "pgxl_manual_connect_no_socket_attempt",
                        "reason": warning,
                    }),
                );
                warn!(
                    event_id = "pgxl_manual_connect_no_socket_attempt",
                    "Flex amplifier is present but no PGXL TCP session started"
                );
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

    Ok(state)
}

async fn run_bridge(cfg: BridgeConfig, config_path: PathBuf) -> Result<()> {
    let evidence = EvidenceRun::start("run", &config_path, &cfg, std::env::args())?;
    let state = start_bridge(&cfg).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    info!("Elecraft Genius Bridge running; press Ctrl+C to stop");
    tokio::signal::ctrl_c()
        .await
        .context("failed waiting for Ctrl+C")?;
    info!("shutdown requested");
    sampler.abort();
    evidence.write_status("status-end.json", &state).await?;
    evidence.finish(&state, None).await?;
    Ok(())
}

async fn run_soak_test(cfg: BridgeConfig, config_path: PathBuf, duration_hours: f64) -> Result<()> {
    if !duration_hours.is_finite() || duration_hours <= 0.0 {
        anyhow::bail!("--duration-hours must be a finite value greater than 0");
    }
    let evidence = EvidenceRun::start("soak-test", &config_path, &cfg, std::env::args())?;
    let state = start_bridge(&cfg).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let duration = Duration::from_secs_f64(duration_hours * 3600.0);
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + duration;
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    interval.tick().await;
    info!(
        duration_hours,
        duration_secs = duration.as_secs(),
        "soak test started"
    );
    print_soak_summary(&state, started.elapsed()).await;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                break;
            }
            _ = interval.tick() => {
                print_soak_summary(&state, started.elapsed()).await;
            }
            result = tokio::signal::ctrl_c() => {
                result.context("failed waiting for Ctrl+C")?;
                info!("soak test interrupted by Ctrl+C");
                break;
            }
        }
    }
    print_soak_summary(&state, started.elapsed()).await;
    sampler.abort();
    evidence.write_status("status-end.json", &state).await?;
    evidence.finish(&state, Some(started.elapsed())).await?;
    info!("soak test finished");
    Ok(())
}

async fn run_evidence_test(
    mode: &str,
    cfg: BridgeConfig,
    config_path: PathBuf,
    duration_minutes: f64,
) -> Result<()> {
    if !duration_minutes.is_finite() || duration_minutes <= 0.0 {
        anyhow::bail!("--duration-minutes must be a finite value greater than 0");
    }
    let evidence = EvidenceRun::start(mode, &config_path, &cfg, std::env::args())?;
    let state = start_bridge(&cfg).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let duration = Duration::from_secs_f64(duration_minutes * 60.0);
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + duration;
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    interval.tick().await;
    info!(
        event_id = "stability_test_started",
        duration_minutes,
        duration_secs = duration.as_secs(),
        "stability test started"
    );
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                break;
            }
            _ = interval.tick() => {
                print_soak_summary(&state, started.elapsed()).await;
            }
            result = tokio::signal::ctrl_c() => {
                result.context("failed waiting for Ctrl+C")?;
                info!("stability test interrupted by Ctrl+C");
                break;
            }
        }
    }
    sampler.abort();
    evidence.write_status("status-end.json", &state).await?;
    let path = write_stability_report(&state, started.elapsed(), Some(evidence.dir())).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    info!(
        event_id = "stability_test_completed",
        report = %path.display(),
        zip = %zip.display(),
        "stability test completed"
    );
    println!("stability report: {}", path.display());
    println!("evidence bundle: {}", zip.display());
    Ok(())
}

async fn run_pgxl_trigger_lab(
    cfg: BridgeConfig,
    config_path: PathBuf,
    duration_minutes: f64,
) -> Result<()> {
    if !duration_minutes.is_finite() || duration_minutes <= 0.0 {
        anyhow::bail!("--duration-minutes must be a finite value greater than 0");
    }
    let evidence = EvidenceRun::start("pgxl-trigger-lab", &config_path, &cfg, std::env::args())?;
    let state = start_bridge(&cfg).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let duration = Duration::from_secs_f64(duration_minutes * 60.0);
    let started = Instant::now();
    tokio::time::sleep(duration).await;
    sampler.abort();
    let guard = state.read().await;
    let analysis = format!(
        "# PGXL Trigger Lab\n\nProfile: `{}`\n\nPGXL sessions started: {}\nPGXL active clients: {}\nNo-socket warnings: {}\nLast no-socket warning: {}\nAmplifier handle: {:?}\nAmplifier reannounce count: {}\nDirect connect expected: {:?}\n",
        cfg.flex_injection.amplifier_status_profile,
        guard.clients.pgxl_session_started_count,
        guard.clients.pgxl_client_count,
        guard.clients.pgxl_manual_connect_no_socket_attempt_count,
        guard
            .clients
            .pgxl_last_no_socket_attempt_warning
            .as_deref()
            .unwrap_or("none"),
        guard.flex_injection.amplifier_handle,
        guard.flex_injection.amplifier_reannounce_count,
        guard.flex_injection.amplifier_direct_connect_expected,
    );
    drop(guard);
    tokio::fs::write(evidence.dir().join("pgxl-trigger-analysis.md"), analysis).await?;
    evidence.write_status("status-end.json", &state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    println!(
        "PGXL trigger lab complete; evidence bundle: {}",
        zip.display()
    );
    Ok(())
}

async fn test_pgxl_direct(host: &str, port: u16) -> Result<()> {
    let log_path = PathBuf::from("logs")
        .join("tests")
        .join(format!("{}-pgxl-direct-selftest.log", timestamp_compact()));
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let addr = format!("{host}:{port}");
    let mut log = format!("PGXL direct self-test target={addr}\n");
    let stream = TcpStream::connect(&addr)
        .await
        .with_context(|| format!("failed to connect to PGXL direct endpoint at {addr}"))?;
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let greeting = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .context("timed out waiting for PGXL greeting")??
        .context("PGXL closed before greeting")?;
    log.push_str(&format!("RX {greeting}\n"));
    if !greeting.starts_with('V') {
        anyhow::bail!("PGXL greeting did not start with V: {greeting}");
    }
    for (seq, command) in [
        (1_u32, "info"),
        (2, "status"),
        (3, "standby"),
        (4, "operate"),
    ] {
        let line = format!("C{seq}|{command}\n");
        log.push_str(&format!("TX {}", line.trim_end()));
        log.push('\n');
        writer.write_all(line.as_bytes()).await?;
        let response = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
            .await
            .with_context(|| format!("timed out waiting for response to {command}"))??
            .with_context(|| format!("PGXL closed before response to {command}"))?;
        log.push_str(&format!("RX {response}\n"));
        if !response.starts_with(&format!("R{seq}|")) {
            anyhow::bail!("unexpected PGXL response for {command}: {response}");
        }
    }
    tokio::fs::write(&log_path, &log).await?;
    append_evidence_line("pgxl-direct-selftest.log", log.clone());
    println!("PGXL direct self-test passed: {addr}");
    println!("log: {}", log_path.display());
    Ok(())
}

async fn write_stability_report(
    state: &SharedState,
    elapsed: Duration,
    dir: Option<&Path>,
) -> Result<PathBuf> {
    let path = dir
        .map(|dir| dir.join("stability-report.json"))
        .unwrap_or_else(|| {
            PathBuf::from("diagnostics").join(format!("egb-stability-{}.json", timestamp_compact()))
        });
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let status: serde_json::Value = serde_json::from_str(&status_json(state).await)?;
    let guard = state.read().await;
    let mut warnings = Vec::new();
    if guard.clients.pgxl_session_started_count == 0
        && guard.clients.tgxl_session_started_count == 0
    {
        warnings.push("No PGXL/TGXL direct client connected during the stability test.");
    }
    if guard.flex_injection.enabled && guard.flex_injection.tuner_appeared_count == 0 {
        warnings.push("No Flex-side SmartSDR/TGXL tuner presence was observed.");
    }
    if guard.flex_injection.tuner_disappeared_count > 0 {
        warnings.push("Flex-side SmartSDR/TGXL tuner presence disappeared during the test.");
    }
    let body = serde_json::json!({
        "elapsed_secs": elapsed.as_secs(),
        "warnings": warnings,
        "summary": {
            "pgxl_session_started_count": guard.clients.pgxl_session_started_count,
            "tgxl_session_started_count": guard.clients.tgxl_session_started_count,
            "smartsdr_tuner_appeared_count": guard.flex_injection.tuner_appeared_count,
            "smartsdr_tuner_disappeared_count": guard.flex_injection.tuner_disappeared_count,
            "last_tuner_disappearance_reason": guard.flex_injection.last_tuner_disappearance_reason,
            "tuner_registration_refresh_count": guard.flex_injection.tuner_registration_refresh_count,
            "tuner_presence_expired_count": guard.flex_injection.tuner_presence_expired_count,
            "tuner_reannounce_count": guard.flex_injection.tuner_reannounce_count,
            "flex_ping_success": guard.flex_injection.ping_count.saturating_sub(guard.flex_injection.ping_failure_count),
            "flex_ping_fail": guard.flex_injection.ping_failure_count,
        },
        "status": status,
    });
    drop(guard);
    tokio::fs::write(&path, serde_json::to_vec_pretty(&body)?).await?;
    if !warnings.is_empty() {
        warn!(
            event_id = "stability_test_warning",
            warning_count = warnings.len(),
            warnings = ?warnings,
            "stability test completed with warnings"
        );
        for warning in warnings {
            println!("warning: {warning}");
        }
    }
    Ok(path)
}

struct EvidenceRun {
    dir: PathBuf,
    zip_path: PathBuf,
}

impl EvidenceRun {
    fn start<I, S>(mode: &str, config_path: &Path, cfg: &BridgeConfig, command: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: ToString,
    {
        let stamp = timestamp_ymdhms();
        let safe_mode = mode.replace(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-', "_");
        let root = PathBuf::from("diagnostics").join("runs");
        let dir = root.join(format!("{stamp}-{safe_mode}"));
        fs::create_dir_all(&dir)?;
        set_evidence_dir(&dir);
        fs::write(dir.join("command.txt"), command_line(command))?;
        fs::write(
            dir.join("config-effective.yaml"),
            serde_yaml::to_string(cfg)?,
        )?;
        fs::write(
            dir.join("egb-run.log"),
            format!(
                "mode={mode}\nconfig={}\nstarted={stamp}\n",
                config_path.display()
            ),
        )?;
        for file in [
            "flex-rx.log",
            "flex-tx.log",
            "pgxl-protocol.log",
            "tgxl-protocol.log",
            "amplifier-status-lines.log",
            "amplifier-reannounce.log",
            "pgxl-direct-selftest.log",
            "pgxl-trigger-analysis.md",
            "kpa500-serial.log",
            "kat500-serial.log",
            "client-sessions.jsonl",
            "disconnect-events.jsonl",
            "warnings-errors.log",
            "status-samples.jsonl",
            "controls-analysis.md",
            "control-events.jsonl",
            "tgxl-control-commands.log",
            "pgxl-control-commands.log",
            "flex-control-commands.log",
        ] {
            File::create(dir.join(file))?;
        }
        Ok(Self {
            zip_path: root.join(format!("{stamp}-{safe_mode}.zip")),
            dir,
        })
    }

    fn dir(&self) -> &Path {
        &self.dir
    }

    async fn write_status(&self, file_name: &str, state: &SharedState) -> Result<()> {
        let body = status_json(state).await;
        tokio::fs::write(self.dir.join(file_name), body).await?;
        Ok(())
    }

    fn start_status_sampler(&self, state: SharedState) -> tokio::task::JoinHandle<()> {
        let dir = self.dir.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                let Ok(status) =
                    serde_json::from_str::<serde_json::Value>(&status_json(&state).await)
                else {
                    continue;
                };
                let sample = serde_json::json!({
                    "timestamp_ms": system_time_ms(Some(SystemTime::now())),
                    "status": status,
                });
                let line = match serde_json::to_string(&sample) {
                    Ok(line) => line,
                    Err(_) => continue,
                };
                let path = dir.join("status-samples.jsonl");
                if let Ok(mut file) = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .await
                {
                    let _ = file.write_all(format!("{line}\n").as_bytes()).await;
                    let _ = file.flush().await;
                }
            }
        })
    }

    async fn finish(&self, state: &SharedState, elapsed: Option<Duration>) -> Result<PathBuf> {
        self.write_status("status-end.json", state).await?;
        let summary = evidence_summary_markdown(state, elapsed).await;
        tokio::fs::write(self.dir.join("summary.md"), summary).await?;
        let analysis = protocol_analysis_markdown(state).await;
        tokio::fs::write(self.dir.join("pgxl-vs-tgxl-analysis.md"), analysis).await?;
        let controls = controls_analysis_markdown(state).await;
        tokio::fs::write(self.dir.join("controls-analysis.md"), controls).await?;
        zip_dir(&self.dir, &self.zip_path)?;
        println!("evidence bundle: {}", self.zip_path.display());
        Ok(self.zip_path.clone())
    }
}

async fn run_test_with_evidence<F>(
    mode: &str,
    config_path: &Path,
    cfg: &BridgeConfig,
    future: F,
) -> Result<()>
where
    F: std::future::Future<Output = Result<()>>,
{
    let evidence = EvidenceRun::start(mode, config_path, cfg, std::env::args())?;
    fs::write(evidence.dir().join("status-start.json"), "{}")?;
    let result = future.await;
    if let Err(err) = &result {
        append_evidence_line("warnings-errors.log", format!("ERROR {err}"));
    }
    let state = shared_default_state();
    fs::write(
        evidence.dir().join("status-end.json"),
        status_json(&state).await,
    )?;
    evidence.finish(&state, None).await?;
    result
}

async fn evidence_summary_markdown(state: &SharedState, elapsed: Option<Duration>) -> String {
    let guard = state.read().await;
    let mut result = "PASS";
    let mut warnings = Vec::new();
    if guard.amp.connection_state == ConnectionState::Degraded
        || guard.tuner.connection_state == ConnectionState::Degraded
        || guard.flex_injection.connection_state == ConnectionState::Degraded
    {
        result = "WARN";
    }
    if guard.flex_injection.tuner_disappeared_count > 0 {
        result = "WARN";
        warnings.push("SmartSDR/Flex tuner presence disappeared during the run.");
    }
    if guard.clients.pgxl_session_started_count == 0
        && guard.clients.tgxl_session_started_count == 0
    {
        warnings.push("No direct PGXL/TGXL client connected during the run.");
    }
    let mut body = String::new();
    body.push_str("# Evidence Summary\n\n");
    body.push_str(&format!("Overall result: **{result}**\n\n"));
    if let Some(elapsed) = elapsed {
        body.push_str(&format!("Uptime: {} seconds\n\n", elapsed.as_secs()));
    }
    body.push_str(&format!(
        "- Amp polls: success={} failure={} reconnects={}\n",
        guard.amp.runtime.poll_success_count,
        guard.amp.runtime.poll_failure_count,
        guard.amp.runtime.reconnect_count
    ));
    body.push_str(&format!(
        "- Tuner polls: success={} failure={} reconnects={}\n",
        guard.tuner.runtime.poll_success_count,
        guard.tuner.runtime.poll_failure_count,
        guard.tuner.runtime.reconnect_count
    ));
    body.push_str(&format!(
        "- KPA500 final state: {} operate={} swr={:.2} fault={}\n",
        guard.amp.connection_state.as_str(),
        guard.amp.operate,
        guard.amp.swr,
        guard.amp.fault.as_deref().unwrap_or("none")
    ));
    body.push_str(&format!(
        "- KAT500 final state: {} antenna={:?} bypass={} swr={:.2} fault={}\n",
        guard.tuner.connection_state.as_str(),
        guard.tuner.selected_antenna,
        guard.tuner.bypass,
        guard.tuner.swr,
        guard.tuner.fault.as_deref().unwrap_or("none")
    ));
    body.push_str(&format!(
        "- Flex state: {} degraded_reason={}\n",
        guard.flex_injection.connection_state.as_str(),
        guard
            .flex_injection
            .degraded_reason
            .as_deref()
            .unwrap_or("none")
    ));
    body.push_str(&format!(
        "- Amplifier handle: {:?}\n- Tuner handle: {:?}\n- Meter handles: {:?}\n- Interlock handle: {:?}\n",
        guard.flex_injection.amplifier_handle,
        guard.flex_injection.tuner_handle,
        guard.flex_injection.meter_handles,
        guard.flex_injection.interlock_handle
    ));
    body.push_str(&format!(
        "- SmartSDR tuner appeared={} disappeared={} last_reason={}\n",
        guard.flex_injection.tuner_appeared_count,
        guard.flex_injection.tuner_disappeared_count,
        guard
            .flex_injection
            .last_tuner_disappearance_reason
            .as_deref()
            .unwrap_or("none")
    ));
    body.push_str(&format!(
        "- Tuner refresh/reannounce/expired counters: {}/{}/{}\n",
        guard.flex_injection.tuner_registration_refresh_count,
        guard.flex_injection.tuner_reannounce_count,
        guard.flex_injection.tuner_presence_expired_count
    ));
    body.push_str(&format!(
        "- PGXL/TGXL sessions started: {}/{}\n",
        guard.clients.pgxl_session_started_count, guard.clients.tgxl_session_started_count
    ));
    body.push_str(&format!(
        "- PGXL no-socket warnings: {} ({})\n",
        guard.clients.pgxl_manual_connect_no_socket_attempt_count,
        guard
            .clients
            .pgxl_last_no_socket_attempt_warning
            .as_deref()
            .unwrap_or("none")
    ));
    body.push_str(&format!(
        "- Amplifier reannounce/direct-expected: {}/{:?}\n",
        guard.flex_injection.amplifier_reannounce_count,
        guard.flex_injection.amplifier_direct_connect_expected
    ));
    body.push_str(&format!(
        "- Last PGXL/TGXL disconnect: {}/{}\n",
        guard
            .clients
            .pgxl_last_disconnect_reason
            .as_deref()
            .unwrap_or("none"),
        guard
            .clients
            .tgxl_last_disconnect_reason
            .as_deref()
            .unwrap_or("none")
    ));
    if !warnings.is_empty() {
        body.push_str("\n## Warnings\n\n");
        for warning in warnings {
            body.push_str(&format!("- {warning}\n"));
        }
    }
    body.push_str("\n## Recommended Next Action\n\n");
    if guard.flex_injection.tuner_disappeared_count > 0 {
        body.push_str("Inspect `disconnect-events.jsonl`, `flex-rx.log`, and `flex-tx.log` around the tuner disappearance.\n");
    } else if guard.clients.pgxl_session_started_count == 0
        && guard.flex_injection.amplifier_handle.is_some()
    {
        body.push_str("The AMP pane can be radio-side visible while direct PGXL never connects; inspect AetherSDR PGXL manual connection settings and `pgxl-protocol.log`.\n");
    } else {
        body.push_str("If the target issue was not reproduced, run another evidence test while SmartSDR is connected.\n");
    }
    body
}

async fn protocol_analysis_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let pgxl_command_count: u64 = guard
        .clients
        .pgxl_sessions
        .iter()
        .map(|session| session.commands_received)
        .sum();
    let tgxl_command_count: u64 = guard
        .clients
        .tgxl_sessions
        .iter()
        .map(|session| session.commands_received)
        .sum();
    let likely_pgxl = if guard.flex_injection.amplifier_handle.is_some()
        && guard.clients.pgxl_session_started_count == 0
    {
        "AMP applet is radio-side visible but direct PGXL never connected; likely AetherSDR direct PGXL endpoint is not configured or rejected before TCP connect."
    } else if guard.clients.pgxl_session_started_count > 0
        && guard.clients.pgxl_client_count == 0
        && guard.clients.pgxl_last_disconnect_reason.is_some()
    {
        "PGXL accepted TCP but disconnected; likely handshake/field/timing mismatch. Inspect pgxl-protocol.log raw_hex entries."
    } else if guard.clients.pgxl_client_count > 0 {
        "PGXL direct session is currently connected."
    } else {
        "No PGXL direct evidence captured."
    };
    let likely_tgxl = if guard.flex_injection.tuner_disappeared_count > 0 {
        "SmartSDR tuner presence disappeared during the run; likely Flex-side presence expiry or SmartSDR rejecting stale tuner lifecycle state."
    } else if guard.clients.tgxl_session_started_count == 0
        && guard.flex_injection.tuner_appeared_count > 0
    {
        "Flex-side tuner presence appeared without a direct TGXL client; SmartSDR may be using radio-side visibility but not opening 9010 in this run."
    } else if guard.clients.tgxl_client_count > 0 {
        "TGXL direct session is currently connected."
    } else {
        "No TGXL direct disconnect evidence captured."
    };
    format!(
        "# PGXL vs TGXL Protocol Analysis\n\n\
        ## Counts\n\n\
        - PGXL sessions started: {}\n\
        - TGXL sessions started: {}\n\
        - PGXL active clients: {}\n\
        - TGXL active clients: {}\n\
        - PGXL commands in active sessions: {}\n\
        - TGXL commands in active sessions: {}\n\
        - PGXL last disconnect: {}\n\
        - TGXL last disconnect: {}\n\
        - Unknown commands: {}\n\
        - Parse failures: {}\n\
        - Unsupported features: {}\n\n\
        ## Likely Cause Heuristics\n\n\
        - PGXL: {}\n\
        - TGXL/SmartSDR tuner: {}\n\n\
        ## Files To Inspect\n\n\
        - `pgxl-protocol.log` for raw PGXL RX/TX and raw hex framing.\n\
        - `tgxl-protocol.log` for TGXL RX/TX.\n\
        - `disconnect-events.jsonl` for client and tuner lifecycle events.\n\
        - `flex-rx.log` and `flex-tx.log` for radio-side tuner/amplifier presence.\n",
        guard.clients.pgxl_session_started_count,
        guard.clients.tgxl_session_started_count,
        guard.clients.pgxl_client_count,
        guard.clients.tgxl_client_count,
        pgxl_command_count,
        tgxl_command_count,
        guard
            .clients
            .pgxl_last_disconnect_reason
            .as_deref()
            .unwrap_or("none"),
        guard
            .clients
            .tgxl_last_disconnect_reason
            .as_deref()
            .unwrap_or("none"),
        guard
            .protocol
            .pgxl
            .unknown_commands
            .saturating_add(guard.protocol.tgxl.unknown_commands),
        guard
            .protocol
            .pgxl
            .parse_failures
            .saturating_add(guard.protocol.tgxl.parse_failures),
        guard
            .protocol
            .pgxl
            .unsupported_features
            .saturating_add(guard.protocol.tgxl.unsupported_features),
        likely_pgxl,
        likely_tgxl,
    )
}

async fn controls_analysis_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let mut body = String::from("# Controls Analysis\n\n");
    body.push_str(&format!(
        "- AetherSDR button command seen: {}\n",
        guard.controls.aethersdr_button_command_seen
    ));
    body.push_str(&format!(
        "- Control requests observed: {}\n",
        guard.controls.control_requested_count
    ));
    body.push_str(&format!(
        "- Last TGXL control command: {}\n",
        guard
            .controls
            .last_tgxl_control_command
            .as_deref()
            .unwrap_or("none")
    ));
    body.push_str(&format!(
        "- Last PGXL control command: {}\n",
        guard
            .controls
            .last_pgxl_control_command
            .as_deref()
            .unwrap_or("none")
    ));
    body.push_str(&format!(
        "- Last Flex amp set/status command: {}\n",
        guard
            .controls
            .last_flex_amp_set_command
            .as_deref()
            .unwrap_or("none")
    ));
    body.push_str(&format!(
        "- Last mapped Elecraft action: {}\n",
        guard
            .controls
            .last_mapped_elecraft_action
            .as_deref()
            .unwrap_or("none")
    ));
    body.push_str(&format!(
        "- Last safety decision: {}\n",
        guard
            .controls
            .last_safety_decision
            .as_deref()
            .unwrap_or("none")
    ));
    body.push_str(&format!(
        "- Blocked by dry_run: {}\n- Blocked by RF-risk gate: {}\n",
        guard.controls.blocked_by_dry_run_count, guard.controls.blocked_by_rf_risk_count
    ));
    body.push_str("\n## Interpretation\n\n");
    if guard.controls.control_requested_count == 0 {
        body.push_str("No control command reached EGB during this run. The likely issue is AetherSDR UI enablement/state fields rather than Elecraft serial control.\n");
    } else if guard.controls.blocked_by_dry_run_count > 0
        || guard.controls.blocked_by_rf_risk_count > 0
    {
        body.push_str(
            "A control command reached EGB and was intentionally blocked by a safety gate.\n",
        );
    } else {
        body.push_str(
            "At least one control command reached EGB and was mapped to desired Elecraft state.\n",
        );
    }
    body
}

fn zip_dir(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(dst)?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    add_dir_to_zip(&mut zip, options, src, src)?;
    zip.finish()?;
    Ok(())
}

fn add_dir_to_zip<W: std::io::Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::FileOptions,
    root: &Path,
    dir: &Path,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            add_dir_to_zip(zip, options, root, &path)?;
        } else if entry.file_type()?.is_file() {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(name, options)?;
            std::io::Write::write_all(zip, &fs::read(path)?)?;
        }
    }
    Ok(())
}

fn command_line<I, S>(command: I) -> String
where
    I: IntoIterator<Item = S>,
    S: ToString,
{
    command
        .into_iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn require_rf_risk_confirmation(allow_rf_risk: bool, confirmation: Option<&str>) -> Result<()> {
    if allow_rf_risk && confirmation != Some("I understand") {
        anyhow::bail!("--confirm-rf-risk \"I understand\" is required with --allow-rf-risk");
    }
    Ok(())
}

async fn apply_mock_config(cfg: &BridgeConfig, state: &SharedState) {
    if !(cfg.kpa500.mock || cfg.kat500.mock) {
        return;
    }

    let mut guard = state.write().await;
    if cfg.kpa500.mock {
        if cfg.mock.pgxl_fault {
            guard.amp.state = AmpOperatingState::Fault;
            guard.amp.connection_state = ConnectionState::Degraded;
            guard.amp.connected = false;
            guard.amp.fault = Some("mock_pgxl_fault".to_string());
        }
        if cfg.mock.high_swr {
            guard.amp.swr = 8.0;
            guard.amp.warning = Some("mock_high_swr".to_string());
        }
    }
    if cfg.kat500.mock {
        if cfg.mock.tgxl_fault {
            guard.tuner.connection_state = ConnectionState::Degraded;
            guard.tuner.connected = false;
            guard.tuner.fault = Some("mock_tgxl_fault".to_string());
        }
        if cfg.mock.high_swr {
            guard.tuner.swr = 8.0;
        }
    }
}

async fn stale_state_watchdog(
    state: SharedState,
    amp_stale_after: Duration,
    tuner_stale_after: Duration,
) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let now = SystemTime::now();
        let (amp_stale, tuner_stale) = {
            let guard = state.read().await;
            (
                guard.amp.connection_state == ConnectionState::Connected
                    && is_stale(guard.amp.last_successful_poll_at, now, amp_stale_after),
                guard.tuner.connection_state == ConnectionState::Connected
                    && is_stale(guard.tuner.last_successful_poll_at, now, tuner_stale_after),
            )
        };
        if amp_stale {
            let mut guard = state.write().await;
            if guard.amp.connection_state != ConnectionState::Connected
                || !is_stale(guard.amp.last_successful_poll_at, now, amp_stale_after)
            {
                continue;
            }
            guard.amp.connection_state = ConnectionState::Degraded;
            guard.amp.connected = false;
            guard.amp.runtime.stale_transition_count =
                guard.amp.runtime.stale_transition_count.saturating_add(1);
            warn!(
                event_id = "stale_device_state",
                device = "KPA500",
                stale_after_ms = amp_stale_after.as_millis(),
                "KPA500 state degraded because polling timestamp is stale"
            );
        }
        if tuner_stale {
            let mut guard = state.write().await;
            if guard.tuner.connection_state != ConnectionState::Connected
                || !is_stale(guard.tuner.last_successful_poll_at, now, tuner_stale_after)
            {
                continue;
            }
            guard.tuner.connection_state = ConnectionState::Degraded;
            guard.tuner.connected = false;
            guard.tuner.runtime.stale_transition_count =
                guard.tuner.runtime.stale_transition_count.saturating_add(1);
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
                let content_length = http_content_length(&body);
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    content_length,
                    body
                )
            } else {
                let body = "{\"error\":\"not_found\"}";
                let content_length = http_content_length(body);
                format!(
                    "HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    content_length,
                    body
                )
            };
            if let Err(err) = socket.write_all(status.as_bytes()).await {
                warn!(%peer, error = %err, "failed to write metrics response");
            }
        });
    }
}

#[allow(clippy::needless_as_bytes)]
fn http_content_length(body: &str) -> usize {
    body.as_bytes().len()
}

async fn status_json(state: &SharedState) -> String {
    let started = BRIDGE_STARTED_AT
        .get()
        .copied()
        .unwrap_or_else(SystemTime::now);
    let guard = state.read().await;
    serde_json::json!({
        "bridge": {
            "version": env!("CARGO_PKG_VERSION"),
            "git_commit": option_env!("GIT_HASH").unwrap_or("unknown"),
            "process_id": std::process::id(),
            "uptime_ms": SystemTime::now().duration_since(started).unwrap_or_default().as_millis(),
            "config_path": "unknown",
        },
        "amp": {
            "connection_state": guard.amp.connection_state.as_str(),
            "connected": guard.amp.is_connected(),
            "operate": guard.amp.operate,
            "state": guard.amp.state.pgxl_state(),
            "forward_power_watts": guard.amp.forward_power_watts,
            "reflected_power_watts": guard.amp.reflected_power_watts,
            "swr": guard.amp.swr,
            "pa_current_amps": guard.amp.pa_current_amps,
            "pa_voltage_volts": guard.amp.pa_voltage_volts,
            "temperature_c": guard.amp.temperature_c,
            "fault": guard.amp.fault,
            "warning": guard.amp.warning,
            "firmware_version": guard.amp.firmware_version,
            "serial_number": guard.amp.serial_number,
            "capabilities": guard.amp.capabilities,
            "last_successful_poll_ms": system_time_ms(guard.amp.last_successful_poll_at),
            "stale_duration_ms": stale_duration_ms(guard.amp.last_successful_poll_at),
            "runtime": runtime_json(&guard.amp.runtime),
        },
        "tuner": {
            "connection_state": guard.tuner.connection_state.as_str(),
            "connected": guard.tuner.is_connected(),
            "operate": guard.tuner.operate,
            "mode": tuner_mode_label(&guard.tuner),
            "bypass": guard.tuner.bypass,
            "tuning": guard.tuner.tuning,
            "selected_antenna": guard.tuner.selected_antenna,
            "relay_c1": guard.tuner.relay_c1,
            "relay_l": guard.tuner.relay_l,
            "relay_c2": guard.tuner.relay_c2,
            "forward_power_watts": guard.tuner.forward_power_watts,
            "reflected_power_watts": 0.0,
            "swr": guard.tuner.swr,
            "fault": guard.tuner.fault,
            "firmware_version": guard.tuner.firmware_version,
            "serial_number": guard.tuner.serial_number,
            "capabilities": guard.tuner.capabilities,
            "last_successful_poll_ms": system_time_ms(guard.tuner.last_successful_poll_at),
            "stale_duration_ms": stale_duration_ms(guard.tuner.last_successful_poll_at),
            "runtime": runtime_json(&guard.tuner.runtime),
        },
        "clients": {
            "pgxl_connected": guard.clients.pgxl_connected,
            "tgxl_connected": guard.clients.tgxl_connected,
            "pgxl_client_count": guard.clients.pgxl_client_count,
            "tgxl_client_count": guard.clients.tgxl_client_count,
            "pgxl_session_started_count": guard.clients.pgxl_session_started_count,
            "tgxl_session_started_count": guard.clients.tgxl_session_started_count,
            "pgxl_sessions": guard.clients.pgxl_sessions,
            "tgxl_sessions": guard.clients.tgxl_sessions,
            "pgxl_last_disconnect_reason": guard.clients.pgxl_last_disconnect_reason,
            "tgxl_last_disconnect_reason": guard.clients.tgxl_last_disconnect_reason,
            "pgxl_manual_connect_no_socket_attempt_count": guard.clients.pgxl_manual_connect_no_socket_attempt_count,
            "pgxl_last_no_socket_attempt_warning": guard.clients.pgxl_last_no_socket_attempt_warning,
        },
        "flex_injection": guard.flex_injection,
        "controls": guard.controls,
        "flex_diagnostics": {
            "ping_count": guard.flex_injection.ping_count,
            "ping_ack_count": guard.flex_injection.ping_ack_count,
            "ping_failures": guard.flex_injection.ping_failure_count,
            "last_ping_latency_ms": guard.flex_injection.last_ping_latency_ms,
            "pending_count": guard.flex_injection.pending_count,
            "expired_pending_count": guard.flex_injection.expired_pending_count,
            "degraded_reason": guard.flex_injection.degraded_reason,
            "smartsdr_tuner_appeared_count": guard.flex_injection.tuner_appeared_count,
            "smartsdr_tuner_disappeared_count": guard.flex_injection.tuner_disappeared_count,
            "smartsdr_tuner_last_disappearance_reason": guard.flex_injection.last_tuner_disappearance_reason,
            "flex_tuner_presence_age_ms": stale_duration_ms(guard.flex_injection.tuner_last_seen_at),
            "registration_refresh_count": guard.flex_injection.tuner_registration_refresh_count,
            "tuner_presence_expired_count": guard.flex_injection.tuner_presence_expired_count,
            "tuner_reannounce_count": guard.flex_injection.tuner_reannounce_count,
            "amplifier_reannounce_count": guard.flex_injection.amplifier_reannounce_count,
            "amplifier_direct_connect_expected": guard.flex_injection.amplifier_direct_connect_expected,
            "tuner_presence_age_ms": stale_duration_ms(guard.flex_injection.tuner_last_seen_at),
            "amplifier_presence_age_ms": stale_duration_ms(guard.flex_injection.amplifier_last_seen_at),
        },
        "protocol": guard.protocol,
    })
    .to_string()
}

fn tuner_mode_label(tuner: &bridge_core::TunerState) -> &'static str {
    if tuner.bypass {
        "bypass"
    } else if tuner.operate {
        "auto"
    } else {
        "manual"
    }
}

async fn print_soak_summary(state: &SharedState, elapsed: Duration) {
    let guard = state.read().await;
    info!(
        event_id = "soak_summary",
        elapsed_secs = elapsed.as_secs(),
        amp_state = guard.amp.connection_state.as_str(),
        amp_poll_success = guard.amp.runtime.poll_success_count,
        amp_poll_failures = guard.amp.runtime.poll_failure_count,
        amp_reconnects = guard.amp.runtime.reconnect_count,
        amp_stale_transitions = guard.amp.runtime.stale_transition_count,
        amp_avg_poll_latency_ms = guard.amp.runtime.average_poll_latency_ms(),
        amp_max_poll_latency_ms = guard.amp.runtime.max_poll_latency_ms,
        amp_stale_duration_ms = stale_duration_ms(guard.amp.last_successful_poll_at),
        tuner_state = guard.tuner.connection_state.as_str(),
        tuner_poll_success = guard.tuner.runtime.poll_success_count,
        tuner_poll_failures = guard.tuner.runtime.poll_failure_count,
        tuner_reconnects = guard.tuner.runtime.reconnect_count,
        tuner_stale_transitions = guard.tuner.runtime.stale_transition_count,
        tuner_avg_poll_latency_ms = guard.tuner.runtime.average_poll_latency_ms(),
        tuner_max_poll_latency_ms = guard.tuner.runtime.max_poll_latency_ms,
        tuner_stale_duration_ms = stale_duration_ms(guard.tuner.last_successful_poll_at),
        pgxl_clients = guard.clients.pgxl_client_count,
        tgxl_clients = guard.clients.tgxl_client_count,
        pgxl_unknown = guard.protocol.pgxl.unknown_commands,
        pgxl_parse_failures = guard.protocol.pgxl.parse_failures,
        tgxl_unknown = guard.protocol.tgxl.unknown_commands,
        tgxl_parse_failures = guard.protocol.tgxl.parse_failures,
        "soak health summary"
    );
    println!(
        "soak elapsed={}s amp={} ok={} fail={} reconnect={} stale={} avg_ms={:?} max_ms={} tuner={} ok={} fail={} reconnect={} stale={} avg_ms={:?} max_ms={} clients(pgxl/tgxl)={}/{} protocol_unknown(pgxl/tgxl)={}/{}",
        elapsed.as_secs(),
        guard.amp.connection_state.as_str(),
        guard.amp.runtime.poll_success_count,
        guard.amp.runtime.poll_failure_count,
        guard.amp.runtime.reconnect_count,
        guard.amp.runtime.stale_transition_count,
        guard.amp.runtime.average_poll_latency_ms(),
        guard.amp.runtime.max_poll_latency_ms,
        guard.tuner.connection_state.as_str(),
        guard.tuner.runtime.poll_success_count,
        guard.tuner.runtime.poll_failure_count,
        guard.tuner.runtime.reconnect_count,
        guard.tuner.runtime.stale_transition_count,
        guard.tuner.runtime.average_poll_latency_ms(),
        guard.tuner.runtime.max_poll_latency_ms,
        guard.clients.pgxl_client_count,
        guard.clients.tgxl_client_count,
        guard.protocol.pgxl.unknown_commands,
        guard.protocol.tgxl.unknown_commands
    );
}

fn runtime_json(stats: &bridge_core::state::DeviceRuntimeStats) -> serde_json::Value {
    serde_json::json!({
        "reconnect_count": stats.reconnect_count,
        "poll_success_count": stats.poll_success_count,
        "poll_failure_count": stats.poll_failure_count,
        "stale_transition_count": stats.stale_transition_count,
        "last_poll_latency_ms": stats.last_poll_latency_ms,
        "max_poll_latency_ms": stats.max_poll_latency_ms,
        "average_poll_latency_ms": stats.average_poll_latency_ms(),
    })
}

fn system_time_ms(value: Option<SystemTime>) -> Option<u128> {
    value.and_then(|time| {
        time.duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_millis())
    })
}

fn timestamp_compact() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn timestamp_ymdhms() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

fn stale_duration_ms(value: Option<SystemTime>) -> Option<u128> {
    value
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .map(|duration| duration.as_millis())
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

async fn serial_probe(
    port: &str,
    baud: u32,
    send: &str,
    terminator: SerialTerminator,
    wait: Duration,
    transcript_dir: Option<PathBuf>,
) -> Result<()> {
    let mut bytes = send.as_bytes().to_vec();
    append_terminator(&mut bytes, terminator);
    println!(
        "serial-probe port={port} baud={baud} send={} terminator={terminator:?} timeout_ms={}",
        printable_bytes(&bytes),
        wait.as_millis()
    );
    let mut stream = tokio_serial::new(port, baud)
        .open_native_async()
        .with_context(|| format!("failed to open serial port {port} at {baud} baud"))?;
    let transcript_path = write_probe_transcript_header(&transcript_dir, port, baud, "probe").await;
    if let Some(path) = &transcript_path {
        println!("transcript={}", path.display());
    }
    append_probe_transcript(&transcript_path, "TX", &bytes).await;
    stream
        .write_all(&bytes)
        .await
        .context("failed to write serial probe command")?;
    stream
        .flush()
        .await
        .context("failed to flush serial port")?;
    let response = read_available_response(&mut stream, wait).await?;
    append_probe_transcript(&transcript_path, "RX", &response).await;
    print_serial_response(&bytes, &response);
    Ok(())
}

async fn serial_probe_batch(
    port: &str,
    baud: u32,
    sends: &[String],
    terminator: SerialTerminator,
    wait: Duration,
    transcript_dir: Option<PathBuf>,
) -> Result<()> {
    println!(
        "serial-probe-batch port={port} baud={baud} commands={} terminator={terminator:?} timeout_ms={}",
        sends.len(),
        wait.as_millis()
    );
    let mut stream = tokio_serial::new(port, baud)
        .open_native_async()
        .with_context(|| format!("failed to open serial port {port} at {baud} baud"))?;
    let transcript_path =
        write_probe_transcript_header(&transcript_dir, port, baud, "probe-batch").await;
    if let Some(path) = &transcript_path {
        println!("transcript={}", path.display());
    }
    for send in sends {
        let mut bytes = send.as_bytes().to_vec();
        append_terminator(&mut bytes, terminator);
        append_probe_transcript(&transcript_path, "TX", &bytes).await;
        stream
            .write_all(&bytes)
            .await
            .with_context(|| format!("failed to write serial probe command {send}"))?;
        stream
            .flush()
            .await
            .context("failed to flush serial port")?;
        match read_available_response(&mut stream, wait).await {
            Ok(response) => {
                append_probe_transcript(&transcript_path, "RX", &response).await;
                print!("command={} ", printable_bytes(&bytes));
                print_serial_response_inline(&bytes, &response);
                println!();
            }
            Err(err) => {
                println!(
                    "command={} classification=no response error={err}",
                    printable_bytes(&bytes)
                );
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

async fn baud_scan(
    port: &str,
    wait: Duration,
    version_query: Option<&str>,
    transcript_dir: Option<PathBuf>,
) -> Result<()> {
    let bauds = [38400_u32, 19200, 9600, 4800];
    println!(
        "baud-scan port={port} timeout_ms={} version_query={}",
        wait.as_millis(),
        version_query.unwrap_or("(none)")
    );
    let default_queries = ["RV;", "SN;", "AN;", "BYP;"];
    let transcript_path =
        write_probe_transcript_header(&transcript_dir, port, 0, "baud-scan").await;
    if let Some(path) = &transcript_path {
        println!("transcript={}", path.display());
    }
    for baud in bauds {
        println!("baud {baud}:");
        match tokio_serial::new(port, baud).open_native_async() {
            Ok(mut stream) => {
                let mut wake_response = None;
                for _ in 0..4 {
                    append_probe_transcript(&transcript_path, &format!("TX {baud}"), b";").await;
                    if let Err(err) = stream.write_all(b";").await {
                        println!("  wake write failed: {err}");
                        break;
                    }
                    let _ = stream.flush().await;
                    match read_available_response(&mut stream, wait).await {
                        Ok(response) if !response.is_empty() => {
                            append_probe_transcript(
                                &transcript_path,
                                &format!("RX {baud}"),
                                &response,
                            )
                            .await;
                            wake_response = Some(response);
                            break;
                        }
                        Ok(_) | Err(_) => {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
                if let Some(response) = wake_response {
                    print!("  wake ");
                    print_serial_response_inline(b";", &response);
                    println!();
                } else {
                    println!("  wake classification=no response");
                }
                let queries: Vec<&str> = if let Some(query) = version_query {
                    vec![query]
                } else {
                    default_queries.to_vec()
                };
                let mut command_response_count = 0_u32;
                for query in queries {
                    let query_bytes = query.as_bytes();
                    append_probe_transcript(&transcript_path, &format!("TX {baud}"), query_bytes)
                        .await;
                    stream
                        .write_all(query_bytes)
                        .await
                        .context("failed to write baud-scan query")?;
                    stream
                        .flush()
                        .await
                        .context("failed to flush serial port")?;
                    match read_available_response(&mut stream, wait).await {
                        Ok(response) => {
                            append_probe_transcript(
                                &transcript_path,
                                &format!("RX {baud}"),
                                &response,
                            )
                            .await;
                            let classification = classify_response(query_bytes, &response);
                            if matches!(
                                classification,
                                ResponseClassification::CommandResponse
                                    | ResponseClassification::EchoWithData
                            ) {
                                command_response_count += 1;
                            }
                            print!("  query={} ", printable_bytes(query_bytes));
                            print_serial_response_inline(query_bytes, &response);
                            println!();
                        }
                        Err(err) => println!(
                            "  query={} classification=no response error={err}",
                            printable_bytes(query_bytes)
                        ),
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                println!(
                    "  likely_baud={}",
                    if command_response_count > 0 {
                        "yes"
                    } else {
                        "unknown-echo-only-or-no-command-response"
                    }
                );
            }
            Err(err) => println!("open failed: {err}"),
        }
    }
    Ok(())
}

fn append_terminator(bytes: &mut Vec<u8>, terminator: SerialTerminator) {
    match terminator {
        SerialTerminator::None => {}
        SerialTerminator::Cr => bytes.push(b'\r'),
        SerialTerminator::Lf => bytes.push(b'\n'),
        SerialTerminator::Crlf => bytes.extend_from_slice(b"\r\n"),
    }
}

async fn read_available_response(stream: &mut SerialStream, wait: Duration) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let read_result = tokio::time::timeout(wait, async {
        loop {
            let mut byte = [0_u8; 1];
            let n = stream.read(&mut byte).await?;
            if n == 0 {
                continue;
            }
            buf.push(byte[0]);
            if byte[0] == b';' || byte[0] == b'\n' {
                break;
            }
        }
        Ok::<(), std::io::Error>(())
    })
    .await;
    match read_result {
        Ok(Ok(())) => Ok(buf),
        Ok(Err(err)) => Err(err).context("failed reading serial response"),
        Err(_) if buf.is_empty() => anyhow::bail!("serial response timed out"),
        Err(_) => Ok(buf),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseClassification {
    NoResponse,
    EchoOnly,
    EchoWithData,
    CommandResponse,
}

impl ResponseClassification {
    fn label(self) -> &'static str {
        match self {
            Self::NoResponse => "no response",
            Self::EchoOnly => "echo only",
            Self::EchoWithData => "echo+data",
            Self::CommandResponse => "command response",
        }
    }
}

fn classify_response(sent: &[u8], response: &[u8]) -> ResponseClassification {
    if response.is_empty() {
        return ResponseClassification::NoResponse;
    }
    if response == sent {
        return ResponseClassification::EchoOnly;
    }
    if response.starts_with(sent) {
        return ResponseClassification::EchoWithData;
    }
    ResponseClassification::CommandResponse
}

fn print_serial_response(sent: &[u8], response: &[u8]) {
    let classification = classify_response(sent, response);
    println!("response bytes={}", response.len());
    println!("classification={}", classification.label());
    println!("hex={}", hex_bytes(response));
    println!("printable={}", printable_bytes(response));
}

fn print_serial_response_inline(sent: &[u8], response: &[u8]) {
    let classification = classify_response(sent, response);
    print!(
        "classification={} hex={} printable={}",
        classification.label(),
        hex_bytes(response),
        printable_bytes(response)
    );
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn printable_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match byte {
            b'\r' => "\\r".to_string(),
            b'\n' => "\\n".to_string(),
            0x20..=0x7e => char::from(*byte).to_string(),
            _ => format!("\\x{byte:02X}"),
        })
        .collect::<Vec<_>>()
        .join("")
}

async fn write_probe_transcript_header(
    dir: &Option<PathBuf>,
    port: &str,
    baud: u32,
    kind: &str,
) -> Option<PathBuf> {
    let dir = dir.as_ref()?;
    if let Err(err) = tokio::fs::create_dir_all(dir).await {
        warn!(dir = %dir.display(), error = %err, "serial probe transcript directory could not be created");
        return None;
    }
    let safe_port = port.replace([':', '\\', '/', '.'], "_");
    let path = dir.join(format!(
        "{kind}-{}-{safe_port}-{baud}.log",
        timestamp_millis()
    ));
    let header = format!("# {kind} port={port} baud={baud}\n");
    if let Err(err) = tokio::fs::write(&path, header).await {
        warn!(path = %path.display(), error = %err, "serial probe transcript could not be created");
        return None;
    }
    Some(path)
}

async fn append_probe_transcript(path: &Option<PathBuf>, direction: &str, bytes: &[u8]) {
    let Some(path) = path else {
        return;
    };
    let row = format!(
        "{} {direction} hex={} printable={}\n",
        timestamp_millis(),
        hex_bytes(bytes),
        printable_bytes(bytes)
    );
    if let Err(err) = append_file(path, row.as_bytes()).await {
        warn!(path = %path.display(), error = %err, "serial probe transcript append failed");
    }
}

async fn append_file(path: &PathBuf, bytes: &[u8]) -> Result<()> {
    use tokio::io::AsyncWriteExt as _;
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .await
        .with_context(|| format!("failed to open transcript {}", path.display()))?;
    file.write_all(bytes)
        .await
        .with_context(|| format!("failed to append transcript {}", path.display()))
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

async fn test_kpa(cfg: &BridgeConfig, allow_control: bool, allow_rf_risk: bool) -> Result<()> {
    println!("KPA500 safety summary:");
    print_bind_safety(cfg)?;
    println!(
        "  port={} baud={} mock={} dry_run={} allow_rf_risk={}",
        cfg.kpa500.com_port,
        cfg.kpa500.baud,
        cfg.kpa500.mock,
        cfg.kpa500.dry_run,
        cfg.kpa500.allow_rf_risk || allow_rf_risk
    );
    println!(
        "  transcript_dir={}",
        cfg.logging
            .serial_transcript_dir
            .as_deref()
            .unwrap_or("(disabled)")
    );
    println!("  planned sequence: connect -> poll_status/read-only; optional controls only when flags permit");
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
            allow_rf_risk: cfg.kpa500.allow_rf_risk || allow_rf_risk,
            control_verify_delay: Duration::from_millis(cfg.control.verify_delay_ms),
            transcript_rotate_bytes: cfg.logging.transcript_rotate_bytes,
            transcript_dir: cfg
                .logging
                .serial_transcript_dir
                .as_ref()
                .map(PathBuf::from),
        },
        shared_default_state(),
    );
    driver.connect().await?;
    println!(
        "KPA500 discovery commands: {}",
        command_wires(kpa_discovery_commands())
    );
    println!(
        "KPA500 read-only poll commands: {}",
        command_wires(kpa_poll_commands())
    );
    let outcomes = driver.poll_status_outcomes().await?;
    print_kpa_outcome_summary(&outcomes);
    if allow_control {
        ensure_local_or_lan_bind(cfg)?;
        println!("KPA500 safe-control summary:");
        println!("  allow_control=true");
        println!("  allow_rf_risk={allow_rf_risk}");
        println!("  WILL SEND: set_standby wire=^OS0; safety=StateChangeSafe");
        println!("  WILL NOT SEND without --allow-rf-risk: set_operate wire=^OS1; safety=RfRisk");
        println!(
            "  verify_delay_ms={} verification=^OS; expects=^OS0;",
            cfg.control.verify_delay_ms
        );
        println!("  rollback: verify-or-force standby (^OS0;) after control test");
        if cfg.kpa500.dry_run {
            println!("KPA500 read-only validation passed.");
            println!(
                "KPA500 safe control blocked intentionally: dry_run=true, so ^OS0; was not sent."
            );
            println!("Result: PASS/WARN (hardware read-only path passed; control path intentionally blocked).");
            return Ok(());
        }
        println!("KPA500 control test: sending set_standby wire=^OS0; safety=StateChangeSafe");
        let result = driver.set_standby().await?;
        print_kpa_control_result("set_standby", &result);
        println!("KPA500 rollback: sending set_standby wire=^OS0; safety=StateChangeSafe");
        let rollback = driver.set_standby().await?;
        print_kpa_control_result("rollback_standby", &rollback);
    }
    if allow_rf_risk {
        ensure_local_or_lan_bind(cfg)?;
        if cfg.kpa500.dry_run {
            println!("KPA500 RF-risk control blocked intentionally: dry_run=true, so ^OS1; was not sent.");
            println!("Result: PASS/WARN (RF-risk request was gated by dry-run safety).");
            return Ok(());
        }
        println!("KPA500 RF-risk test: sending set_operate wire=^OS1; safety=RfRisk");
        let operate = driver.set_operate_verified().await?;
        print_kpa_control_result("set_operate", &operate);
        println!("KPA500 rollback: sending set_standby wire=^OS0; safety=StateChangeSafe");
        let rollback = driver.set_standby().await?;
        print_kpa_control_result("rollback_standby", &rollback);
    }
    Ok(())
}

async fn test_kpa_operate(cfg: &BridgeConfig, allow_rf_risk: bool) -> Result<()> {
    if !allow_rf_risk && !cfg.kpa500.allow_rf_risk {
        anyhow::bail!("test-kpa-operate requires --allow-rf-risk or kpa500.allow_rf_risk=true");
    }
    ensure_local_or_lan_bind(cfg)?;
    println!("KPA500 operate safety summary:");
    print_bind_safety(cfg)?;
    println!(
        "  port={} baud={} mock={} dry_run={} allow_rf_risk={}",
        cfg.kpa500.com_port,
        cfg.kpa500.baud,
        cfg.kpa500.mock,
        cfg.kpa500.dry_run,
        cfg.kpa500.allow_rf_risk || allow_rf_risk
    );
    println!("  workflow: verify standby -> ^OS1; verify ^OS1; immediate ^OS0; verify ^OS0;");
    if cfg.kpa500.dry_run {
        println!("KPA500 operate validation blocked intentionally: dry_run=true, so ^OS1; and ^OS0; were not sent.");
        println!("Result: PASS/WARN (serial/config path loaded; RF-risk control requires dry_run=false).");
        return Ok(());
    }
    let state = shared_default_state();
    let driver = Kpa500Driver::new(
        Kpa500Settings {
            com_port: cfg.kpa500.com_port.clone(),
            baud: cfg.kpa500.baud,
            polling_interval: Duration::from_millis(cfg.kpa500.polling_interval_ms),
            mock: cfg.kpa500.mock,
            dry_run: cfg.kpa500.dry_run,
            allow_rf_risk: cfg.kpa500.allow_rf_risk || allow_rf_risk,
            control_verify_delay: Duration::from_millis(cfg.control.verify_delay_ms),
            transcript_rotate_bytes: cfg.logging.transcript_rotate_bytes,
            transcript_dir: cfg
                .logging
                .serial_transcript_dir
                .as_ref()
                .map(PathBuf::from),
        },
        state,
    );
    driver.connect().await?;
    println!("KPA500 preflight: sending standby ^OS0;");
    let standby = driver.set_standby().await?;
    print_kpa_control_result("preflight_standby", &standby);
    println!("KPA500 RF-risk: sending operate ^OS1;");
    let operate = driver.set_operate_verified().await?;
    print_kpa_control_result("set_operate", &operate);
    println!("KPA500 rollback: sending standby ^OS0;");
    let rollback = driver.set_standby().await?;
    print_kpa_control_result("rollback_standby", &rollback);
    if rollback.verify_result != Some(KpaCommandResultState::Verified) {
        anyhow::bail!("rollback standby was not verified");
    }
    Ok(())
}

async fn test_kat(cfg: &BridgeConfig, allow_control: bool, allow_rf_risk: bool) -> Result<()> {
    println!("KAT500 safety summary:");
    print_bind_safety(cfg)?;
    println!(
        "  port={} baud={} mock={} dry_run={}",
        cfg.kat500.com_port, cfg.kat500.baud, cfg.kat500.mock, cfg.kat500.dry_run
    );
    println!(
        "  transcript_dir={}",
        cfg.logging
            .serial_transcript_dir
            .as_deref()
            .unwrap_or("(disabled)")
    );
    println!("  planned sequence: connect -> poll_status/read-only; optional controls only when flags permit");
    print_kat_command_summary(
        kat_command_map(),
        allow_control,
        allow_rf_risk,
        cfg.kat500.dry_run,
    );

    let state = shared_default_state();
    let driver = Kat500Driver::new(
        Kat500Settings {
            com_port: cfg.kat500.com_port.clone(),
            baud: cfg.kat500.baud,
            polling_interval: Duration::from_millis(cfg.kat500.polling_interval_ms),
            mock: cfg.kat500.mock,
            dry_run: cfg.kat500.dry_run,
            allow_rf_risk: cfg.kat500.allow_rf_risk || allow_rf_risk,
            transcript_rotate_bytes: cfg.logging.transcript_rotate_bytes,
            transcript_dir: cfg
                .logging
                .serial_transcript_dir
                .as_ref()
                .map(PathBuf::from),
        },
        state.clone(),
    );
    driver.discover_once().await?;
    println!(
        "KAT500 read-only test: wake/baud discovery, then RV;, SN;, AN;, BYP;, MD;, TP;, FLT;, VSWR;, VFWD;"
    );
    println!(
        "KAT500 discovery commands: {}",
        command_wires(kat_discovery_commands())
    );
    println!(
        "KAT500 read-only poll commands: {}",
        command_wires(kat_poll_commands())
    );
    let outcomes = driver.poll_status_outcomes().await?;
    print_kat_outcome_summary(&outcomes);
    print_kat_parsed_state(&state).await;
    if allow_control {
        println!("KAT500 control request blocked in Phase 12: bypass and antenna changes remain disabled");
    }
    if allow_rf_risk {
        println!("KAT500 RF-risk request blocked in Phase 12: tune remains disabled");
    }
    Ok(())
}

fn print_bind_safety(cfg: &BridgeConfig) -> Result<()> {
    let server_ip = parse_ip("server.bind_ip", &cfg.server.bind_ip)?;
    let metrics_ip = parse_ip("metrics.bind_ip", &cfg.metrics.bind_ip)?;
    println!(
        "  bind server={} scope={} metrics={} scope={}",
        cfg.server.bind_ip,
        bind_scope(server_ip),
        cfg.metrics.bind_ip,
        bind_scope(metrics_ip)
    );
    Ok(())
}

fn ensure_local_or_lan_bind(cfg: &BridgeConfig) -> Result<()> {
    let server_ip = parse_ip("server.bind_ip", &cfg.server.bind_ip)?;
    let metrics_ip = parse_ip("metrics.bind_ip", &cfg.metrics.bind_ip)?;
    if !is_local_or_private(server_ip) {
        anyhow::bail!(
            "refusing control test: server.bind_ip={} is not loopback/private LAN",
            cfg.server.bind_ip
        );
    }
    if cfg.metrics.enabled && !is_local_or_private(metrics_ip) {
        anyhow::bail!(
            "refusing control test: metrics.bind_ip={} is not loopback/private LAN",
            cfg.metrics.bind_ip
        );
    }
    Ok(())
}

fn parse_ip(label: &str, value: &str) -> Result<IpAddr> {
    value
        .parse()
        .with_context(|| format!("{label} passed config validation but failed to parse"))
}

fn bind_scope(ip: IpAddr) -> &'static str {
    if ip.is_loopback() {
        "loopback"
    } else if is_lan_or_loopback_or_cgnat(ip) && !ip.is_loopback() {
        "private-lan"
    } else if ip.is_unspecified() {
        "public-or-all-interfaces"
    } else {
        "public-or-non-private"
    }
}

fn is_local_or_private(ip: IpAddr) -> bool {
    is_lan_or_loopback_or_cgnat(ip)
}

fn command_wires<T>(commands: &[T]) -> String
where
    T: CommandWire,
{
    commands
        .iter()
        .map(CommandWire::wire)
        .collect::<Vec<_>>()
        .join(", ")
}

trait CommandWire {
    fn wire(&self) -> &'static str;
}

impl CommandWire for elecraft_kpa500::ElecraftCommand {
    fn wire(&self) -> &'static str {
        self.wire
    }
}

impl CommandWire for elecraft_kat500::ElecraftCommand {
    fn wire(&self) -> &'static str {
        self.wire
    }
}

fn print_kpa_outcome_summary(outcomes: &[KpaCommandOutcome]) {
    let succeeded = outcomes
        .iter()
        .filter(|outcome| outcome.response.is_some())
        .map(|outcome| outcome.command.label)
        .collect::<Vec<_>>();
    let failed = outcomes
        .iter()
        .filter(|outcome| outcome.error.is_some())
        .map(|outcome| outcome.command.label)
        .collect::<Vec<_>>();
    println!("KPA500 read-only summary:");
    println!("  succeeded={}", succeeded.join(", "));
    println!("  timeout_or_failed={}", failed.join(", "));
    for outcome in outcomes {
        match (&outcome.response, &outcome.error) {
            (Some(response), _) => println!(
                "  ok {} wire={} response={}",
                outcome.command.label, outcome.command.wire, response
            ),
            (_, Some(error)) => println!(
                "  failed {} wire={} error={}",
                outcome.command.label, outcome.command.wire, error
            ),
            _ => {}
        }
    }
}

fn print_kpa_control_result(label: &str, result: &KpaControlCommandResult) {
    println!("{label}:");
    println!(
        "  command={} wire={}",
        result.command.label, result.command.wire
    );
    println!("  send_result={}", control_state_label(result.send_result));
    println!(
        "  verify_result={}",
        result
            .verify_result
            .map(control_state_label)
            .unwrap_or("none")
    );
    println!(
        "  verification_response={}",
        result.verification_response.as_deref().unwrap_or("none")
    );
    println!(
        "  final_state={}",
        result
            .final_state
            .map(|state| format!("{state:?}"))
            .unwrap_or_else(|| "unknown".to_string())
    );
}

fn control_state_label(state: KpaCommandResultState) -> &'static str {
    match state {
        KpaCommandResultState::Acknowledged => "acknowledged",
        KpaCommandResultState::Verified => "verified",
        KpaCommandResultState::SentNoAck => "sent_no_ack",
        KpaCommandResultState::VerifyFailed => "verify_failed",
        KpaCommandResultState::Timeout => "timeout",
        KpaCommandResultState::ParseFailed => "parse_failed",
    }
}

fn print_kat_outcome_summary(outcomes: &[KatCommandOutcome]) {
    let succeeded = outcomes
        .iter()
        .filter(|outcome| outcome.response.is_some())
        .map(|outcome| outcome.command.label)
        .collect::<Vec<_>>();
    let failed = outcomes
        .iter()
        .filter(|outcome| outcome.error.is_some())
        .map(|outcome| outcome.command.label)
        .collect::<Vec<_>>();
    println!("KAT500 read-only summary:");
    println!("  succeeded={}", succeeded.join(", "));
    println!("  timeout_or_failed={}", failed.join(", "));
    for outcome in outcomes {
        match (&outcome.response, &outcome.error) {
            (Some(response), _) => println!(
                "  ok {} wire={} response={}",
                outcome.command.label, outcome.command.wire, response
            ),
            (_, Some(error)) => println!(
                "  failed {} wire={} error={}",
                outcome.command.label, outcome.command.wire, error
            ),
            _ => {}
        }
        for line in &outcome.unsolicited {
            println!("    unsolicited during {}: {}", outcome.command.label, line);
        }
    }
}

async fn print_kat_parsed_state(state: &SharedState) {
    let guard = state.read().await;
    let mode = guard
        .tuner
        .capabilities
        .iter()
        .find_map(|capability| capability.strip_prefix("mode="))
        .unwrap_or("unknown");
    let tune_power = guard
        .tuner
        .capabilities
        .iter()
        .find_map(|capability| capability.strip_prefix("tune_power="))
        .unwrap_or("unknown");
    let fault = guard.tuner.fault.as_deref().unwrap_or("0");
    println!("KAT500 parsed state:");
    println!(
        "  firmware={}",
        guard.tuner.firmware_version.as_deref().unwrap_or("unknown")
    );
    println!(
        "  serial={}",
        guard.tuner.serial_number.as_deref().unwrap_or("unknown")
    );
    println!(
        "  antenna={}",
        guard
            .tuner
            .selected_antenna
            .map(|antenna| antenna.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!("  bypass={}", guard.tuner.bypass);
    println!("  mode={mode}");
    println!("  tune_power={tune_power}");
    println!("  fault={fault}");
    println!("  swr={:.2}", guard.tuner.swr);
    println!("  forward_power={:.0}", guard.tuner.forward_power_watts);
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn stability_test_alias_is_wired() {
        let command = Cli::command();
        let names = command
            .get_subcommands()
            .map(|cmd| cmd.get_name().to_string())
            .collect::<Vec<_>>();
        assert!(names.contains(&"stability-test".to_string()));
        assert!(names.contains(&"evidence-test".to_string()));
    }

    #[test]
    fn rf_risk_confirmation_string_is_required() {
        assert!(require_rf_risk_confirmation(false, None).is_ok());
        assert!(require_rf_risk_confirmation(true, None).is_err());
        assert!(require_rf_risk_confirmation(true, Some("wrong")).is_err());
        assert!(require_rf_risk_confirmation(true, Some("I understand")).is_ok());
    }

    #[test]
    fn summary_markdown_reports_warning_when_no_clients_connected() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let state = shared_default_state();
        let summary = rt.block_on(evidence_summary_markdown(
            &state,
            Some(Duration::from_secs(1)),
        ));
        assert!(summary.contains("Overall result"));
        assert!(summary.contains("No direct PGXL/TGXL client connected"));
    }

    #[test]
    fn zip_dir_creates_bundle() {
        let root = PathBuf::from("target-msvc/test-evidence");
        let src = root.join(format!("src-{}", timestamp_compact()));
        let dst = root.join(format!("bundle-{}.zip", timestamp_compact()));
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("summary.md"), "# Summary\n").unwrap();
        zip_dir(&src, &dst).unwrap();
        assert!(dst.exists());
    }

    #[test]
    fn timestamp_formatter_produces_expected_epoch_date() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }
}
