#![recursion_limit = "256"]

use anyhow::{Context, Result};
use bridge_core::state::{shared_default_state, shared_mock_state};
use bridge_core::{
    append_evidence_json, append_evidence_line, set_evidence_dir, AmpOperatingState,
    ConnectionState, EffectiveControlPolicy, RuntimeConfigIdentity, SharedState,
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
use std::process::Command as ProcessCommand;
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
    ConnectionRegressionTest {
        #[arg(
            long,
            default_value = "config.aethersdr-last-known-good-operational.yaml"
        )]
        config: PathBuf,
        #[arg(long, default_value_t = 5.0)]
        duration_minutes: f64,
    },
    ControlLab {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long)]
        duration_minutes: f64,
    },
    AethersdrSmokeTest {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long)]
        duration_minutes: f64,
    },
    AethersdrProtocolTest {
        #[arg(long, default_value = "config.aethersdr-operational.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 600.0)]
        duration_seconds: f64,
    },
    EcosystemSoakTest {
        #[arg(long, default_value = "config.aethersdr-operational.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 30.0)]
        duration_minutes: f64,
    },
    ReplaySession {
        #[arg(long)]
        bundle: PathBuf,
    },
    AethersdrOperationalTest {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 60.0)]
        duration_seconds: f64,
    },
    FullOperationalTest {
        #[arg(long, default_value = "config.aethersdr-operational.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 180.0)]
        duration_seconds: f64,
    },
    FullAethersdrFunctionalTest {
        #[arg(
            long,
            default_value = "config.aethersdr-last-known-good-real-controls.yaml"
        )]
        config: PathBuf,
        #[arg(long, default_value_t = 5.0)]
        duration_minutes: f64,
    },
    OperationalGapTest {
        #[arg(
            long,
            default_value = "config.aethersdr-last-known-good-real-controls.yaml"
        )]
        config: PathBuf,
        #[arg(long, default_value_t = 5.0)]
        duration_minutes: f64,
    },
    PgxlTriggerStrategyTest {
        #[arg(
            long,
            default_value = "config.aethersdr-last-known-good-real-controls.yaml"
        )]
        config: PathBuf,
        #[arg(long, default_value = "current")]
        strategy: String,
        #[arg(long, default_value_t = 120.0)]
        duration_seconds: f64,
    },
    AethersdrOpenTriggerTest {
        #[arg(
            long,
            default_value = "config.aethersdr-last-known-good-real-controls.yaml"
        )]
        config: PathBuf,
        #[arg(long, default_value = "current")]
        variant: String,
        #[arg(long, default_value_t = 120.0)]
        duration_seconds: f64,
    },
    BandFollowTest {
        #[arg(long, default_value = "config.aethersdr-kpa-band-follow-test.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 5.0)]
        duration_minutes: f64,
    },
    ComparePgxlProfiles {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 60.0)]
        duration_seconds: f64,
    },
    CompareAethersdrProfiles {
        #[arg(long, default_value = "config.aethersdr-compatible-operational.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 60.0)]
        duration_seconds: f64,
    },
    CompareCreateProfiles {
        #[arg(long, default_value = "config.aethersdr-compatible-operational.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 60.0)]
        duration_seconds: f64,
    },
    AmplifierOperateLab {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 60.0)]
        duration_seconds: f64,
    },
    PgxlDirectTriggerMatrix {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 60.0)]
        duration_seconds: f64,
    },
    TestStartupSequence {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long, default_value_t = 30.0)]
        duration_seconds: f64,
    },
    CheckConfig {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    ProtocolAudit {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    SimulateControl {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long, value_enum)]
        action: SimulatedControlAction,
    },
    SimulatePgxlControl {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long, value_enum)]
        command: PgxlControlCommand,
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
    PgxlSelfProbe {
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
    PgxlPairingLab {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
        #[arg(long, default_value = "strict_real_pgxl")]
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SimulatedControlAction {
    Tune,
    Standby,
    Operate,
    FlexOperate,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PgxlControlCommand {
    Standby,
    Operate,
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
        Commands::ConnectionRegressionTest {
            config,
            duration_minutes,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_connection_regression_test(cfg, config, duration_minutes).await
        }
        Commands::ControlLab {
            config,
            duration_minutes,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_evidence_test("control-lab", cfg, config, duration_minutes).await
        }
        Commands::AethersdrSmokeTest {
            config,
            duration_minutes,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_evidence_test("aethersdr-smoke-test", cfg, config, duration_minutes).await
        }
        Commands::AethersdrProtocolTest {
            config,
            duration_seconds,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_evidence_test(
                "aethersdr-protocol-test",
                cfg,
                config,
                duration_seconds / 60.0,
            )
            .await
        }
        Commands::EcosystemSoakTest {
            config,
            duration_minutes,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_ecosystem_soak_test(cfg, config, duration_minutes).await
        }
        Commands::ReplaySession { bundle } => {
            init_logging("info");
            replay_session_bundle(&bundle).await
        }
        Commands::AethersdrOperationalTest {
            config,
            duration_seconds,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_aethersdr_operational_test(cfg, config, duration_seconds).await
        }
        Commands::FullOperationalTest {
            config,
            duration_seconds,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_full_operational_test(cfg, config, duration_seconds).await
        }
        Commands::FullAethersdrFunctionalTest {
            config,
            duration_minutes,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_evidence_test(
                "full-aethersdr-functional-test",
                cfg,
                config,
                duration_minutes,
            )
            .await
        }
        Commands::OperationalGapTest {
            config,
            duration_minutes,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_evidence_test("operational-gap-test", cfg, config, duration_minutes).await
        }
        Commands::PgxlTriggerStrategyTest {
            config,
            strategy,
            duration_seconds,
        } => {
            let mut cfg = BridgeConfig::load(&config)?;
            cfg.flex_injection.pgxl_startup_trigger_strategy = strategy;
            cfg.validate()?;
            init_logging(&cfg.logging.level);
            run_evidence_test(
                "pgxl-trigger-strategy-test",
                cfg,
                config,
                duration_seconds / 60.0,
            )
            .await
        }
        Commands::AethersdrOpenTriggerTest {
            config,
            variant,
            duration_seconds,
        } => {
            let mut cfg = BridgeConfig::load(&config)?;
            cfg.flex_injection.aethersdr_open_trigger_variant = variant;
            cfg.flex_injection.trace_amplifier_advertisements = true;
            cfg.validate()?;
            init_logging(&cfg.logging.level);
            run_aethersdr_open_trigger_test(cfg, config, duration_seconds).await
        }
        Commands::BandFollowTest {
            config,
            duration_minutes,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_band_follow_test(cfg, config, duration_minutes).await
        }
        Commands::ComparePgxlProfiles {
            config,
            duration_seconds,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            compare_pgxl_profiles(cfg, config, duration_seconds).await
        }
        Commands::CompareAethersdrProfiles {
            config,
            duration_seconds,
        } => {
            let mut cfg = BridgeConfig::load(&config)?;
            cfg.flex_injection.amplifier_status_profile = "aethersdr_minimal".to_string();
            cfg.flex_injection.pgxl_connect_assist = false;
            cfg.validate()?;
            init_logging(&cfg.logging.level);
            compare_aethersdr_profiles(cfg, config, duration_seconds).await
        }
        Commands::CompareCreateProfiles {
            config,
            duration_seconds,
        } => {
            let mut cfg = BridgeConfig::load(&config)?;
            cfg.flex_injection.amplifier_status_profile = "aethersdr_minimal".to_string();
            cfg.flex_injection.pgxl_connect_assist = false;
            cfg.validate()?;
            init_logging(&cfg.logging.level);
            compare_create_profiles(cfg, config, duration_seconds).await
        }
        Commands::AmplifierOperateLab {
            config,
            duration_seconds,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_amplifier_operate_lab(cfg, config, duration_seconds).await
        }
        Commands::PgxlDirectTriggerMatrix {
            config,
            duration_seconds,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            run_pgxl_direct_trigger_matrix(cfg, config, duration_seconds).await
        }
        Commands::TestStartupSequence {
            config,
            duration_seconds,
        } => {
            let cfg = BridgeConfig::load(&config)?;
            init_logging(&cfg.logging.level);
            test_startup_sequence(cfg, config, duration_seconds).await
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
        Commands::ProtocolAudit { config } => {
            let cfg = BridgeConfig::load(&config)?;
            print_protocol_audit(&cfg);
            Ok(())
        }
        Commands::SimulateControl { config, action } => {
            let cfg = BridgeConfig::load(&config)?;
            simulate_control(&cfg, action)
        }
        Commands::SimulatePgxlControl { config, command } => {
            let cfg = BridgeConfig::load(&config)?;
            simulate_pgxl_control(&cfg, command)
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
        Commands::PgxlSelfProbe { host, port } => {
            init_logging("debug");
            pgxl_self_probe(&host, port).await
        }
        Commands::PgxlTriggerLab {
            config,
            profile,
            duration_minutes,
        }
        | Commands::PgxlPairingLab {
            config,
            profile,
            duration_minutes,
        } => {
            let mut cfg = BridgeConfig::load(&config)?;
            cfg.flex_injection.amplifier_status_profile = profile;
            cfg.validate()?;
            init_logging(&cfg.logging.level);
            run_pgxl_pairing_lab(cfg, config, duration_minutes).await
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

fn runtime_git_commit() -> &'static str {
    option_env!("GIT_HASH").unwrap_or("unknown")
}

fn runtime_build_timestamp() -> &'static str {
    option_env!("BUILD_TIMESTAMP").unwrap_or("unknown")
}

fn executable_path_text() -> String {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|err| format!("unknown ({err})"))
}

fn working_dir_text() -> String {
    std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|err| format!("unknown ({err})"))
}

fn print_runtime_startup_identity(config_path: Option<&Path>) {
    let config = config_path
        .map(|path| {
            path.canonicalize()
                .unwrap_or_else(|_| path.to_path_buf())
                .display()
                .to_string()
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("egb executable_path={}", executable_path_text());
    println!("egb working_dir={}", working_dir_text());
    println!("egb git_commit={}", runtime_git_commit());
    println!("egb build_timestamp={}", runtime_build_timestamp());
    println!("egb config_path={config}");
    info!(
        event_id = "runtime_identity",
        executable_path = %executable_path_text(),
        working_dir = %working_dir_text(),
        git_commit = runtime_git_commit(),
        build_timestamp = runtime_build_timestamp(),
        config_path = %config,
        "runtime identity"
    );
}

fn git_head_commit() -> Option<String> {
    let in_repo = ProcessCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    if String::from_utf8_lossy(&in_repo.stdout).trim().is_empty() {
        return None;
    }
    ProcessCommand::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn enforce_current_binary_if_possible() -> Result<()> {
    let embedded = runtime_git_commit();
    if embedded == "unknown" {
        warn!(
            event_id = "runtime_commit_unknown",
            "binary has no embedded git commit; stale-build enforcement skipped"
        );
        return Ok(());
    }
    let Some(head) = git_head_commit() else {
        info!(
            event_id = "runtime_head_unavailable",
            "git HEAD unavailable; assuming release-folder execution and skipping stale-build enforcement"
        );
        return Ok(());
    };
    if embedded != head {
        anyhow::bail!(
            "RUNTIME_COMMIT_MISMATCH: executable commit {embedded} does not match repository HEAD {head}. Rebuild and run the current egb.exe. executable_path={}",
            executable_path_text()
        );
    }
    Ok(())
}

fn advertised_pgxl_ip(cfg: &BridgeConfig) -> Result<IpAddr> {
    cfg.flex_injection
        .force_advertised_pgxl_ip
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&cfg.flex_injection.amplifier_ip)
        .parse()
        .context("flex_injection advertised PGXL IP passed validation but failed to parse")
}

fn loopback_pgxl_ip_is_intentional(cfg: &BridgeConfig) -> bool {
    let server_loopback = cfg
        .server
        .bind_ip
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false);
    let radio_loopback = cfg
        .flex_injection
        .radio_ip
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false);
    server_loopback && radio_loopback
}

fn validate_operational_start_config(cfg: &BridgeConfig, mode: BridgeStartMode) -> Result<()> {
    if cfg.flex_injection.enabled {
        let advertised = advertised_pgxl_ip(cfg)?;
        if advertised.is_loopback() && !loopback_pgxl_ip_is_intentional(cfg) {
            anyhow::bail!(
                "INVALID_PGXL_ADVERTISED_IP: flex_injection advertises loopback IP {advertised}, but the Flex radio/client path is not local-only. Set flex_injection.amplifier_ip or force_advertised_pgxl_ip to this PC's reachable LAN IP."
            );
        }
        if mode == BridgeStartMode::Operational && cfg.flex_injection.pgxl_connect_assist {
            anyhow::bail!(
                "PGXL_CONNECT_ASSIST_DISABLED_FOR_OPERATIONAL_RUN: flex_injection.pgxl_connect_assist sends a rejected Flex operate command and is no longer allowed in operational/evidence runs. Disable it or use a lab command."
            );
        }
        if mode == BridgeStartMode::Operational
            && amplifier_create_profile_emits_nonstandard_fields(
                &cfg.flex_injection.amplifier_status_profile,
            )
        {
            anyhow::bail!(
                "UNSAFE_LAB_AMPLIFIER_CREATE_PROFILE: flex_injection.amplifier_status_profile={} is a lab-only profile for operational/evidence runs. Use aethersdr_force_direct for the locked AetherSDR regression baseline, aethersdr_minimal for minimal AetherSDR compatibility, or official_pgxl/pgxl_paired/minimal/strict_real_pgxl for strict registration tests.",
                cfg.flex_injection.amplifier_status_profile
            );
        }
        let command = flex_injection::amplifier_create_command_with_state(
            advertised,
            cfg.flex_injection.amplifier_port,
            &cfg.flex_injection.amplifier_model,
            &cfg.flex_injection.serial,
            &cfg.flex_injection.ant_map,
            &cfg.flex_injection.amplifier_status_profile,
            Some("STANDBY"),
        );
        if let Err(err) = flex_injection::validate_amplifier_create_for_profile(
            &cfg.flex_injection.amplifier_status_profile,
            &command,
        ) {
            anyhow::bail!("INVALID_AMPLIFIER_CREATE_PROFILE: {err}");
        }
    }
    Ok(())
}

fn amplifier_create_profile_emits_nonstandard_fields(profile: &str) -> bool {
    matches!(
        profile,
        "pgxl_verbose" | "old_good_pgxl" | "aethersdr_pgxl_direct_lab"
    )
}

async fn run_startup_preflights(
    cfg: &BridgeConfig,
    state: &SharedState,
    policy: &EffectiveControlPolicy,
    config_path: Option<&Path>,
    mode: BridgeStartMode,
) -> Result<()> {
    validate_operational_start_config(cfg, mode)?;
    if cfg.kpa500.enabled && !cfg.kpa500.mock && (cfg.pgxl.enabled || cfg.flex_injection.enabled) {
        run_kpa_startup_preflight(cfg, state, policy).await?;
    }
    if cfg.kat500.enabled && !cfg.kat500.mock && cfg.tgxl.enabled {
        run_kat_startup_preflight(cfg, state, policy).await?;
    }
    append_evidence_line(
        "startup-preflight.log",
        format!(
            "startup preflight passed config_path={} mode={mode:?}",
            config_path
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ),
    );
    Ok(())
}

async fn run_kpa_startup_preflight(
    cfg: &BridgeConfig,
    state: &SharedState,
    policy: &EffectiveControlPolicy,
) -> Result<()> {
    append_evidence_line(
        "kpa-startup-diagnostics.md",
        format!(
            "- KPA500 preflight: port `{}` baud `{}` dry_run `{}`.",
            cfg.kpa500.com_port, cfg.kpa500.baud, policy.effective_kpa_dry_run
        ),
    );
    info!(
        event_id = "kpa500_startup_preflight",
        port = %cfg.kpa500.com_port,
        baud = cfg.kpa500.baud,
        dry_run = policy.effective_kpa_dry_run,
        "KPA500 startup preflight started"
    );
    let driver = Kpa500Driver::new(
        Kpa500Settings {
            com_port: cfg.kpa500.com_port.clone(),
            baud: cfg.kpa500.baud,
            polling_interval: Duration::from_millis(cfg.kpa500.polling_interval_ms),
            mock: false,
            dry_run: policy.effective_kpa_dry_run,
            allow_control: policy.effective_kpa_allow_control,
            allow_rf_risk: policy.effective_kpa_allow_rf_risk,
            follow_flex_band: cfg.kpa500.follow_flex_band,
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
    let outcomes = driver.poll_status_outcomes().await.map_err(|err| {
        anyhow::anyhow!(
            "KPA500_PORT_LOCKED_OR_UNAVAILABLE: failed KPA500 preflight on {} at {} baud before PGXL/Flex startup: {}. Close Elecraft KPA500 Remote, other EGB instances, or any process holding the COM port.",
            cfg.kpa500.com_port,
            cfg.kpa500.baud,
            err
        )
    })?;
    let failed = outcomes
        .iter()
        .filter(|outcome| outcome.error.is_some())
        .map(|outcome| {
            format!(
                "{}: {}",
                outcome.command.label,
                outcome.error.as_deref().unwrap_or("unknown error")
            )
        })
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        anyhow::bail!(
            "KPA500_PORT_LOCKED_OR_UNAVAILABLE: KPA500 preflight opened {} but required read-only commands failed: {}",
            cfg.kpa500.com_port,
            failed.join("; ")
        );
    }
    append_evidence_line(
        "kpa-startup-diagnostics.md",
        "- KPA500 preflight passed: `^OS;`, `^WS;`, `^TM;`, `^VI;`, and `^FL;` returned valid responses before PGXL/Flex startup.",
    );
    info!(
        event_id = "kpa500_startup_preflight_passed",
        port = %cfg.kpa500.com_port,
        "KPA500 startup preflight passed"
    );
    Ok(())
}

async fn run_kat_startup_preflight(
    cfg: &BridgeConfig,
    state: &SharedState,
    policy: &EffectiveControlPolicy,
) -> Result<()> {
    append_evidence_line(
        "kpa-startup-diagnostics.md",
        format!(
            "- KAT500 preflight: port `{}` baud `{}` dry_run `{}`.",
            cfg.kat500.com_port, cfg.kat500.baud, policy.effective_kat_dry_run
        ),
    );
    info!(
        event_id = "kat500_startup_preflight",
        port = %cfg.kat500.com_port,
        baud = cfg.kat500.baud,
        dry_run = policy.effective_kat_dry_run,
        "KAT500 startup preflight started"
    );
    let driver = Kat500Driver::new(
        Kat500Settings {
            com_port: cfg.kat500.com_port.clone(),
            baud: cfg.kat500.baud,
            polling_interval: Duration::from_millis(cfg.kat500.polling_interval_ms),
            mock: false,
            dry_run: policy.effective_kat_dry_run,
            allow_control: policy.effective_kat_allow_control,
            allow_rf_risk: policy.effective_kat_allow_rf_risk,
            follow_flex_frequency: cfg.kat500.follow_flex_frequency,
            transcript_rotate_bytes: cfg.logging.transcript_rotate_bytes,
            transcript_dir: cfg
                .logging
                .serial_transcript_dir
                .as_ref()
                .map(PathBuf::from),
        },
        state.clone(),
    );
    let outcomes = driver.poll_status_outcomes().await.map_err(|err| {
        anyhow::anyhow!(
            "KAT500_PORT_LOCKED_OR_UNAVAILABLE: failed KAT500 preflight on {} at {} baud before TGXL startup: {}. Close Elecraft KAT500 Utility, other EGB instances, or any process holding the COM port.",
            cfg.kat500.com_port,
            cfg.kat500.baud,
            err
        )
    })?;
    let failed = outcomes
        .iter()
        .filter(|outcome| outcome.error.is_some())
        .map(|outcome| {
            format!(
                "{}: {}",
                outcome.command.label,
                outcome.error.as_deref().unwrap_or("unknown error")
            )
        })
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        anyhow::bail!(
            "KAT500_PORT_LOCKED_OR_UNAVAILABLE: KAT500 preflight opened {} but required read-only commands failed: {}",
            cfg.kat500.com_port,
            failed.join("; ")
        );
    }
    append_evidence_line(
        "kpa-startup-diagnostics.md",
        "- KAT500 preflight passed before TGXL startup.",
    );
    info!(
        event_id = "kat500_startup_preflight_passed",
        port = %cfg.kat500.com_port,
        "KAT500 startup preflight passed"
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeStartMode {
    Operational,
    Lab,
}

async fn start_bridge(
    cfg: &BridgeConfig,
    config_path: Option<&Path>,
    mode: BridgeStartMode,
) -> Result<SharedState> {
    let _ = BRIDGE_STARTED_AT.set(SystemTime::now());
    print_runtime_startup_identity(config_path);
    enforce_current_binary_if_possible()?;
    let all_mock = cfg.kpa500.mock && cfg.kat500.mock;
    let state = if all_mock {
        shared_mock_state()
    } else {
        shared_default_state()
    };
    apply_mock_config(cfg, &state).await;
    let control_policy = effective_control_policy(cfg);
    let config_identity = runtime_config_identity(cfg, config_path)?;
    let kpa_dry_run = control_policy.effective_kpa_dry_run;
    let kat_dry_run = control_policy.effective_kat_dry_run;
    let kpa_allow_control = control_policy.effective_kpa_allow_control;
    let kat_allow_control = control_policy.effective_kat_allow_control;
    let kpa_allow_rf_risk = control_policy.effective_kpa_allow_rf_risk;
    let kat_allow_rf_risk = control_policy.effective_kat_allow_rf_risk;
    {
        let mut guard = state.write().await;
        guard.effective_controls = control_policy.clone();
        guard.config_identity = config_identity;
        guard.flex_injection.active_amplifier_status_profile =
            Some(cfg.flex_injection.amplifier_status_profile.clone());
        guard.flex_injection.active_tgxl_control_profile = Some(cfg.tgxl.control_profile.clone());
        guard.amp.startup_state_policy =
            Some(cfg.flex_injection.amplifier_startup_state_policy.clone());
    }

    run_startup_preflights(cfg, &state, &control_policy, config_path, mode).await?;

    if cfg.kpa500.enabled {
        let driver = Kpa500Driver::new(
            Kpa500Settings {
                com_port: cfg.kpa500.com_port.clone(),
                baud: cfg.kpa500.baud,
                polling_interval: Duration::from_millis(cfg.kpa500.polling_interval_ms),
                mock: cfg.kpa500.mock,
                dry_run: kpa_dry_run,
                allow_control: kpa_allow_control,
                allow_rf_risk: kpa_allow_rf_risk,
                follow_flex_band: cfg.kpa500.follow_flex_band,
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
                dry_run: kat_dry_run,
                allow_control: kat_allow_control,
                allow_rf_risk: kat_allow_rf_risk,
                follow_flex_frequency: cfg.kat500.follow_flex_frequency,
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
            status_profile: cfg.pgxl.status_profile.clone(),
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
        let advertised_amplifier_ip = advertised_pgxl_ip(cfg)?;
        let settings = FlexInjectionSettings {
            radio_addr: SocketAddr::new(radio_ip, cfg.flex_injection.radio_port),
            amplifier_ip: advertised_amplifier_ip,
            amplifier_port: cfg.flex_injection.amplifier_port,
            amplifier_model: cfg.flex_injection.amplifier_model.clone(),
            serial: cfg.flex_injection.serial.clone(),
            handle_label: cfg.flex_injection.handle.clone(),
            ant_map: cfg.flex_injection.ant_map.clone(),
            amplifier_status_profile: cfg.flex_injection.amplifier_status_profile.clone(),
            trace_amplifier_advertisements: cfg.flex_injection.trace_amplifier_advertisements,
            pgxl_force_operate_advertisement: cfg.flex_injection.pgxl_force_operate_advertisement,
            flex_force_operate_via_radio: cfg.flex_injection.flex_force_operate_via_radio,
            pgxl_connect_assist: cfg.flex_injection.pgxl_connect_assist,
            amplifier_startup_state_policy: cfg
                .flex_injection
                .amplifier_startup_state_policy
                .clone(),
            wait_first_kpa_poll_timeout: Duration::from_millis(
                cfg.flex_injection.wait_first_kpa_poll_timeout_ms,
            ),
            full_pgxl_registration: cfg.flex_injection.full_pgxl_registration,
            create_meters: cfg.flex_injection.create_meters,
            create_interlock: cfg.flex_injection.create_interlock,
            disable_amp_interlock: cfg.flex_injection.disable_amp_interlock,
            allow_rf_risk: kpa_allow_rf_risk,
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
            pgxl_startup_trigger_strategy: cfg.flex_injection.pgxl_startup_trigger_strategy.clone(),
            aethersdr_open_trigger_variant: cfg
                .flex_injection
                .aethersdr_open_trigger_variant
                .clone(),
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
                guard.clients.pgxl_last_no_socket_warning_at_ms = Some(timestamp_millis());
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

    if cfg.kpa500.enabled && !cfg.kpa500.mock {
        let state = state.clone();
        let port = cfg.kpa500.com_port.clone();
        let baud = cfg.kpa500.baud;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let guard = state.read().await;
            if !guard.amp.first_poll_completed {
                let warning = format!(
                    "kpa500_not_polling: no successful KPA500 poll within 5 seconds on {port} at {baud} baud; port_error={}; first_poll_error={}",
                    guard.amp.serial_port_open_error.as_deref().unwrap_or("none"),
                    guard.amp.first_poll_error.as_deref().unwrap_or("none")
                );
                drop(guard);
                append_evidence_line("warnings-errors.log", warning.clone());
                append_evidence_line("first-poll-sequence.log", warning.clone());
                warn!(
                    event_id = "kpa500_not_polling",
                    port = %port,
                    baud = baud,
                    "KPA500 did not complete a successful poll within startup window"
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

    {
        let state = state.clone();
        tokio::spawn(async move {
            state_advertisement_watchdog(state).await;
        });
    }

    Ok(state)
}

fn operational_controls_confirmed(cfg: &BridgeConfig) -> bool {
    cfg.operational.enable_real_controls && cfg.operational.controls_confirmed()
}

fn enabled_reason(enabled: bool, enabled_text: &str, disabled_text: &str) -> String {
    if enabled {
        enabled_text.to_string()
    } else {
        disabled_text.to_string()
    }
}

fn effective_control_policy(cfg: &BridgeConfig) -> EffectiveControlPolicy {
    let confirmed = operational_controls_confirmed(cfg);
    let operational_confirmation_valid = cfg.operational.controls_confirmed();

    let effective_kat_tune_enabled = (!cfg.kat500.dry_run && cfg.kat500.allow_rf_risk)
        || (confirmed && cfg.operational.enable_kat_tune);
    let effective_kat_bypass_enabled = (!cfg.kat500.dry_run && cfg.kat500.allow_control)
        || (confirmed && cfg.operational.enable_kat_bypass);
    let effective_kat_antenna_enabled = (!cfg.kat500.dry_run && cfg.kat500.allow_control)
        || (confirmed && cfg.operational.enable_kat_antenna);
    let effective_kpa_standby_enabled = (!cfg.kpa500.dry_run && cfg.kpa500.allow_control)
        || (confirmed && cfg.operational.enable_kpa_standby);
    let effective_kpa_operate_enabled = (!cfg.kpa500.dry_run && cfg.kpa500.allow_rf_risk)
        || (confirmed && cfg.operational.enable_kpa_operate);
    let effective_clear_fault_enabled = confirmed && cfg.operational.enable_clear_fault;

    let effective_kpa_dry_run = !(effective_kpa_standby_enabled
        || effective_kpa_operate_enabled
        || effective_clear_fault_enabled)
        && cfg.kpa500.dry_run;
    let effective_kat_dry_run = !(effective_kat_tune_enabled
        || effective_kat_bypass_enabled
        || effective_kat_antenna_enabled)
        && cfg.kat500.dry_run;

    EffectiveControlPolicy {
        raw_kpa_dry_run: cfg.kpa500.dry_run,
        raw_kpa_allow_control: cfg.kpa500.allow_control,
        raw_kpa_allow_rf_risk: cfg.kpa500.allow_rf_risk,
        raw_kat_dry_run: cfg.kat500.dry_run,
        raw_kat_allow_control: cfg.kat500.allow_control,
        raw_kat_allow_rf_risk: cfg.kat500.allow_rf_risk,
        operational_enabled: cfg.operational.enable_real_controls,
        operational_confirmation_valid,
        operational_override_active: confirmed,
        effective_kat_tune_enabled,
        effective_kat_bypass_enabled,
        effective_kat_antenna_enabled,
        effective_kpa_standby_enabled,
        effective_kpa_operate_enabled,
        effective_clear_fault_enabled,
        effective_kpa_dry_run,
        effective_kpa_allow_control: cfg.kpa500.allow_control || effective_kpa_standby_enabled,
        effective_kpa_allow_rf_risk: cfg.kpa500.allow_rf_risk || effective_kpa_operate_enabled,
        effective_kat_dry_run,
        effective_kat_allow_control: cfg.kat500.allow_control
            || effective_kat_bypass_enabled
            || effective_kat_antenna_enabled,
        effective_kat_allow_rf_risk: cfg.kat500.allow_rf_risk || effective_kat_tune_enabled,
        kat_tune_reason: enabled_reason(
            effective_kat_tune_enabled,
            "enabled by operational KAT tune or raw kat500.allow_rf_risk",
            "disabled: enable operational real controls, set enable_kat_tune=true, and confirm",
        ),
        kat_bypass_reason: enabled_reason(
            effective_kat_bypass_enabled,
            "enabled by operational KAT bypass/standby or raw kat500.allow_control",
            "disabled: enable operational KAT bypass/standby or raw kat500.allow_control",
        ),
        kat_antenna_reason: enabled_reason(
            effective_kat_antenna_enabled,
            "enabled by operational KAT antenna switching or raw kat500.allow_control",
            "disabled: enable operational KAT antenna switching or raw kat500.allow_control",
        ),
        kpa_standby_reason: enabled_reason(
            effective_kpa_standby_enabled,
            "enabled by operational KPA standby or raw kpa500.allow_control",
            "disabled: enable operational KPA standby or raw kpa500.allow_control",
        ),
        kpa_operate_reason: enabled_reason(
            effective_kpa_operate_enabled,
            "enabled by operational KPA operate or raw kpa500.allow_rf_risk",
            "disabled: RF-risk gate closed; enable KPA operate only for deliberate local testing",
        ),
        clear_fault_reason: enabled_reason(
            effective_clear_fault_enabled,
            "enabled by operational clear-fault gate",
            "disabled: clear fault remains advanced/destructive by default",
        ),
    }
}

fn stable_hash_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn runtime_config_identity(
    cfg: &BridgeConfig,
    config_path: Option<&Path>,
) -> Result<RuntimeConfigIdentity> {
    let effective_yaml = serde_yaml::to_string(cfg)?;
    let effective_hash = stable_hash_hex(effective_yaml.as_bytes());
    let (path_text, source_hash, hash_match) = if let Some(path) = config_path {
        let display_path = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string();
        let source = fs::read(path).unwrap_or_default();
        let source_hash = if source.is_empty() {
            None
        } else {
            Some(stable_hash_hex(&source))
        };
        let parsed_source_hash = BridgeConfig::load(path)
            .ok()
            .and_then(|loaded| serde_yaml::to_string(&loaded).ok())
            .map(|yaml| stable_hash_hex(yaml.as_bytes()));
        let hash_match = parsed_source_hash
            .as_ref()
            .map(|parsed| parsed == &effective_hash);
        (Some(display_path), source_hash, hash_match)
    } else {
        (None, None, None)
    };
    Ok(RuntimeConfigIdentity {
        config_path: path_text,
        config_hash: Some(effective_hash.clone()),
        config_loaded_at_ms: system_time_ms(Some(SystemTime::now())),
        config_source_hash: source_hash,
        config_effective_hash: Some(effective_hash),
        config_hash_match: hash_match,
    })
}

fn effective_kpa_dry_run(cfg: &BridgeConfig) -> bool {
    effective_control_policy(cfg).effective_kpa_dry_run
}

fn effective_kat_dry_run(cfg: &BridgeConfig) -> bool {
    effective_control_policy(cfg).effective_kat_dry_run
}

fn effective_kpa_allow_control(cfg: &BridgeConfig) -> bool {
    effective_control_policy(cfg).effective_kpa_allow_control
}

fn effective_kat_allow_control(cfg: &BridgeConfig) -> bool {
    effective_control_policy(cfg).effective_kat_allow_control
}

fn effective_kpa_allow_rf_risk(cfg: &BridgeConfig) -> bool {
    effective_control_policy(cfg).effective_kpa_allow_rf_risk
}

fn effective_kat_allow_rf_risk(cfg: &BridgeConfig) -> bool {
    effective_control_policy(cfg).effective_kat_allow_rf_risk
}

fn simulate_control(cfg: &BridgeConfig, action: SimulatedControlAction) -> Result<()> {
    cfg.validate()?;
    let policy = effective_control_policy(cfg);
    let (source, mapped, allowed, reason) = match action {
        SimulatedControlAction::Tune => (
            "TGXL direct autotune",
            "KAT500 T;",
            policy.effective_kat_tune_enabled,
            policy.kat_tune_reason.as_str(),
        ),
        SimulatedControlAction::Standby => (
            "PGXL/Flex standby",
            "KPA500 ^OS0;",
            policy.effective_kpa_standby_enabled,
            policy.kpa_standby_reason.as_str(),
        ),
        SimulatedControlAction::Operate | SimulatedControlAction::FlexOperate => (
            "Flex amplifier set operate=1",
            "KPA500 ^OS1;",
            policy.effective_kpa_operate_enabled,
            policy.kpa_operate_reason.as_str(),
        ),
    };
    println!("simulation_source={source}");
    println!("mapped_elecraft_command={mapped}");
    println!("allowed={allowed}");
    println!(
        "decision={}",
        if allowed { "would_execute" } else { "blocked" }
    );
    println!("reason={reason}");
    println!(
        "operational_override_active={}",
        policy.operational_override_active
    );
    println!("raw_kpa_dry_run={}", policy.raw_kpa_dry_run);
    println!("raw_kat_dry_run={}", policy.raw_kat_dry_run);
    Ok(())
}

fn simulate_pgxl_control(cfg: &BridgeConfig, command: PgxlControlCommand) -> Result<()> {
    let action = match command {
        PgxlControlCommand::Standby => SimulatedControlAction::Standby,
        PgxlControlCommand::Operate => SimulatedControlAction::Operate,
    };
    println!("simulated_pgxl_command={command:?}");
    simulate_control(cfg, action)
}

fn operation_mode_label(policy: &EffectiveControlPolicy) -> &'static str {
    if policy.effective_kpa_operate_enabled {
        "RF-RISK OPERATE"
    } else if policy.effective_kat_tune_enabled || policy.effective_kpa_standby_enabled {
        "OPERATIONAL TUNE/STANDBY"
    } else {
        "MONITOR ONLY"
    }
}

fn operation_mode_detail(policy: &EffectiveControlPolicy) -> String {
    format!(
        "kat_tune={} kpa_standby={} kpa_operate={} override_active={}",
        policy.effective_kat_tune_enabled,
        policy.effective_kpa_standby_enabled,
        policy.effective_kpa_operate_enabled,
        policy.operational_override_active
    )
}

fn print_mode_banner(cfg: &BridgeConfig, mode: &str) {
    let policy = effective_control_policy(cfg);
    let label = operation_mode_label(&policy);
    let detail = operation_mode_detail(&policy);
    let border = "=".repeat(72);
    println!("{border}");
    println!("EGB {mode} MODE: {label}");
    println!("{detail}");
    println!("{border}");
    append_evidence_line(
        "mode-banner.log",
        format!("mode={mode} label={label} {detail}"),
    );
    info!(
        event_id = "mode_banner",
        mode,
        label,
        detail = %detail,
        "runtime mode banner"
    );
}

async fn run_bridge(cfg: BridgeConfig, config_path: PathBuf) -> Result<()> {
    let evidence = EvidenceRun::start("run", &config_path, &cfg, std::env::args())?;
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Operational).await?;
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
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Operational).await?;
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
    print_mode_banner(&cfg, mode);
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Operational).await?;
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

async fn run_connection_regression_test(
    cfg: BridgeConfig,
    config_path: PathBuf,
    duration_minutes: f64,
) -> Result<()> {
    if !duration_minutes.is_finite() || duration_minutes <= 0.0 {
        anyhow::bail!("--duration-minutes must be a finite value greater than 0");
    }
    let evidence = EvidenceRun::start(
        "connection-regression-test",
        &config_path,
        &cfg,
        std::env::args(),
    )?;
    print_mode_banner(&cfg, "connection-regression-test");
    append_evidence_line(
        "last-known-good-comparison.md",
        "Connection regression test uses BridgeStartMode::Lab so the locked Phase 50-era `aethersdr_force_direct` profile can be tested without altering the normal operational safety gates.",
    );
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Lab).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let duration = Duration::from_secs_f64(duration_minutes * 60.0);
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + duration;
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    interval.tick().await;
    info!(
        event_id = "connection_regression_test_started",
        duration_minutes, "connection regression test started"
    );
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            _ = interval.tick() => print_soak_summary(&state, started.elapsed()).await,
            result = tokio::signal::ctrl_c() => {
                result.context("failed waiting for Ctrl+C")?;
                info!("connection regression test interrupted by Ctrl+C");
                break;
            }
        }
    }
    sampler.abort();
    evidence.write_status("status-end.json", &state).await?;
    let report = connection_regression_report_markdown(&state).await;
    tokio::fs::write(evidence.dir().join("connection-regression-test.md"), report).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    let guard = state.read().await;
    println!(
        "connection regression: flex={} amp_removed={} pgxl_sessions={} tgxl_sessions={} pgxl_commands={} tgxl_commands={}",
        guard.flex_injection.connection_state.as_str(),
        guard.flex_injection.amplifier_removed_count,
        guard.clients.pgxl_session_started_count,
        guard.clients.tgxl_session_started_count,
        guard.protocol.pgxl.commands_received,
        guard.protocol.tgxl.commands_received
    );
    drop(guard);
    println!("connection regression bundle: {}", zip.display());
    Ok(())
}

async fn run_ecosystem_soak_test(
    cfg: BridgeConfig,
    config_path: PathBuf,
    duration_minutes: f64,
) -> Result<()> {
    if !duration_minutes.is_finite() || duration_minutes <= 0.0 {
        anyhow::bail!("--duration-minutes must be a finite value greater than 0");
    }
    let evidence = EvidenceRun::start("ecosystem-soak-test", &config_path, &cfg, std::env::args())?;
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Operational).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let duration = Duration::from_secs_f64(duration_minutes * 60.0);
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + duration;
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    interval.tick().await;
    info!(
        event_id = "ecosystem_soak_started",
        duration_minutes, "ecosystem soak test started"
    );
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            _ = interval.tick() => {
                print_soak_summary(&state, started.elapsed()).await;
            }
            result = tokio::signal::ctrl_c() => {
                result.context("failed waiting for Ctrl+C")?;
                info!("ecosystem soak interrupted by Ctrl+C");
                break;
            }
        }
    }
    sampler.abort();
    evidence.write_status("status-end.json", &state).await?;
    let report = write_ecosystem_report(&state, started.elapsed(), evidence.dir()).await?;
    let stability = write_stability_report(&state, started.elapsed(), Some(evidence.dir())).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    info!(
        event_id = "ecosystem_soak_completed",
        report = %report.display(),
        stability_report = %stability.display(),
        zip = %zip.display(),
        "ecosystem soak test completed"
    );
    println!("ecosystem report: {}", report.display());
    println!("evidence bundle: {}", zip.display());
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ProtocolAuditSummary {
    tgxl_full: u32,
    tgxl_partial: u32,
    tgxl_missing: u32,
    pgxl_full: u32,
    pgxl_partial: u32,
    pgxl_missing: u32,
}

impl ProtocolAuditSummary {
    fn tgxl_percent(self) -> u32 {
        percentage(self.tgxl_full, self.tgxl_partial, self.tgxl_missing)
    }

    fn pgxl_percent(self) -> u32 {
        percentage(self.pgxl_full, self.pgxl_partial, self.pgxl_missing)
    }

    fn unsupported_count(self) -> u32 {
        self.tgxl_missing
            .saturating_add(self.pgxl_missing)
            .saturating_add(self.tgxl_partial)
            .saturating_add(self.pgxl_partial)
    }
}

fn percentage(full: u32, partial: u32, missing: u32) -> u32 {
    let total = full.saturating_add(partial).saturating_add(missing);
    if total == 0 {
        0
    } else {
        ((full.saturating_mul(100)).saturating_add(partial.saturating_mul(50))) / total
    }
}

fn protocol_audit_summary() -> ProtocolAuditSummary {
    ProtocolAuditSummary {
        tgxl_full: 15,
        tgxl_partial: 4,
        tgxl_missing: 1,
        pgxl_full: 12,
        pgxl_partial: 5,
        pgxl_missing: 2,
    }
}

fn print_protocol_audit(_cfg: &BridgeConfig) {
    let summary = protocol_audit_summary();
    println!("Official API compliance audit");
    println!("TGXL compliance: {}%", summary.tgxl_percent());
    println!("PGXL compliance: {}%", summary.pgxl_percent());
    println!(
        "Unsupported/partial command groups: {}",
        summary.unsupported_count()
    );
    println!();
    println!("TGXL partial or intentionally unsupported:");
    println!("  discovery UDP 9010: missing for MVP; direct TCP/manual IP remains primary");
    println!("  btl: intentionally blocked; bootloader reset is not safe for bridge emulation");
    println!("  setup/ifconf set/save: accepted as safe no-op where possible; no real network config mutation");
    println!("  tune relay/move: accepted but not sent to KAT500 until relay mapping is hardware-verified");
    println!("  operate set: tracks virtual TGXL state only; no KAT500 RF action");
    println!();
    println!("PGXL partial or intentionally unsupported:");
    println!("  PGXL direct config set/save: blocked; EGB config is managed by YAML/GUI");
    println!("  PGXL direct CAT/Flex set: blocked or read-only; Flex API is the authoritative radio context");
    println!("  meter value publication: meters are created; no verified Flex TCP meter-value publish command is implemented");
    println!("  connect-assist: compatibility workaround only, not real KPA500 operate");
}

async fn write_ecosystem_report(
    state: &SharedState,
    elapsed: Duration,
    dir: &Path,
) -> Result<PathBuf> {
    let guard = state.read().await;
    let path = dir.join("ecosystem-soak-report.md");
    let body = format!(
        "# Ecosystem Soak Report\n\nElapsed: {} seconds\n\n\
## Stability Checks\n\n\
- Flex lifecycle: `{}` ({})\n\
- Amplifier lifecycle: `{}` ({})\n\
- PGXL lifecycle: `{}` ({})\n\
- TGXL lifecycle: `{}` ({})\n\
- Tune lifecycle: `{}` ({})\n\n\
## Counts\n\n\
- Amplifier create count: {}\n\
- Amplifier remove count: {}\n\
- Amplifier handle changes: {}\n\
- Duplicate amplifier creates: {}\n\
- Duplicate subscriptions: {}\n\
- PGXL sessions started: {}\n\
- TGXL sessions started: {}\n\
- Flex ping sent/ack/fail: {}/{}/{}\n\
- Tune requested/executed/failed/suppressed: {}/{}/{}/{}\n\n\
## Last Reasons\n\n\
- Last amplifier remove reason: {}\n\
- Last PGXL disconnect: {}\n\
- Last TGXL disconnect: {}\n\
- Last tune result: {}\n",
        elapsed.as_secs(),
        guard.lifecycle.flex_session.state.as_str(),
        guard
            .lifecycle
            .flex_session
            .last_transition_reason
            .as_deref()
            .unwrap_or("none"),
        guard.lifecycle.amplifier.state.as_str(),
        guard
            .lifecycle
            .amplifier
            .last_transition_reason
            .as_deref()
            .unwrap_or("none"),
        guard.lifecycle.pgxl.state.as_str(),
        guard
            .lifecycle
            .pgxl
            .last_transition_reason
            .as_deref()
            .unwrap_or("none"),
        guard.lifecycle.tgxl.state.as_str(),
        guard
            .lifecycle
            .tgxl
            .last_transition_reason
            .as_deref()
            .unwrap_or("none"),
        guard.lifecycle.tune.state.as_str(),
        guard
            .lifecycle
            .tune
            .last_transition_reason
            .as_deref()
            .unwrap_or("none"),
        guard.flex_injection.amplifier_create_count,
        guard.flex_injection.amplifier_removed_count,
        guard.flex_injection.amplifier_handle_change_count,
        guard.flex_injection.duplicate_amplifier_create_count,
        guard.flex_injection.duplicate_subscription_count,
        guard.clients.pgxl_session_started_count,
        guard.clients.tgxl_session_started_count,
        guard.flex_injection.ping_count,
        guard.flex_injection.ping_ack_count,
        guard.flex_injection.ping_failure_count,
        guard.controls.tune_requested_count,
        guard.controls.tune_executed_count,
        guard.controls.tune_failed_count,
        guard.controls.duplicate_autotune_suppressed_count,
        guard
            .flex_injection
            .last_amplifier_removed_reason
            .as_deref()
            .unwrap_or("none"),
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
        guard.controls.last_tune_result.as_deref().unwrap_or("none"),
    );
    drop(guard);
    tokio::fs::write(&path, body).await?;
    Ok(path)
}

async fn replay_session_bundle(bundle: &Path) -> Result<()> {
    let summary = if bundle.is_dir() {
        replay_summary_from_dir(bundle)?
    } else {
        replay_summary_from_zip(bundle)?
    };
    println!("{summary}");
    Ok(())
}

fn replay_summary_from_dir(dir: &Path) -> Result<String> {
    let files = [
        "flex-rx.log",
        "flex-tx.log",
        "pgxl-protocol.log",
        "tgxl-protocol.log",
        "disconnect-events.jsonl",
        "lifecycle-events.jsonl",
    ];
    let mut out = String::from("# Replay Session Summary\n\n");
    for file in files {
        let path = dir.join(file);
        let text = fs::read_to_string(&path).unwrap_or_default();
        out.push_str(&format!(
            "- `{file}`: {} lines, {} amplifier removals, {} PGXL RX/TX lines, {} TGXL RX/TX lines\n",
            text.lines().count(),
            text.matches("amplifier_removed").count() + text.matches("removed").count(),
            text.matches("PGXL").count(),
            text.matches("TGXL").count(),
        ));
    }
    Ok(out)
}

fn replay_summary_from_zip(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read ZIP {}", path.display()))?;
    let mut out = String::from("# Replay Session Summary\n\n");
    for name in [
        "flex-rx.log",
        "flex-tx.log",
        "pgxl-protocol.log",
        "tgxl-protocol.log",
        "disconnect-events.jsonl",
        "lifecycle-events.jsonl",
    ] {
        let mut count = 0usize;
        let mut removals = 0usize;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            if !file.name().ends_with(name) {
                continue;
            }
            let mut text = String::new();
            use std::io::Read;
            file.read_to_string(&mut text)?;
            count += text.lines().count();
            removals += text.matches("amplifier_removed").count() + text.matches("removed").count();
        }
        out.push_str(&format!(
            "- `{name}`: {count} lines, {removals} removal markers\n"
        ));
    }
    Ok(out)
}

async fn run_pgxl_pairing_lab(
    cfg: BridgeConfig,
    config_path: PathBuf,
    duration_minutes: f64,
) -> Result<()> {
    if !duration_minutes.is_finite() || duration_minutes <= 0.0 {
        anyhow::bail!("--duration-minutes must be a finite value greater than 0");
    }
    let evidence = EvidenceRun::start("pgxl-pairing-lab", &config_path, &cfg, std::env::args())?;
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Lab).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let duration = Duration::from_secs_f64(duration_minutes * 60.0);
    let started = Instant::now();
    tokio::time::sleep(duration).await;
    sampler.abort();
    let guard = state.read().await;
    let analysis = format!(
        "# PGXL Pairing Lab\n\nProfile: `{}`\n\nAdvertised IP: `{}`\n\nPGXL sessions started: {}\nPGXL active clients: {}\nTCP 9008 attempted after amplifier status: {}\nNo-socket warnings: {}\nLast no-socket warning: {}\nAmplifier handle: {:?}\nLast amplifier status line: `{}`\nCandidate fields: {:?}\nAmplifier reannounce count: {}\nLast reannounce reason: {}\nDirect connect expected: {:?}\n",
        cfg.flex_injection.amplifier_status_profile,
        cfg.flex_injection
            .force_advertised_pgxl_ip
            .as_deref()
            .unwrap_or(&cfg.flex_injection.amplifier_ip),
        guard.clients.pgxl_session_started_count,
        guard.clients.pgxl_client_count,
        guard
            .flex_injection
            .amplifier_pgxl_tcp_attempted_after_status,
        guard.clients.pgxl_manual_connect_no_socket_attempt_count,
        guard
            .clients
            .pgxl_last_no_socket_attempt_warning
            .as_deref()
            .unwrap_or("none"),
        guard.flex_injection.amplifier_handle,
        guard
            .flex_injection
            .last_amplifier_status_line
            .as_deref()
            .unwrap_or("none"),
        guard.flex_injection.amplifier_pairing_candidate_fields,
        guard.flex_injection.amplifier_reannounce_count,
        guard
            .flex_injection
            .last_amplifier_reannounce_reason
            .as_deref()
            .unwrap_or("none"),
        guard.flex_injection.amplifier_direct_connect_expected,
    );
    drop(guard);
    tokio::fs::write(evidence.dir().join("pgxl-trigger-analysis.md"), analysis).await?;
    let pairing = tokio::fs::read_to_string(evidence.dir().join("pgxl-trigger-analysis.md"))
        .await
        .unwrap_or_default();
    tokio::fs::write(evidence.dir().join("pgxl-pairing-analysis.md"), pairing).await?;
    evidence.write_status("status-end.json", &state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    println!(
        "PGXL trigger lab complete; evidence bundle: {}",
        zip.display()
    );
    Ok(())
}

async fn run_aethersdr_open_trigger_test(
    cfg: BridgeConfig,
    config_path: PathBuf,
    duration_seconds: f64,
) -> Result<()> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        anyhow::bail!("--duration-seconds must be a finite value greater than 0");
    }
    let evidence = EvidenceRun::start(
        "aethersdr-open-trigger-test",
        &config_path,
        &cfg,
        std::env::args(),
    )?;
    append_evidence_line(
        "aethersdr-open-trigger-test.md",
        format!(
            "# AetherSDR Open Trigger Test\n\nVariant: `{}`\n\nThis test varies only the Flex amplifier advertisement/create fields. It does not alter KPA/KAT polling, PGXL/TGXL direct protocol formatting, or real control gates.\n",
            cfg.flex_injection.aethersdr_open_trigger_variant
        ),
    );
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Lab).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs_f64(duration_seconds)).await;
    sampler.abort();
    evidence.write_status("status-end.json", &state).await?;
    evidence.write_pgxl_delayed_connect_analysis(&state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    let guard = state.read().await;
    println!(
        "AetherSDR open trigger test complete: variant={} pgxl_sessions={} tgxl_sessions={} first_pgxl_accept_ms={:?} evidence={}",
        cfg.flex_injection.aethersdr_open_trigger_variant,
        guard.clients.pgxl_session_started_count,
        guard.clients.tgxl_session_started_count,
        guard.clients.pgxl_first_accept_at_ms,
        zip.display()
    );
    Ok(())
}

