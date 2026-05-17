use anyhow::{Context, Result};
use bridge_core::state::{shared_default_state, shared_mock_state};
use bridge_core::{AmpOperatingState, ConnectionState, SharedState};
use clap::{Parser, Subcommand, ValueEnum};
use egb_config::BridgeConfig;
use elecraft_kat500::{
    command_map as kat_command_map, read_only_discovery_commands as kat_discovery_commands,
    read_only_poll_commands as kat_poll_commands, CommandOutcome as KatCommandOutcome,
    CommandSafety as KatCommandSafety, Kat500Driver, Kat500Settings,
};
use elecraft_kpa500::{
    command_map as kpa_command_map, read_only_discovery_commands as kpa_discovery_commands,
    read_only_poll_commands as kpa_poll_commands, CommandOutcome as KpaCommandOutcome,
    CommandSafety as KpaCommandSafety, Kpa500Driver, Kpa500Settings,
};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_serial::{SerialPortBuilderExt, SerialPortType, SerialStream};
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

async fn run_bridge(cfg: BridgeConfig) -> Result<()> {
    let all_mock = cfg.kpa500.mock && cfg.kat500.mock;
    let state = if all_mock {
        shared_mock_state()
    } else {
        shared_default_state()
    };
    apply_mock_config(&cfg, &state).await;

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
            strict_emulation: cfg.pgxl.strict_emulation,
            startup_delay: Duration::from_millis(cfg.pgxl.startup_delay_ms),
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
        "  port={} baud={} mock={} dry_run={}",
        cfg.kpa500.com_port, cfg.kpa500.baud, cfg.kpa500.mock, cfg.kpa500.dry_run
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
        println!("  rollback: force standby (^OS0;) after control test");
        println!("KPA500 control test: sending set_standby wire=^OS0; safety=StateChangeSafe");
        driver.set_standby().await?;
        println!("KPA500 rollback: sending set_standby wire=^OS0; safety=StateChangeSafe");
        driver.set_standby().await?;
    }
    if allow_rf_risk {
        ensure_local_or_lan_bind(cfg)?;
        println!("KPA500 RF-risk test: sending set_operate wire=^OS1; safety=RfRisk");
        driver.set_operate().await?;
        println!("KPA500 rollback: sending set_standby wire=^OS0; safety=StateChangeSafe");
        driver.set_standby().await?;
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
            transcript_dir: cfg
                .logging
                .serial_transcript_dir
                .as_ref()
                .map(PathBuf::from),
        },
        state.clone(),
    );
    driver.connect().await?;
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
    } else if is_private_lan(ip) {
        "private-lan"
    } else if ip.is_unspecified() {
        "public-or-all-interfaces"
    } else {
        "public-or-non-private"
    }
}

fn is_local_or_private(ip: IpAddr) -> bool {
    ip.is_loopback() || is_private_lan(ip)
}

fn is_private_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => {
            value.is_private() || (value.octets()[0] == 169 && value.octets()[1] == 254)
        }
        IpAddr::V6(value) => value.segments()[0] & 0xfe00 == 0xfc00 || value == Ipv6Addr::LOCALHOST,
    }
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
