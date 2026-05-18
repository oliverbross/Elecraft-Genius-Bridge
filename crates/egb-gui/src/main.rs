use anyhow::{Context, Result};
use eframe::egui;
use egb_config::BridgeConfig;
use serde::Deserialize;
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_CONFIG: &str = "config.flex-injection-readonly.yaml";
const LOG_LIMIT: usize = 100;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1220.0, 820.0])
            .with_min_inner_size([980.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Elecraft Genius Bridge",
        native_options,
        Box::new(|_cc| Box::new(GuiApp::new())),
    )
}

struct GuiApp {
    config_path: PathBuf,
    config: BridgeConfig,
    status: Option<StatusSnapshot>,
    bridge: BridgeProcess,
    tab: Tab,
    rf_acknowledged: bool,
    last_status_poll: Instant,
    status_error: Option<String>,
    serial_ports: Vec<String>,
    kpa_probe_result: String,
    kat_probe_result: String,
    diagnostics: VecDeque<String>,
    tx: Sender<AsyncMessage>,
    rx: Receiver<AsyncMessage>,
}

impl GuiApp {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let config_path = PathBuf::from(DEFAULT_CONFIG);
        let config = load_config_or_default(&config_path);
        let mut app = Self {
            config_path,
            config,
            status: None,
            bridge: BridgeProcess::default(),
            tab: Tab::Dashboard,
            rf_acknowledged: false,
            last_status_poll: Instant::now() - Duration::from_secs(10),
            status_error: None,
            serial_ports: Vec::new(),
            kpa_probe_result: String::new(),
            kat_probe_result: String::new(),
            diagnostics: VecDeque::new(),
            tx,
            rx,
        };
        app.push_log("GUI started");
        app
    }

    fn push_log(&mut self, line: impl Into<String>) {
        self.diagnostics
            .push_back(format!("{} {}", timestamp_hms(), line.into()));
        while self.diagnostics.len() > LOG_LIMIT {
            self.diagnostics.pop_front();
        }
    }

    fn poll_messages(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                AsyncMessage::SerialPorts(ports) => {
                    self.serial_ports = ports;
                    self.push_log(format!(
                        "serial port scan returned {}",
                        self.serial_ports.len()
                    ));
                }
                AsyncMessage::CommandResult { label, output } => {
                    if label == "test-kpa" || label == "probe-kpa" {
                        self.kpa_probe_result = output.clone();
                    } else if label == "test-kat" || label == "probe-kat" {
                        self.kat_probe_result = output.clone();
                    }
                    self.push_log(format!("{label} completed"));
                }
                AsyncMessage::CommandError { label, error } => {
                    self.push_log(format!("{label} failed: {error}"));
                }
                AsyncMessage::BridgeLog(line) => self.push_log(line),
            }
        }
    }

    fn poll_status_if_due(&mut self) {
        if self.last_status_poll.elapsed() < Duration::from_millis(800) {
            return;
        }
        self.last_status_poll = Instant::now();
        match fetch_status(&self.config.metrics.bind_ip, self.config.metrics.port) {
            Ok(status) => {
                self.status = Some(status);
                self.status_error = None;
            }
            Err(err) => {
                self.status_error = Some(err.to_string());
            }
        }
    }

    fn validate_config(&mut self) {
        match self.config.validate() {
            Ok(()) => self.push_log("config validation OK"),
            Err(err) => self.push_log(format!("config validation failed: {err}")),
        }
    }

    fn load_config(&mut self) {
        match BridgeConfig::load(&self.config_path) {
            Ok(config) => {
                self.config = config;
                self.rf_acknowledged = self.config.kpa500.allow_rf_risk;
                self.push_log(format!("loaded {}", self.config_path.display()));
            }
            Err(err) => self.push_log(format!("load failed: {err}")),
        }
    }

    fn save_config(&mut self) {
        if self.config.kpa500.allow_rf_risk && !self.rf_acknowledged {
            self.push_log("refusing to save allow_rf_risk=true until warning is acknowledged");
            return;
        }
        match save_config(&self.config_path, &self.config) {
            Ok(()) => self.push_log(format!("saved {}", self.config_path.display())),
            Err(err) => self.push_log(format!("save failed: {err}")),
        }
    }

    fn start_bridge(&mut self) {
        if self.bridge.is_running() {
            self.push_log("bridge already running");
            return;
        }
        if self.config.kpa500.allow_rf_risk && !self.rf_acknowledged {
            self.push_log("refusing to start with RF-risk enabled until warning is acknowledged");
            return;
        }
        match self.bridge.start(&self.config_path, self.tx.clone()) {
            Ok(()) => self.push_log("bridge process started"),
            Err(err) => self.push_log(format!("bridge start failed: {err}")),
        }
    }

    fn stop_bridge(&mut self) {
        match self.bridge.stop() {
            Ok(()) => self.push_log("bridge process stopped"),
            Err(err) => self.push_log(format!("bridge stop failed: {err}")),
        }
    }

    fn restart_bridge(&mut self) {
        let _ = self.bridge.stop();
        self.start_bridge();
    }

    fn run_egb_command(&mut self, label: &'static str, args: Vec<String>) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = run_egb_capture(&args);
            let message = match result {
                Ok(output) => AsyncMessage::CommandResult {
                    label: label.to_string(),
                    output,
                },
                Err(err) => AsyncMessage::CommandError {
                    label: label.to_string(),
                    error: err.to_string(),
                },
            };
            let _ = tx.send(message);
        });
    }

    fn scan_serial_ports(&mut self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let ports = tokio_serial::available_ports()
                .map(|ports| {
                    ports
                        .into_iter()
                        .map(|port| port.port_name)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let _ = tx.send(AsyncMessage::SerialPorts(ports));
        });
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_messages();
        self.poll_status_if_due();
        self.bridge.refresh();

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Elecraft Genius Bridge");
                ui.separator();
                ui.selectable_value(&mut self.tab, Tab::Dashboard, "Dashboard");
                ui.selectable_value(&mut self.tab, Tab::Config, "Configuration");
                ui.selectable_value(&mut self.tab, Tab::Diagnostics, "Diagnostics");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    process_badge(
                        ui,
                        effective_process_state(self.bridge.state(), self.status.as_ref()),
                    );
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Dashboard => self.ui_dashboard(ui),
            Tab::Config => self.ui_config(ui),
            Tab::Diagnostics => self.ui_diagnostics(ui),
        });

        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