async fn run_band_follow_test(
    cfg: BridgeConfig,
    config_path: PathBuf,
    duration_minutes: f64,
) -> Result<()> {
    if !duration_minutes.is_finite() || duration_minutes <= 0.0 {
        anyhow::bail!("--duration-minutes must be a finite value greater than 0");
    }
    let evidence = EvidenceRun::start("band-follow-test", &config_path, &cfg, std::env::args())?;
    print_mode_banner(&cfg, "band-follow-test");
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Operational).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs_f64(duration_minutes * 60.0)).await;
    sampler.abort();
    evidence.write_status("status-end.json", &state).await?;
    let summary = band_follow_summary_markdown(&cfg, &state).await;
    tokio::fs::write(evidence.dir().join("band-follow-summary.md"), summary).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    let guard = state.read().await;
    println!(
        "band-follow test complete: kat_sent={} kat_exact_match={:?} kpa_sent={} kpa_exact_match={:?} evidence={}",
        guard.radio_context.kat500_follow_sent_count,
        guard.radio_context.last_kat500_follow_confirmation_match,
        guard.radio_context.kpa500_follow_sent_count,
        guard.radio_context.last_kpa500_follow_confirmation_match,
        zip.display()
    );
    Ok(())
}

async fn compare_pgxl_profiles(
    mut cfg: BridgeConfig,
    config_path: PathBuf,
    duration_seconds: f64,
) -> Result<()> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        anyhow::bail!("--duration-seconds must be a finite value greater than 0");
    }
    cfg.flex_injection.trace_amplifier_advertisements = true;
    let evidence = EvidenceRun::start(
        "compare-pgxl-profiles",
        &config_path,
        &cfg,
        std::env::args(),
    )?;
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Lab).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs_f64(duration_seconds)).await;
    sampler.abort();
    let report = pgxl_profile_comparison_markdown(&cfg, &state).await;
    tokio::fs::write(evidence.dir().join("pgxl-regression-diff.md"), report).await?;
    evidence.write_status("status-end.json", &state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    println!(
        "PGXL profile comparison complete; evidence bundle: {}",
        zip.display()
    );
    Ok(())
}

