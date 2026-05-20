use anyhow::{Context, Result};
use eframe::egui;
use egb_config::BridgeConfig;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_CONFIG: &str = "config.flex-injection-readonly.yaml";
const LOG_LIMIT: usize = 500;
const GUI_SETTINGS_PATH: &str = "egb-gui-settings.yaml";

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
    latest_evidence_bundle: Option<PathBuf>,
    current_evidence_dir: Option<PathBuf>,
    settings: GuiSettings,
    controls: ControlFlags,
    log_filter: LogFilter,
    logs_paused: bool,
    tx: Sender<AsyncMessage>,
    rx: Receiver<AsyncMessage>,
}

impl GuiApp {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let config_path = PathBuf::from(DEFAULT_CONFIG);
        let config = load_config_or_default(&config_path);
        let settings = GuiSettings::load(Path::new(GUI_SETTINGS_PATH)).unwrap_or_default();
        let controls = ControlFlags::default();
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
            latest_evidence_bundle: latest_evidence_bundle(),
            current_evidence_dir: None,
            settings,
            controls,
            log_filter: LogFilter::All,
            logs_paused: false,
            tx,
            rx,
        };
        app.push_log("GUI started");
        if app.settings.start_bridge_on_launch {
            app.start_bridge();
        }
        app
    }

    fn push_log(&mut self, line: impl Into<String>) {
        if self.logs_paused {
            return;
        }
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
                    if let Some(path) = extract_evidence_bundle_path(&output) {
                        self.latest_evidence_bundle = Some(path.clone());
                        self.current_evidence_dir = path.file_stem().map(|stem| {
                            PathBuf::from("diagnostics")
                                .join("runs")
                                .join(stem.to_string_lossy().to_string())
                        });
                        self.push_log(format!("evidence bundle: {}", path.display()));
                    }
                    if label.starts_with("stability-test") || label.starts_with("evidence-test") {
                        if output.contains("warning:") {
                            self.push_log(
                                "stability test reported warnings; export includes report",
                            );
                        }
                        match self.export_diagnostics_bundle() {
                            Ok(path) => self.push_log(format!(
                                "stability diagnostics bundle written to {}",
                                path.display()
                            )),
                            Err(err) => {
                                self.push_log(format!("stability diagnostics export failed: {err}"))
                            }
                        }
                    }
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
        if let Err(err) = self.settings.save(Path::new(GUI_SETTINGS_PATH)) {
            self.push_log(format!("GUI settings save failed: {err}"));
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
            let result = run_egb_capture_logged(label, &args);
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

    fn export_diagnostics_bundle(&self) -> Result<PathBuf> {
        export_diagnostics_bundle(
            &self.config_path,
            self.status.as_ref(),
            &self.diagnostics,
            &self.settings,
            self.settings.redact_diagnostics,
        )
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_modern_style(ctx);
        self.poll_messages();
        self.poll_status_if_due();
        self.bridge.refresh();

        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(190.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.heading("EGB");
                ui.label("Elecraft Genius Bridge");
                ui.separator();
                nav_button(ui, &mut self.tab, Tab::Dashboard, "Dashboard");
                nav_button(ui, &mut self.tab, Tab::Config, "Configuration");
                nav_button(ui, &mut self.tab, Tab::Operational, "Operational");
                nav_button(ui, &mut self.tab, Tab::Controls, "Controls");
                nav_button(ui, &mut self.tab, Tab::Diagnostics, "Diagnostics");
                nav_button(ui, &mut self.tab, Tab::Logs, "Logs");
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    process_badge(
                        ui,
                        effective_process_state(self.bridge.state(), self.status.as_ref()),
                    );
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Dashboard => self.ui_dashboard(ui),
            Tab::Config => self.ui_config(ui),
            Tab::Operational => self.ui_operational(ui),
            Tab::Controls => self.ui_controls(ui),
            Tab::Diagnostics => self.ui_diagnostics(ui),
            Tab::Logs => self.ui_logs(ui),
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
            if ui
                .button("Start Stability Test and Export Evidence")
                .clicked()
            {
                self.tab = Tab::Dashboard;
                self.run_egb_command(
                    "evidence-test-10min",
                    vec![
                        "evidence-test".into(),
                        "--config".into(),
                        self.config_path.display().to_string(),
                        "--duration-minutes".into(),
                        "10".into(),
                    ],
                );
                self.push_log("10-minute evidence test started");
            }
            if ui.button("Export Last Evidence Bundle").clicked() {
                match self.export_diagnostics_bundle() {
                    Ok(path) => {
                        self.latest_evidence_bundle = Some(path.clone());
                        self.push_log(format!("diagnostics bundle written to {}", path.display()));
                    }
                    Err(err) => self.push_log(format!("diagnostics bundle failed: {err}")),
                }
            }
            if ui.button("Open Evidence Folder").clicked() {
                open_path(Path::new("diagnostics/runs"));
            }
            if ui.button("Copy Latest Bundle Path").clicked() {
                if let Some(path) = &self.latest_evidence_bundle {
                    ui.output_mut(|out| out.copied_text = path.display().to_string());
                    self.push_log("latest evidence bundle path copied");
                }
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
            if ui.button("Export Full Diagnostics Bundle").clicked() {
                match self.export_diagnostics_bundle() {
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
        ui.horizontal_wrapped(|ui| {
            field(
                ui,
                "Latest evidence",
                self.latest_evidence_bundle
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
            field(
                ui,
                "Current run dir",
                self.current_evidence_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
            let warning_count = self
                .diagnostics
                .iter()
                .filter(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower.contains("warn") || lower.contains("error") || lower.contains("failed")
                })
                .count();
            field(ui, "Captured warnings/errors", warning_count);
            if let Some(status) = &self.status {
                field(
                    ui,
                    "SmartSDR tuner flaps",
                    status.flex_diagnostics.smartsdr_tuner_disappeared_count,
                );
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            self.summary_card(
                ui,
                "Bridge",
                self.bridge.state().label(),
                self.bridge.state().color(),
            );
            if let Some(status) = &self.status {
                self.summary_card(
                    ui,
                    "PGXL",
                    bool_text(Some(status.clients.pgxl_connected)),
                    status_color(status.clients.pgxl_connected),
                );
                self.summary_card(
                    ui,
                    "TGXL",
                    bool_text(Some(status.clients.tgxl_connected)),
                    status_color(status.clients.tgxl_connected),
                );
                self.summary_card(
                    ui,
                    "Flex",
                    &status.flex_injection.connection_state,
                    connection_color(&status.flex_injection.connection_state),
                );
            }
        });
        ui.add_space(8.0);
        egui::Grid::new("dashboard_grid")
            .num_columns(3)
            .spacing([14.0, 12.0])
            .show(ui, |ui| {
                ui.vertical(|ui| self.ui_kpa_panel(ui));
                ui.vertical(|ui| self.ui_kat_panel(ui));
                ui.vertical(|ui| self.ui_flex_card(ui));
                ui.end_row();
            });
    }

    fn summary_card(&self, ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(22, 35, 56))
            .rounding(8.0)
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.set_min_width(130.0);
                ui.label(egui::RichText::new(label).color(egui::Color32::from_rgb(160, 178, 205)));
                ui.label(egui::RichText::new(value).color(color).size(20.0).strong());
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
                "First poll",
                amp.and_then(|a| a.first_poll_completed)
                    .map(|value| bool_text(Some(value)))
                    .unwrap_or("unknown"),
            );
            field(
                ui,
                "First poll error",
                amp.and_then(|a| a.first_poll_error.as_deref())
                    .unwrap_or("-"),
            );
            field(
                ui,
                "Port open error",
                amp.and_then(|a| a.serial_port_open_error.as_deref())
                    .unwrap_or("-"),
            );
            field(
                ui,
                "Last KPA command",
                amp.and_then(|a| a.last_successful_command.as_deref())
                    .unwrap_or("-"),
            );
            field(
                ui,
                "Last KPA raw",
                amp.and_then(|a| a.last_raw_response.as_deref()).unwrap_or("-"),
            );
            field(
                ui,
                "Startup policy",
                amp.and_then(|a| a.startup_state_policy.as_deref())
                    .unwrap_or("-"),
            );
            field(
                ui,
                "Ad waiting",
                amp.and_then(|a| a.advertisement_waiting_for_first_poll)
                    .map(|value| bool_text(Some(value)))
                    .unwrap_or("unknown"),
            );
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

    fn ui_flex_card(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.heading("Flex / Clients");
            if let Some(status) = &self.status {
                field(ui, "Flex state", &status.flex_injection.connection_state);
                field(
                    ui,
                    "Real KPA state",
                    status.amp.state.as_deref().unwrap_or("-"),
                );
                field(
                    ui,
                    "Advertised Flex",
                    status
                        .flex_injection
                        .last_advertised_flex_amp_state
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "EGB desired amp",
                    status
                        .flex_injection
                        .flex_desired_amp_state
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Radio-rewritten amp",
                    status
                        .flex_injection
                        .radio_rewritten_amp_state
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Advertised PGXL",
                    status
                        .flex_injection
                        .last_advertised_pgxl_state
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "TGXL operate",
                    status
                        .flex_injection
                        .last_advertised_tgxl_operate
                        .map(|value| bool_text(Some(value)))
                        .unwrap_or("unknown"),
                );
                field(
                    ui,
                    "State mismatch",
                    status
                        .flex_injection
                        .state_advertisement_mismatch
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Amp profile",
                    status
                        .flex_injection
                        .active_amplifier_status_profile
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "TGXL profile",
                    status
                        .flex_injection
                        .active_tgxl_control_profile
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Degraded reason",
                    status
                        .flex_injection
                        .degraded_reason
                        .as_deref()
                        .unwrap_or("-"),
                );
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
                    "Amplifier",
                    status
                        .flex_injection
                        .amplifier_handle
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Interlock",
                    status
                        .flex_injection
                        .interlock_handle
                        .as_deref()
                        .unwrap_or("-"),
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
                    "Sessions seen",
                    format!(
                        "PGXL {} / TGXL {}",
                        status.clients.pgxl_session_started_count,
                        status.clients.tgxl_session_started_count
                    ),
                );
                field(
                    ui,
                    "Last TGXL close",
                    status
                        .clients
                        .tgxl_last_disconnect_reason
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Ping ok/fail",
                    format!(
                        "{}/{}",
                        status.flex_injection.ping_count, status.flex_injection.ping_failure_count
                    ),
                );
                field(
                    ui,
                    "SmartSDR tuner",
                    format!(
                        "appeared {} / disappeared {}",
                        status.flex_diagnostics.smartsdr_tuner_appeared_count,
                        status.flex_diagnostics.smartsdr_tuner_disappeared_count
                    ),
                );
                field(
                    ui,
                    "Tuner disappear",
                    status
                        .flex_diagnostics
                        .smartsdr_tuner_last_disappearance_reason
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Flex tuner age",
                    format_ms_age(status.flex_diagnostics.flex_tuner_presence_age_ms),
                );
                field(
                    ui,
                    "Pending/expired",
                    format!(
                        "{}/{}",
                        status.flex_injection.pending_count,
                        status.flex_injection.expired_pending_count
                    ),
                );
                field(
                    ui,
                    "PGXL expected direct",
                    status
                        .flex_diagnostics
                        .amplifier_direct_connect_expected
                        .map(|value| bool_text(Some(value)))
                        .unwrap_or("unknown"),
                );
                field(
                    ui,
                    "PGXL direct attempted",
                    bool_text(Some(
                        status
                            .flex_injection
                            .amplifier_pgxl_tcp_attempted_after_status,
                    )),
                );
                field(
                    ui,
                    "Flex operate lab",
                    format!(
                        "sent={} accepted={}",
                        status.flex_injection.flex_operate_lab_command_count,
                        status.flex_injection.flex_operate_lab_accept_count
                    ),
                );
                field(
                    ui,
                    "PGXL connect-assist",
                    format!(
                        "enabled={} sent={} result={} tcp={}",
                        bool_text(Some(status.flex_injection.pgxl_connect_assist_enabled)),
                        status.flex_injection.pgxl_connect_assist_sent_count,
                        status
                            .flex_injection
                            .pgxl_connect_assist_last_result
                            .as_deref()
                            .unwrap_or("-"),
                        bool_text(Some(
                            status.flex_injection.pgxl_connect_assist_triggered_tcp
                        ))
                    ),
                );
                field(
                    ui,
                    "PGXL no socket",
                    format!(
                        "{} {}",
                        status.clients.pgxl_manual_connect_no_socket_attempt_count,
                        status
                            .clients
                            .pgxl_last_no_socket_attempt_warning
                            .as_deref()
                            .unwrap_or("")
                    ),
                );
                field(
                    ui,
                    "Last amplifier ad",
                    status
                        .flex_injection
                        .last_amplifier_status_line
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(ui, "State lag", format_ms_age(status.amp.stale_duration_ms));
                field(
                    ui,
                    "Button command seen",
                    bool_text(Some(status.controls.aethersdr_button_command_seen)),
                );
                field(
                    ui,
                    "Last TGXL control",
                    status
                        .controls
                        .last_tgxl_control_command
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Last PGXL control",
                    status
                        .controls
                        .last_pgxl_control_command
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Last Flex amp set",
                    status
                        .controls
                        .last_flex_amp_set_command
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Safety blocks",
                    format!(
                        "dry_run {} / RF {}",
                        status.controls.blocked_by_dry_run_count,
                        status.controls.blocked_by_rf_risk_count
                    ),
                );
                field(
                    ui,
                    "Meters",
                    status
                        .flex_injection
                        .meter_handles
                        .iter()
                        .map(|meter| meter.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            } else {
                ui.label("Start the bridge and enable metrics for live client state.");
            }
        });
    }

    fn ui_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Guarded Controls");
        ui.colored_label(
            egui::Color32::YELLOW,
            "Controls show the exact command path. Risky actions remain off unless explicitly enabled for this GUI session.",
        );
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.controls.kpa_safe, "Enable KPA safe controls");
            ui.checkbox(
                &mut self.controls.kpa_rf_risk,
                "Enable KPA RF-risk controls",
            );
            ui.checkbox(&mut self.controls.kat_safe, "Enable KAT safe controls");
            ui.checkbox(
                &mut self.controls.kat_rf_risk,
                "Enable KAT tune/RF-risk controls",
            );
            ui.checkbox(
                &mut self.controls.kat_antenna,
                "Enable KAT antenna switching",
            );
            ui.checkbox(
                &mut self.controls.advanced,
                "Enable destructive/advanced actions",
            );
            ui.checkbox(
                &mut self.controls.remember_rf_confirm,
                "Remember RF confirmation this session",
            );
        });
        ui.separator();
        ui.columns(2, |columns| {
            columns[0].group(|ui| {
                ui.heading("KPA500");
                ui.monospace("Standby: Flex amplifier set operate=0 -> KPA500 ^OS0;");
                if ui
                    .add_enabled(self.controls.kpa_safe, egui::Button::new("Send Standby"))
                    .clicked()
                {
                    self.run_egb_command(
                        "kpa-standby",
                        vec![
                            "test-kpa".into(),
                            "--config".into(),
                            self.config_path.display().to_string(),
                            "--allow-control".into(),
                        ],
                    );
                }
                ui.monospace("Operate: Flex amplifier set operate=1 -> KPA500 ^OS1;");
                let operate_enabled = self.controls.kpa_rf_risk;
                if ui
                    .add_enabled(
                        operate_enabled,
                        egui::Button::new("Operate Then Fail-Safe Standby"),
                    )
                    .clicked()
                {
                    if !self.controls.remember_rf_confirm && !self.rf_acknowledged {
                        self.rf_acknowledged = true;
                        self.push_log(
                            "RF-risk operate armed for one click; press Operate again to execute",
                        );
                    } else {
                        self.run_egb_command(
                            "kpa-operate",
                            vec![
                                "test-kpa-operate".into(),
                                "--config".into(),
                                self.config_path.display().to_string(),
                                "--allow-rf-risk".into(),
                                "--confirm-rf-risk".into(),
                                "I understand".into(),
                            ],
                        );
                        if !self.controls.remember_rf_confirm {
                            self.rf_acknowledged = false;
                        }
                    }
                }
                ui.add_enabled(false, egui::Button::new("Clear Fault (^FLC;)"));
            });
            columns[1].group(|ui| {
                ui.heading("KAT500");
                ui.monospace("Tune: TGXL autotune -> KAT500 tune command path");
                ui.add_enabled(
                    self.controls.kat_rf_risk,
                    egui::Button::new("Tune (not wired yet)"),
                );
                ui.monospace("Bypass: BYP control command, disabled until validated");
                ui.add_enabled(self.controls.kat_safe, egui::Button::new("Bypass On/Off"));
                ui.monospace("Antenna: AN1;/AN2;/AN3;");
                ui.horizontal(|ui| {
                    ui.add_enabled(self.controls.kat_antenna, egui::Button::new("ANT 1"));
                    ui.add_enabled(self.controls.kat_antenna, egui::Button::new("ANT 2"));
                    ui.add_enabled(self.controls.kat_antenna, egui::Button::new("ANT 3"));
                });
            });
        });
    }

    fn ui_operational(&mut self, ui: &mut egui::Ui) {
        ui.heading("Operational Mode");
        ui.colored_label(
            egui::Color32::YELLOW,
            "Operational mode can send real serial control commands. Keep RF-risk controls off until the station is ready.",
        );
        ui.group(|ui| {
            ui.checkbox(
                &mut self.config.operational.enable_real_controls,
                "Enable Real Hardware Controls",
            );
            ui.checkbox(
                &mut self.config.operational.enable_kat_tune,
                "Allow KAT500 Tune (T;)",
            );
            ui.checkbox(
                &mut self.config.operational.enable_kat_bypass,
                "Allow KAT500 Bypass/Standby (BYPB;/BYPN;)",
            );
            ui.checkbox(
                &mut self.config.operational.enable_kat_antenna,
                "Allow KAT500 Antenna 1/2/3 (AN1;/AN2;/AN3;)",
            );
            ui.checkbox(
                &mut self.config.operational.enable_kpa_standby,
                "Allow KPA500 Standby (^OS0;)",
            );
            ui.checkbox(
                &mut self.config.operational.enable_kpa_operate,
                "Allow KPA500 Operate (^OS1;)",
            );
            ui.checkbox(
                &mut self.config.operational.enable_clear_fault,
                "Allow Clear Fault (^FLC;)",
            );
            ui.checkbox(
                &mut self.config.operational.persist_rf_risk,
                "Persist RF-risk controls across restart",
            );
            text_field(
                ui,
                "Typed confirmation",
                &mut self.config.operational.confirm_real_hardware_control,
            );
            if self.config.operational.enable_kpa_operate
                || self.config.operational.enable_kat_tune
                || self.config.operational.enable_clear_fault
            {
                ui.colored_label(
                    egui::Color32::RED,
                    "RF-risk controls require confirmation text: I understand",
                );
            }
        });
        ui.separator();
        ui.heading("Command Map");
        ui.monospace("KAT500 Tune: T;");
        ui.monospace("KAT500 Bypass: BYPB; / BYPN;");
        ui.monospace("KAT500 Antenna: AN1; / AN2; / AN3;");
        ui.monospace("KPA500 Standby: ^OS0;");
        ui.monospace("KPA500 Operate: ^OS1;");
        ui.monospace("KPA500 Clear Fault: ^FLC;");
        ui.separator();
        if ui.button("Start Full Operational Test").clicked() {
            self.run_egb_command(
                "full-operational-test",
                vec![
                    "full-operational-test".into(),
                    "--config".into(),
                    self.config_path.display().to_string(),
                    "--duration-seconds".into(),
                    "180".into(),
                ],
            );
        }
    }

    fn ui_logs(&mut self, ui: &mut egui::Ui) {
        ui.heading("Logs");
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("Filter")
                .selected_text(self.log_filter.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.log_filter, LogFilter::All, "all");
                    ui.selectable_value(&mut self.log_filter, LogFilter::Error, "error");
                    ui.selectable_value(&mut self.log_filter, LogFilter::Warn, "warn");
                    ui.selectable_value(&mut self.log_filter, LogFilter::Info, "info");
                    ui.selectable_value(&mut self.log_filter, LogFilter::Debug, "debug");
                });
            ui.checkbox(&mut self.logs_paused, "Pause");
            if ui.button("Clear View").clicked() {
                self.diagnostics.clear();
            }
            if ui.button("Export Visible Logs").clicked() {
                match export_visible_logs(&self.diagnostics, self.log_filter) {
                    Ok(path) => {
                        self.push_log(format!("visible logs exported to {}", path.display()))
                    }
                    Err(err) => self.push_log(format!("log export failed: {err}")),
                }
            }
            if ui.button("Open Test Logs Folder").clicked() {
                open_path(Path::new("logs/tests"));
            }
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in self
                .diagnostics
                .iter()
                .filter(|line| self.log_filter.matches(line))
            {
                ui.monospace(line);
            }
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
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.settings.start_bridge_on_launch, "Start bridge when GUI launches");
                ui.checkbox(&mut self.settings.start_minimized_to_tray, "Start minimized to tray");
                ui.checkbox(&mut self.settings.close_to_tray, "Close to tray");
                ui.checkbox(&mut self.settings.redact_diagnostics, "Redact diagnostics export");
            });
            ui.label("Native tray menu is planned; these settings are persisted and documented for service-like operation.");
            ui.separator();
            text_field(ui, "Bind IP", &mut self.config.server.bind_ip);
            port_field(ui, "PGXL port", &mut self.config.pgxl.port);
            port_field(ui, "TGXL port", &mut self.config.tgxl.port);
            checkbox(ui, "PGXL AetherSDR compatibility", &mut self.config.pgxl.aethersdr_compat);
            egui::ComboBox::from_label("PGXL compatibility profile")
                .selected_text(self.config.pgxl.compat_profile.as_str())
                .show_ui(ui, |ui| {
                    for profile in ["strict", "aethersdr", "smartsdr", "permissive"] {
                        ui.selectable_value(
                            &mut self.config.pgxl.compat_profile,
                            profile.to_string(),
                            profile,
                        );
                    }
                });
            checkbox(ui, "PGXL direct connected diagnostic", &mut self.config.pgxl.force_direct_connected_test);
            checkbox(ui, "TGXL AetherSDR compatibility", &mut self.config.tgxl.aethersdr_compat);
            checkbox(ui, "TGXL SmartSDR compatibility", &mut self.config.tgxl.smartsdr_compat);
            egui::ComboBox::from_label("TGXL control profile")
                .selected_text(self.config.tgxl.control_profile.as_str())
                .show_ui(ui, |ui| {
                    for profile in ["readonly", "control_ready", "verbose_control"] {
                        ui.selectable_value(
                            &mut self.config.tgxl.control_profile,
                            profile.to_string(),
                            profile,
                        );
                    }
                });
            checkbox(ui, "TGXL direct presence test", &mut self.config.tgxl.force_presence_test);
            checkbox(ui, "TGXL experimental presence refresh", &mut self.config.tgxl.experimental_presence_refresh);
            checkbox(ui, "Metrics enabled", &mut self.config.metrics.enabled);
            text_field(ui, "Metrics bind IP", &mut self.config.metrics.bind_ip);
            port_field(ui, "Metrics/status port", &mut self.config.metrics.port);

            ui.separator();
            ui.heading("KPA500");
            serial_port_field(ui, "KPA500 COM port", &mut self.config.kpa500.com_port, &self.serial_ports);
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
            serial_port_field(ui, "KAT500 COM port", &mut self.config.kat500.com_port, &self.serial_ports);
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
            egui::ComboBox::from_label("PGXL status profile")
                .selected_text(self.config.flex_injection.amplifier_status_profile.as_str())
                .show_ui(ui, |ui| {
                    for profile in [
                        "minimal",
                        "pgxl_paired",
                        "pgxl_verbose",
                        "old_good_pgxl",
                        "aethersdr_force_direct",
                        "aethersdr_pgxl_direct_lab",
                        "strict_real_pgxl",
                    ] {
                        ui.selectable_value(
                            &mut self.config.flex_injection.amplifier_status_profile,
                            profile.to_string(),
                            profile,
                        );
                    }
                });
            checkbox(ui, "Full PGXL registration", &mut self.config.flex_injection.full_pgxl_registration);
            checkbox(ui, "Create AMP meters", &mut self.config.flex_injection.create_meters);
            checkbox(ui, "Create AMP interlock", &mut self.config.flex_injection.create_interlock);
            checkbox(ui, "Trace amplifier advertisements", &mut self.config.flex_injection.trace_amplifier_advertisements);
            checkbox(ui, "Enable PGXL connect-assist for AetherSDR", &mut self.config.flex_injection.pgxl_connect_assist);
            if self.config.flex_injection.pgxl_connect_assist {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "Connect-assist may mark the Flex-side virtual amplifier active to trigger PGXL connection. It does not switch the real KPA500 to operate.",
                );
            }
            egui::ComboBox::from_label("Amplifier startup policy")
                .selected_text(self.config.flex_injection.amplifier_startup_state_policy.as_str())
                .show_ui(ui, |ui| {
                    for policy in [
                        "wait_for_first_kpa_poll",
                        "advertise_standby_immediately",
                        "advertise_configured_default",
                    ] {
                        ui.selectable_value(
                            &mut self.config.flex_injection.amplifier_startup_state_policy,
                            policy.to_string(),
                            policy,
                        );
                    }
                });
            u64_field(
                ui,
                "First KPA poll timeout ms",
                &mut self.config.flex_injection.wait_first_kpa_poll_timeout_ms,
            );
            u64_field(
                ui,
                "Amplifier reannounce interval ms",
                &mut self.config.flex_injection.amplifier_reannounce_interval_ms,
            );
            u64_field(
                ui,
                "Tuner refresh interval ms",
                &mut self.config.flex_injection.tuner_refresh_interval_ms,
            );

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
                if ui.button("Run PGXL Direct Self-Test").clicked() {
                    self.run_egb_command(
                        "pgxl-direct-selftest",
                        vec![
                            "test-pgxl-direct".into(),
                            "--host".into(),
                            self.config.server.bind_ip.clone(),
                            "--port".into(),
                            self.config.pgxl.port.to_string(),
                        ],
                    );
                }
                if ui.button("Run PGXL Trigger Lab").clicked() {
                    self.run_egb_command(
                        "pgxl-trigger-lab",
                        vec![
                            "pgxl-trigger-lab".into(),
                            "--config".into(),
                            self.config_path.display().to_string(),
                            "--profile".into(),
                            self.config.flex_injection.amplifier_status_profile.clone(),
                            "--duration-minutes".into(),
                            "5".into(),
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
                ui.heading("Bridge");
                field(
                    ui,
                    "Version",
                    status.bridge.version.as_deref().unwrap_or("unknown"),
                );
                field(
                    ui,
                    "Commit",
                    status.bridge.git_commit.as_deref().unwrap_or("unknown"),
                );
                field(
                    ui,
                    "PID",
                    status
                        .bridge
                        .process_id
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                );
                field(
                    ui,
                    "Uptime",
                    status
                        .bridge
                        .uptime_ms
                        .map(|value| format!("{value} ms"))
                        .unwrap_or_else(|| "-".to_string()),
                );
                field(
                    ui,
                    "Config path",
                    status.bridge.config_path.as_deref().unwrap_or("unknown"),
                );
            });
            ui.separator();
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
                ui.heading("Control Path");
                field(
                    ui,
                    "AetherSDR button command seen",
                    bool_text(Some(status.controls.aethersdr_button_command_seen)),
                );
                field(
                    ui,
                    "Requests",
                    status.controls.control_requested_count.to_string(),
                );
                field(
                    ui,
                    "Last TGXL control command",
                    status
                        .controls
                        .last_tgxl_control_command
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Last PGXL control command",
                    status
                        .controls
                        .last_pgxl_control_command
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Last Flex amp set command",
                    status
                        .controls
                        .last_flex_amp_set_command
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Mapped Elecraft action",
                    status
                        .controls
                        .last_mapped_elecraft_action
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Safety decision",
                    status
                        .controls
                        .last_safety_decision
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "Blocked dry_run/RF",
                    format!(
                        "{}/{}",
                        status.controls.blocked_by_dry_run_count,
                        status.controls.blocked_by_rf_risk_count
                    ),
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
                    "PGXL last close",
                    status
                        .clients
                        .pgxl_last_disconnect_reason
                        .as_deref()
                        .unwrap_or("-"),
                );
                field(
                    ui,
                    "TGXL last close",
                    status
                        .clients
                        .tgxl_last_disconnect_reason
                        .as_deref()
                        .unwrap_or("-"),
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
            ui.separator();
            ui.group(|ui| {
                ui.heading("TGXL Sessions");
                for session in &status.clients.tgxl_sessions {
                    field(
                        ui,
                        &format!("{} #{} {}", session.protocol, session.id, session.peer),
                        format!(
                            "cmd={} rsp={} parse={} unknown={} last={} max={} connected_at={}",
                            session.commands_received,
                            session.responses_sent,
                            session.parse_failures,
                            session.unknown_commands,
                            session.last_response_latency_ms,
                            session.max_response_latency_ms,
                            session.connected_at_ms
                        ),
                    );
                    if let Some(command) = &session.last_command {
                        field(ui, "last command", command);
                    }
                }
                if status.clients.tgxl_sessions.is_empty() {
                    ui.label("No active TGXL sessions.");
                }
                ui.heading("PGXL Sessions");
                for session in &status.clients.pgxl_sessions {
                    field(
                        ui,
                        &format!("{} #{} {}", session.protocol, session.id, session.peer),
                        format!(
                            "cmd={} rsp={} parse={} unknown={} last={} max={} connected_at={}",
                            session.commands_received,
                            session.responses_sent,
                            session.parse_failures,
                            session.unknown_commands,
                            session.last_response_latency_ms,
                            session.max_response_latency_ms,
                            session.connected_at_ms
                        ),
                    );
                }
                if status.clients.pgxl_sessions.is_empty() {
                    ui.label("No active PGXL sessions.");
                }
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
    Operational,
    Controls,
    Diagnostics,
    Logs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GuiSettings {
    start_bridge_on_launch: bool,
    start_minimized_to_tray: bool,
    close_to_tray: bool,
    redact_diagnostics: bool,
}

impl Default for GuiSettings {
    fn default() -> Self {
        Self {
            start_bridge_on_launch: false,
            start_minimized_to_tray: false,
            close_to_tray: true,
            redact_diagnostics: true,
        }
    }
}

impl GuiSettings {
    fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        serde_yaml::from_str(&text).context("failed to parse GUI settings")
    }

    fn save(&self, path: &Path) -> Result<()> {
        fs::write(path, serde_yaml::to_string(self)?).context("failed to write GUI settings")
    }
}

#[derive(Debug, Clone, Default)]
struct ControlFlags {
    kpa_safe: bool,
    kpa_rf_risk: bool,
    kat_safe: bool,
    kat_rf_risk: bool,
    kat_antenna: bool,
    advanced: bool,
    remember_rf_confirm: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogFilter {
    All,
    Error,
    Warn,
    Info,
    Debug,
}

impl LogFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    fn matches(self, line: &str) -> bool {
        let lower = line.to_ascii_lowercase();
        match self {
            Self::All => true,
            Self::Error => lower.contains("error"),
            Self::Warn => lower.contains("warn"),
            Self::Info => lower.contains("info"),
            Self::Debug => lower.contains("debug"),
        }
    }
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

impl ProcessState {
    fn label(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Degraded => "degraded",
            Self::Error => "error",
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Self::Stopped => egui::Color32::GRAY,
            Self::Starting => egui::Color32::YELLOW,
            Self::Running => egui::Color32::GREEN,
            Self::Degraded => egui::Color32::YELLOW,
            Self::Error => egui::Color32::RED,
        }
    }
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

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StatusSnapshot {
    #[serde(default)]
    bridge: BridgeStatus,
    amp: DeviceStatus,
    tuner: DeviceStatus,
    clients: ClientStatus,
    flex_injection: FlexStatus,
    #[serde(default)]
    controls: ControlStatus,
    #[serde(default)]
    flex_diagnostics: FlexDiagnostics,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct BridgeStatus {
    version: Option<String>,
    git_commit: Option<String>,
    process_id: Option<u32>,
    uptime_ms: Option<u128>,
    config_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    #[serde(default)]
    first_poll_completed: Option<bool>,
    #[serde(default)]
    first_poll_error: Option<String>,
    #[serde(default)]
    serial_port_open_error: Option<String>,
    #[serde(default)]
    last_raw_response: Option<String>,
    #[serde(default)]
    last_successful_command: Option<String>,
    #[serde(default)]
    startup_state_policy: Option<String>,
    #[serde(default)]
    advertisement_waiting_for_first_poll: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RuntimeStatus {
    reconnect_count: u64,
    poll_success_count: u64,
    poll_failure_count: u64,
    stale_transition_count: u64,
    last_poll_latency_ms: u64,
    max_poll_latency_ms: u64,
    average_poll_latency_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ClientStatus {
    pgxl_connected: bool,
    tgxl_connected: bool,
    pgxl_client_count: usize,
    tgxl_client_count: usize,
    #[serde(default)]
    pgxl_session_started_count: u64,
    #[serde(default)]
    tgxl_session_started_count: u64,
    #[serde(default)]
    pgxl_sessions: Vec<ClientSession>,
    #[serde(default)]
    tgxl_sessions: Vec<ClientSession>,
    #[serde(default)]
    pgxl_last_disconnect_reason: Option<String>,
    #[serde(default)]
    tgxl_last_disconnect_reason: Option<String>,
    #[serde(default)]
    pgxl_manual_connect_no_socket_attempt_count: u64,
    #[serde(default)]
    pgxl_last_no_socket_attempt_warning: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ClientSession {
    id: u64,
    protocol: String,
    peer: String,
    connected_at_ms: u128,
    last_command: Option<String>,
    commands_received: u64,
    responses_sent: u64,
    parse_failures: u64,
    unknown_commands: u64,
    last_response_latency_ms: u64,
    max_response_latency_ms: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct FlexStatus {
    enabled: bool,
    connection_state: String,
    #[serde(default)]
    active_amplifier_status_profile: Option<String>,
    #[serde(default)]
    active_tgxl_control_profile: Option<String>,
    client_handle: Option<String>,
    amplifier_handle: Option<String>,
    meter_handles: Vec<MeterHandle>,
    interlock_handle: Option<String>,
    last_command: Option<String>,
    last_response: Option<String>,
    command_success_count: u64,
    command_failure_count: u64,
    ping_count: u64,
    #[serde(default)]
    ping_failure_count: u64,
    #[serde(default)]
    pending_count: usize,
    #[serde(default)]
    expired_pending_count: u64,
    #[serde(default)]
    degraded_reason: Option<String>,
    #[serde(default)]
    tuner_handle: Option<String>,
    #[serde(default)]
    tuner_appeared_count: u64,
    #[serde(default)]
    tuner_disappeared_count: u64,
    #[serde(default)]
    last_tuner_disappearance_reason: Option<String>,
    #[serde(default)]
    amplifier_pgxl_tcp_attempted_after_status: bool,
    #[serde(default)]
    last_amplifier_status_line: Option<String>,
    #[serde(default)]
    last_advertised_flex_amp_state: Option<String>,
    #[serde(default)]
    last_advertised_pgxl_state: Option<String>,
    #[serde(default)]
    last_advertised_tgxl_operate: Option<bool>,
    #[serde(default)]
    state_advertisement_mismatch: Option<String>,
    #[serde(default)]
    state_advertisement_mismatch_count: u64,
    #[serde(default)]
    flex_desired_amp_state: Option<String>,
    #[serde(default)]
    radio_rewritten_amp_state: Option<String>,
    #[serde(default)]
    flex_force_operate_via_radio: bool,
    #[serde(default)]
    flex_operate_lab_command_count: u64,
    #[serde(default)]
    flex_operate_lab_accept_count: u64,
    #[serde(default)]
    pgxl_connect_assist_enabled: bool,
    #[serde(default)]
    pgxl_connect_assist_sent_count: u64,
    #[serde(default)]
    pgxl_connect_assist_last_result: Option<String>,
    #[serde(default)]
    pgxl_connect_assist_triggered_tcp: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct FlexDiagnostics {
    #[serde(default)]
    ping_count: u64,
    #[serde(default)]
    ping_failures: u64,
    #[serde(default)]
    pending_count: usize,
    #[serde(default)]
    expired_pending_count: u64,
    #[serde(default)]
    degraded_reason: Option<String>,
    #[serde(default)]
    smartsdr_tuner_appeared_count: u64,
    #[serde(default)]
    smartsdr_tuner_disappeared_count: u64,
    #[serde(default)]
    smartsdr_tuner_last_disappearance_reason: Option<String>,
    #[serde(default)]
    flex_tuner_presence_age_ms: Option<u128>,
    #[serde(default)]
    amplifier_direct_connect_expected: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct ControlStatus {
    #[serde(default)]
    aethersdr_button_command_seen: bool,
    #[serde(default)]
    last_tgxl_control_command: Option<String>,
    #[serde(default)]
    last_pgxl_control_command: Option<String>,
    #[serde(default)]
    last_flex_amp_set_command: Option<String>,
    #[serde(default)]
    last_mapped_elecraft_action: Option<String>,
    #[serde(default)]
    last_safety_decision: Option<String>,
    #[serde(default)]
    blocked_by_dry_run_count: u64,
    #[serde(default)]
    blocked_by_rf_risk_count: u64,
    #[serde(default)]
    control_requested_count: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

fn export_diagnostics_bundle(
    config_path: &Path,
    status: Option<&StatusSnapshot>,
    logs: &VecDeque<String>,
    settings: &GuiSettings,
    redact: bool,
) -> Result<PathBuf> {
    let dir = PathBuf::from("diagnostics");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("egb-diagnostics-{}.zip", timestamp_filename()));
    let file = fs::File::create(&path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let config_text = fs::read_to_string(config_path).unwrap_or_default();
    zip_text(
        &mut zip,
        options,
        "config.yaml",
        if redact {
            redact_text(&config_text)
        } else {
            config_text
        },
    )?;
    zip_text(
        &mut zip,
        options,
        "gui-settings.yaml",
        serde_yaml::to_string(settings)?,
    )?;
    zip_text(
        &mut zip,
        options,
        "gui-visible-logs.txt",
        logs.iter().cloned().collect::<Vec<_>>().join("\n"),
    )?;
    if let Some(status) = status {
        zip_text(
            &mut zip,
            options,
            "status.json",
            serde_json::to_string_pretty(status)?,
        )?;
    }
    zip_text(&mut zip, options, "windows-info.txt", windows_info())?;
    zip_text(
        &mut zip,
        options,
        "serial-ports.txt",
        tokio_serial::available_ports()
            .map(|ports| {
                ports
                    .into_iter()
                    .map(|port| port.port_name)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|err| format!("serial port scan failed: {err}")),
    )?;
    add_dir_to_zip(&mut zip, options, Path::new("logs/protocol"), "protocol")?;
    add_dir_to_zip(&mut zip, options, Path::new("logs/serial"), "serial")?;
    add_dir_to_zip(&mut zip, options, Path::new("logs/tests"), "tests")?;
    add_dir_to_zip(&mut zip, options, Path::new("logs"), "logs-root")?;
    add_dir_to_zip(&mut zip, options, Path::new("diagnostics"), "diagnostics")?;
    zip.finish()?;
    Ok(path)
}

fn zip_text<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::FileOptions,
    name: &str,
    text: String,
) -> Result<()> {
    zip.start_file(name, options)?;
    zip.write_all(text.as_bytes())?;
    Ok(())
}

fn add_dir_to_zip<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::FileOptions,
    dir: &Path,
    prefix: &str,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            if entry.path().extension().is_some_and(|ext| ext == "zip") {
                continue;
            }
            let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
            zip.start_file(name, options)?;
            zip.write_all(&fs::read(entry.path())?)?;
        }
    }
    Ok(())
}

fn redact_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("radio_ip:")
                || trimmed.starts_with("amplifier_ip:")
                || trimmed.starts_with("bind_ip:")
                || trimmed.starts_with("token:")
            {
                let indent = &line[..line.len() - trimmed.len()];
                format!(
                    "{indent}{}: <redacted>",
                    trimmed.split(':').next().unwrap_or("value")
                )
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn windows_info() -> String {
    #[cfg(windows)]
    {
        run_command_text("cmd", &["/C", "ver"])
    }
    #[cfg(not(windows))]
    {
        std::env::consts::OS.to_string()
    }
}

fn run_command_text(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
        .unwrap_or_else(|err| format!("failed to run {program}: {err}"))
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

fn run_egb_capture_logged(label: &str, args: &[String]) -> Result<String> {
    let started = Instant::now();
    let started_stamp = timestamp_filename();
    let result = run_egb_capture(args);
    fs::create_dir_all("logs/tests")?;
    let path = PathBuf::from("logs/tests").join(format!("{started_stamp}-{label}.log"));
    let mut body = String::new();
    body.push_str(&format!("test={label}\n"));
    body.push_str(&format!("timestamp={started_stamp}\n"));
    body.push_str(&format!("duration_ms={}\n", started.elapsed().as_millis()));
    body.push_str(&format!("command=egb {}\n\n", args.join(" ")));
    match &result {
        Ok(output) => {
            body.push_str("exit=0\n\n");
            body.push_str(output);
        }
        Err(err) => {
            body.push_str("exit=error\n\n");
            body.push_str(&err.to_string());
        }
    }
    fs::write(path, body)?;
    result
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

fn export_visible_logs(logs: &VecDeque<String>, filter: LogFilter) -> Result<PathBuf> {
    fs::create_dir_all("logs")?;
    let path = PathBuf::from("logs").join(format!("egb-visible-logs-{}.txt", timestamp_filename()));
    let body = logs
        .iter()
        .filter(|line| filter.matches(line))
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, body)?;
    Ok(path)
}

fn extract_evidence_bundle_path(output: &str) -> Option<PathBuf> {
    output.lines().find_map(|line| {
        line.strip_prefix("evidence bundle:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    })
}

fn latest_evidence_bundle() -> Option<PathBuf> {
    let dir = Path::new("diagnostics/runs");
    let entries = fs::read_dir(dir).ok()?;
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "zip"))
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn apply_modern_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(10, 16, 28);
    visuals.window_fill = egui::Color32::from_rgb(16, 24, 39);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(31, 45, 70);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(42, 77, 122);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(56, 112, 180);
    visuals.selection.bg_fill = egui::Color32::from_rgb(43, 124, 220);
    ctx.set_visuals(visuals);
}

fn nav_button(ui: &mut egui::Ui, tab: &mut Tab, candidate: Tab, label: &str) {
    let selected = *tab == candidate;
    if ui
        .add_sized([160.0, 32.0], egui::SelectableLabel::new(selected, label))
        .clicked()
    {
        *tab = candidate;
    }
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

fn status_color(ok: bool) -> egui::Color32 {
    if ok {
        egui::Color32::from_rgb(72, 210, 135)
    } else {
        egui::Color32::from_rgb(225, 96, 96)
    }
}

fn connection_color(state: &str) -> egui::Color32 {
    match state {
        "connected" => egui::Color32::from_rgb(72, 210, 135),
        "connecting" => egui::Color32::YELLOW,
        "degraded" => egui::Color32::YELLOW,
        "error" => egui::Color32::RED,
        _ => egui::Color32::GRAY,
    }
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

fn serial_port_field(ui: &mut egui::Ui, label: &str, value: &mut String, ports: &[String]) {
    ui.horizontal(|ui| {
        ui.label(label);
        if ports.is_empty() {
            ui.text_edit_singleline(value);
        } else {
            egui::ComboBox::from_id_source(label)
                .selected_text(value.as_str())
                .show_ui(ui, |ui| {
                    for port in ports {
                        ui.selectable_value(value, port.clone(), port);
                    }
                });
        }
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

fn u64_field(ui: &mut egui::Ui, label: &str, value: &mut u64) {
    let mut text = value.to_string();
    ui.horizontal(|ui| {
        ui.label(label);
        if ui.text_edit_singleline(&mut text).lost_focus() {
            if let Ok(parsed) = text.parse::<u64>() {
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

fn timestamp_filename() -> String {
    timestamp_compact()
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
          "bridge": {"version": "0.1.0", "git_commit": "test", "process_id": 42, "uptime_ms": 100, "config_path": "config.yaml"},
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
        assert_eq!(status.bridge.process_id, Some(42));
    }

    #[test]
    fn diagnostics_redaction_masks_local_values() {
        let text = "bind_ip: 192.168.0.10\nradio_ip: 192.168.0.199\namplifier_ip: 192.168.0.189\ntoken: secret\n";
        let redacted = redact_text(text);
        assert!(redacted.contains("bind_ip: <redacted>"));
        assert!(redacted.contains("radio_ip: <redacted>"));
        assert!(!redacted.contains("192.168.0.199"));
    }

    #[test]
    fn gui_settings_round_trip() {
        let settings = GuiSettings {
            start_bridge_on_launch: true,
            start_minimized_to_tray: true,
            close_to_tray: true,
            redact_diagnostics: true,
        };
        let text = serde_yaml::to_string(&settings).unwrap();
        let parsed: GuiSettings = serde_yaml::from_str(&text).unwrap();
        assert!(parsed.start_bridge_on_launch);
        assert!(parsed.redact_diagnostics);
    }

    #[test]
    fn diagnostics_bundle_is_created() {
        let mut logs = VecDeque::new();
        logs.push_back("INFO test diagnostic line".to_string());
        let path = export_diagnostics_bundle(
            Path::new("does-not-exist-test-config.yaml"),
            None,
            &logs,
            &GuiSettings::default(),
            true,
        )
        .unwrap();
        assert!(path.exists());
        let _ = fs::remove_file(path);
    }
}