impl GuiApp {
    fn ui_dashboard(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Start Bridge").clicked() {
                self.start_bridge();
            }
            if ui.button("Stop Bridge").clicked() {
                self.stop_bridge();
            }
            if ui.button("Restart Bridge").clicked() {
                self.restart_bridge();
            }
            if ui.button("Open Logs Folder").clicked() {
                open_path(Path::new("logs"));
            }
            if ui.button("Open /status").clicked() {
                open_url(&format!(
                    "http://{}:{}/status",
                    self.config.metrics.bind_ip, self.config.metrics.port
                ));
            }
            if ui.button("Copy Diagnostics Bundle").clicked() {
                match copy_diagnostics_bundle(&self.config_path) {
                    Ok(path) => {
                        self.push_log(format!("diagnostics bundle written to {}", path.display()))
                    }
                    Err(err) => self.push_log(format!("diagnostics bundle failed: {err}")),
                }
            }
        });
        if let Some(error) = &self.status_error {
            ui.colored_label(
                egui::Color32::YELLOW,
                format!("/status unavailable: {error}"),
            );
        }
        ui.separator();
        egui::Grid::new("dashboard_grid")
            .num_columns(2)
            .spacing([16.0, 12.0])
            .show(ui, |ui| {
                ui.vertical(|ui| self.ui_kpa_panel(ui));
                ui.vertical(|ui| self.ui_kat_panel(ui));
                ui.end_row();
            });
    }

    fn ui_kpa_panel(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.heading("KPA500");
            let amp = self.status.as_ref().map(|status| &status.amp);
            field(
                ui,
                "Connection",
                amp.map(|a| a.connection_state.as_str())
                    .unwrap_or("unknown"),
            );
            field(ui, "Connected", bool_text(amp.map(|a| a.connected)));
            field(ui, "Operate", bool_text(amp.and_then(|a| a.operate)));
            field(
                ui,
                "State",
                amp.and_then(|a| a.state.as_deref()).unwrap_or("unknown"),
            );
            field(
                ui,
                "Firmware",
                amp.and_then(|a| a.firmware_version.as_deref())
                    .unwrap_or("unknown"),
            );
            field(ui, "Forward", format_watts(amp.and_then(|a| a.forward_power_watts)));
            field(ui, "SWR", format_ratio(amp.and_then(|a| a.swr)));
            field(ui, "Current", format_amps(amp.and_then(|a| a.pa_current_amps)));
            field(ui, "Voltage", format_volts(amp.and_then(|a| a.pa_voltage_volts)));
            field(ui, "Temperature", format_temp(amp.and_then(|a| a.temperature_c)));
            field(ui, "Fault", amp.and_then(|a| a.fault.as_deref()).unwrap_or("none"));
            field(
                ui,
                "Last poll",
                format_ms_age(amp.and_then(|a| a.stale_duration_ms)),
            );
            field(
                ui,
                "Last poll epoch",
                amp.and_then(|a| a.last_successful_poll_ms)
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
            field(
                ui,
                "Timeouts",
                amp.map(|a| a.runtime.poll_failure_count.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
            field(
                ui,
                "Capabilities",
                amp.map(|a| a.capabilities.join(", "))
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "-".to_string()),
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.config.kpa500.dry_run, egui::Button::new("Standby"))
                    .clicked()
                {
                    self.run_egb_command(
                        "test-kpa-standby",
                        vec![
                            "test-kpa".into(),
                            "--config".into(),
                            self.config_path.display().to_string(),
                            "--allow-control".into(),
                        ],
                    );
                }
                let operate_enabled =
                    self.config.kpa500.allow_rf_risk && self.rf_acknowledged && !self.config.kpa500.dry_run;
                if ui
                    .add_enabled(operate_enabled, egui::Button::new("Operate"))
                    .clicked()
                {
                    self.run_egb_command(
                        "test-kpa-operate",
                        vec![
                            "test-kpa-operate".into(),
                            "--config".into(),
                            self.config_path.display().to_string(),
                            "--allow-rf-risk".into(),
                        ],
                    );
                }
                ui.add_enabled(false, egui::Button::new("Clear Fault"));
            });
            if self.config.kpa500.allow_rf_risk {
                ui.colored_label(
                    egui::Color32::RED,
                    "RF-risk enabled: operate can key the amplifier. Use dummy load/local-only testing.",
                );
            }
        });
    }

    fn ui_kat_panel(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.heading("KAT500");
            let tuner = self.status.as_ref().map(|status| &status.tuner);
            field(
                ui,
                "Connection",
                tuner
                    .map(|t| t.connection_state.as_str())
                    .unwrap_or("unknown"),
            );
            field(ui, "Connected", bool_text(tuner.map(|t| t.connected)));
            field(ui, "Operate", bool_text(tuner.and_then(|t| t.operate)));
            field(
                ui,
                "Firmware",
                tuner
                    .and_then(|t| t.firmware_version.as_deref())
                    .unwrap_or("unknown"),
            );
            field(
                ui,
                "Mode",
                tuner.and_then(|t| t.mode.as_deref()).unwrap_or("unknown"),
            );
            field(
                ui,
                "Antenna",
                tuner
                    .and_then(|t| t.selected_antenna)
                    .map(|ant| ant.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
            field(ui, "Bypass", bool_text(tuner.and_then(|t| t.bypass)));
            field(ui, "Tuning", bool_text(tuner.and_then(|t| t.tuning)));
            field(ui, "SWR", format_ratio(tuner.and_then(|t| t.swr)));
            field(
                ui,
                "VFWD",
                format_watts(tuner.and_then(|t| t.forward_power_watts)),
            );
            field(
                ui,
                "VRFL",
                format_watts(tuner.and_then(|t| t.reflected_power_watts)),
            );
            field(
                ui,
                "Fault",
                tuner.and_then(|t| t.fault.as_deref()).unwrap_or("none"),
            );
            field(
                ui,
                "Last poll",
                format_ms_age(tuner.and_then(|t| t.stale_duration_ms)),
            );
            field(
                ui,
                "Last poll epoch",
                tuner
                    .and_then(|t| t.last_successful_poll_ms)
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
            field(
                ui,
                "Unsolicited",
                tuner
                    .and_then(|t| t.unsolicited_count)
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "trace logs".to_string()),
            );
            field(
                ui,
                "Capabilities",
                tuner
                    .map(|t| t.capabilities.join(", "))
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "-".to_string()),
            );
            ui.separator();
            ui.horizontal(|ui| {
                ui.add_enabled(false, egui::Button::new("Tune"));
                ui.add_enabled(false, egui::Button::new("Bypass"));
                ui.add_enabled(false, egui::Button::new("Antenna"));
            });
            ui.label("KAT500 controls remain disabled until control mappings are validated.");
        });
    }

    fn ui_config(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Config file");
            let mut path = self.config_path.display().to_string();
            if ui.text_edit_singleline(&mut path).changed() {
                self.config_path = PathBuf::from(path);
            }
            if ui.button("Load").clicked() {
                self.load_config();
            }
            if ui.button("Save").clicked() {
                self.save_config();
            }
            if ui.button("Validate").clicked() {
                self.validate_config();
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Network");
            text_field(ui, "Bind IP", &mut self.config.server.bind_ip);
            port_field(ui, "PGXL port", &mut self.config.pgxl.port);
            port_field(ui, "TGXL port", &mut self.config.tgxl.port);
            checkbox(ui, "PGXL AetherSDR compatibility", &mut self.config.pgxl.aethersdr_compat);
            checkbox(ui, "PGXL direct connected diagnostic", &mut self.config.pgxl.force_direct_connected_test);
            checkbox(ui, "TGXL AetherSDR compatibility", &mut self.config.tgxl.aethersdr_compat);
            checkbox(ui, "TGXL direct presence test", &mut self.config.tgxl.force_presence_test);
            checkbox(ui, "Metrics enabled", &mut self.config.metrics.enabled);
            text_field(ui, "Metrics bind IP", &mut self.config.metrics.bind_ip);
            port_field(ui, "Metrics/status port", &mut self.config.metrics.port);

            ui.separator();
            ui.heading("KPA500");
            text_field(ui, "KPA500 COM port", &mut self.config.kpa500.com_port);
            u32_field(ui, "KPA500 baud", &mut self.config.kpa500.baud);
            checkbox(ui, "KPA500 dry run", &mut self.config.kpa500.dry_run);
            let mut rf = self.config.kpa500.allow_rf_risk;
            if checkbox(ui, "KPA500 allow RF-risk", &mut rf).changed() {
                if rf {
                    self.rf_acknowledged = false;
                }
                self.config.kpa500.allow_rf_risk = rf;
            }
            if self.config.kpa500.allow_rf_risk {
                ui.colored_label(
                    egui::Color32::RED,
                    "RF-risk allows operate commands. Acknowledge before saving or starting.",
                );
                ui.checkbox(&mut self.rf_acknowledged, "I understand this can key the amplifier and will test locally with RF safety controls");
            }

            ui.separator();
            ui.heading("KAT500");
            text_field(ui, "KAT500 COM port", &mut self.config.kat500.com_port);
            u32_field(ui, "KAT500 baud", &mut self.config.kat500.baud);
            checkbox(ui, "KAT500 dry run", &mut self.config.kat500.dry_run);

            ui.separator();
            ui.heading("Flex Injection");
            checkbox(ui, "Enable Flex injection", &mut self.config.flex_injection.enabled);
            text_field(ui, "Flex radio IP", &mut self.config.flex_injection.radio_ip);
            port_field(ui, "Flex radio port", &mut self.config.flex_injection.radio_port);
            text_field(ui, "EGB amplifier IP", &mut self.config.flex_injection.amplifier_ip);
            port_field(ui, "EGB amplifier port", &mut self.config.flex_injection.amplifier_port);
            text_field(ui, "Amplifier model", &mut self.config.flex_injection.amplifier_model);
            text_field(ui, "Serial", &mut self.config.flex_injection.serial);
            text_field(ui, "Antenna map", &mut self.config.flex_injection.ant_map);
            checkbox(ui, "Full PGXL registration", &mut self.config.flex_injection.full_pgxl_registration);
            checkbox(ui, "Create AMP meters", &mut self.config.flex_injection.create_meters);
            checkbox(ui, "Create AMP interlock", &mut self.config.flex_injection.create_interlock);

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("List Serial Ports").clicked() {
                    self.scan_serial_ports();
                }
                if ui.button("Test KPA500").clicked() {
                    self.run_egb_command(
                        "test-kpa",
                        vec![
                            "test-kpa".into(),
                            "--config".into(),
                            self.config_path.display().to_string(),
                        ],
                    );
                }
                if ui.button("Probe KPA500 ^RVM;").clicked() {
                    self.run_egb_command(
                        "probe-kpa",
                        vec![
                            "serial-probe".into(),
                            "--port".into(),
                            self.config.kpa500.com_port.clone(),
                            "--baud".into(),
                            self.config.kpa500.baud.to_string(),
                            "--send".into(),
                            "^RVM;".into(),
                            "--timeout-ms".into(),
                            "1000".into(),
                        ],
                    );
                }
                if ui.button("Test KAT500").clicked() {
                    self.run_egb_command(
                        "test-kat",
                        vec![
                            "test-kat".into(),
                            "--config".into(),
                            self.config_path.display().to_string(),
                        ],
                    );
                }
            });
            if !self.serial_ports.is_empty() {
                ui.label(format!("Serial ports: {}", self.serial_ports.join(", ")));
            }
            if !self.kpa_probe_result.is_empty() {
                ui.collapsing("KPA500 test/probe output", |ui| {
                    ui.monospace(&self.kpa_probe_result);
                });
            }
            if !self.kat_probe_result.is_empty() {
                ui.collapsing("KAT500 test output", |ui| {
                    ui.monospace(&self.kat_probe_result);
                });
            }
        });
    }

    fn ui_diagnostics(&mut self, ui: &mut egui::Ui) {
        ui.heading("Diagnostics");
        if let Some(status) = &self.status {
            ui.group(|ui| {
                ui.heading("Flex Injection");
                field(
                    ui,
                    "Enabled",
                    bool_text(Some(status.flex_injection.enabled)),
                );
                field(ui, "State", &status.flex_injection.connection_state);
                field(
                    ui,
                    "Client handle",
                    status
                        .flex_injection
                        .client_handle
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Amplifier handle",
                    status
                        .flex_injection
                        .amplifier_handle
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Interlock handle",
                    status
                        .flex_injection
                        .interlock_handle
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Meter handles",
                    status
                        .flex_injection
                        .meter_handles
                        .iter()
                        .map(|meter| format!("{}={}", meter.name, meter.handle))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                field(
                    ui,
                    "Commands OK/fail",
                    format!(
                        "{}/{}",
                        status.flex_injection.command_success_count,
                        status.flex_injection.command_failure_count
                    ),
                );
                field(
                    ui,
                    "Ping count",
                    status.flex_injection.ping_count.to_string(),
                );
                field(
                    ui,
                    "Last command",
                    status.flex_injection.last_command.as_deref().unwrap_or("-"),
                );
                field(
                    ui,
                    "Last response",
                    status
                        .flex_injection
                        .last_response
                        .as_deref()
                        .unwrap_or("-"),
                );
            });
            ui.separator();
            ui.group(|ui| {
                ui.heading("Clients And Runtime");
                field(
                    ui,
                    "PGXL client",
                    bool_text(Some(status.clients.pgxl_connected)),
                );
                field(
                    ui,
                    "TGXL client",
                    bool_text(Some(status.clients.tgxl_connected)),
                );
                field(
                    ui,
                    "PGXL clients",
                    status.clients.pgxl_client_count.to_string(),
                );
                field(
                    ui,
                    "TGXL clients",
                    status.clients.tgxl_client_count.to_string(),
                );
                field(
                    ui,
                    "KPA reconnects",
                    status.amp.runtime.reconnect_count.to_string(),
                );
                field(
                    ui,
                    "KAT reconnects",
                    status.tuner.runtime.reconnect_count.to_string(),
                );
                field(
                    ui,
                    "KPA stale transitions",
                    status.amp.runtime.stale_transition_count.to_string(),
                );
                field(
                    ui,
                    "KAT stale transitions",
                    status.tuner.runtime.stale_transition_count.to_string(),
                );
                field(
                    ui,
                    "KPA poll latency",
                    format!(
                        "ok {} avg {:?} ms last {} ms max {} ms",
                        status.amp.runtime.poll_success_count,
                        status.amp.runtime.average_poll_latency_ms,
                        status.amp.runtime.last_poll_latency_ms,
                        status.amp.runtime.max_poll_latency_ms
                    ),
                );
                field(
                    ui,
                    "KAT poll latency",
                    format!(
                        "ok {} avg {:?} ms last {} ms max {} ms",
                        status.tuner.runtime.poll_success_count,
                        status.tuner.runtime.average_poll_latency_ms,
                        status.tuner.runtime.last_poll_latency_ms,
                        status.tuner.runtime.max_poll_latency_ms
                    ),
                );
            });
        }
        ui.separator();
        ui.heading("Last Important Events");
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for line in &self.diagnostics {
                    ui.monospace(line);
                }
            });
        ui.separator();
        ui.label("KPA500 busy-port hint: if ^RVM; times out or the COM port fails to open, close Elecraft KPA500 Remote, EGB, terminal probes, and any serial monitor. Then confirm the COM number in Windows Device Manager.");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Dashboard,
    Config,
    Diagnostics,
}