async fn compare_aethersdr_profiles(
    mut cfg: BridgeConfig,
    config_path: PathBuf,
    duration_seconds: f64,
) -> Result<()> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        anyhow::bail!("--duration-seconds must be a finite value greater than 0");
    }
    cfg.flex_injection.trace_amplifier_advertisements = true;
    let evidence = EvidenceRun::start(
        "compare-aethersdr-profiles",
        &config_path,
        &cfg,
        std::env::args(),
    )?;
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Operational).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs_f64(duration_seconds)).await;
    sampler.abort();
    let report = aethersdr_profile_comparison_markdown(&cfg, &state).await;
    tokio::fs::write(
        evidence.dir().join("aethersdr-profile-comparison.md"),
        report,
    )
    .await?;
    evidence.write_status("status-end.json", &state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    println!(
        "AetherSDR profile comparison complete; active profile=aethersdr_minimal; evidence bundle: {}",
        zip.display()
    );
    Ok(())
}

async fn compare_create_profiles(
    mut cfg: BridgeConfig,
    config_path: PathBuf,
    duration_seconds: f64,
) -> Result<()> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        anyhow::bail!("--duration-seconds must be a finite value greater than 0");
    }
    cfg.flex_injection.trace_amplifier_advertisements = true;
    let evidence = EvidenceRun::start(
        "compare-create-profiles",
        &config_path,
        &cfg,
        std::env::args(),
    )?;
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Operational).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs_f64(duration_seconds)).await;
    sampler.abort();
    let report = create_profile_analysis_markdown(&cfg, &state).await;
    tokio::fs::write(evidence.dir().join("create-profile-analysis.md"), report).await?;
    evidence.write_status("status-end.json", &state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    println!(
        "Create profile comparison complete; active profile=aethersdr_minimal; evidence bundle: {}",
        zip.display()
    );
    Ok(())
}

async fn run_amplifier_operate_lab(
    mut cfg: BridgeConfig,
    config_path: PathBuf,
    duration_seconds: f64,
) -> Result<()> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        anyhow::bail!("--duration-seconds must be a finite value greater than 0");
    }
    cfg.flex_injection.trace_amplifier_advertisements = true;
    cfg.flex_injection.pgxl_force_operate_advertisement = true;
    cfg.flex_injection.flex_force_operate_via_radio = true;
    cfg.flex_injection.amplifier_status_profile = "aethersdr_pgxl_direct_lab".to_string();
    cfg.validate()?;

    let evidence = EvidenceRun::start(
        "amplifier-operate-lab",
        &config_path,
        &cfg,
        std::env::args(),
    )?;
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Lab).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs_f64(duration_seconds)).await;
    sampler.abort();
    tokio::fs::write(
        evidence.dir().join("amplifier-operate-lab.md"),
        amplifier_operate_lab_markdown(&state).await,
    )
    .await?;
    tokio::fs::write(
        evidence.dir().join("amplifier-state-rewrite-analysis.md"),
        amplifier_state_rewrite_analysis_markdown(&state).await,
    )
    .await?;
    evidence.write_status("status-end.json", &state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    println!(
        "amplifier operate lab complete; evidence bundle: {}",
        zip.display()
    );
    Ok(())
}

async fn run_aethersdr_operational_test(
    mut cfg: BridgeConfig,
    config_path: PathBuf,
    duration_seconds: f64,
) -> Result<()> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        anyhow::bail!("--duration-seconds must be a finite value greater than 0");
    }
    cfg.flex_injection.trace_amplifier_advertisements = true;
    cfg.validate()?;
    let evidence = EvidenceRun::start(
        "aethersdr-operational-test",
        &config_path,
        &cfg,
        std::env::args(),
    )?;
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Operational).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs_f64(duration_seconds)).await;
    sampler.abort();
    tokio::fs::write(
        evidence.dir().join("aethersdr-operational-test.md"),
        aethersdr_operational_test_markdown(&state).await,
    )
    .await?;
    evidence.write_status("status-end.json", &state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    println!(
        "AetherSDR operational test complete; evidence bundle: {}",
        zip.display()
    );
    Ok(())
}

async fn run_full_operational_test(
    mut cfg: BridgeConfig,
    config_path: PathBuf,
    duration_seconds: f64,
) -> Result<()> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        anyhow::bail!("--duration-seconds must be a finite value greater than 0");
    }
    cfg.flex_injection.trace_amplifier_advertisements = true;
    cfg.validate()?;
    let evidence = EvidenceRun::start(
        "full-operational-test",
        &config_path,
        &cfg,
        std::env::args(),
    )?;
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Operational).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs_f64(duration_seconds)).await;
    sampler.abort();
    tokio::fs::write(
        evidence.dir().join("operational-readiness.md"),
        operational_readiness_markdown(&cfg, &state).await,
    )
    .await?;
    tokio::fs::write(
        evidence.dir().join("operational-readiness-verdict.md"),
        operational_readiness_verdict_markdown(&cfg, &state).await,
    )
    .await?;
    tokio::fs::write(
        evidence.dir().join("effective-control-policy.md"),
        effective_control_policy_markdown(&state).await,
    )
    .await?;
    tokio::fs::write(
        evidence.dir().join("flex-injection-health.md"),
        flex_injection_health_markdown(&cfg, &state).await,
    )
    .await?;
    tokio::fs::write(
        evidence.dir().join("applet-visibility-paths.md"),
        applet_visibility_paths_markdown(),
    )
    .await?;
    tokio::fs::write(
        evidence.dir().join("smartsdr-interlock-analysis.md"),
        smartsdr_interlock_analysis_markdown(&cfg, &state).await,
    )
    .await?;
    tokio::fs::write(
        evidence.dir().join("smartsdr-visibility-analysis.md"),
        smartsdr_visibility_analysis_markdown(&state).await,
    )
    .await?;
    evidence.write_status("status-end.json", &state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    println!(
        "full operational test complete; evidence bundle: {}",
        zip.display()
    );
    Ok(())
}

async fn run_pgxl_direct_trigger_matrix(
    mut cfg: BridgeConfig,
    config_path: PathBuf,
    duration_seconds: f64,
) -> Result<()> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        anyhow::bail!("--duration-seconds must be a finite value greater than 0");
    }
    cfg.flex_injection.trace_amplifier_advertisements = true;
    cfg.flex_injection.amplifier_status_profile = "aethersdr_pgxl_direct_lab".to_string();
    cfg.flex_injection.pgxl_force_operate_advertisement = true;
    cfg.validate()?;

    let evidence = EvidenceRun::start(
        "pgxl-direct-trigger-matrix",
        &config_path,
        &cfg,
        std::env::args(),
    )?;
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Lab).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs_f64(duration_seconds)).await;
    sampler.abort();
    tokio::fs::write(
        evidence.dir().join("pgxl-trigger-matrix.md"),
        pgxl_trigger_matrix_markdown(&cfg, &state).await,
    )
    .await?;
    tokio::fs::write(
        evidence.dir().join("radio-stripped-amplifier-fields.md"),
        radio_stripped_amplifier_fields_markdown(&state).await,
    )
    .await?;
    tokio::fs::write(
        evidence.dir().join("aethersdr-amp-parser-notes.md"),
        aethersdr_amp_parser_notes_markdown(),
    )
    .await?;
    evidence.write_status("status-end.json", &state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    println!(
        "PGXL direct trigger matrix complete; evidence bundle: {}",
        zip.display()
    );
    Ok(())
}

