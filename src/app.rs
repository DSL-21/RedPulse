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

const MAX_UI_SAMPLES: usize = 20_000;

#[derive(Clone)]
struct RunSnapshot {
    target: String,
    method: String,
    workers: usize,
    duration_secs: u64,
    request_rate: u32,
    timeout_secs: u64,
    actual_elapsed_secs: f64,
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
    samples: VecDeque<Sample>,
    recent_latencies: VecDeque<(f64, f64)>,
    started: Option<Instant>,
    elapsed_seconds: f64,
    status: String,
    error: Option<String>,
    last_export: Option<String>,
    total_received: u64,
    total_success: u64,
    total_client_errors: u64,
    total_server_errors: u64,
    total_network_errors: u64,
    http_response_count: u64,
    latency_sum_ms: f64,
    latency_min_ms: Option<f64>,
    latency_max_ms: Option<f64>,
    histogram: Histogram<u64>,
    last_run: Option<RunSnapshot>,
    plot_view_revision: u64,
}

impl RedPulseApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(17, 22, 31);
        visuals.window_fill = egui::Color32::from_rgb(21, 27, 38);
        visuals.extreme_bg_color = egui::Color32::from_rgb(10, 14, 21);
        visuals.faint_bg_color = egui::Color32::from_rgb(29, 37, 51);
        visuals.override_text_color = Some(egui::Color32::from_rgb(226, 233, 243));
        visuals.selection.bg_fill = egui::Color32::from_rgb(34, 125, 190);
        visuals.hyperlink_color = egui::Color32::from_rgb(102, 190, 255);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(24, 31, 43);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(37, 47, 63);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(47, 65, 87);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(35, 119, 174);
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 9.0);
            style.spacing.button_padding = egui::vec2(13.0, 8.0);
        });

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
            samples: VecDeque::new(),
            recent_latencies: VecDeque::new(),
            started: None,
            elapsed_seconds: 0.0,
            status: "Ready".into(),
            error: None,
            last_export: None,
            total_received: 0,
            total_success: 0,
            total_client_errors: 0,
            total_server_errors: 0,
            total_network_errors: 0,
            http_response_count: 0,
            latency_sum_ms: 0.0,
            latency_min_ms: None,
            latency_max_ms: None,
            histogram: Histogram::<u64>::new(3).expect("valid histogram configuration"),
            last_run: None,
            plot_view_revision: 0,
        }
    }

    fn reset_results(&mut self) {
        self.samples.clear();
        self.recent_latencies.clear();
        self.total_received = 0;
        self.total_success = 0;
        self.total_client_errors = 0;
        self.total_server_errors = 0;
        self.total_network_errors = 0;
        self.http_response_count = 0;
        self.latency_sum_ms = 0.0;
        self.latency_min_ms = None;
        self.latency_max_ms = None;
        self.histogram.reset();
        self.elapsed_seconds = 0.0;
        self.last_run = None;
        self.plot_view_revision = self.plot_view_revision.wrapping_add(1);
    }

    fn start_test(&mut self) {
        self.reset_results();
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

        let snapshot = RunSnapshot {
            target: config.url.clone(),
            method: config.method.clone(),
            workers: config.workers,
            duration_secs: config.duration_secs,
            request_rate: config.max_requests_per_second,
            timeout_secs: config.timeout_secs,
            actual_elapsed_secs: 0.0,
        };

        match engine::start(config) {
            Ok(handle) => {
                self.last_run = Some(snapshot);
                self.running = Some(handle);
                self.started = None;
                self.status = "Starting".into();
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

    fn record_sample(&mut self, sample: Sample) {
        self.total_received += 1;
        if sample.status >= 200 && sample.status < 400 {
            self.total_success += 1;
        } else if sample.status >= 400 && sample.status < 500 {
            self.total_client_errors += 1;
        } else if sample.status >= 500 {
            self.total_server_errors += 1;
        } else if sample.status == 0 {
            self.total_network_errors += 1;
        }

        // Response-time statistics describe actual HTTP responses only.
        // Transport failures (status 0), including timeouts, are counted
        // separately and do not distort HTTP latency percentiles.
        if sample.status != 0 && sample.latency_ms.is_finite() && sample.latency_ms >= 0.0 {
            self.http_response_count += 1;
            self.latency_sum_ms += sample.latency_ms;
            self.latency_min_ms = Some(
                self.latency_min_ms
                    .map_or(sample.latency_ms, |current| current.min(sample.latency_ms)),
            );
            self.latency_max_ms = Some(
                self.latency_max_ms
                    .map_or(sample.latency_ms, |current| current.max(sample.latency_ms)),
            );

            let micros = (sample.latency_ms * 1000.0).max(1.0) as u64;
            let _ = self.histogram.record(micros.min(60_000_000));

            self.recent_latencies
                .push_back((sample.elapsed_ms as f64 / 1000.0, sample.latency_ms));
            while self.recent_latencies.len() > 600 {
                self.recent_latencies.pop_front();
            }
        }

        if self.samples.len() == MAX_UI_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    fn poll_engine(&mut self) {
        // Drain the channel before mutating application state. This keeps the
        // immutable borrow of TestHandle separate from mutable UI updates.
        let events: Vec<EngineEvent> = self
            .running
            .as_ref()
            .map(|handle| handle.rx.try_iter().collect())
            .unwrap_or_default();

        let mut finished = false;
        for event in events {
            match event {
                EngineEvent::Started { at } => {
                    self.started = Some(at);
                    self.status = "Running".into();
                }
                EngineEvent::Sample(sample) => self.record_sample(sample),
                EngineEvent::Finished {
                    stopped,
                    elapsed_secs,
                } => {
                    self.elapsed_seconds = elapsed_secs;
                    if let Some(run) = self.last_run.as_mut() {
                        run.actual_elapsed_secs = elapsed_secs;
                    }
                    self.started = None;
                    self.status = if stopped {
                        "Stopped".into()
                    } else {
                        "Completed".into()
                    };
                    finished = true;
                }
                EngineEvent::Fatal(message) => {
                    self.error = Some(message);
                    self.status = "Failed".into();
                    finished = true;
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

    fn metrics(&self) -> (f64, f64, f64, f64, f64, f64) {
        if self.http_response_count == 0 {
            return (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        }

        (
            self.latency_sum_ms / self.http_response_count as f64,
            self.latency_min_ms.unwrap_or(0.0),
            self.histogram.value_at_quantile(0.50) as f64 / 1000.0,
            self.histogram.value_at_quantile(0.95) as f64 / 1000.0,
            self.histogram.value_at_quantile(0.99) as f64 / 1000.0,
            self.latency_max_ms.unwrap_or(0.0),
        )
    }

    fn calculate_rps(requests: u64, elapsed_seconds: f64) -> f64 {
        if !elapsed_seconds.is_finite() || elapsed_seconds <= 0.0 {
            0.0
        } else {
            requests as f64 / elapsed_seconds
        }
    }

    fn observed_rps(&self) -> f64 {
        Self::calculate_rps(self.total_received, self.elapsed_seconds)
    }

    fn export(&mut self, csv: bool) {
        let path = PathBuf::from(if csv {
            "redpulse-report.csv"
        } else {
            "redpulse-report.json"
        });
        let all_samples: Vec<Sample> = self.samples.iter().cloned().collect();
        let result = if csv {
            report::export_csv(&path, &all_samples)
        } else if let Some(run) = &self.last_run {
            let config = report::RunConfig {
                target: &run.target,
                method: &run.method,
                workers: run.workers,
                duration_secs: run.duration_secs,
                request_rate: run.request_rate,
                timeout_secs: run.timeout_secs,
                actual_elapsed_secs: run.actual_elapsed_secs,
            };
            report::export_json(&path, &config, &all_samples)
        } else {
            Err(anyhow::anyhow!("There is no test run to export."))
        };
        match result {
            Ok(()) => self.last_export = Some(format!("Saved {}", path.display())),
            Err(error) => self.error = Some(format!("Export failed: {error}")),
        }
    }

    fn metric_card(ui: &mut egui::Ui, label: &str, value: impl ToString, unit: &str) {
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(25, 34, 48))
            .stroke(egui::Stroke::new(
                1.0_f32,
                egui::Color32::from_rgb(48, 65, 86),
            ))
            .corner_radius(12)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(68.0);
                ui.label(
                    egui::RichText::new(label)
                        .size(10.5)
                        .strong()
                        .color(egui::Color32::from_rgb(145, 163, 185)),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.heading(
                        egui::RichText::new(value.to_string())
                            .size(24.0)
                            .strong()
                            .color(egui::Color32::from_rgb(232, 241, 251)),
                    );
                    ui.label(
                        egui::RichText::new(unit)
                            .small()
                            .color(egui::Color32::from_rgb(145, 163, 185)),
                    );
                });
            });
    }

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("◈")
                    .size(24.0)
                    .strong()
                    .color(egui::Color32::from_rgb(68, 180, 245)),
            );
            ui.heading(
                egui::RichText::new("REDPULSE")
                    .size(21.0)
                    .strong()
                    .color(egui::Color32::from_rgb(230, 239, 250)),
            );
            ui.label(
                egui::RichText::new("HTTP BENCHMARK")
                    .size(10.0)
                    .strong()
                    .color(egui::Color32::from_rgb(130, 151, 176)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (color, status) = if self.running.is_some() {
                    (egui::Color32::from_rgb(73, 205, 145), "RUNNING")
                } else if self.status == "Starting" {
                    (egui::Color32::from_rgb(105, 174, 238), "STARTING")
                } else if self.status == "Completed" {
                    (egui::Color32::from_rgb(76, 181, 132), "COMPLETED")
                } else if self.status == "Failed" {
                    (egui::Color32::from_rgb(235, 103, 110), "FAILED")
                } else if self.status == "Stopped" {
                    (egui::Color32::from_rgb(237, 183, 87), "STOPPED")
                } else {
                    (egui::Color32::from_rgb(148, 164, 184), "READY")
                };
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(31, 42, 57))
                    .corner_radius(egui::CornerRadius::same(20))
                    .inner_margin(egui::Margin::symmetric(11, 5))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("● {status}"))
                                .size(10.5)
                                .strong()
                                .color(color),
                        );
                    });
            });
        });
    }

    fn tester_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.heading(
            egui::RichText::new("HTTP Benchmark")
                .size(23.0)
                .strong()
                .color(egui::Color32::from_rgb(235, 242, 250)),
        );
        ui.label(
            egui::RichText::new(
                "Configure a request profile and watch the server response in real time.",
            )
            .color(egui::Color32::from_rgb(143, 160, 181)),
        );
        ui.add_space(12.0);
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
                ui.add_enabled(
                    self.running.is_none(),
                    egui::Slider::new(&mut self.workers, 1..=64),
                );
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label("Duration");
                ui.add_enabled(
                    self.running.is_none(),
                    egui::Slider::new(&mut self.duration, 1..=300).suffix(" s"),
                );
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label("Request rate");
                ui.add_enabled(
                    self.running.is_none(),
                    egui::Slider::new(&mut self.rps, 1..=200).suffix(" req/s"),
                );
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label("Timeout");
                ui.add_enabled(
                    self.running.is_none(),
                    egui::Slider::new(&mut self.timeout, 1..=60).suffix(" s"),
                );
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
            if ui
                .add_enabled(
                    can_start,
                    egui::Button::new("▶  Start test").fill(egui::Color32::from_rgb(25, 135, 90)),
                )
                .clicked()
            {
                self.start_test();
            }
            if ui
                .add_enabled(
                    self.running.is_some(),
                    egui::Button::new("■  Stop").fill(egui::Color32::from_rgb(160, 55, 65)),
                )
                .clicked()
            {
                self.stop_test();
            }
            if ui.button("Clear results").clicked() && self.running.is_none() {
                self.reset_results();
            }
        });

        if let Some(error) = &self.error {
            ui.add_space(8.0);
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }

        ui.add_space(14.0);
        ui.separator();
        ui.heading("Live metrics");
        let rps = self.observed_rps();
        let (avg, min, p50, p95, p99, max) = self.metrics();

        ui.columns(4, |columns| {
            Self::metric_card(&mut columns[0], "TOTAL REQUESTS", self.total_received, "");
            Self::metric_card(
                &mut columns[1],
                "OBSERVED THROUGHPUT",
                format!("{rps:.2}"),
                "RPS",
            );
            Self::metric_card(&mut columns[2], "SUCCESSFUL", self.total_success, "");
            Self::metric_card(
                &mut columns[3],
                "CLIENT ERRORS",
                self.total_client_errors,
                "",
            );
        });
        ui.add_space(8.0);
        ui.columns(4, |columns| {
            Self::metric_card(
                &mut columns[0],
                "SERVER ERRORS",
                self.total_server_errors,
                "",
            );
            Self::metric_card(
                &mut columns[1],
                "NETWORK ERRORS",
                self.total_network_errors,
                "",
            );
            Self::metric_card(
                &mut columns[2],
                "AVERAGE LATENCY",
                format!("{avg:.1}"),
                "ms",
            );
            Self::metric_card(
                &mut columns[3],
                "P95 / P99",
                format!("{p95:.1} / {p99:.1}"),
                "ms",
            );
        });
        ui.label(
            egui::RichText::new(format!(
                "HTTP latency: min {min:.1} · P50 {p50:.1} · avg {avg:.1} · P95 {p95:.1} · P99 {p99:.1} · max {max:.1} ms"
            ))
            .small()
            .color(egui::Color32::from_rgb(135, 153, 176)),
        );
        ui.label(
            egui::RichText::new("HTTP latency excludes transport errors and timeouts; HTTP 4xx/5xx responses remain included.")
                .small()
                .color(egui::Color32::from_rgb(135, 153, 176)),
        );

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.heading("Response time");
            if ui.button("↺  Reset view").clicked() {
                self.plot_view_revision = self.plot_view_revision.wrapping_add(1);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} samples", self.recent_latencies.len()))
                        .small()
                        .color(egui::Color32::from_rgb(135, 153, 176)),
                );
            });
        });

        let points: PlotPoints = self
            .recent_latencies
            .iter()
            .map(|(x, y)| [*x, *y])
            .collect();
        if self.recent_latencies.is_empty() {
            ui.label("Waiting for HTTP response samples…");
        } else {
            let plot = Plot::new(("latency_plot", self.plot_view_revision))
                .height(205.0)
                .allow_zoom(true)
                .allow_drag(true)
                .allow_boxed_zoom(true)
                .allow_double_click_reset(true)
                .x_axis_label("Elapsed time (s)")
                .y_axis_label("Response latency (ms)")
                .include_x(0.0)
                .include_y(0.0);
            plot.show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new("Response latency", points)
                        .color(egui::Color32::from_rgb(83, 183, 246))
                        .width(2.0),
                );
                if self.http_response_count > 0 {
                    let x_end = self.elapsed_seconds.max(0.01);
                    let p50_points: PlotPoints = vec![[0.0, p50], [x_end, p50]].into();
                    let p95_points: PlotPoints = vec![[0.0, p95], [x_end, p95]].into();
                    plot_ui.line(
                        Line::new("P50", p50_points)
                            .color(egui::Color32::from_rgb(76, 195, 142))
                            .width(1.2_f32),
                    );
                    plot_ui.line(
                        Line::new("P95", p95_points)
                            .color(egui::Color32::from_rgb(238, 178, 82))
                            .width(1.2_f32),
                    );
                }
            });
        }

        ui.horizontal(|ui| {
            ui.label(format!("Engine elapsed: {:.3}s", self.elapsed_seconds));
            ui.label(format!("Retained samples: {}", self.samples.len()));
        });
    }

    fn results_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            egui::RichText::new("Test results")
                .size(23.0)
                .strong()
                .color(egui::Color32::from_rgb(235, 242, 250)),
        );
        if let Some(run) = &self.last_run {
            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_rgb(22, 29, 41))
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgb(45, 61, 81),
                ))
                .corner_radius(12)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("RUN CONFIGURATION")
                            .small()
                            .strong()
                            .color(egui::Color32::from_rgb(139, 159, 184)),
                    );
                    ui.label(
                        egui::RichText::new(&run.target)
                            .strong()
                            .color(egui::Color32::from_rgb(105, 190, 250)),
                    );
                    ui.label(format!(
                        "{} · {} workers · {} s configured · {} req/s cap · {} s timeout · {:.3} s measured",
                        run.method,
                        run.workers,
                        run.duration_secs,
                        run.request_rate,
                        run.timeout_secs,
                        run.actual_elapsed_secs
                    ));
                });
        }
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
        let (avg, min, p50, p95, p99, max) = self.metrics();
        ui.label(format!(
            "{} requests · {} HTTP responses · {} successful · {} client errors · {} server errors · {} network errors",
            self.total_received,
            self.http_response_count,
            self.total_success,
            self.total_client_errors,
            self.total_server_errors,
            self.total_network_errors
        ));
        ui.label(format!(
            "Min {min:.2} ms · Average {avg:.2} ms · P50 {p50:.2} ms · P95 {p95:.2} ms · P99 {p99:.2} ms · Max {max:.2} ms"
        ));
        ui.label(format!(
            "Observed throughput: {:.2} RPS over {:.3} seconds",
            self.observed_rps(),
            self.elapsed_seconds
        ));
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("sample_table")
                .striped(true)
                .show(ui, |ui| {
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
                            if sample.status == 0 {
                                "ERR".into()
                            } else {
                                sample.status.to_string()
                            },
                        );
                        ui.label(format!("{:.1} ms", sample.latency_ms));
                        ui.label(sample.bytes.to_string());
                        let details = sample.error.as_deref().unwrap_or(if sample.ok {
                            "HTTP response received"
                        } else {
                            "HTTP error response"
                        });
                        let response = ui.label(details);
                        if sample.error.is_some() {
                            response.on_hover_text(details);
                        }
                        ui.end_row();
                    }
                });
        });
    }

    fn about_ui(&self, ui: &mut egui::Ui) {
        ui.heading(
            egui::RichText::new("About RedPulse")
                .size(23.0)
                .strong()
                .color(egui::Color32::from_rgb(235, 242, 250)),
        );
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

        egui::TopBottomPanel::top("topbar")
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(14, 19, 28))
                    .inner_margin(egui::Margin::symmetric(16, 11)),
            )
            .show(ctx, |ui| self.header(ui));
        egui::SidePanel::left("navigation")
            .resizable(false)
            .default_width(158.0)
            .frame(egui::Frame::new().fill(egui::Color32::from_rgb(15, 20, 29)))
            .show(ctx, |ui| {
                ui.add_space(12.0);
                if ui
                    .selectable_label(self.tab == Tab::Tester, "◉  Tester")
                    .clicked()
                {
                    self.tab = Tab::Tester;
                }
                if ui
                    .selectable_label(self.tab == Tab::Results, "▤  Results")
                    .clicked()
                {
                    self.tab = Tab::Results;
                }
                if ui
                    .selectable_label(self.tab == Tab::About, "ⓘ  About")
                    .clicked()
                {
                    self.tab = Tab::About;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(concat!("REDPULSE ", env!("CARGO_PKG_VERSION")))
                            .small()
                            .color(egui::Color32::GRAY),
                    );
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

#[cfg(test)]
mod tests {
    use super::RedPulseApp;

    #[test]
    fn observed_rps_uses_measured_elapsed_time() {
        assert!((RedPulseApp::calculate_rps(150, 30.0) - 5.0).abs() < 0.000_001);
        assert!((RedPulseApp::calculate_rps(10, 2.0) - 5.0).abs() < 0.000_001);
    }

    #[test]
    fn observed_rps_handles_zero_or_invalid_elapsed_time() {
        assert_eq!(RedPulseApp::calculate_rps(10, 0.0), 0.0);
        assert_eq!(RedPulseApp::calculate_rps(10, -1.0), 0.0);
        assert_eq!(RedPulseApp::calculate_rps(10, f64::NAN), 0.0);
        assert_eq!(RedPulseApp::calculate_rps(10, f64::INFINITY), 0.0);
    }
}