#[derive(Default)]
struct BridgeProcess {
    child: Option<Child>,
    state: ProcessState,
}

impl BridgeProcess {
    fn is_running(&mut self) -> bool {
        self.refresh();
        matches!(self.state, ProcessState::Running | ProcessState::Starting)
    }

    fn state(&self) -> ProcessState {
        self.state
    }

    fn refresh(&mut self) {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.state = if status.success() {
                        ProcessState::Stopped
                    } else {
                        ProcessState::Error
                    };
                    self.child = None;
                }
                Ok(None) => self.state = ProcessState::Running,
                Err(_) => self.state = ProcessState::Error,
            }
        } else if self.state != ProcessState::Error {
            self.state = ProcessState::Stopped;
        }
    }

    fn start(&mut self, config_path: &Path, tx: Sender<AsyncMessage>) -> Result<()> {
        let egb = find_egb_binary()?;
        let mut child = Command::new(egb)
            .arg("run")
            .arg("--config")
            .arg(config_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn egb bridge")?;
        if let Some(stdout) = child.stdout.take() {
            spawn_log_reader(stdout, tx.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_log_reader(stderr, tx);
        }
        self.child = Some(child);
        self.state = ProcessState::Starting;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            child.kill().context("failed to terminate bridge process")?;
            let _ = child.wait();
        }
        self.state = ProcessState::Stopped;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ProcessState {
    #[default]
    Stopped,
    Starting,
    Running,
    Degraded,
    Error,
}

enum AsyncMessage {
    SerialPorts(Vec<String>),
    CommandResult { label: String, output: String },
    CommandError { label: String, error: String },
    BridgeLog(String),
}

fn spawn_log_reader<R>(reader: R, tx: Sender<AsyncMessage>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines().map_while(std::result::Result::ok) {
            let _ = tx.send(AsyncMessage::BridgeLog(line));
        }
    });
}