async fn test_startup_sequence(
    cfg: BridgeConfig,
    config_path: PathBuf,
    duration_seconds: f64,
) -> Result<()> {
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        anyhow::bail!("--duration-seconds must be a finite value greater than 0");
    }
    let evidence = EvidenceRun::start(
        "test-startup-sequence",
        &config_path,
        &cfg,
        std::env::args(),
    )?;
    append_evidence_line(
        "first-poll-sequence.log",
        "Starting bridge with KPA first-poll gate before Flex amplifier advertisement",
    );
    let state = start_bridge(&cfg, Some(&config_path), BridgeStartMode::Operational).await?;
    evidence.write_status("status-start.json", &state).await?;
    let sampler = evidence.start_status_sampler(state.clone());
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs_f64(duration_seconds)).await;
    sampler.abort();
    evidence.write_status("status-end.json", &state).await?;
    let zip = evidence.finish(&state, Some(started.elapsed())).await?;
    println!(
        "startup sequence test complete; evidence bundle: {}",
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

async fn pgxl_self_probe(host: &str, port: u16) -> Result<()> {
    let log_path = PathBuf::from("logs")
        .join("tests")
        .join(format!("{}-pgxl-self-probe.log", timestamp_compact()));
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let addr = format!("{host}:{port}");
    let mut log = format!("PGXL self-probe target={addr}\n");
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
    for (seq, command) in [(1_u32, "info"), (2, "status")] {
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
    append_evidence_line("pgxl-self-probe.log", log.clone());
    println!("PGXL self-probe passed: {addr}");
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
        let source_config = fs::read_to_string(config_path).unwrap_or_default();
        fs::write(dir.join("config-source.yaml"), &source_config)?;
        let identity = runtime_config_identity(cfg, Some(config_path))?;
        fs::write(
            dir.join("config-hashes.json"),
            serde_json::to_vec_pretty(&identity)?,
        )?;
        if identity.config_hash_match == Some(false) {
            fs::write(
                dir.join("warnings-errors.log"),
                "CONFIG_MISMATCH: loaded source config does not match effective runtime config\n",
            )?;
        }
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
            "listener-startup.log",
            "pgxl-protocol.log",
            "tgxl-protocol.log",
            "amplifier-status-lines.log",
            "amplifier-reannounce.log",
            "pgxl-direct-selftest.log",
            "pgxl-trigger-analysis.md",
            "pgxl-pairing-analysis.md",
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
            "kat500-frequency-follow.log",
            "advertised-state-history.jsonl",
            "state-mismatch-events.jsonl",
            "profile-summary.md",
            "last-known-good-comparison.md",
            "pgxl-status-mapping.md",
            "flex-state-mapping.md",
            "latest-kpa-telemetry.json",
            "latest-pgxl-advertised-status.json",
            "pgxl-regression-diff.md",
            "amplifier-advertisements.jsonl",
            "amplifier-operate-lab.md",
            "flex-amplifier-operate-sequence.log",
            "amplifier-state-rewrite-analysis.md",
            "pgxl-connect-assist.md",
            "real-vs-ui-amp-state.md",
            "aethersdr-operational-test.md",
            "full-aethersdr-functional-test.md",
            "operational-readiness.md",
            "operational-readiness-verdict.md",
            "effective-control-policy.md",
            "flex-injection-health.md",
            "control-execution-events.jsonl",
            "applet-visibility-paths.md",
            "smartsdr-interlock-analysis.md",
            "smartsdr-visibility-analysis.md",
            "pgxl-trigger-matrix.md",
            "radio-stripped-amplifier-fields.md",
            "aethersdr-amp-parser-notes.md",
            "pgxl-connect-attempt-timeline.md",
            "kpa-startup-diagnostics.md",
            "first-poll-sequence.log",
            "startup-advertisement-policy.md",
            "serial-port-open-errors.log",
            "connection-regression-test.md",
            "pgxl-delayed-connect-analysis.md",
        ] {
            let path = dir.join(file);
            if !path.exists() {
                File::create(path)?;
            }
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
                let advertised = serde_json::json!({
                    "timestamp_ms": system_time_ms(Some(SystemTime::now())),
                    "real_kpa500_state": status.pointer("/amp/state").cloned().unwrap_or(serde_json::Value::Null),
                    "advertised_flex_amp_state": status.pointer("/flex_injection/last_advertised_flex_amp_state").cloned().unwrap_or(serde_json::Value::Null),
                    "advertised_pgxl_state": status.pointer("/flex_injection/last_advertised_pgxl_state").cloned().unwrap_or(serde_json::Value::Null),
                    "advertised_tgxl_operate": status.pointer("/flex_injection/last_advertised_tgxl_operate").cloned().unwrap_or(serde_json::Value::Null),
                    "state_mismatch": status.pointer("/flex_injection/state_advertisement_mismatch").cloned().unwrap_or(serde_json::Value::Null),
                });
                if let Ok(line) = serde_json::to_string(&advertised) {
                    let path = dir.join("advertised-state-history.jsonl");
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
        let profile_summary = profile_summary_markdown(state).await;
        tokio::fs::write(self.dir.join("profile-summary.md"), profile_summary).await?;
        let comparison = last_known_good_comparison_markdown(state).await;
        tokio::fs::write(self.dir.join("last-known-good-comparison.md"), comparison).await?;
        self.write_pgxl_delayed_connect_analysis(state).await?;
        let pgxl_mapping = pgxl_status_mapping_markdown(state).await;
        tokio::fs::write(self.dir.join("pgxl-status-mapping.md"), pgxl_mapping).await?;
        let flex_mapping = flex_state_mapping_markdown(state).await;
        tokio::fs::write(self.dir.join("flex-state-mapping.md"), flex_mapping).await?;
        tokio::fs::write(
            self.dir.join("full-aethersdr-functional-test.md"),
            full_aethersdr_functional_test_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("operational-gap-test.md"),
            operational_gap_test_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("latest-kpa-telemetry.json"),
            serde_json::to_string_pretty(&latest_kpa_telemetry_json(state).await)?,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("latest-pgxl-advertised-status.json"),
            serde_json::to_string_pretty(&latest_pgxl_advertised_status_json(state).await)?,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("pgxl-regression-diff.md"),
            pgxl_regression_diff_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("pgxl-connect-attempt-timeline.md"),
            pgxl_connect_attempt_timeline_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("radio-stripped-amplifier-fields.md"),
            radio_stripped_amplifier_fields_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("amplifier-state-rewrite-analysis.md"),
            amplifier_state_rewrite_analysis_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("amplifier-operate-lab.md"),
            amplifier_operate_lab_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("pgxl-connect-assist.md"),
            pgxl_connect_assist_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("real-vs-ui-amp-state.md"),
            real_vs_ui_amp_state_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("aethersdr-operational-test.md"),
            aethersdr_operational_test_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("operational-readiness.md"),
            operational_readiness_from_state_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("aethersdr-amp-parser-notes.md"),
            aethersdr_amp_parser_notes_markdown(),
        )
        .await?;
        tokio::fs::write(
            self.dir.join("kpa-startup-diagnostics.md"),
            kpa_startup_diagnostics_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("startup-advertisement-policy.md"),
            startup_advertisement_policy_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("interlock-registration-audit.md"),
            interlock_registration_audit_markdown(state).await,
        )
        .await?;
        tokio::fs::write(
            self.dir.join("flex-registration-health.md"),
            flex_registration_health_markdown(state).await,
        )
        .await?;
        let create_analysis_path = self.dir.join("create-profile-analysis.md");
        if !create_analysis_path.exists() {
            tokio::fs::write(
                create_analysis_path,
                create_profile_analysis_from_state_markdown(state).await,
            )
            .await?;
        }
        zip_dir(&self.dir, &self.zip_path)?;
        println!("evidence bundle: {}", self.zip_path.display());
        Ok(self.zip_path.clone())
    }

    async fn write_pgxl_delayed_connect_analysis(&self, state: &SharedState) -> Result<()> {
        let guard = state.read().await;
        let tgxl_first = guard
            .clients
            .tgxl_sessions
            .iter()
            .map(|session| session.connected_at_ms)
            .min();
        let pgxl_first = guard
            .clients
            .pgxl_sessions
            .iter()
            .map(|session| session.connected_at_ms)
            .min();
        let delta = match (tgxl_first, pgxl_first) {
            (Some(tgxl), Some(pgxl)) if pgxl >= tgxl => Some(pgxl - tgxl),
            _ => None,
        };
        let listener_ready = guard.clients.pgxl_listener_ready_at_ms;
        let first_accept = guard.clients.pgxl_first_accept_at_ms.or(pgxl_first);
        let classification = if first_accept.is_none() {
            "A: AetherSDR did not attempt/complete TCP 9008 during this run."
        } else if let (Some(ready), Some(accept)) = (listener_ready, first_accept) {
            if accept > ready + 5_000 {
                "A: PGXL listener was ready, but AetherSDR did not open TCP until the delayed accept."
            } else {
                "B/C not indicated: EGB accepted PGXL TCP promptly; inspect protocol logs for handshake/close details."
            }
        } else {
            "inconclusive: listener or accept timestamp missing."
        };
        let body = format!(
            "# PGXL Delayed Connect Analysis\n\n\
            - Classification: **{}**\n\
            - Listener ready ms: `{}`\n\
            - Amplifier object seen ms: `{}`\n\
            - First AetherSDR PGXL TCP accept ms: `{}`\n\
            - PGXL no-socket warning ms: `{}`\n\
            - Reannounce count before first accept: `{}`\n\
            - `sub amplifier all` count before first accept: `{}`\n\
            - Last amplifier status before first accept: `{}`\n\
            - TGXL first connect timestamp: `{}`\n\
            - PGXL first connect timestamp: `{}`\n\
            - PGXL minus TGXL delta seconds: `{}`\n\
            - PGXL listener ready: `{}`\n\
            - TGXL listener ready: `{}`\n\
            - Amplifier create TX: `{}`\n\
            - First meter create TX: `{}`\n\
            - Interlock create TX: `{}`\n\
            - Keepalive enable TX: `{}`\n\
            - `sub amplifier all` TX: `{}`\n\
            - Amplifier create accepted: {}\n\
            - Amplifier handle observed: `{}`\n\
            - Registration continued without handle: {}\n\
            - Sub amplifier all accepted: {}\n\
            - PGXL TCP attempted after amplifier status: {}\n\n\
            Interpretation: if classification is A, the bridge listener was ready and no earlier accepted socket exists; the remaining delay is AetherSDR-side eligibility/retry timing unless packet capture proves SYNs were sent earlier. If classification is B/C, inspect `pgxl-protocol.log` for handshake failures or early closes.\n",
            classification,
            listener_ready
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            guard
                .flex_injection
                .amplifier_object_seen_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            first_accept
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            guard
                .clients
                .pgxl_last_no_socket_warning_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            guard
                .clients
                .pgxl_reannounce_count_at_first_accept
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            guard
                .clients
                .pgxl_sub_amp_all_count_at_first_accept
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            guard
                .clients
                .pgxl_last_amp_status_before_accept
                .as_deref()
                .unwrap_or("none"),
            tgxl_first
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            pgxl_first
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            delta
                .map(|value| format!("{:.3}", value as f64 / 1000.0))
                .unwrap_or_else(|| "n/a".to_string()),
            first_line_containing(&self.dir.join("listener-startup.log"), "PGXL listener started")
                .unwrap_or_else(|| "none".to_string()),
            first_line_containing(&self.dir.join("listener-startup.log"), "TGXL listener started")
                .unwrap_or_else(|| "none".to_string()),
            first_line_containing(&self.dir.join("flex-tx.log"), "amplifier create")
                .unwrap_or_else(|| "none".to_string()),
            first_line_containing(&self.dir.join("flex-tx.log"), "meter create")
                .unwrap_or_else(|| "none".to_string()),
            first_line_containing(&self.dir.join("flex-tx.log"), "interlock create")
                .unwrap_or_else(|| "none".to_string()),
            first_line_containing(&self.dir.join("flex-tx.log"), "keepalive enable")
                .unwrap_or_else(|| "none".to_string()),
            first_line_containing(&self.dir.join("flex-tx.log"), "sub amplifier all")
                .unwrap_or_else(|| "none".to_string()),
            guard.flex_injection.amplifier_create_accepted,
            guard
                .flex_injection
                .amplifier_handle
                .as_deref()
                .unwrap_or("none"),
            guard.flex_injection.registration_continued_without_handle,
            guard.flex_injection.sub_amplifier_all_accepted,
            guard.flex_injection.amplifier_pgxl_tcp_attempted_after_status,
        );
        drop(guard);
        tokio::fs::write(self.dir.join("pgxl-delayed-connect-analysis.md"), body).await?;
        Ok(())
    }
}

fn first_line_containing(path: &Path, needle: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .find(|line| line.contains(needle))
        .map(str::to_string)
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
    if guard.flex_injection.amplifier_removed_count > 0 {
        result = "FAIL";
        warnings.push("Flex removed the amplifier object during the run.");
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
        "- Last amplifier status line: `{}`\n",
        guard
            .flex_injection
            .last_amplifier_status_line
            .as_deref()
            .unwrap_or("none")
    ));
    body.push_str(&format!(
        "- Pairing candidate fields: {:?}\n- PGXL TCP attempted after status: {}\n",
        guard.flex_injection.amplifier_pairing_candidate_fields,
        guard
            .flex_injection
            .amplifier_pgxl_tcp_attempted_after_status
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

async fn profile_summary_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# Active Profiles\n\n\
        - Amplifier status profile: `{}`\n\
        - TGXL control profile: `{}`\n\
        - Real KPA500 state: `{}`\n\
        - Advertised Flex amp state: `{}`\n\
        - Advertised PGXL state: `{}`\n\
        - Advertised TGXL operate: `{}`\n\
        - State mismatch: `{}`\n",
        guard
            .flex_injection
            .active_amplifier_status_profile
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .active_tgxl_control_profile
            .as_deref()
            .unwrap_or("unknown"),
        guard.amp.state.pgxl_state(),
        guard
            .flex_injection
            .last_advertised_flex_amp_state
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .last_advertised_pgxl_state
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .last_advertised_tgxl_operate
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        guard
            .flex_injection
            .state_advertisement_mismatch
            .as_deref()
            .unwrap_or("none"),
    )
}

async fn last_known_good_comparison_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let profile = guard
        .flex_injection
        .active_amplifier_status_profile
        .as_deref()
        .unwrap_or("unknown");
    let recommendation = match profile {
        "official_pgxl" | "strict_real_pgxl" => {
            "Use `config.aethersdr-compatible-operational.yaml` for AetherSDR live testing; strict profiles are retained for protocol audits."
        }
        "aethersdr_minimal" | "aethersdr_operational" => {
            "This run used the recommended AetherSDR minimal compatibility profile."
        }
        _ => {
            "If PGXL does not open TCP 9008, repeat with `config.aethersdr-compatible-operational.yaml` before changing code."
        }
    };
    format!(
        "# Last Known Good Comparison\n\n\
        The known-good AetherSDR path uses Flex injection plus a direct-connect amplifier profile and TGXL `control_ready` status.\n\n\
        - Current amplifier profile: `{profile}`\n\
        - Current TGXL profile: `{}`\n\
        - PGXL sessions started: {}\n\
        - TGXL sessions started: {}\n\
        - Flex amp state follows live KPA500: {}\n\
        - Recommendation: {}\n",
        guard
            .flex_injection
            .active_tgxl_control_profile
            .as_deref()
            .unwrap_or("unknown"),
        guard.clients.pgxl_session_started_count,
        guard.clients.tgxl_session_started_count,
        guard.flex_injection.state_advertisement_mismatch.is_none(),
        recommendation,
    )
}

async fn pgxl_regression_diff_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let live_state = advertised_amp_state_for_status(&guard.amp);
    format!(
        "# PGXL Regression Diff\n\n\
        ## Working-era trigger candidate\n\n\
        Phase 27-era `aethersdr_force_direct` and `pgxl_verbose` create commands carried direct-connect status fields on the amplifier create line, including `state`, `connected`, `configured`, and `enabled`.\n\n\
        ## Broken-era candidate\n\n\
        Phase 29 removed the create-time `state` field to avoid hard-coded standby. AetherSDR can see the AMP applet from Flex status but may not attempt TCP 9008 when create-time pairing fields are incomplete.\n\n\
        ## Current correction\n\n\
        `aethersdr_minimal` restores only the suspected trigger field, `state=<live>`, for AetherSDR. It keeps `pgxl_connect_assist` disabled and does not append fields Flex strips (`connected`, `configured`, `enabled`, `direct`, `lan`).\n\n\
        - Active profile: `{}`\n\
        - Live KPA state: `{live_state}`\n\
        - Last Flex advertised state: `{}`\n\
        - Last PGXL advertised state: `{}`\n\
        - PGXL sessions started: {}\n\
        - PGXL direct attempted after status: {}\n\
        - Last amplifier line: `{}`\n",
        guard
            .flex_injection
            .active_amplifier_status_profile
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .last_advertised_flex_amp_state
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .last_advertised_pgxl_state
            .as_deref()
            .unwrap_or("unknown"),
        guard.clients.pgxl_session_started_count,
        guard
            .flex_injection
            .amplifier_pgxl_tcp_attempted_after_status,
        guard
            .flex_injection
            .last_amplifier_status_line
            .as_deref()
            .unwrap_or("none"),
    )
}

async fn pgxl_connect_attempt_timeline_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# PGXL Connect Attempt Timeline\n\n\
        - PGXL sessions started: {}\n\
        - Active PGXL clients: {}\n\
        - Manual no-socket warnings: {}\n\
        - Last no-socket warning: `{}`\n\
        - Amplifier reannounce count: {}\n\
        - Last reannounce reason: `{}`\n\
        - Amplifier direct expected: {:?}\n",
        guard.clients.pgxl_session_started_count,
        guard.clients.pgxl_client_count,
        guard.clients.pgxl_manual_connect_no_socket_attempt_count,
        guard
            .clients
            .pgxl_last_no_socket_attempt_warning
            .as_deref()
            .unwrap_or("none"),
        guard.flex_injection.amplifier_reannounce_count,
        guard
            .flex_injection
            .last_amplifier_reannounce_reason
            .as_deref()
            .unwrap_or("none"),
        guard.flex_injection.amplifier_direct_connect_expected,
    )
}

async fn kpa_startup_diagnostics_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# KPA500 Startup Diagnostics\n\n\
        - Connection state: `{}`\n\
        - Connected: {}\n\
        - First poll completed: {}\n\
        - First poll error: `{}`\n\
        - Serial port open error: `{}`\n\
        - Last successful command: `{}`\n\
        - Last raw response: `{}`\n\
        - Poll successes: {}\n\
        - Poll failures: {}\n\
        - Reconnects: {}\n\n\
        Probable causes when disconnected: wrong COM port, KPA500 USB cable disconnected, KPA500 powered off, or another process such as Elecraft KPA500 Remote has the COM port locked.\n",
        guard.amp.connection_state.as_str(),
        guard.amp.is_connected(),
        guard.amp.first_poll_completed,
        guard.amp.first_poll_error.as_deref().unwrap_or("none"),
        guard.amp.serial_port_open_error.as_deref().unwrap_or("none"),
        guard.amp.last_successful_command.as_deref().unwrap_or("none"),
        guard.amp.last_raw_response.as_deref().unwrap_or("none"),
        guard.amp.runtime.poll_success_count,
        guard.amp.runtime.poll_failure_count,
        guard.amp.runtime.reconnect_count,
    )
}

async fn startup_advertisement_policy_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# Startup Advertisement Policy\n\n\
        - Policy: `{}`\n\
        - Waiting for first poll: {}\n\
        - First KPA poll completed: {}\n\
        - Last advertised Flex state: `{}`\n\
        - Last advertised PGXL state: `{}`\n\
        - Last amplifier status line: `{}`\n\n\
        With `wait_for_first_kpa_poll`, EGB delays the direct-connect amplifier create until KPA500 has returned `^OS;`, `^TM;`, `^VI;`, and `^FL;`, or until the configured timeout emits `kpa500_not_polling`.\n",
        guard
            .amp
            .startup_state_policy
            .as_deref()
            .unwrap_or("unknown"),
        guard.amp.advertisement_waiting_for_first_poll,
        guard.amp.first_poll_completed,
        guard
            .flex_injection
            .last_advertised_flex_amp_state
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .last_advertised_pgxl_state
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .last_amplifier_status_line
            .as_deref()
            .unwrap_or("none"),
    )
}

fn flex_settings_for_markdown(cfg: &BridgeConfig) -> FlexInjectionSettings {
    let advertised_ip = cfg
        .flex_injection
        .force_advertised_pgxl_ip
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&cfg.flex_injection.amplifier_ip)
        .parse()
        .unwrap_or_else(|_| IpAddr::from([127, 0, 0, 1]));
    let radio_ip = cfg
        .flex_injection
        .radio_ip
        .parse()
        .unwrap_or_else(|_| IpAddr::from([127, 0, 0, 1]));
    FlexInjectionSettings {
        radio_addr: SocketAddr::new(radio_ip, cfg.flex_injection.radio_port),
        amplifier_ip: advertised_ip,
        amplifier_port: cfg.flex_injection.amplifier_port,
        amplifier_model: cfg.flex_injection.amplifier_model.clone(),
        serial: cfg.flex_injection.serial.clone(),
        handle_label: cfg.flex_injection.handle.clone(),
        ant_map: cfg.flex_injection.ant_map.clone(),
        amplifier_status_profile: cfg.flex_injection.amplifier_status_profile.clone(),
        trace_amplifier_advertisements: cfg.flex_injection.trace_amplifier_advertisements,
        pgxl_force_operate_advertisement: cfg.flex_injection.pgxl_force_operate_advertisement,
        flex_force_operate_via_radio: cfg.flex_injection.flex_force_operate_via_radio,
        pgxl_connect_assist: cfg.flex_injection.pgxl_connect_assist,
        amplifier_startup_state_policy: cfg.flex_injection.amplifier_startup_state_policy.clone(),
        wait_first_kpa_poll_timeout: Duration::from_millis(
            cfg.flex_injection.wait_first_kpa_poll_timeout_ms,
        ),
        full_pgxl_registration: cfg.flex_injection.full_pgxl_registration,
        create_meters: cfg.flex_injection.create_meters,
        create_interlock: cfg.flex_injection.create_interlock,
        disable_amp_interlock: cfg.flex_injection.disable_amp_interlock,
        allow_rf_risk: effective_kpa_allow_rf_risk(cfg),
        reconnect_initial: Duration::from_millis(cfg.flex_injection.reconnect_initial_ms),
        reconnect_max: Duration::from_millis(cfg.flex_injection.reconnect_max_ms),
        ping_interval: Duration::from_millis(cfg.flex_injection.ping_interval_ms),
        tuner_presence_refresh: cfg.tgxl.experimental_presence_refresh,
        tuner_refresh_interval: Duration::from_millis(cfg.flex_injection.tuner_refresh_interval_ms),
        amplifier_reannounce_interval: Duration::from_millis(
            cfg.flex_injection.amplifier_reannounce_interval_ms,
        ),
        pgxl_startup_trigger_strategy: cfg.flex_injection.pgxl_startup_trigger_strategy.clone(),
        aethersdr_open_trigger_variant: cfg.flex_injection.aethersdr_open_trigger_variant.clone(),
    }
}

