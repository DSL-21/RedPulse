use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use hdrhistogram::Histogram;

use crate::{
    engine::{self, EngineEvent, Sample, TestConfig, TestHandle},
    report,
};

#[derive(PartialEq, Eq)]
enum Tab {
    Tester,
    Results,
    About,
}

pub struct RedPulseApp {
    tab: Tab,
    target: String,
    method: String,
    workers: usize,
    duration: u64,
    rps: u32,
    timeout: u64,
    body: String,
    running: Option<TestHandle>,
    samples: Vec<Sample>,
    recent_latencies: VecDeque<(f64, f64)>,
    started: Option<Instant>,
    elapsed_seconds: f64,
    status: String,
    error: Option<String>,
    last_export: Option<String>,
}

impl RedPulseApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.window_rounding = egui::CornerRadius::same(10);
        cc.egui_ctx.set_visuals(visuals);

        Self {
            tab: Tab::Tester,
            target: "https://example.com/".into(),
            method: "GET".into(),
            workers: 5,
            duration: 30,
            rps: 5,
            timeout: 10,
            body: r#"{ "ping": "redpulse" }"#.into(),
            running: None,
            samples: Vec::new(),
            recent_latencies: VecDeque::new(),
            started: None,
            elapsed_seconds: 0.0,
            status: "Ready".into(),
            error: None,
            last_export: None,
        }
    }

    fn start_test(&mut self) {
        self.samples.clear();
        self.recent_latencies.clear();
        self.elapsed_seconds = 0.0;
        self.error = None;
        self.last_export = None;

        let config = TestConfig {
            url: self.target.trim().to_string(),
            method: self.method.clone(),
            workers: self.workers,
            duration_secs: self.duration,
            max_requests_per_second: self.rps,
            timeout_secs: self.timeout,
            body: self.body.clone(),
        };

        match engine::start(config) {
            Ok(handle) => {
                self.running = Some(handle);
                self.started = Some(Instant::now());
                self.status = "Running".into();
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn stop_test(&mut self) {
        if let Some(handle) = &self.running {
            handle.stop();
            self.status = "Stopping…".into();
        }
    }

    fn poll_engine(&mut self) {
        let mut finished = false;
        if let Some(handle) = &self.running {
            while let Ok(event) = handle.rx.try_recv() {
                match event {
                    EngineEvent::Started => self.status = "Running".into(),
                    EngineEvent::Sample(sample) => {
                        self.recent_latencies.push_back((
                            sample.elapsed_ms as f64 / 1000.0,
                            sample.latency_ms,
                        ));
                        while self.recent_latencies.len() > 600 {
                            self.recent_latencies.pop_front();
                        }
                        self.samples.push(sample);
                    }
                    EngineEvent::Finished { stopped } => {
                        self.status = if stopped { "Stopped".into() } else { "Completed".into() };
                        finished = true;
                    }
                    EngineEvent::Fatal(message) => {
                        self.error = Some(message);
                        self.status = "Failed".into();
                        finished = true;
                    }
                }
            }
        }

        if finished {
            if let Some(mut handle) = self.running.take() {
                handle.join();
            }
            self.started = None;
        }

        if let Some(started) = self.started {
            self.elapsed_seconds = started.elapsed().as_secs_f64();
        }
    }

    fn metrics(&self) -> (usize, usize, usize, usize, f64, f64, f64, f64) {
        let total = self.samples.len();
        let ok = self.samples.iter().filter(|sample| {
            (200..400).contains(&sample.status)
        }).count();
        let client = self.samples.iter().filter(|sample| {
            (400..500).contains(&sample.status)
        }).count();
        let other = self.samples.iter().filter(|sample| {
            sample.status >= 500 || sample.status == 0
        }).count();

        let avg = if total > 0 {
            self.samples.iter().map(|sample| sample.latency_ms).sum::<f64>() / total as f64
        } else {
            0.0
        };

        let mut histogram = Histogram::<u64>::new(3).unwrap();
        for sample in &self.samples {
            let micros = (sample.latency_ms * 1000.0).max(1.0) as u64;
            let _ = histogram.record(micros.min(60_000_000));
        }
        let percentile = |p: f64| histogram.value_at_quantile(p) as f64 / 1000.0;

        (total, ok, client, other, avg, percentile(0.50), percentile(0.95), percentile(0.99))
    }

    fn export(&mut self, csv: bool) {
        let path = PathBuf::from(if csv { "redpulse-report.csv" } else { "redpulse-report.json" });
        let result = if csv {
            report::export_csv(&path, &self.samples)
        } else {
            report::export_json(&path, &self.target, &self.method, self.workers, &self.samples)
        };
        match result {
            Ok(()) => self.last_export = Some(format!("Saved {}", path.display())),
            Err(error) => self.error = Some(format!("Export failed: {error}")),
        }
    }

    fn metric_card(ui: &mut egui::Ui, label: &str, value: impl ToString, unit: &str) {
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(27, 34, 46))
            .corner_radius(10)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.label(egui::RichText::new(label).small().color(egui::Color32::GRAY));
                ui.horizontal(|ui| {
                    ui.heading(egui::RichText::new(value.to_string()).size(25).strong());
                    ui.label(egui::RichText::new(unit).small().color(egui::Color32::GRAY));
                });
            });
    }

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(egui::RichText::new("◉ REDPULSE")
                .size(22)
                .strong()
                .color(egui::Color32::from_rgb(93, 180, 255)));
            ui.label(egui::RichText::new("HTTP BENCHMARK WORKBENCH")
                .small()
                .color(egui::Color32::GRAY));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let color = if self.running.is_some() {
                    egui::Color32::from_rgb(70, 200, 130)
                } else {
                    egui::Color32::GRAY
                };
                ui.label(egui::RichText::new(format!("● {}", self.status.to_uppercase()))
                    .color(color)
                    .strong());
            });
        });
        ui.add_space(10.0);
        ui.separator();
    }

    fn tester_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Test configuration");
        ui.add_space(8.0);
        ui.label("Target URL");
        ui.add_enabled(
            self.running.is_none(),
            egui::TextEdit::singleline(&mut self.target)
                .hint_text("https://your-lab.example")
                .desired_width(f32::INFINITY),
        );

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label("Method");
                egui::ComboBox::from_id_salt("method")
                    .selected_text(&self.method)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.method, "GET".into(), "GET");
                        ui.selectable_value(&mut self.method, "POST".into(), "POST");
                    });
            });

            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label("Workers");
                ui.add_enabled(self.running.is_none(), egui::Slider::new(&mut self.workers, 1..=64));
            });

            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label("Duration");
                ui.add_enabled(self.running.is_none(), egui::Slider::new(&mut self.duration, 1..=300).suffix(" s"));
            });

            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label("Request rate");
                ui.add_enabled(self.running.is_none(), egui::Slider::new(&mut self.rps, 1..=200).suffix(" req/s"));
            });

            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label("Timeout");
                ui.add_enabled(self.running.is_none(), egui::Slider::new(&mut self.timeout, 1..=60).suffix(" s"));
            });
        });

        if self.method == "POST" {
            ui.add_space(8.0);
            ui.label("JSON request body");
            ui.add_enabled(
                self.running.is_none(),
                egui::TextEdit::multiline(&mut self.body)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let can_start = self.running.is_none() && !self.target.trim().is_empty();
            if ui.add_enabled(can_start, egui::Button::new("▶  Start test")
                .fill(egui::Color32::from_rgb(25, 135, 90))).clicked() {
                self.start_test();
            }
            if ui.add_enabled(self.running.is_some(), egui::Button::new("■  Stop")
                .fill(egui::Color32::from_rgb(160, 55, 65))).clicked() {
                self.stop_test();
            }
            if ui.button("Clear results").clicked() && self.running.is_none() {
                self.samples.clear();
                self.recent_latencies.clear();
            }
        });

        if let Some(error) = &self.error {
            ui.add_space(8.0);
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }

        ui.add_space(14.0);
        ui.separator();
        ui.heading("Live metrics");
        let (total, ok, client, other, avg, p50, p95, p99) = self.metrics();
        let rps = total as f64 / self.elapsed_seconds.max(0.001);

        egui::Grid::new("metrics_grid")
            .num_columns(4)
            .spacing([10.0, 10.0])
            .show(ui, |ui| {
                Self::metric_card(ui, "TOTAL REQUESTS", total, "");
                Self::metric_card(ui, "REQUESTS / SEC", format!("{rps:.2}"), "RPS");
                Self::metric_card(ui, "SUCCESS", ok, "");
                Self::metric_card(ui, "4xx", client, "");
                ui.end_row();

                Self::metric_card(ui, "5xx / NETWORK ERRORS", other, "");
                Self::metric_card(ui, "AVG LATENCY", format!("{avg:.1}"), "ms");
                Self::metric_card(ui, "P50 LATENCY", format!("{p50:.1}"), "ms");
                Self::metric_card(ui, "P95 / P99", format!("{p95:.1} / {p99:.1}"), "ms");
                ui.end_row();
            });

        ui.add_space(12.0);
        ui.heading("Response time");
        let points: PlotPoints = self.recent_latencies
            .iter()
            .map(|(x, y)| [*x, *y])
            .collect();

        Plot::new("latency_plot")
            .height(190.0)
            .allow_zoom(true)
            .allow_drag(true)
            .show(ui, |plot_ui| plot_ui.line(Line::new("Latency (ms)", points)));

        ui.horizontal(|ui| {
            ui.label(format!("Elapsed: {:.1}s", self.elapsed_seconds));
            ui.label(format!("Visible samples: {}", self.recent_latencies.len()));
        });
    }

    fn results_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Test results");
        ui.horizontal(|ui| {
            if ui.button("Export JSON").clicked() {
                self.export(false);
            }
            if ui.button("Export CSV").clicked() {
                self.export(true);
            }
            if let Some(message) = &self.last_export {
                ui.label(message);
            }
        });

        ui.add_space(8.0);
        let (total, ok, client, other, avg, p50, p95, p99) = self.metrics();
        ui.label(format!("{total} requests · {ok} successful · {client} client errors · {other} server/network errors"));
        ui.label(format!("Average {avg:.2} ms · P50 {p50:.2} ms · P95 {p95:.2} ms · P99 {p99:.2} ms"));
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("sample_table").striped(true).show(ui, |ui| {
                for heading in ["Elapsed", "Status", "Latency", "Bytes", "Result"] {
                    ui.strong(heading);
                }
                ui.end_row();

                for sample in self.samples.iter().rev().take(500) {
                    ui.label(format!("{:.2}s", sample.elapsed_ms as f64 / 1000.0));
                    let color = if sample.status >= 500 || sample.status == 0 {
                        egui::Color32::LIGHT_RED
                    } else if sample.status >= 400 {
                        egui::Color32::YELLOW
                    } else {
                        egui::Color32::LIGHT_GREEN
                    };
                    ui.colored_label(
                        color,
                        if sample.status == 0 { "ERR".into() } else { sample.status.to_string() },
                    );
                    ui.label(format!("{:.1} ms", sample.latency_ms));
                    ui.label(sample.bytes.to_string());
                    ui.label(sample.error.as_deref().unwrap_or(if sample.ok { "OK" } else { "HTTP error" }));
                    ui.end_row();
                }
            });
        });
    }

    fn about_ui(&self, ui: &mut egui::Ui) {
        ui.heading("About RedPulse");
        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        ui.label("A native Rust desktop HTTP benchmarking workbench.");
        ui.add_space(8.0);
        ui.label("Engine: Tokio + Reqwest + Rustls");
        ui.label("Interface: egui / eframe");
        ui.label("Metrics: response status, throughput and latency percentiles");
        ui.label("Reports: JSON and CSV");
        ui.separator();
        ui.label("The configured request-rate ceiling is global across all workers.");
        ui.label("Redirect following is disabled; each sample measures the configured endpoint response.");
    }
}

impl eframe::App for RedPulseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_engine();

        if self.running.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        egui::TopBottomPanel::top("topbar").show(ctx, |ui| self.header(ui));

        egui::SidePanel::left("navigation")
            .resizable(false)
            .default_width(175.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                if ui.selectable_label(self.tab == Tab::Tester, "◉  Tester").clicked() {
                    self.tab = Tab::Tester;
                }
                if ui.selectable_label(self.tab == Tab::Results, "▤  Results").clicked() {
                    self.tab = Tab::Results;
                }
                if ui.selectable_label(self.tab == Tab::About, "ⓘ  About").clicked() {
                    self.tab = Tab::About;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.separator();
                    ui.label(egui::RichText::new("REDPULSE 0.1.0").small().color(egui::Color32::GRAY));
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                Tab::Tester => self.tester_ui(ui),
                Tab::Results => self.results_ui(ui),
                Tab::About => self.about_ui(ui),
            });
        });
    }
}