#[derive(Debug, Deserialize, Clone)]
struct StatusSnapshot {
    amp: DeviceStatus,
    tuner: DeviceStatus,
    clients: ClientStatus,
    flex_injection: FlexStatus,
}

#[derive(Debug, Deserialize, Clone)]
struct DeviceStatus {
    connection_state: String,
    connected: bool,
    firmware_version: Option<String>,
    capabilities: Vec<String>,
    last_successful_poll_ms: Option<u128>,
    stale_duration_ms: Option<u128>,
    runtime: RuntimeStatus,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    operate: Option<bool>,
    #[serde(default)]
    forward_power_watts: Option<f32>,
    #[serde(default)]
    reflected_power_watts: Option<f32>,
    #[serde(default)]
    swr: Option<f32>,
    #[serde(default)]
    pa_current_amps: Option<f32>,
    #[serde(default)]
    pa_voltage_volts: Option<f32>,
    #[serde(default)]
    temperature_c: Option<f32>,
    #[serde(default)]
    fault: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    bypass: Option<bool>,
    #[serde(default)]
    tuning: Option<bool>,
    #[serde(default)]
    selected_antenna: Option<u8>,
    #[serde(default)]
    unsolicited_count: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
struct RuntimeStatus {
    reconnect_count: u64,
    poll_success_count: u64,
    poll_failure_count: u64,
    stale_transition_count: u64,
    last_poll_latency_ms: u64,
    max_poll_latency_ms: u64,
    average_poll_latency_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
struct ClientStatus {
    pgxl_connected: bool,
    tgxl_connected: bool,
    pgxl_client_count: usize,
    tgxl_client_count: usize,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct FlexStatus {
    enabled: bool,
    connection_state: String,
    client_handle: Option<String>,
    amplifier_handle: Option<String>,
    meter_handles: Vec<MeterHandle>,
    interlock_handle: Option<String>,
    last_command: Option<String>,
    last_response: Option<String>,
    command_success_count: u64,
    command_failure_count: u64,
    ping_count: u64,
}

#[derive(Debug, Deserialize, Clone)]
struct MeterHandle {
    name: String,
    handle: String,
}

fn load_config_or_default(path: &Path) -> BridgeConfig {
    BridgeConfig::load(path).unwrap_or_else(|_| BridgeConfig::default())
}

fn save_config(path: &Path, config: &BridgeConfig) -> Result<()> {
    config.validate()?;
    let text = serde_yaml::to_string(config).context("failed to encode YAML")?;
    fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))
}

fn fetch_status(host: &str, port: u16) -> Result<StatusSnapshot> {
    let addr = (host, port)
        .to_socket_addrs()
        .context("failed to resolve status endpoint")?
        .next()
        .context("status endpoint did not resolve")?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(250))
        .with_context(|| format!("failed to connect to http://{host}:{port}/status"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok();
    stream.write_all(b"GET /status HTTP/1.1\r\nHost: egb\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (_, body) = response
        .split_once("\r\n\r\n")
        .context("status endpoint returned malformed HTTP")?;
    serde_json::from_str(body).context("failed to parse status JSON")
}

fn run_egb_capture(args: &[String]) -> Result<String> {
    let output = Command::new(find_egb_binary()?)
        .args(args)
        .output()
        .context("failed to run egb command")?;
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(text)
    } else {
        anyhow::bail!("egb exited with {}:\n{text}", output.status)
    }
}

fn find_egb_binary() -> Result<PathBuf> {
    let current = std::env::current_exe().context("failed to locate GUI executable")?;
    let dir = current.parent().context("GUI executable has no parent")?;
    let adjacent = dir.join("egb.exe");
    if adjacent.exists() {
        return Ok(adjacent);
    }
    for candidate in [
        PathBuf::from("target-msvc/debug/egb.exe"),
        PathBuf::from("target/debug/egb.exe"),
        PathBuf::from("target/release/egb.exe"),
    ] {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Ok(PathBuf::from("egb.exe"))
}

fn copy_diagnostics_bundle(config_path: &Path) -> Result<PathBuf> {
    let out_dir = PathBuf::from("logs").join(format!("diagnostics-{}", timestamp_compact()));
    fs::create_dir_all(&out_dir)?;
    if config_path.exists() {
        let _ = fs::copy(config_path, out_dir.join("config.yaml"));
    }
    for dir in ["logs/protocol", "logs/serial"] {
        let path = Path::new(dir);
        if !path.exists() {
            continue;
        }
        let target = out_dir.join(path.file_name().unwrap_or_default());
        fs::create_dir_all(&target)?;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let _ = fs::copy(entry.path(), target.join(entry.file_name()));
            }
        }
    }
    Ok(out_dir)
}

fn process_badge(ui: &mut egui::Ui, state: ProcessState) {
    let (text, color) = match state {
        ProcessState::Stopped => ("stopped", egui::Color32::GRAY),
        ProcessState::Starting => ("starting", egui::Color32::YELLOW),
        ProcessState::Running => ("running", egui::Color32::GREEN),
        ProcessState::Degraded => ("degraded", egui::Color32::YELLOW),
        ProcessState::Error => ("error", egui::Color32::RED),
    };
    ui.colored_label(color, text);
}

fn effective_process_state(state: ProcessState, status: Option<&StatusSnapshot>) -> ProcessState {
    if state != ProcessState::Running {
        return state;
    }
    let Some(status) = status else {
        return state;
    };
    if status.amp.connection_state == "degraded"
        || status.tuner.connection_state == "degraded"
        || status.flex_injection.connection_state == "degraded"
    {
        ProcessState::Degraded
    } else {
        ProcessState::Running
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: impl ToString) {
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        ui.monospace(value.to_string());
    });
}