async fn pgxl_trigger_matrix_markdown(cfg: &BridgeConfig, state: &SharedState) -> String {
    let settings = flex_settings_for_markdown(cfg);
    let guard = state.read().await;
    let mut body = String::from("# PGXL Direct Trigger Matrix\n\n");
    body.push_str("This run uses lab-only `aethersdr_pgxl_direct_lab` plus `pgxl_force_operate_advertisement=true`. That advertises `state=OPERATE` without sending any KPA500 command.\n\n");
    body.push_str(&format!(
        "- Runtime profile: `{}`\n- Advertised IP: `{}`\n- PGXL sessions started: {}\n- PGXL direct attempted after status: {}\n- No-socket warnings: {}\n- Last amplifier status line: `{}`\n- Last emitted advertisement: `{}`\n\n",
        cfg.flex_injection.amplifier_status_profile,
        settings.amplifier_ip,
        guard.clients.pgxl_session_started_count,
        guard.flex_injection.amplifier_pgxl_tcp_attempted_after_status,
        guard.clients.pgxl_manual_connect_no_socket_attempt_count,
        guard.flex_injection.last_amplifier_status_line.as_deref().unwrap_or("none"),
        guard.flex_injection.last_emitted_amplifier_advertisement_line.as_deref().unwrap_or("none"),
    ));
    drop(guard);
    body.push_str(
        "| Variant | State | Candidate status line | Notes |\n| --- | --- | --- | --- |\n",
    );
    for variant in flex_injection::pgxl_direct_trigger_variants(&settings) {
        body.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} |\n",
            variant.name, variant.state, variant.line, variant.notes
        ));
    }
    body.push_str("\nIf this run still shows zero PGXL sessions, compare `radio-stripped-amplifier-fields.md` to see whether the Flex radio forwarded the trigger fields to AetherSDR.\n");
    body
}

async fn radio_stripped_amplifier_fields_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let emitted = guard
        .flex_injection
        .last_emitted_amplifier_advertisement_line
        .as_deref()
        .unwrap_or("none");
    let observed = guard
        .flex_injection
        .last_amplifier_status_line
        .as_deref()
        .unwrap_or("none");
    let emitted_fields = kv_field_names(emitted);
    let observed_fields = kv_field_names(observed);
    let stripped = emitted_fields
        .iter()
        .filter(|field| !observed_fields.contains(*field))
        .cloned()
        .collect::<Vec<_>>();
    format!(
        "# Radio-Stripped Amplifier Fields\n\n\
        This compares the last amplifier advertisement EGB emitted/logged with the last amplifier status line observed back from the Flex API.\n\n\
        - Last emitted advertisement kind: `{}`\n\
        - Last emitted advertisement: `{emitted}`\n\
        - Last observed radio-side status: `{observed}`\n\
        - Emitted fields: {:?}\n\
        - Observed fields: {:?}\n\
        - Fields present in emitted line but missing from observed line: {:?}\n\n\
        Interpretation: if `port`, `connected`, `configured`, `enabled`, `direct`, or `lan` are missing from the observed line, the Flex radio/client path may be stripping those trigger hints before AetherSDR sees them.\n",
        guard
            .flex_injection
            .last_emitted_amplifier_advertisement_kind
            .as_deref()
            .unwrap_or("unknown"),
        emitted_fields,
        observed_fields,
        stripped,
    )
}

async fn band_follow_summary_markdown(cfg: &BridgeConfig, state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# Band Follow Test Summary\n\n\
        - Flex frequency Hz: `{:?}`\n\
        - Flex band: `{}`\n\
        - KAT follow enabled: `{}`\n\
        - KAT F sent count: `{}`\n\
        - KAT F stale response count: `{}`\n\
        - KAT F retry count: `{}`\n\
        - Last KAT F wire: `{}`\n\
        - Last KAT requested kHz: `{:?}`\n\
        - Last KAT confirmed kHz: `{:?}`\n\
        - Last KAT confirmation match: `{:?}`\n\
        - KPA band-follow enabled: `{}`\n\
        - KPA BN sent count: `{}`\n\
        - KPA BN stale response count: `{}`\n\
        - KPA BN retry count: `{}`\n\
        - Last KPA BN wire: `{}`\n\
        - Last KPA requested BN: `{:?}`\n\
        - Last KPA confirmed BN: `{:?}`\n\
        - Last KPA confirmation match: `{:?}`\n\
        - Last KPA result: `{}`\n\n\
        KPA500 supports verified band-follow through `^BNnn;`, not direct frequency follow. KAT500 remains the device that receives the Flex transmit frequency with `F <kHz>;`.\n",
        guard.radio_context.frequency_hz,
        guard.radio_context.band.as_str(),
        cfg.kat500.follow_flex_frequency,
        guard.radio_context.kat500_follow_sent_count,
        guard.radio_context.kat500_follow_stale_response_count,
        guard.radio_context.kat500_follow_retry_count,
        guard
            .radio_context
            .last_kat500_follow_wire
            .as_deref()
            .unwrap_or("none"),
        guard.radio_context.last_kat500_follow_requested_khz,
        guard.radio_context.last_kat500_follow_confirmed_khz,
        guard.radio_context.last_kat500_follow_confirmation_match,
        cfg.kpa500.follow_flex_band,
        guard.radio_context.kpa500_follow_sent_count,
        guard.radio_context.kpa500_follow_stale_response_count,
        guard.radio_context.kpa500_follow_retry_count,
        guard
            .radio_context
            .last_kpa500_follow_wire
            .as_deref()
            .unwrap_or("none"),
        guard.radio_context.last_kpa500_follow_requested_bn,
        guard.radio_context.last_kpa500_follow_confirmed_bn,
        guard.radio_context.last_kpa500_follow_confirmation_match,
        guard
            .radio_context
            .last_kpa500_follow_result
            .as_deref()
            .unwrap_or("none"),
    )
}

async fn amplifier_operate_lab_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# Amplifier Operate Lab\n\n\
        This lab sends `amplifier set <handle> operate=1` to the Flex API only. It does not send `^OS1;` to the KPA500.\n\n\
        - Flex operate lab enabled: {}\n\
        - Lab command count: {}\n\
        - Lab command accepted count: {}\n\
        - Flex desired amp state: `{}`\n\
        - Last observed radio-side amp state rewrite: `{}`\n\
        - Last emitted amplifier advertisement: `{}`\n\
        - Last observed amplifier status: `{}`\n\
        - PGXL sessions started: {}\n\
        - PGXL direct attempted after status: {}\n\n\
        If `Lab command accepted count` increases but the observed status remains `state=STANDBY`, the Flex radio owns or rewrites amplifier operate state separately from create/status metadata.\n",
        guard.flex_injection.flex_force_operate_via_radio,
        guard.flex_injection.flex_operate_lab_command_count,
        guard.flex_injection.flex_operate_lab_accept_count,
        guard
            .flex_injection
            .flex_desired_amp_state
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .radio_rewritten_amp_state
            .as_deref()
            .unwrap_or("none"),
        guard
            .flex_injection
            .last_emitted_amplifier_advertisement_line
            .as_deref()
            .unwrap_or("none"),
        guard
            .flex_injection
            .last_amplifier_status_line
            .as_deref()
            .unwrap_or("none"),
        guard.clients.pgxl_session_started_count,
        guard
            .flex_injection
            .amplifier_pgxl_tcp_attempted_after_status,
    )
}

async fn amplifier_state_rewrite_analysis_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let emitted = guard
        .flex_injection
        .last_emitted_amplifier_advertisement_line
        .as_deref()
        .unwrap_or("none");
    let observed = guard
        .flex_injection
        .last_amplifier_status_line
        .as_deref()
        .unwrap_or("none");
    let emitted_state = field_value(emitted, "state").unwrap_or("unknown");
    let observed_state = field_value(observed, "state").unwrap_or("unknown");
    let conclusion = if guard.flex_injection.flex_operate_lab_accept_count > 0
        && observed_state == "OPERATE"
    {
        "Flex accepted `amplifier set operate=1` and later advertised OPERATE."
    } else if guard.flex_injection.flex_operate_lab_accept_count > 0 && observed_state == "STANDBY"
    {
        "Flex accepted the lab command but still advertised STANDBY, so the radio is rewriting or refusing effective operate state."
    } else {
        "The lab has not yet proven whether `amplifier set operate=1` changes the observed radio-side amplifier state."
    };
    format!(
        "# Amplifier State Rewrite Analysis\n\n\
        - EGB emitted state: `{emitted_state}`\n\
        - Flex observed state: `{observed_state}`\n\
        - Lab command count: {}\n\
        - Lab accepted count: {}\n\
        - Radio rewritten amp state: `{}`\n\
        - PGXL sessions started: {}\n\n\
        Conclusion: {conclusion}\n",
        guard.flex_injection.flex_operate_lab_command_count,
        guard.flex_injection.flex_operate_lab_accept_count,
        guard
            .flex_injection
            .radio_rewritten_amp_state
            .as_deref()
            .unwrap_or("none"),
        guard.clients.pgxl_session_started_count,
    )
}

async fn pgxl_connect_assist_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# PGXL Connect Assist\n\n\
        Connect-assist sends one Flex-side `amplifier set <handle> operate=1` after the injected amplifier handle is observed. It does not send `^OS1;` to the KPA500.\n\n\
        - Enabled: {}\n\
        - Sent count: {}\n\
        - Last result: `{}`\n\
        - Triggered PGXL TCP: {}\n\
        - PGXL sessions started: {}\n\
        - PGXL clients active: {}\n\
        - Real KPA state: `{}`\n\
        - Last PGXL advertised state: `{}`\n",
        guard.flex_injection.pgxl_connect_assist_enabled,
        guard.flex_injection.pgxl_connect_assist_sent_count,
        guard
            .flex_injection
            .pgxl_connect_assist_last_result
            .as_deref()
            .unwrap_or("none"),
        guard.flex_injection.pgxl_connect_assist_triggered_tcp,
        guard.clients.pgxl_session_started_count,
        guard.clients.pgxl_client_count,
        guard.amp.state.pgxl_state(),
        guard
            .flex_injection
            .last_advertised_pgxl_state
            .as_deref()
            .unwrap_or("unknown"),
    )
}

async fn real_vs_ui_amp_state_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# Real vs UI Amplifier State\n\n\
        - Real KPA RF state: `{}` from serial polling\n\
        - Flex UI desired/connect-assist state: `{}`\n\
        - Flex observed/rebroadcast state: `{}`\n\
        - PGXL direct advertised state: `{}`\n\
        - RF-risk gate enabled: not represented here; KPA500 `^OS1;` remains blocked unless the normal KPA safety settings allow it.\n\n\
        Interpretation: Flex-side connect-assist may briefly mark the virtual amplifier active for AetherSDR pairing, but PGXL direct telemetry remains authoritative to the real KPA500 state.\n",
        guard.amp.state.pgxl_state(),
        guard
            .flex_injection
            .flex_desired_amp_state
            .as_deref()
            .unwrap_or("none"),
        guard
            .flex_injection
            .last_advertised_flex_amp_state
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .last_advertised_pgxl_state
            .as_deref()
            .unwrap_or("unknown"),
    )
}

async fn aethersdr_operational_test_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# AetherSDR Operational Test\n\n\
        - PGXL connected: {}\n\
        - TGXL connected: {}\n\
        - Real KPA state: `{}`\n\
        - UI connect-assist used: {}\n\
        - Connect-assist last result: `{}`\n\
        - Controls seen: {}\n\
        - Last blocked reason: `{}`\n\
        - Last TGXL control: `{}`\n\
        - Last PGXL/Flex control: `{}`\n",
        guard.clients.pgxl_session_started_count > 0,
        guard.clients.tgxl_session_started_count > 0,
        guard.amp.state.pgxl_state(),
        guard.flex_injection.pgxl_connect_assist_sent_count > 0,
        guard
            .flex_injection
            .pgxl_connect_assist_last_result
            .as_deref()
            .unwrap_or("none"),
        guard.controls.control_requested_count > 0,
        guard
            .controls
            .last_safety_decision
            .as_deref()
            .unwrap_or("none"),
        guard
            .controls
            .last_tgxl_control_command
            .as_deref()
            .unwrap_or("none"),
        guard
            .controls
            .last_flex_amp_set_command
            .as_deref()
            .unwrap_or("none"),
    )
}

async fn full_aethersdr_functional_test_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let tgxl_first = guard
        .clients
        .tgxl_sessions
        .iter()
        .map(|session| session.connected_at_ms)
        .min();
    let pgxl_first = guard
        .clients
        .pgxl_sessions
        .iter()
        .map(|session| session.connected_at_ms)
        .min();
    let delay = match (tgxl_first, pgxl_first) {
        (Some(tgxl), Some(pgxl)) if pgxl >= tgxl => {
            format!("{:.3}s", (pgxl - tgxl) as f64 / 1000.0)
        }
        _ => "n/a".to_string(),
    };
    format!(
        "# Full AetherSDR Functional Test\n\n\
        - TGXL first connect timestamp: `{}`\n\
        - PGXL first connect timestamp: `{}`\n\
        - PGXL connect delay after TGXL: `{delay}`\n\
        - KPA health: `{}` polls={} failures={}\n\
        - KAT health: `{}` polls={} failures={}\n\
        - Current Flex TX frequency: `{:?}` Hz\n\
        - Current Flex band: `{}`\n\
        - KAT500 frequency-follow sent count: {}\n\
        - KAT500 frequency-follow skipped count: {}\n\
        - Last KAT500 frequency-follow wire: `{}`\n\
        - Last KAT500 frequency requested kHz: `{:?}`\n\
        - Last KAT500 frequency confirmed kHz: `{:?}`\n\
        - KAT500 frequency confirmation match: `{}`\n\
        - KAT500 stale frequency response count: {}\n\
        - KAT500 frequency retry count: {}\n\
        - KPA500 band-follow sent count: {}\n\
        - KPA500 band-follow skipped count: {}\n\
        - Last KPA500 band-follow wire: `{}`\n\
        - Last KPA500 band-follow result: `{}`\n\
        - Tune commands executed: {}\n\
        - Meter publish count: {}\n\
        - SmartSDR PGXL data status: `{}`\n\
        - SmartSDR TGXL/tuner status: `{}`\n\
        - Interlock mode: `{}`\n\
        - Last tune frequency: `{:?}` Hz\n\
        - Last tune band: `{}`\n\
        - AetherSDR AMP command seen: `{}`\n\
        - Last interlock state: `{}`\n\
        - Last interlock reason: `{}`\n\
        - Last interlock tx_allowed: `{}`\n\
        - Interlock blocked count: {}\n\
        - Amplifier removed count: {}\n\
        - Amplifier handle change count: {}\n\
        - Readiness verdict: `{}`\n",
        tgxl_first
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        pgxl_first
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        guard.amp.connection_state.as_str(),
        guard.amp.runtime.poll_success_count,
        guard.amp.runtime.poll_failure_count,
        guard.tuner.connection_state.as_str(),
        guard.tuner.runtime.poll_success_count,
        guard.tuner.runtime.poll_failure_count,
        guard.radio_context.frequency_hz,
        guard.radio_context.band.as_str(),
        guard.radio_context.kat500_follow_sent_count,
        guard.radio_context.kat500_follow_skipped_count,
        guard
            .radio_context
            .last_kat500_follow_wire
            .as_deref()
            .unwrap_or("none"),
        guard.radio_context.last_kat500_follow_requested_khz,
        guard.radio_context.last_kat500_follow_confirmed_khz,
        guard
            .radio_context
            .last_kat500_follow_confirmation_match
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        guard.radio_context.kat500_follow_stale_response_count,
        guard.radio_context.kat500_follow_retry_count,
        guard.radio_context.kpa500_follow_sent_count,
        guard.radio_context.kpa500_follow_skipped_count,
        guard
            .radio_context
            .last_kpa500_follow_wire
            .as_deref()
            .unwrap_or("none"),
        guard
            .radio_context
            .last_kpa500_follow_result
            .as_deref()
            .unwrap_or("none"),
        guard.controls.tune_executed_count,
        guard.flex_injection.meter_publish_count,
        guard
            .flex_injection
            .meter_publish_last_result
            .as_deref()
            .unwrap_or("not implemented"),
        "SmartSDR TGXL/tuner registration remains unsupported without a verified Flex tuner object API",
        if guard.flex_injection.interlock_disabled_for_test {
            "INTERLOCK_DISABLED_FOR_TEST"
        } else {
            "normal"
        },
        guard.controls.last_tune_frequency_hz,
        guard.controls.last_tune_band.as_deref().unwrap_or("none"),
        guard.controls.last_flex_amp_set_command.is_some()
            || guard.controls.last_pgxl_control_command.is_some(),
        guard
            .flex_injection
            .last_interlock_state
            .as_deref()
            .unwrap_or("none"),
        guard
            .flex_injection
            .last_interlock_reason
            .as_deref()
            .unwrap_or("none"),
        guard
            .flex_injection
            .last_interlock_tx_allowed
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        guard.flex_injection.interlock_blocked_count,
        guard.flex_injection.amplifier_removed_count,
        guard.flex_injection.amplifier_handle_change_count,
        if guard.clients.pgxl_session_started_count > 0
            && guard.clients.tgxl_session_started_count > 0
            && guard.flex_injection.amplifier_removed_count == 0
            && guard.amp.runtime.poll_failure_count == 0
            && guard.tuner.runtime.poll_failure_count == 0
        {
            "PASS_OR_LIVE_VERIFY_CONTROLS"
        } else {
            "WARN"
        },
    )
}

async fn operational_gap_test_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let tgxl_first = guard
        .clients
        .tgxl_sessions
        .iter()
        .map(|session| session.connected_at_ms)
        .min();
    let pgxl_first = guard
        .clients
        .pgxl_sessions
        .iter()
        .map(|session| session.connected_at_ms)
        .min();
    let delay = match (tgxl_first, pgxl_first) {
        (Some(tgxl), Some(pgxl)) if pgxl >= tgxl => {
            format!("{:.3}s", (pgxl - tgxl) as f64 / 1000.0)
        }
        _ => "n/a".to_string(),
    };
    let kat_match = guard
        .radio_context
        .last_kat500_follow_confirmation_match
        .unwrap_or(false);
    let tuning_stuck = guard.tuner.tuning
        && guard
            .lifecycle
            .tune
            .entered_at_ms
            .and_then(|entered| timestamp_millis().checked_sub(entered))
            .is_some_and(|age| age > 30_000);
    format!(
        "# Operational Gap Test\n\n\
        - PGXL connection delay: `{delay}`\n\
        - PGXL direct session: {}\n\
        - TGXL direct session: {}\n\
        - AMP command received: {}\n\
        - KAT500 F exact confirmation match: {}\n\
        - KAT500 F requested kHz: `{:?}`\n\
        - KAT500 F confirmed kHz: `{:?}`\n\
        - KAT500 stale F responses: {}\n\
        - KAT500 F retries: {}\n\
        - KAT500 tuning stuck: {}\n\
        - KPA500 band-follow enabled by config/effective runtime: {}\n\
        - KPA500 band-follow sent count: {}\n\
        - KPA500 band-follow skipped count: {}\n\
        - Last KPA500 band-follow result: `{}`\n\
        - SmartSDR PGXL meter status: `{}`\n\
        - SmartSDR TGXL/tuner status: `unsupported without verified Flex tuner/accessory registration API`\n\
        - Amplifier removed count: {}\n\
        - Interlock mode: `{}`\n\
        - Last interlock tx_allowed: `{}`\n",
        guard.clients.pgxl_session_started_count > 0,
        guard.clients.tgxl_session_started_count > 0,
        guard.controls.last_flex_amp_set_command.is_some()
            || guard.controls.last_pgxl_control_command.is_some(),
        kat_match,
        guard.radio_context.last_kat500_follow_requested_khz,
        guard.radio_context.last_kat500_follow_confirmed_khz,
        guard.radio_context.kat500_follow_stale_response_count,
        guard.radio_context.kat500_follow_retry_count,
        tuning_stuck,
        guard.radio_context.kpa500_follow_sent_count > 0
            || guard.radio_context.kpa500_follow_skipped_count > 0,
        guard.radio_context.kpa500_follow_sent_count,
        guard.radio_context.kpa500_follow_skipped_count,
        guard
            .radio_context
            .last_kpa500_follow_result
            .as_deref()
            .unwrap_or("none"),
        guard
            .flex_injection
            .meter_publish_last_result
            .as_deref()
            .unwrap_or("not implemented"),
        guard.flex_injection.amplifier_removed_count,
        if guard.flex_injection.interlock_disabled_for_test {
            "INTERLOCK_DISABLED_FOR_TEST"
        } else {
            "normal"
        },
        guard
            .flex_injection
            .last_interlock_tx_allowed
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
    )
}

async fn operational_readiness_markdown(cfg: &BridgeConfig, state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# Operational Readiness\n\n\
        - Real controls enabled: {}\n\
        - Confirmation accepted: {}\n\
        - KAT tune enabled: {}\n\
        - KAT bypass enabled: {}\n\
        - KAT antenna enabled: {}\n\
        - KPA standby enabled: {}\n\
        - KPA operate enabled: {}\n\
        - Effective KPA dry_run: {}\n\
        - Effective KPA allow_control: {}\n\
        - Effective KPA allow_rf_risk: {}\n\
        - Effective KAT dry_run: {}\n\
        - Effective KAT allow_control: {}\n\
        - Effective KAT allow_rf_risk: {}\n\
        - PGXL connected: {}\n\
        - TGXL connected: {}\n\
        - Control path ready: {}\n\
        - Final KPA state: `{}`\n\
        - Final KAT antenna: {:?}, SWR {:.2}\n\
        - Last control safety decision: `{}`\n",
        cfg.operational.enable_real_controls,
        cfg.operational.controls_confirmed(),
        cfg.operational.enable_kat_tune,
        cfg.operational.enable_kat_bypass,
        cfg.operational.enable_kat_antenna,
        cfg.operational.enable_kpa_standby,
        cfg.operational.enable_kpa_operate,
        effective_kpa_dry_run(cfg),
        effective_kpa_allow_control(cfg),
        effective_kpa_allow_rf_risk(cfg),
        effective_kat_dry_run(cfg),
        effective_kat_allow_control(cfg),
        effective_kat_allow_rf_risk(cfg),
        guard.clients.pgxl_session_started_count > 0,
        guard.clients.tgxl_session_started_count > 0,
        control_path_ready(&guard),
        guard.amp.state.pgxl_state(),
        guard.tuner.selected_antenna,
        guard.tuner.swr,
        guard
            .controls
            .last_safety_decision
            .as_deref()
            .unwrap_or("none"),
    )
}

async fn operational_readiness_from_state_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# Operational Readiness\n\n\
        - PGXL connected: {}\n\
        - TGXL connected: {}\n\
        - KPA state: `{}`\n\
        - KPA polls: success={} failure={}\n\
        - KAT polls: success={} failure={}\n\
        - Controls seen: {}\n\
        - Control path ready: {}\n\
        - Last safety decision: `{}`\n",
        guard.clients.pgxl_session_started_count > 0,
        guard.clients.tgxl_session_started_count > 0,
        guard.amp.state.pgxl_state(),
        guard.amp.runtime.poll_success_count,
        guard.amp.runtime.poll_failure_count,
        guard.tuner.runtime.poll_success_count,
        guard.tuner.runtime.poll_failure_count,
        guard.controls.control_requested_count,
        control_path_ready(&guard),
        guard
            .controls
            .last_safety_decision
            .as_deref()
            .unwrap_or("none"),
    )
}

fn control_path_ready(guard: &bridge_core::state::BridgeState) -> bool {
    let direct_clients_ready = guard.clients.pgxl_session_started_count > 0
        && guard.clients.tgxl_session_started_count > 0;
    let hardware_ready = guard.amp.is_connected() && guard.tuner.is_connected();
    let effective_controls_ready = guard.effective_controls.effective_kat_tune_enabled
        || guard.effective_controls.effective_kpa_standby_enabled
        || guard.effective_controls.effective_kpa_operate_enabled;
    let flex_ready = guard.flex_injection.amplifier_create_accepted
        && guard.flex_injection.sub_amplifier_all_accepted;
    direct_clients_ready && hardware_ready && effective_controls_ready && flex_ready
}

async fn effective_control_policy_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let policy = &guard.effective_controls;
    format!(
        "# Effective Control Policy\n\n\
        - Operational override active: {}\n\
        - Operational confirmation valid: {}\n\
        - Raw KPA dry_run: {}\n\
        - Raw KPA allow_control: {}\n\
        - Raw KPA allow_rf_risk: {}\n\
        - Raw KAT dry_run: {}\n\
        - Raw KAT allow_control: {}\n\
        - Raw KAT allow_rf_risk: {}\n\
        - Effective KAT tune: {} ({})\n\
        - Effective KAT bypass: {} ({})\n\
        - Effective KAT antenna: {} ({})\n\
        - Effective KPA standby: {} ({})\n\
        - Effective KPA operate: {} ({})\n\
        - Effective clear fault: {} ({})\n",
        policy.operational_override_active,
        policy.operational_confirmation_valid,
        policy.raw_kpa_dry_run,
        policy.raw_kpa_allow_control,
        policy.raw_kpa_allow_rf_risk,
        policy.raw_kat_dry_run,
        policy.raw_kat_allow_control,
        policy.raw_kat_allow_rf_risk,
        policy.effective_kat_tune_enabled,
        policy.kat_tune_reason,
        policy.effective_kat_bypass_enabled,
        policy.kat_bypass_reason,
        policy.effective_kat_antenna_enabled,
        policy.kat_antenna_reason,
        policy.effective_kpa_standby_enabled,
        policy.kpa_standby_reason,
        policy.effective_kpa_operate_enabled,
        policy.kpa_operate_reason,
        policy.effective_clear_fault_enabled,
        policy.clear_fault_reason,
    )
}

async fn flex_injection_health_markdown(cfg: &BridgeConfig, state: &SharedState) -> String {
    let guard = state.read().await;
    let flex = &guard.flex_injection;
    format!(
        "# Flex Injection Health\n\n\
        - Enabled in config: {}\n\
        - Attempted radio endpoint: `{}`\n\
        - Connection state: `{}`\n\
        - Degraded reason: `{}`\n\
        - Last error: `{}`\n\
        - TCP connect success count: {}\n\
        - H/client handle received: {}\n\
        - Client handle: `{}`\n\
        - Amplifier create sent: {}\n\
        - Amplifier create accepted: {}\n\
        - Amplifier handle: `{}`\n\
        - Post-amplifier registration sent: {}\n\
        - Continued without amplifier handle: {}\n\
        - Meter handles: {:?}\n\
        - Meter publish supported: `{}`\n\
        - Meter publish count: {}\n\
        - Meter publish last result: `{}`\n\
        - Interlock handle: `{}`\n\
        - Keepalive enable accepted: {}\n\
        - Last interlock status: `{}`\n\
        - Last interlock state: `{}`\n\
        - Last interlock reason: `{}`\n\
        - Last interlock tx_allowed: `{}`\n\
        - Empty interlock amplifier field: {}\n\
        - Empty interlock amplifier count: {}\n\
        - Interlock blocked count: {}\n\
        - `sub amplifier all` accepted: {}\n\
        - Last Flex TX: `{}`\n\
        - Last Flex RX: `{}`\n\n\
        Operational note: if Flex injection is degraded or no amplifier handle is present, the AMP applet may not appear even while PGXL/TGXL direct sockets are healthy.\n",
        cfg.flex_injection.enabled,
        flex.radio_addr.as_deref().unwrap_or("unknown"),
        flex.connection_state.as_str(),
        flex.degraded_reason.as_deref().unwrap_or("none"),
        flex.last_error.as_deref().unwrap_or("none"),
        flex.tcp_connect_success_count,
        flex.client_handle_received,
        flex.client_handle.as_deref().unwrap_or("none"),
        flex.amplifier_create_sent,
        flex.amplifier_create_accepted,
        flex.amplifier_handle.as_deref().unwrap_or("none"),
        flex.post_amplifier_registration_sent,
        flex.registration_continued_without_handle,
        flex.meter_handles,
        flex.meter_publish_supported
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        flex.meter_publish_count,
        flex.meter_publish_last_result.as_deref().unwrap_or("none"),
        flex.interlock_handle.as_deref().unwrap_or("none"),
        flex.keepalive_enable_accepted,
        flex.last_interlock_status_line.as_deref().unwrap_or("none"),
        flex.last_interlock_state.as_deref().unwrap_or("none"),
        flex.last_interlock_reason.as_deref().unwrap_or("none"),
        flex.last_interlock_tx_allowed
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        flex.interlock_amplifier_field_empty,
        flex.interlock_empty_amplifier_count,
        flex.interlock_blocked_count,
        flex.sub_amplifier_all_accepted,
        flex.last_tx_line.as_deref().unwrap_or("none"),
        flex.last_rx_line.as_deref().unwrap_or("none"),
    )
}

async fn connection_regression_report_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# Connection Regression Test\n\n\
        Baseline target: Phase 50 known-good AetherSDR connection path (`f36d718`, `config.aethersdr-known-good.yaml`, `aethersdr_force_direct`).\n\n\
        | Check | Result |\n\
        | --- | --- |\n\
        | Flex API connected | {} |\n\
        | Amplifier create sent | {} |\n\
        | Amplifier create accepted | {} |\n\
        | Amplifier removed count | {} |\n\
        | Amplifier handle | `{}` |\n\
        | PGXL session started | {} |\n\
        | TGXL session started | {} |\n\
        | PGXL direct commands seen | {} |\n\
        | TGXL direct commands seen | {} |\n\
        | Active amplifier profile | `{}` |\n\
        | Last amplifier status line | `{}` |\n\
        | Last Flex TX | `{}` |\n\
        | Last Flex RX | `{}` |\n\n\
        PASS requires Flex connected, amplifier create accepted, `amplifier_removed_count=0`, `PGXL session started=true`, and `TGXL session started=true`. If PGXL/TGXL remain false, inspect `flex-rx.log`, `flex-tx.log`, `listener-startup.log`, `pgxl-protocol.log`, and `tgxl-protocol.log` in this bundle.\n",
        guard.flex_injection.connection_state == ConnectionState::Connected,
        guard.flex_injection.amplifier_create_sent,
        guard.flex_injection.amplifier_create_accepted,
        guard.flex_injection.amplifier_removed_count,
        guard
            .flex_injection
            .amplifier_handle
            .as_deref()
            .unwrap_or("none"),
        guard.clients.pgxl_session_started_count > 0,
        guard.clients.tgxl_session_started_count > 0,
        guard.protocol.pgxl.commands_received > 0,
        guard.protocol.tgxl.commands_received > 0,
        guard
            .flex_injection
            .active_amplifier_status_profile
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .last_amplifier_status_line
            .as_deref()
            .unwrap_or("none"),
        guard
            .flex_injection
            .last_tx_line
            .as_deref()
            .unwrap_or("none"),
        guard
            .flex_injection
            .last_rx_line
            .as_deref()
            .unwrap_or("none"),
    )
}

async fn interlock_registration_audit_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let flex = &guard.flex_injection;
    format!(
        "# Interlock Registration Audit\n\n\
        ## Current Command\n\n\
        EGB creates the AMP interlock after the Flex radio accepts `amplifier create`. If a radio-side amplifier handle/status is not observed within the fallback window, EGB continues with meter/interlock/keepalive/subscription registration using the documented serial/name association.\n\n\
        ```text\n\
        interlock create type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=EGB-KPA500\n\
        ```\n\n\
        ## Official API Check\n\n\
        The PGXL Amplifier-to-Radio API documents `interlock create` parameters `type`, `valid_antennas`, `name`, and `serial`; it does not document an explicit amplifier handle parameter. The same document's example uses `type=AMP valid_antennas=ANT1,ANT2 name=PG-XL serial=<pgxl-serial>`.\n\n\
        ## Latest Runtime Evidence\n\n\
        - Amplifier handle: `{}`\n\
        - Interlock handle: `{}`\n\
        - Interlock disabled for test: {}\n\
        - Registration continued without handle: {}\n\
        - Last interlock status: `{}`\n\
        - Last interlock state: `{}`\n\
        - Last interlock reason: `{}`\n\
        - Last interlock tx_allowed: `{}`\n\
        - Empty `amplifier=` observed: {}\n\
        - Empty `amplifier=` count: {}\n\
        - Interlock blocked count: {}\n\
        - Last amplifier status: `{}`\n\
        - Last Flex TX: `{}`\n\
        - Last Flex RX: `{}`\n\n\
        If `reason=AMP:PG-XL` still arrives with `amplifier=` empty after this two-stage registration change, the remaining suspect is not command ordering but a hidden radio-side association requirement such as serial format or antenna/source topology.\n",
        flex.amplifier_handle.as_deref().unwrap_or("none"),
        flex.interlock_handle.as_deref().unwrap_or("none"),
        flex.interlock_disabled_for_test,
        flex.registration_continued_without_handle,
        flex.last_interlock_status_line.as_deref().unwrap_or("none"),
        flex.last_interlock_state.as_deref().unwrap_or("none"),
        flex.last_interlock_reason.as_deref().unwrap_or("none"),
        flex.last_interlock_tx_allowed
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        flex.interlock_amplifier_field_empty,
        flex.interlock_empty_amplifier_count,
        flex.interlock_blocked_count,
        flex.last_amplifier_status_line.as_deref().unwrap_or("none"),
        flex.last_tx_line.as_deref().unwrap_or("none"),
        flex.last_rx_line.as_deref().unwrap_or("none"),
    )
}

async fn flex_registration_health_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let flex = &guard.flex_injection;
    format!(
        "# Flex Registration Health\n\n\
        - Amplifier create sent: {}\n\
        - Amplifier create accepted: {}\n\
        - Amplifier handle: `{}`\n\
        - Meter handles: {:?}\n\
        - Meter publish supported: `{}`\n\
        - Meter publish count: {}\n\
        - Meter publish last result: `{}`\n\
        - Interlock handle: `{}`\n\
        - Interlock disabled for test: {}\n\
        - Post-amplifier registration sent: {}\n\
        - Continued without amplifier handle: {}\n\
        - Keepalive enable accepted: {}\n\
        - Interlock amplifier field empty: {}\n\
        - Amplifier removed count: {}\n\
        - Duplicate amplifier create count: {}\n\
        - Sub amplifier all accepted: {}\n\
        - Connection state: `{}`\n\
        - Degraded reason: `{}`\n\n\
        Registration is healthy if the amplifier handle is stable, or if Flex accepted `amplifier create` and post-registration completed through the serial/name fallback. Meter/interlock handles should be present when enabled, `amplifier_removed_count` must remain zero, and interlock `tx_allowed=0` with `reason=AMP:PG-XL` is a blocker. Empty `amplifier=` with `tx_allowed=1` is tracked as a warning only.\n",
        flex.amplifier_create_sent,
        flex.amplifier_create_accepted,
        flex.amplifier_handle.as_deref().unwrap_or("none"),
        flex.meter_handles,
        flex.meter_publish_supported
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        flex.meter_publish_count,
        flex.meter_publish_last_result.as_deref().unwrap_or("none"),
        flex.interlock_handle.as_deref().unwrap_or("none"),
        flex.interlock_disabled_for_test,
        flex.post_amplifier_registration_sent,
        flex.registration_continued_without_handle,
        flex.keepalive_enable_accepted,
        flex.interlock_amplifier_field_empty,
        flex.amplifier_removed_count,
        flex.duplicate_amplifier_create_count,
        flex.sub_amplifier_all_accepted,
        flex.connection_state.as_str(),
        flex.degraded_reason.as_deref().unwrap_or("none"),
    )
}

async fn operational_readiness_verdict_markdown(cfg: &BridgeConfig, state: &SharedState) -> String {
    let guard = state.read().await;
    let mut failures: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    if guard.amp.runtime.poll_success_count == 0 {
        failures.push("KPA500 polling did not succeed".to_string());
    }
    if guard.tuner.runtime.poll_success_count == 0 {
        failures.push("KAT500 polling did not succeed".to_string());
    }
    if guard.clients.pgxl_session_started_count == 0 {
        warnings.push("No PGXL direct client connected during the test".to_string());
    }
    if guard.clients.tgxl_session_started_count == 0 {
        warnings.push("No TGXL direct client connected during the test".to_string());
    }
    if cfg.flex_injection.enabled {
        if let Err(err) = validate_operational_start_config(cfg, BridgeStartMode::Operational) {
            failures.push(err.to_string());
        }
        if guard.flex_injection.connection_state != ConnectionState::Connected {
            failures.push("Flex injection is not connected".to_string());
        }
        if guard.flex_injection.amplifier_handle.is_none()
            && !guard.flex_injection.sub_amplifier_all_accepted
        {
            failures.push("Flex amplifier handle was not created".to_string());
        } else if guard.flex_injection.amplifier_handle.is_none()
            && guard.flex_injection.sub_amplifier_all_accepted
        {
            warnings.push(
                "Flex did not broadcast an amplifier handle, but registration completed via create-accepted fallback".to_string(),
            );
        }
        if guard.flex_injection.amplifier_removed_count > 0 {
            failures.push("Flex removed the amplifier object".to_string());
        }
        if guard.flex_injection.duplicate_amplifier_create_count > 0 {
            failures.push("Duplicate amplifier create attempts occurred".to_string());
        }
        if guard.flex_injection.interlock_blocked_count > 0
            || guard.flex_injection.last_interlock_tx_allowed == Some(false)
        {
            failures.push(
                "INTERLOCK_BLOCKED: Flex interlock reports AMP:PG-XL with tx_allowed=0".to_string(),
            );
        } else if guard.flex_injection.interlock_amplifier_field_empty {
            warnings.push(
                "Flex interlock reports AMP:PG-XL with an empty amplifier field while TX is allowed"
                    .to_string(),
            );
        }
    }
    if let Some(head) = git_head_commit() {
        let embedded = runtime_git_commit();
        if embedded != "unknown" && embedded != head {
            failures.push(
                "RUNTIME_COMMIT_MISMATCH: running binary commit differs from repository HEAD"
                    .to_string(),
            );
        }
    }
    if guard.config_identity.config_hash_match == Some(false) {
        failures
            .push("CONFIG_MISMATCH: source config and effective runtime config differ".to_string());
    }
    if guard.effective_controls.operational_override_active
        && guard
            .controls
            .last_safety_decision
            .as_deref()
            .is_some_and(|reason| {
                reason.contains("dry_run") && !reason.contains("intentionally blocked")
            })
    {
        warnings.push(
            "A control was still blocked by dry_run despite operational override".to_string(),
        );
    }
    let verdict = if failures.is_empty() {
        if warnings.is_empty() {
            "PASS"
        } else {
            "WARN"
        }
    } else {
        "FAIL"
    };
    format!(
        "# Operational Readiness Verdict\n\n\
        - Verdict: **{}**\n\
        - Failures: {}\n\
        - Warnings: {}\n",
        verdict,
        if failures.is_empty() {
            "none".to_string()
        } else {
            failures.join("; ")
        },
        if warnings.is_empty() {
            "none".to_string()
        } else {
            warnings.join("; ")
        },
    )
}

fn applet_visibility_paths_markdown() -> &'static str {
    "# Applet Visibility Paths\n\n\
    - AetherSDR TGXL/TUN can work through the direct TGXL TCP path on port 9010.\n\
    - AetherSDR and SmartSDR AMP visibility depends on Flex amplifier injection, not PGXL TCP alone.\n\
    - SmartSDR tuner visibility requires Flex-side tuner/accessory registration. Direct TGXL TCP alone is not enough for SmartSDR.\n\
    - If Flex injection is degraded or no amplifier handle is present, direct sockets may still connect while the AMP applet is absent.\n"
}

async fn smartsdr_interlock_analysis_markdown(cfg: &BridgeConfig, state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# SmartSDR Interlock Analysis\n\n\
        - AMP interlock creation enabled: {}\n\
        - Interlock handle: {:?}\n\
        - Antenna map: `{}`\n\
        - Valid antenna mapping used by EGB interlock: `ANT1,ANT2`\n\
        - Last Flex response: `{}`\n\
        - Last Flex command: `{}`\n\n\
        Current blocker: SmartSDR interlock behaviour requires a live SmartSDR/Flex transcript. EGB now captures `flex-rx.log`, `flex-tx.log`, and control events in `full-operational-test` evidence so the rejected tune/interlock line can be tied to an exact Flex response.\n",
        cfg.flex_injection.create_interlock,
        guard.flex_injection.interlock_handle,
        cfg.flex_injection.ant_map,
        guard
            .flex_injection
            .last_response
            .as_deref()
            .unwrap_or("none"),
        guard
            .flex_injection
            .last_command
            .as_deref()
            .unwrap_or("none"),
    )
}

async fn smartsdr_visibility_analysis_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# SmartSDR Visibility Analysis\n\n\
        - PGXL amplifier handle: {:?}\n\
        - TGXL/tuner handle: {:?}\n\
        - Flex tuner appeared count: {}\n\
        - Flex tuner disappeared count: {}\n\
        - TGXL direct sessions: {}\n\
        - PGXL direct sessions: {}\n\n\
        AetherSDR direct TGXL is supported through TCP 9010. SmartSDR TGXL visibility depends on the Flex-side tuner/accessory status path; if `tuner_handle` remains absent while TGXL direct sessions work, SmartSDR TGXL injection remains the active blocker.\n",
        guard.flex_injection.amplifier_handle,
        guard.flex_injection.tuner_handle,
        guard.flex_injection.tuner_appeared_count,
        guard.flex_injection.tuner_disappeared_count,
        guard.clients.tgxl_session_started_count,
        guard.clients.pgxl_session_started_count,
    )
}