fn text_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value);
    });
}

fn checkbox(ui: &mut egui::Ui, label: &str, value: &mut bool) -> egui::Response {
    ui.checkbox(value, label)
}

fn port_field(ui: &mut egui::Ui, label: &str, value: &mut u16) {
    let mut text = value.to_string();
    ui.horizontal(|ui| {
        ui.label(label);
        if ui.text_edit_singleline(&mut text).lost_focus() {
            if let Ok(parsed) = text.parse::<u16>() {
                *value = parsed;
            }
        }
    });
}

fn u32_field(ui: &mut egui::Ui, label: &str, value: &mut u32) {
    let mut text = value.to_string();
    ui.horizontal(|ui| {
        ui.label(label);
        if ui.text_edit_singleline(&mut text).lost_focus() {
            if let Ok(parsed) = text.parse::<u32>() {
                *value = parsed;
            }
        }
    });
}

fn bool_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn format_watts(value: Option<f32>) -> String {
    value
        .map(|value| format!("{value:.1} W"))
        .unwrap_or_else(|| "-".to_string())
}

fn format_ratio(value: Option<f32>) -> String {
    value
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| "-".to_string())
}

fn format_amps(value: Option<f32>) -> String {
    value
        .map(|value| format!("{value:.1} A"))
        .unwrap_or_else(|| "-".to_string())
}