fn field_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
}

fn kv_field_names(line: &str) -> Vec<String> {
    line.split_whitespace()
        .filter_map(|token| token.split_once('=').map(|(key, _)| key.to_string()))
        .collect()
}

fn aethersdr_amp_parser_notes_markdown() -> String {
    "# AetherSDR Amp Parser Notes\n\n\
    Source inspected: `research/AetherSDR/src/models/RadioModel.cpp` and `research/AetherSDR/src/gui/MainWindow.cpp`.\n\n\
    - `RadioModel.cpp` parses `amplifier <handle> ...` status/event lines.\n\
    - `model=TunerGeniusXL` is routed to the tuner model.\n\
    - Any non-empty `model` that is not `TunerGeniusXL` is treated as a power amplifier.\n\
    - On first power amplifier status, AetherSDR stores `m_ampIp = kvs.value(\"ip\")`, stores the model, and emits `amplifierChanged(true)`.\n\
    - `MainWindow.cpp` handles `amplifierChanged(true)` by calling `m_pgxlConn.connectToPgxl(m_radioModel.ampIp())` if the IP is non-empty and PGXL direct is not already connected.\n\
    - The direct auto-connect path does not inspect `port`; the manual peripherals UI defaults PGXL to port 9008.\n\
    - The parser updates operate state from `state=IDLE|OPERATE|TRANSMIT*`; `STANDBY` is present but not an operate state.\n\n\
    Current implication: if AetherSDR receives an amplifier line with non-empty `model` and `ip`, it should attempt PGXL TCP unless the installed binary differs, the status line is not delivered through the subscription path, or an internal UI/peripheral setting suppresses direct connect.\n"
        .to_string()
}

async fn pgxl_profile_comparison_markdown(cfg: &BridgeConfig, state: &SharedState) -> String {
    let guard = state.read().await;
    let live_state = advertised_amp_state_for_status(&guard.amp);
    let advertised_ip = cfg
        .flex_injection
        .force_advertised_pgxl_ip
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&cfg.flex_injection.amplifier_ip)
        .parse()
        .unwrap_or_else(|_| IpAddr::from([127, 0, 0, 1]));
    let mut body = String::from("# PGXL Profile Comparison\n\n");
    body.push_str(&format!(
        "Observed during this run: PGXL sessions started={} active_clients={} direct_attempted_after_status={}\n\n",
        guard.clients.pgxl_session_started_count,
        guard.clients.pgxl_client_count,
        guard
            .flex_injection
            .amplifier_pgxl_tcp_attempted_after_status
    ));
    body.push_str("| Profile | Create command with live state |\n| --- | --- |\n");
    for profile in [
        "official_pgxl",
        "aethersdr_minimal",
        "aethersdr_operational",
        "strict_real_pgxl",
        "aethersdr_force_direct",
        "old_good_pgxl",
        "pgxl_verbose",
        "pgxl_paired",
    ] {
        let command = flex_injection::amplifier_create_command_with_state(
            advertised_ip,
            cfg.flex_injection.amplifier_port,
            &cfg.flex_injection.amplifier_model,
            &cfg.flex_injection.serial,
            &cfg.flex_injection.ant_map,
            profile,
            Some(live_state),
        );
        body.push_str(&format!("| `{profile}` | `{command}` |\n"));
    }
    body.push_str("\n`aethersdr_minimal` is the recommended AetherSDR client profile. It adds only `state=<live>` to the strict create line. `official_pgxl` is intentionally strict for Flex/SmartSDR registration audits and may not trigger AetherSDR direct TCP.\n");
    body
}

async fn aethersdr_profile_comparison_markdown(cfg: &BridgeConfig, state: &SharedState) -> String {
    let guard = state.read().await;
    let live_state = advertised_amp_state_for_status(&guard.amp);
    let advertised_ip = cfg
        .flex_injection
        .force_advertised_pgxl_ip
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&cfg.flex_injection.amplifier_ip)
        .parse()
        .unwrap_or_else(|_| IpAddr::from([127, 0, 0, 1]));
    let official = flex_injection::amplifier_create_command_with_state(
        advertised_ip,
        cfg.flex_injection.amplifier_port,
        &cfg.flex_injection.amplifier_model,
        &cfg.flex_injection.serial,
        &cfg.flex_injection.ant_map,
        "official_pgxl",
        Some(live_state),
    );
    let compatible = flex_injection::amplifier_create_command_with_state(
        advertised_ip,
        cfg.flex_injection.amplifier_port,
        &cfg.flex_injection.amplifier_model,
        &cfg.flex_injection.serial,
        &cfg.flex_injection.ant_map,
        "aethersdr_minimal",
        Some(live_state),
    );
    let observed = &guard.flex_injection;
    let mut body = String::from("# AetherSDR Profile Comparison\n\n");
    body.push_str("This command runs the bridge with `aethersdr_minimal` and records live outcomes. `official_pgxl` is shown as the strict baseline create command for side-by-side comparison.\n\n");
    body.push_str("| Profile | Create command |\n| --- | --- |\n");
    body.push_str(&format!("| `official_pgxl` | `{official}` |\n"));
    body.push_str(&format!("| `aethersdr_minimal` | `{compatible}` |\n\n"));
    body.push_str(&format!(
        "- Active run profile: `{}`\n- Flex accepted amplifier handle: {}\n- Amplifier removed count: {}\n- Amplifier create count: {}\n- Amplifier handle changes: {}\n- PGXL TCP sessions started: {}\n- TGXL TCP sessions started: {}\n- PGXL TCP attempted after amplifier status: {}\n- Last amplifier removed reason: {}\n",
        observed
            .active_amplifier_status_profile
            .as_deref()
            .unwrap_or("unknown"),
        observed.amplifier_handle.as_deref().unwrap_or("none"),
        observed.amplifier_removed_count,
        observed.amplifier_create_count,
        observed.amplifier_handle_change_count,
        guard.clients.pgxl_session_started_count,
        guard.clients.tgxl_session_started_count,
        observed.amplifier_pgxl_tcp_attempted_after_status,
        observed
            .amplifier_recreate_reason
            .as_deref()
            .unwrap_or("none")
    ));
    body
}

async fn create_profile_analysis_markdown(cfg: &BridgeConfig, state: &SharedState) -> String {
    let guard = state.read().await;
    let live_state = advertised_amp_state_for_status(&guard.amp);
    let advertised_ip = cfg
        .flex_injection
        .force_advertised_pgxl_ip
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&cfg.flex_injection.amplifier_ip)
        .parse()
        .unwrap_or_else(|_| IpAddr::from([127, 0, 0, 1]));
    let command_for = |profile: &str| {
        flex_injection::amplifier_create_command_with_state(
            advertised_ip,
            cfg.flex_injection.amplifier_port,
            &cfg.flex_injection.amplifier_model,
            &cfg.flex_injection.serial,
            &cfg.flex_injection.ant_map,
            profile,
            Some(live_state),
        )
    };
    let official = command_for("official_pgxl");
    let minimal = command_for("aethersdr_minimal");
    let old_operational = format!("{minimal} connected=1 configured=1 enabled=1 direct=1 lan=1");
    let observed = &guard.flex_injection;
    format!(
        "# Create Profile Analysis\n\n\
        ## Create Lines\n\n\
        | Profile | Create line |\n\
        | --- | --- |\n\
        | `official_pgxl` | `{official}` |\n\
        | `aethersdr_minimal` | `{minimal}` |\n\
        | `old aethersdr_operational` | `{old_operational}` |\n\n\
        ## Live Outcome\n\n\
        - Active run profile: `{}`\n\
        - Flex accepted amplifier create: {}\n\
        - Amplifier handle: `{}`\n\
        - Amplifier removed count: {}\n\
        - PGXL sessions started: {}\n\
        - TGXL sessions started: {}\n\
        - Active PGXL clients: {}\n\
        - Active TGXL clients: {}\n\
        - PGXL direct attempted after amplifier status: {}\n\
        - Last amplifier status line from Flex: `{}`\n\
        - Last emitted create/status line: `{}`\n\n\
        ## Stripped Fields From Phase 52 Evidence\n\n\
        Flex echoed back `ip`, `model`, `serial_num`, `ant`, and `state`, while stripping `connected`, `configured`, `enabled`, `direct`, and `lan`. `aethersdr_minimal` therefore keeps only `state=<live-state>` as the AetherSDR compatibility field.\n",
        observed
            .active_amplifier_status_profile
            .as_deref()
            .unwrap_or("unknown"),
        observed.amplifier_create_accepted,
        observed.amplifier_handle.as_deref().unwrap_or("none"),
        observed.amplifier_removed_count,
        guard.clients.pgxl_session_started_count,
        guard.clients.tgxl_session_started_count,
        guard.clients.pgxl_client_count,
        guard.clients.tgxl_client_count,
        observed.amplifier_pgxl_tcp_attempted_after_status,
        observed
            .last_amplifier_status_line
            .as_deref()
            .unwrap_or("none"),
        observed
            .last_emitted_amplifier_advertisement_line
            .as_deref()
            .unwrap_or("none"),
    )
}

async fn create_profile_analysis_from_state_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let observed = &guard.flex_injection;
    format!(
        "# Create Profile Analysis\n\n\
        - Active profile: `{}`\n\
        - Last emitted create/status line: `{}`\n\
        - Last Flex amplifier status line: `{}`\n\
        - Flex accepted amplifier create: {}\n\
        - Amplifier removed count: {}\n\
        - PGXL sessions started: {}\n\
        - TGXL sessions started: {}\n\
        - Active PGXL clients: {}\n\
        - Active TGXL clients: {}\n\
        - PGXL direct attempted after amplifier status: {}\n\n\
        Latest live evidence showed Flex strips `connected`, `configured`, `enabled`, `direct`, and `lan` from the create/status path while preserving `state`. The normal AetherSDR profile is therefore `aethersdr_minimal`, which adds only `state=<live-kpa-state>` to the strict create line.\n",
        observed
            .active_amplifier_status_profile
            .as_deref()
            .unwrap_or("unknown"),
        observed
            .last_emitted_amplifier_advertisement_line
            .as_deref()
            .unwrap_or("none"),
        observed
            .last_amplifier_status_line
            .as_deref()
            .unwrap_or("none"),
        observed.amplifier_create_accepted,
        observed.amplifier_removed_count,
        guard.clients.pgxl_session_started_count,
        guard.clients.tgxl_session_started_count,
        guard.clients.pgxl_client_count,
        guard.clients.tgxl_client_count,
        observed.amplifier_pgxl_tcp_attempted_after_status,
    )
}

async fn pgxl_status_mapping_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    let pgxl_state = advertised_amp_state_for_status(&guard.amp);
    format!(
        "# PGXL Status Mapping\n\n\
        - KPA500 operate flag: {}\n\
        - KPA500 state: `{}`\n\
        - KPA500 fault: `{}`\n\
        - PGXL advertised state: `{pgxl_state}`\n\
        - Forward power watts: {:.1}\n\
        - SWR ratio: {:.2}\n\
        - Temperature C: {:.1}\n\
        - PA voltage V: {:.1}\n\
        - PA current A: {:.1}\n\
        - PGXL VAC: `{}` (0 means unknown/not mapped from KPA500 supply voltage)\n\
        - MEFFA: `{}`\n",
        guard.amp.operate,
        guard.amp.state.pgxl_state(),
        guard.amp.fault.as_deref().unwrap_or("none"),
        guard.amp.forward_power_watts,
        guard.amp.swr,
        guard.amp.temperature_c,
        guard.amp.pa_voltage_volts,
        guard.amp.pa_current_amps,
        pgxl_vac_value_for_status(&guard.amp),
        pgxl_meffa_for_status(&guard.amp),
    )
}

async fn flex_state_mapping_markdown(state: &SharedState) -> String {
    let guard = state.read().await;
    format!(
        "# Flex State Mapping\n\n\
        - Live KPA500 state: `{}`\n\
        - Last advertised Flex amp state: `{}`\n\
        - Last advertised PGXL state: `{}`\n\
        - Active amplifier profile: `{}`\n\
        - Last amplifier line: `{}`\n\
        - State mismatch: `{}`\n",
        advertised_amp_state_for_status(&guard.amp),
        guard
            .flex_injection
            .last_advertised_flex_amp_state
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .last_advertised_pgxl_state
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .active_amplifier_status_profile
            .as_deref()
            .unwrap_or("unknown"),
        guard
            .flex_injection
            .last_amplifier_status_line
            .as_deref()
            .unwrap_or("none"),
        guard
            .flex_injection
            .state_advertisement_mismatch
            .as_deref()
            .unwrap_or("none"),
    )
}

async fn latest_kpa_telemetry_json(state: &SharedState) -> serde_json::Value {
    let guard = state.read().await;
    serde_json::json!({
        "connection_state": guard.amp.connection_state.as_str(),
        "operate": guard.amp.operate,
        "state": guard.amp.state.pgxl_state(),
        "fault": guard.amp.fault,
        "meffa": guard.amp.meffa,
        "forward_power_watts": guard.amp.forward_power_watts,
        "swr": guard.amp.swr,
        "temperature_c": guard.amp.temperature_c,
        "pa_voltage_volts": guard.amp.pa_voltage_volts,
        "pa_current_amps": guard.amp.pa_current_amps,
        "firmware_version": guard.amp.firmware_version,
        "serial_number": guard.amp.serial_number,
        "last_successful_poll_ms": system_time_ms(guard.amp.last_successful_poll_at),
        "first_poll_completed": guard.amp.first_poll_completed,
        "first_poll_error": guard.amp.first_poll_error,
        "serial_port_open_error": guard.amp.serial_port_open_error,
        "last_raw_response": guard.amp.last_raw_response,
        "last_successful_command": guard.amp.last_successful_command,
        "startup_state_policy": guard.amp.startup_state_policy,
        "advertisement_waiting_for_first_poll": guard.amp.advertisement_waiting_for_first_poll,
    })
}

async fn latest_pgxl_advertised_status_json(state: &SharedState) -> serde_json::Value {
    let guard = state.read().await;
    serde_json::json!({
        "state": advertised_amp_state_for_status(&guard.amp),
        "peakfwd_dbm": watts_to_dbm_for_status(guard.amp.forward_power_watts),
        "return_loss_db": return_loss_for_status(guard.amp.swr),
        "temp": guard.amp.temperature_c,
        "id": guard.amp.pa_current_amps,
        "vac": pgxl_vac_value_for_status(&guard.amp),
        "meffa": pgxl_meffa_for_status(&guard.amp),
        "fault": guard.amp.fault,
        "last_advertised_pgxl_state": guard.flex_injection.last_advertised_pgxl_state,
        "last_advertised_flex_amp_state": guard.flex_injection.last_advertised_flex_amp_state,
    })
}

fn advertised_amp_state_for_status(amp: &bridge_core::state::AmpState) -> &'static str {
    if !amp.first_poll_completed
        && amp.startup_state_policy.as_deref() == Some("wait_for_first_kpa_poll")
    {
        return "UNKNOWN";
    }
    if amp.fault.is_some() || amp.state == AmpOperatingState::Fault {
        "FAULT"
    } else {
        amp.state.pgxl_state()
    }
}

fn pgxl_meffa_for_status(amp: &bridge_core::state::AmpState) -> &str {
    if amp.fault.is_none() && (amp.meffa == "UNKNOWN" || amp.meffa.trim().is_empty()) {
        "OK"
    } else {
        amp.meffa.as_str()
    }
}

fn pgxl_vac_value_for_status(amp: &bridge_core::state::AmpState) -> u16 {
    if amp.pa_voltage_volts >= 100.0 {
        amp.pa_voltage_volts.round().clamp(0.0, f32::from(u16::MAX)) as u16
    } else {
        0
    }
}

fn watts_to_dbm_for_status(watts: f32) -> f32 {
    if watts <= 0.0 {
        -120.0
    } else {
        10.0 * (watts * 1000.0).log10()
    }
}

fn return_loss_for_status(swr: f32) -> f32 {
    if swr.is_finite() && swr > 1.0 {
        let rho = ((swr - 1.0) / (swr + 1.0)).clamp(0.001, 0.999);
        20.0 * rho.log10()
    } else {
        -30.0
    }
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

async fn state_advertisement_watchdog(state: SharedState) {
    let mut pending: Option<(String, Instant)> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let mismatch = {
            let guard = state.read().await;
            state_advertisement_mismatch_reason(&guard)
        };
        match (mismatch, pending.as_mut()) {
            (Some(reason), Some((pending_reason, since))) if *pending_reason == reason => {
                if since.elapsed() >= Duration::from_secs(2) {
                    let mut guard = state.write().await;
                    if guard.flex_injection.state_advertisement_mismatch.as_deref()
                        == Some(reason.as_str())
                    {
                        continue;
                    }
                    guard.flex_injection.state_advertisement_mismatch = Some(reason.clone());
                    guard.flex_injection.state_advertisement_mismatch_count = guard
                        .flex_injection
                        .state_advertisement_mismatch_count
                        .saturating_add(1);
                    append_evidence_line("warnings-errors.log", reason.clone());
                    append_evidence_json(
                        "state-mismatch-events.jsonl",
                        &serde_json::json!({
                            "event": "state_advertisement_mismatch",
                            "reason": reason,
                            "timestamp_ms": system_time_ms(Some(SystemTime::now())),
                        }),
                    );
                    warn!(
                        event_id = "state_advertisement_mismatch",
                        "Advertised protocol state does not match live serial/shared state"
                    );
                }
            }
            (Some(reason), _) => {
                pending = Some((reason, Instant::now()));
            }
            (None, _) => {
                pending = None;
                let mut guard = state.write().await;
                if guard.flex_injection.state_advertisement_mismatch.is_some() {
                    guard.flex_injection.state_advertisement_mismatch = None;
                }
            }
        }
    }
}

fn state_advertisement_mismatch_reason(guard: &bridge_core::state::BridgeState) -> Option<String> {
    let real_amp = advertised_amp_state_for_status(&guard.amp);
    let fault = guard.amp.fault.as_deref().unwrap_or("none");
    if let Some(advertised) = guard
        .flex_injection
        .last_advertised_flex_amp_state
        .as_deref()
    {
        if advertised != real_amp {
            return Some(format!(
                "Flex amp advertised state mismatch: live_kpa_state={real_amp} pgxl_advertised_state={} flex_advertised_state={advertised} kpa_fault={fault} profile={} reason=flex_status_not_following_live_kpa",
                guard
                    .flex_injection
                    .last_advertised_pgxl_state
                    .as_deref()
                    .unwrap_or("unknown"),
                guard
                    .flex_injection
                    .active_amplifier_status_profile
                    .as_deref()
                    .unwrap_or("unknown")
            ));
        }
    }
    if let Some(advertised) = guard.flex_injection.last_advertised_pgxl_state.as_deref() {
        if advertised != real_amp {
            return Some(format!(
                "PGXL advertised state mismatch: live_kpa_state={real_amp} pgxl_advertised_state={advertised} flex_advertised_state={} kpa_fault={fault} profile={} reason=pgxl_status_not_following_live_kpa",
                guard
                    .flex_injection
                    .last_advertised_flex_amp_state
                    .as_deref()
                    .unwrap_or("unknown"),
                guard
                    .flex_injection
                    .active_amplifier_status_profile
                    .as_deref()
                    .unwrap_or("unknown")
            ));
        }
    }
    None
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
            "build_timestamp": option_env!("BUILD_TIMESTAMP").unwrap_or("unknown"),
            "process_id": std::process::id(),
            "executable_path": std::env::current_exe().ok().map(|path| path.display().to_string()),
            "working_dir": std::env::current_dir().ok().map(|path| path.display().to_string()),
            "uptime_ms": SystemTime::now().duration_since(started).unwrap_or_default().as_millis(),
            "config_path": guard.config_identity.config_path,
            "config_loaded_at": guard.config_identity.config_loaded_at_ms,
            "config_hash": guard.config_identity.config_hash,
            "config_source_hash": guard.config_identity.config_source_hash,
            "config_effective_hash": guard.config_identity.config_effective_hash,
            "config_hash_match": guard.config_identity.config_hash_match,
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
            "first_poll_completed": guard.amp.first_poll_completed,
            "first_poll_error": guard.amp.first_poll_error,
            "serial_port_open_error": guard.amp.serial_port_open_error,
            "last_raw_response": guard.amp.last_raw_response,
            "last_successful_command": guard.amp.last_successful_command,
            "startup_state_policy": guard.amp.startup_state_policy,
            "advertisement_waiting_for_first_poll": guard.amp.advertisement_waiting_for_first_poll,
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
        "radio_context": {
            "active_tx_slice": guard.radio_context.active_tx_slice,
            "frequency_hz": guard.radio_context.frequency_hz,
            "band": guard.radio_context.band.as_str(),
            "mode": guard.radio_context.mode,
            "tx_antenna": guard.radio_context.tx_antenna,
            "rx_antenna": guard.radio_context.rx_antenna,
            "radio_serial": guard.radio_context.radio_serial,
            "radio_nickname": guard.radio_context.radio_nickname,
            "radio_callsign": guard.radio_context.radio_callsign,
            "source": guard.radio_context.source,
            "last_tune_frequency_hz": guard.radio_context.last_tune_frequency_hz,
            "last_tune_band": guard.radio_context.last_tune_band.map(|band| band.as_str()),
            "last_kat500_follow_frequency_hz": guard.radio_context.last_kat500_follow_frequency_hz,
            "last_kat500_follow_wire": guard.radio_context.last_kat500_follow_wire,
            "last_kat500_follow_requested_khz": guard.radio_context.last_kat500_follow_requested_khz,
            "last_kat500_follow_confirmed_khz": guard.radio_context.last_kat500_follow_confirmed_khz,
            "last_kat500_follow_confirmation_match": guard.radio_context.last_kat500_follow_confirmation_match,
            "kat500_follow_sent_count": guard.radio_context.kat500_follow_sent_count,
            "kat500_follow_skipped_count": guard.radio_context.kat500_follow_skipped_count,
            "kat500_follow_stale_response_count": guard.radio_context.kat500_follow_stale_response_count,
            "kat500_follow_retry_count": guard.radio_context.kat500_follow_retry_count,
            "last_kpa500_follow_band": guard.radio_context.last_kpa500_follow_band.map(|band| band.as_str()),
            "last_kpa500_follow_wire": guard.radio_context.last_kpa500_follow_wire,
            "last_kpa500_follow_result": guard.radio_context.last_kpa500_follow_result,
            "last_kpa500_follow_requested_bn": guard.radio_context.last_kpa500_follow_requested_bn,
            "last_kpa500_follow_confirmed_bn": guard.radio_context.last_kpa500_follow_confirmed_bn,
            "last_kpa500_follow_confirmation_match": guard.radio_context.last_kpa500_follow_confirmation_match,
            "kpa500_follow_stale_response_count": guard.radio_context.kpa500_follow_stale_response_count,
            "kpa500_follow_retry_count": guard.radio_context.kpa500_follow_retry_count,
            "kpa500_follow_sent_count": guard.radio_context.kpa500_follow_sent_count,
            "kpa500_follow_skipped_count": guard.radio_context.kpa500_follow_skipped_count,
            "context_age_ms": stale_duration_ms(guard.radio_context.updated_at),
            "last_tune_age_ms": stale_duration_ms(guard.radio_context.last_tune_at),
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
            "pgxl_listener_ready_at_ms": guard.clients.pgxl_listener_ready_at_ms,
            "tgxl_listener_ready_at_ms": guard.clients.tgxl_listener_ready_at_ms,
            "pgxl_first_accept_at_ms": guard.clients.pgxl_first_accept_at_ms,
            "tgxl_first_accept_at_ms": guard.clients.tgxl_first_accept_at_ms,
            "pgxl_last_no_socket_warning_at_ms": guard.clients.pgxl_last_no_socket_warning_at_ms,
            "pgxl_reannounce_count_at_first_accept": guard.clients.pgxl_reannounce_count_at_first_accept,
            "pgxl_sub_amp_all_count_at_first_accept": guard.clients.pgxl_sub_amp_all_count_at_first_accept,
            "pgxl_last_amp_status_before_accept": guard.clients.pgxl_last_amp_status_before_accept,
        },
        "flex_injection": guard.flex_injection,
        "lifecycle": guard.lifecycle,
        "controls": guard.controls,
        "effective_controls": guard.effective_controls,
        "protocol_audit": {
            "tgxl_compliance_percent": protocol_audit_summary().tgxl_percent(),
            "pgxl_compliance_percent": protocol_audit_summary().pgxl_percent(),
            "unsupported_or_partial_count": protocol_audit_summary().unsupported_count(),
        },
        "pgxl_lifecycle": pgxl_lifecycle_json(&guard),
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
            "sub_amplifier_all_command_count": guard.flex_injection.sub_amplifier_all_command_count,
            "amplifier_object_seen_at_ms": guard.flex_injection.amplifier_object_seen_at_ms,
            "amplifier_handle_change_count": guard.flex_injection.amplifier_handle_change_count,
            "amplifier_removed_count": guard.flex_injection.amplifier_removed_count,
            "post_amplifier_registration_sent": guard.flex_injection.post_amplifier_registration_sent,
            "registration_continued_without_handle": guard.flex_injection.registration_continued_without_handle,
            "keepalive_enable_accepted": guard.flex_injection.keepalive_enable_accepted,
            "sub_amplifier_all_accepted": guard.flex_injection.sub_amplifier_all_accepted,
            "amp_widget_visibility_risk": amp_widget_visibility_risk(&guard),
            "amplifier_direct_connect_expected": guard.flex_injection.amplifier_direct_connect_expected,
            "last_amplifier_removed_reason": guard.flex_injection.last_amplifier_removed_reason,
            "meter_publish_count": guard.flex_injection.meter_publish_count,
            "meter_publish_supported": guard.flex_injection.meter_publish_supported,
            "meter_publish_last_result": guard.flex_injection.meter_publish_last_result,
            "last_interlock_state": guard.flex_injection.last_interlock_state,
            "last_interlock_reason": guard.flex_injection.last_interlock_reason,
            "last_interlock_tx_allowed": guard.flex_injection.last_interlock_tx_allowed,
            "empty_amplifier_field_count": guard.flex_injection.interlock_empty_amplifier_count,
            "interlock_blocked_count": guard.flex_injection.interlock_blocked_count,
            "interlock_created": guard.flex_injection.interlock_handle.is_some(),
            "interlock_disabled_for_test": guard.flex_injection.interlock_disabled_for_test,
            "tuner_presence_age_ms": stale_duration_ms(guard.flex_injection.tuner_last_seen_at),
            "amplifier_presence_age_ms": stale_duration_ms(guard.flex_injection.amplifier_last_seen_at),
        },
        "protocol": guard.protocol,
    })
    .to_string()
}

fn amp_widget_visibility_risk(guard: &bridge_core::state::BridgeState) -> Option<String> {
    if !guard.flex_injection.enabled {
        return Some("Flex amplifier injection is disabled".to_string());
    }
    if guard.flex_injection.connection_state != bridge_core::ConnectionState::Connected {
        return Some(format!(
            "Flex injection is {}",
            guard.flex_injection.connection_state.as_str()
        ));
    }
    if guard.flex_injection.amplifier_handle.is_none() {
        if guard.flex_injection.sub_amplifier_all_accepted {
            return None;
        }
        return Some("No Flex amplifier handle observed".to_string());
    }
    if guard.flex_injection.amplifier_handle_change_count > 1 {
        return Some(format!(
            "Amplifier handle changed {} times",
            guard.flex_injection.amplifier_handle_change_count
        ));
    }
    if guard.flex_injection.state_advertisement_mismatch.is_some() {
        return guard.flex_injection.state_advertisement_mismatch.clone();
    }
    if guard.clients.pgxl_session_started_count == 0 {
        return Some("AetherSDR has not opened PGXL TCP yet".to_string());
    }
    None
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

fn pgxl_lifecycle_json(guard: &bridge_core::state::BridgeState) -> serde_json::Value {
    let (state, reason) = if !guard.flex_injection.enabled {
        ("NOT_STARTED", "Flex amplifier injection is disabled")
    } else if guard.flex_injection.amplifier_handle.is_none() {
        if guard.flex_injection.sub_amplifier_all_accepted {
            (
                "PGXL_TCP_PENDING",
                "Flex accepted amplifier create and subscriptions; waiting for AetherSDR PGXL TCP connection",
            )
        } else if guard.flex_injection.amplifier_create_accepted {
            (
                "FLEX_ACCEPTED",
                "Flex accepted amplifier create; waiting for amplifier handle/status or fallback registration",
            )
        } else if guard.flex_injection.amplifier_create_sent {
            (
                "AMP_ADVERTISED",
                "Amplifier create sent; waiting for Flex response",
            )
        } else {
            ("FLEX_CONNECTED", "Waiting to advertise amplifier to Flex")
        }
    } else if guard.clients.pgxl_client_count > 0 && guard.clients.pgxl_session_started_count > 0 {
        if guard.protocol.pgxl.parse_failures == 0 && guard.protocol.pgxl.unknown_commands == 0 {
            ("PGXL_STABLE", "PGXL TCP connected with no protocol errors")
        } else {
            (
                "PGXL_DEGRADED",
                "PGXL TCP connected but protocol counters show errors",
            )
        }
    } else if guard.clients.pgxl_session_started_count > 0 {
        (
            "PGXL_CONNECTED",
            guard
                .clients
                .pgxl_last_disconnect_reason
                .as_deref()
                .unwrap_or("PGXL TCP session has started but no client is currently active"),
        )
    } else if guard
        .flex_injection
        .amplifier_pgxl_tcp_attempted_after_status
    {
        (
            "PGXL_TCP_PENDING",
            "AetherSDR attempted PGXL TCP after amplifier status",
        )
    } else if guard.clients.pgxl_manual_connect_no_socket_attempt_count > 2 {
        (
            "PGXL_DEGRADED",
            "Amplifier is advertised but AetherSDR has not attempted PGXL TCP",
        )
    } else {
        (
            "PGXL_TCP_PENDING",
            "Amplifier handle exists; waiting for AetherSDR PGXL TCP connection",
        )
    };
    let transition_count = guard
        .flex_injection
        .amplifier_reannounce_count
        .saturating_add(guard.clients.pgxl_session_started_count)
        .saturating_add(guard.clients.pgxl_manual_connect_no_socket_attempt_count);
    serde_json::json!({
        "state": state,
        "reason": reason,
        "last_disconnect_reason": guard.clients.pgxl_last_disconnect_reason,
        "sessions_started": guard.clients.pgxl_session_started_count,
        "active_clients": guard.clients.pgxl_client_count,
        "transition_count": transition_count,
        "last_transition_at_ms": system_time_ms(guard.flex_injection.amplifier_last_seen_at.or(guard.amp.last_successful_poll_at)),
    })
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
            allow_control: cfg.kpa500.allow_control || allow_control,
            allow_rf_risk: cfg.kpa500.allow_rf_risk || allow_rf_risk,
            follow_flex_band: cfg.kpa500.follow_flex_band,
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
            allow_control: true,
            allow_rf_risk: cfg.kpa500.allow_rf_risk || allow_rf_risk,
            follow_flex_band: cfg.kpa500.follow_flex_band,
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
            allow_control: cfg.kat500.allow_control || allow_control,
            allow_rf_risk: cfg.kat500.allow_rf_risk || allow_rf_risk,
            follow_flex_frequency: cfg.kat500.follow_flex_frequency,
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
        assert!(names.contains(&"aethersdr-smoke-test".to_string()));
        assert!(names.contains(&"aethersdr-operational-test".to_string()));
        assert!(names.contains(&"ecosystem-soak-test".to_string()));
        assert!(names.contains(&"full-operational-test".to_string()));
        assert!(names.contains(&"full-aethersdr-functional-test".to_string()));
        assert!(names.contains(&"operational-gap-test".to_string()));
        assert!(names.contains(&"pgxl-trigger-strategy-test".to_string()));
        assert!(names.contains(&"aethersdr-open-trigger-test".to_string()));
        assert!(names.contains(&"band-follow-test".to_string()));
        assert!(names.contains(&"replay-session".to_string()));
        assert!(names.contains(&"simulate-control".to_string()));
        assert!(names.contains(&"simulate-pgxl-control".to_string()));
        assert!(names.contains(&"pgxl-self-probe".to_string()));
        assert!(names.contains(&"test-startup-sequence".to_string()));
        assert!(names.contains(&"protocol-audit".to_string()));
        assert!(names.contains(&"pgxl-pairing-lab".to_string()));
        assert!(names.contains(&"pgxl-direct-trigger-matrix".to_string()));
        assert!(names.contains(&"amplifier-operate-lab".to_string()));
        assert!(names.contains(&"compare-create-profiles".to_string()));
    }

    #[test]
    fn protocol_audit_summary_is_exposed() {
        let summary = protocol_audit_summary();
        assert_eq!(summary.tgxl_percent(), 85);
        assert_eq!(summary.pgxl_percent(), 76);
        assert_eq!(summary.unsupported_count(), 12);
    }

    #[test]
    fn rf_risk_confirmation_string_is_required() {
        assert!(require_rf_risk_confirmation(false, None).is_ok());
        assert!(require_rf_risk_confirmation(true, None).is_err());
        assert!(require_rf_risk_confirmation(true, Some("wrong")).is_err());
        assert!(require_rf_risk_confirmation(true, Some("I understand")).is_ok());
    }

    #[test]
    fn operational_policy_overrides_raw_dry_run_for_selected_controls() {
        let mut cfg = BridgeConfig::default();
        cfg.kpa500.dry_run = true;
        cfg.kpa500.allow_control = false;
        cfg.kpa500.allow_rf_risk = false;
        cfg.kat500.dry_run = true;
        cfg.kat500.allow_control = false;
        cfg.kat500.allow_rf_risk = false;
        cfg.operational.enable_real_controls = true;
        cfg.operational.enable_kat_tune = true;
        cfg.operational.enable_kpa_standby = true;
        cfg.operational.confirm_real_hardware_control = "I understand".to_string();

        let policy = effective_control_policy(&cfg);
        assert!(policy.operational_override_active);
        assert!(policy.effective_kat_tune_enabled);
        assert!(policy.effective_kpa_standby_enabled);
        assert!(!policy.effective_kpa_operate_enabled);
        assert!(!policy.effective_kat_dry_run);
        assert!(!policy.effective_kpa_dry_run);
        assert!(policy.effective_kat_allow_rf_risk);
        assert!(policy.effective_kpa_allow_control);
        assert!(!policy.effective_kpa_allow_rf_risk);
    }

    #[test]
    fn mode_banner_labels_monitor_operational_and_rf_risk() {
        let monitor = BridgeConfig::default();
        assert_eq!(
            operation_mode_label(&effective_control_policy(&monitor)),
            "MONITOR ONLY"
        );

        let mut tune_standby = BridgeConfig::default();
        tune_standby.operational.enable_real_controls = true;
        tune_standby.operational.enable_kat_tune = true;
        tune_standby.operational.enable_kpa_standby = true;
        tune_standby.operational.confirm_real_hardware_control = "I understand".to_string();
        assert_eq!(
            operation_mode_label(&effective_control_policy(&tune_standby)),
            "OPERATIONAL TUNE/STANDBY"
        );

        let mut rf_risk = tune_standby.clone();
        rf_risk.operational.enable_kpa_operate = true;
        assert_eq!(
            operation_mode_label(&effective_control_policy(&rf_risk)),
            "RF-RISK OPERATE"
        );
    }

    #[test]
    fn simulate_control_uses_effective_policy() {
        let mut cfg = BridgeConfig::default();
        cfg.kat500.dry_run = true;
        cfg.operational.enable_real_controls = true;
        cfg.operational.enable_kat_tune = true;
        cfg.operational.confirm_real_hardware_control = "I understand".to_string();
        assert!(simulate_control(&cfg, SimulatedControlAction::Tune).is_ok());

        let mut blocked = cfg.clone();
        blocked.operational.confirm_real_hardware_control = "not confirmed".to_string();
        assert!(blocked.validate().is_err());
    }

    #[test]
    fn runtime_config_identity_reports_effective_hash_match() {
        let cfg = BridgeConfig::default();
        let identity = runtime_config_identity(&cfg, None).unwrap();
        assert!(identity.config_hash.is_some());
        assert!(identity.config_effective_hash.is_some());
        assert_eq!(identity.config_hash, identity.config_effective_hash);
    }

    #[test]
    fn operational_start_rejects_loopback_pgxl_ip_for_lan_radio() {
        let mut cfg = BridgeConfig::default();
        cfg.flex_injection.enabled = true;
        cfg.flex_injection.radio_ip = "192.168.0.199".to_string();
        cfg.flex_injection.amplifier_ip = "127.0.0.1".to_string();
        cfg.server.bind_ip = "127.0.0.1".to_string();
        let err = validate_operational_start_config(&cfg, BridgeStartMode::Operational)
            .unwrap_err()
            .to_string();
        assert!(err.contains("INVALID_PGXL_ADVERTISED_IP"));
    }

    #[test]
    fn operational_start_allows_loopback_pgxl_ip_for_local_radio_path() {
        let mut cfg = BridgeConfig::default();
        cfg.flex_injection.enabled = true;
        cfg.flex_injection.radio_ip = "127.0.0.1".to_string();
        cfg.flex_injection.amplifier_ip = "127.0.0.1".to_string();
        cfg.server.bind_ip = "127.0.0.1".to_string();
        validate_operational_start_config(&cfg, BridgeStartMode::Operational).unwrap();
    }

    #[test]
    fn operational_start_rejects_pgxl_connect_assist() {
        let mut cfg = BridgeConfig::default();
        cfg.flex_injection.enabled = true;
        cfg.flex_injection.radio_ip = "192.168.0.199".to_string();
        cfg.flex_injection.amplifier_ip = "192.168.0.189".to_string();
        cfg.flex_injection.pgxl_connect_assist = true;
        let err = validate_operational_start_config(&cfg, BridgeStartMode::Operational)
            .unwrap_err()
            .to_string();
        assert!(err.contains("PGXL_CONNECT_ASSIST_DISABLED"));
        validate_operational_start_config(&cfg, BridgeStartMode::Lab).unwrap();
    }

    #[test]
    fn operational_start_rejects_nonstandard_amplifier_create_profiles() {
        let mut cfg = BridgeConfig::default();
        cfg.flex_injection.enabled = true;
        cfg.flex_injection.radio_ip = "192.168.0.199".to_string();
        cfg.flex_injection.amplifier_ip = "192.168.0.189".to_string();

        for profile in ["pgxl_verbose", "old_good_pgxl", "aethersdr_pgxl_direct_lab"] {
            cfg.flex_injection.amplifier_status_profile = profile.to_string();
            let err = validate_operational_start_config(&cfg, BridgeStartMode::Operational)
                .unwrap_err()
                .to_string();
            assert!(err.contains("UNSAFE_LAB_AMPLIFIER_CREATE_PROFILE"));
            validate_operational_start_config(&cfg, BridgeStartMode::Lab).unwrap();
        }

        for profile in [
            "official_pgxl",
            "aethersdr_minimal",
            "aethersdr_operational",
            "aethersdr_force_direct",
            "pgxl_paired",
            "minimal",
            "strict_real_pgxl",
        ] {
            cfg.flex_injection.amplifier_status_profile = profile.to_string();
            validate_operational_start_config(&cfg, BridgeStartMode::Operational).unwrap();
        }
    }

    #[test]
    fn status_exposes_effective_controls_and_connect_assist() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let state = shared_default_state();
        rt.block_on(async {
            let mut guard = state.write().await;
            guard.effective_controls = EffectiveControlPolicy {
                operational_enabled: true,
                operational_confirmation_valid: true,
                operational_override_active: true,
                effective_kat_tune_enabled: true,
                ..EffectiveControlPolicy::default()
            };
            guard.flex_injection.pgxl_connect_assist_enabled = true;
            drop(guard);
            let status: serde_json::Value =
                serde_json::from_str(&status_json(&state).await).unwrap();
            assert_eq!(
                status["effective_controls"]["operational_enabled"],
                serde_json::json!(true)
            );
            assert_eq!(
                status["effective_controls"]["effective_kat_tune_enabled"],
                serde_json::json!(true)
            );
            assert_eq!(
                status["flex_injection"]["pgxl_connect_assist_enabled"],
                serde_json::json!(true)
            );
            assert_eq!(
                status["protocol_audit"]["tgxl_compliance_percent"],
                serde_json::json!(85)
            );
        });
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

    #[test]
    fn detects_amp_advertisement_mismatch() {
        let mut state = bridge_core::state::BridgeState::default();
        state.amp.connection_state = ConnectionState::Connected;
        state.amp.connected = true;
        state.amp.operate = true;
        state.amp.state = AmpOperatingState::Operate;
        state.flex_injection.active_amplifier_status_profile =
            Some("aethersdr_force_direct".to_string());
        state.flex_injection.last_advertised_flex_amp_state = Some("STANDBY".to_string());
        let reason = state_advertisement_mismatch_reason(&state).unwrap();
        assert!(reason.contains("live_kpa_state=OPERATE"));
        assert!(reason.contains("flex_advertised_state=STANDBY"));
        assert!(reason.contains("kpa_fault=none"));

        state.flex_injection.last_advertised_flex_amp_state = Some("OPERATE".to_string());
        assert!(state_advertisement_mismatch_reason(&state).is_none());
    }
}