fn format_volts(value: Option<f32>) -> String {
    value
        .map(|value| format!("{value:.1} V"))
        .unwrap_or_else(|| "-".to_string())
}

fn format_temp(value: Option<f32>) -> String {
    value
        .map(|value| format!("{value:.1} C"))
        .unwrap_or_else(|| "-".to_string())
}

fn format_ms_age(value: Option<u128>) -> String {
    value
        .map(|value| format!("{value} ms ago"))
        .unwrap_or_else(|| "-".to_string())
}

fn open_path(path: &Path) {
    #[cfg(windows)]
    {
        let _ = Command::new("explorer").arg(path).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }
}

fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(url).spawn();
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}

fn timestamp_hms() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

fn timestamp_compact() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_and_preserves_safety_default() {
        let cfg = BridgeConfig::default();
        let text = serde_yaml::to_string(&cfg).unwrap();
        let parsed: BridgeConfig = serde_yaml::from_str(&text).unwrap();
        assert!(!parsed.kpa500.allow_rf_risk);
        parsed.validate().unwrap();
    }

    #[test]
    fn parses_status_json_with_live_fields() {
        let json = r#"{
          "amp": {
            "connection_state": "connected",
            "connected": true,
            "firmware_version": "01.54",
            "capabilities": ["read_operate_status"],
            "last_successful_poll_ms": 1,
            "stale_duration_ms": 10,
            "runtime": {"reconnect_count":0,"poll_success_count":2,"poll_failure_count":0,"stale_transition_count":0,"last_poll_latency_ms":5,"max_poll_latency_ms":9,"average_poll_latency_ms":7},
            "state": "STANDBY",
            "forward_power_watts": 30.0,
            "swr": 1.1,
            "pa_current_amps": 0.0,
            "pa_voltage_volts": 68.9,
            "temperature_c": 30.0,
            "fault": null
          },
          "tuner": {
            "connection_state": "connected",
            "connected": true,
            "firmware_version": "02.16",
            "capabilities": [],
            "last_successful_poll_ms": 1,
            "stale_duration_ms": 10,
            "runtime": {"reconnect_count":0,"poll_success_count":2,"poll_failure_count":0,"stale_transition_count":0,"last_poll_latency_ms":5,"max_poll_latency_ms":9,"average_poll_latency_ms":7},
            "mode": "auto",
            "bypass": false,
            "tuning": false,
            "selected_antenna": 2,
            "swr": 1.11,
            "forward_power_watts": 0.0
          },
          "clients": {"pgxl_connected": true, "tgxl_connected": true, "pgxl_client_count": 1, "tgxl_client_count": 1},
          "flex_injection": {"enabled": true, "connection_state": "connected", "client_handle": "1", "amplifier_handle": "32", "meter_handles": [{"name":"FWD","handle":"101"}], "interlock_handle": "9", "last_command": "ping", "last_response": "R9|0|", "command_success_count": 9, "command_failure_count": 0, "ping_count": 1},
          "protocol": {}
        }"#;
        let status: StatusSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(status.amp.state.as_deref(), Some("STANDBY"));
        assert_eq!(status.amp.forward_power_watts, Some(30.0));
        assert_eq!(status.tuner.selected_antenna, Some(2));
        assert_eq!(status.flex_injection.meter_handles[0].name, "FWD");
    }
}
