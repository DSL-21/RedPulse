use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use hdrhistogram::Histogram;

use crate::{
    engine::{
        self, EngineEvent, EngineStats, LoadModel, MAX_REQUEST_BODY_BYTES, MAX_RPS,
        MAX_TOTAL_REQUESTS, Sample, TestConfig, TestHandle,
    },
    recon::ReconState,
    report,
};

#[derive(PartialEq, Eq)]
enum Tab {
    Recon,
    Tester,
    Results,
    About,
}

const MAX_UI_SAMPLES: usize = MAX_TOTAL_REQUESTS as usize;
const MAX_PLOT_POINTS: usize = 2_000;

fn downsample_min_max(points: &VecDeque<(f64, f64)>, max_points: usize) -> Vec<[f64; 2]> {
    let mut sorted: Vec<[f64; 2]> = points.iter().map(|(x, y)| [*x, *y]).collect();
    sorted.sort_by(|left, right| left[0].total_cmp(&right[0]));

    if sorted.len() <= max_points {
        return sorted;
    }
    if max_points < 4 || sorted.len() < 3 {
        return sorted.into_iter().take(max_points).collect();
    }

    let interior_len = sorted.len() - 2;
    let bucket_count = ((max_points - 2) / 2).max(1).min(interior_len);
    let mut reduced = Vec::with_capacity(max_points);
    reduced.push(sorted[0]);

    for bucket in 0..bucket_count {
        let start = 1 + bucket * interior_len / bucket_count;
        let end = 1 + (bucket + 1) * interior_len / bucket_count;
        if start >= end {
            continue;
        }

        let mut min_index = start;
        let mut max_index = start;
        for index in (start + 1)..end {
            if sorted[index][1] < sorted[min_index][1] {
                min_index = index;
            }
            if sorted[index][1] > sorted[max_index][1] {
                max_index = index;
            }
        }

        if min_index == max_index {
            reduced.push(sorted[min_index]);
        } else if min_index < max_index {
            reduced.push(sorted[min_index]);
            reduced.push(sorted[max_index]);
        } else {
            reduced.push(sorted[max_index]);
            reduced.push(sorted[min_index]);
        }
    }

    reduced.push(*sorted.last().expect("non-empty point series"));
    reduced
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponseClass {
    NetworkError,
    Success,
    Redirect,
    ClientError,
    ServerError,
    Other,
}

fn classify_status(status: u16) -> ResponseClass {
    match status / 100 {
        0 => ResponseClass::NetworkError,
        2 => ResponseClass::Success,
        3 => ResponseClass::Redirect,
        4 => ResponseClass::ClientError,
        5 => ResponseClass::ServerError,
        _ => ResponseClass::Other,
    }
}

#[derive(Clone)]
struct RunSnapshot {
    target: String,
    method: String,
    load_model: LoadModel,
    workers: usize,
    duration_secs: u64,
    request_rate: Option<u32>,
    timeout_secs: u64,
    actual_elapsed_secs: f64,
}

pub struct RedPulseApp {
    tab: Tab,
    recon: ReconState,
    target: String,
    method: String,
    load_model: LoadModel,
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
    total_redirects: u64,
    total_http_4xx: u64,
    total_http_5xx: u64,
    total_network_errors: u64,
    engine_stats: EngineStats,
    http_response_count: u64,
    latency_sum_ms: f64,
    latency_min_ms: Option<f64>,
    latency_max_ms: Option<f64>,
    histogram: Histogram<u64>,
    last_run: Option<RunSnapshot>,
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
            tab: Tab::Recon,
            recon: ReconState::new(),
            target: "https://example.com/".into(),
            method: "GET".into(),
            load_model: LoadModel::ConstantArrivalRate,
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
            total_redirects: 0,
            total_http_4xx: 0,
            total_http_5xx: 0,
            total_network_errors: 0,
            engine_stats: EngineStats::default(),
            http_response_count: 0,
            latency_sum_ms: 0.0,
            latency_min_ms: None,
            latency_max_ms: None,
            histogram: Histogram::<u64>::new(3).expect("valid histogram configuration"),
            last_run: None,
        }
    }

    fn reset_results(&mut self) {
        self.samples.clear();
        self.recent_latencies.clear();
        self.total_received = 0;
        self.total_success = 0;
        self.total_redirects = 0;
        self.total_http_4xx = 0;
        self.total_http_5xx = 0;
        self.total_network_errors = 0;
        self.engine_stats = EngineStats::default();
        self.http_response_count = 0;
        self.latency_sum_ms = 0.0;
        self.latency_min_ms = None;
        self.latency_max_ms = None;
        self.histogram.reset();
        self.elapsed_seconds = 0.0;
        self.last_run = None;
    }

    fn start_test(&mut self) {
        self.reset_results();
        self.error = None;
        self.last_export = None;

        let config = TestConfig {
            url: self.target.trim().to_string(),
            method: self.method.clone(),
            load_model: self.load_model,
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
            request_rate: (config.load_model == LoadModel::ConstantArrivalRate)
                .then_some(config.max_requests_per_second),
            load_model: config.load_model,
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
        match classify_status(sample.status) {
            ResponseClass::Success => self.total_success += 1,
            ResponseClass::Redirect => self.total_redirects += 1,
            ResponseClass::ClientError => self.total_http_4xx += 1,
            ResponseClass::ServerError => self.total_http_5xx += 1,
            ResponseClass::NetworkError => self.total_network_errors += 1,
            ResponseClass::Other => {}
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
                .push_back((sample.elapsed_us as f64 / 1_000_000.0, sample.latency_ms));
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
                EngineEvent::Stats(stats) => self.engine_stats = stats,
                EngineEvent::Sample(sample) => self.record_sample(sample),
                EngineEvent::Finished {
                    stopped,
                    elapsed_secs,
                    stats,
                } => {
                    self.engine_stats = stats;
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
                load_model: run.load_model,
                timeout_secs: run.timeout_secs,
                actual_elapsed_secs: run.actual_elapsed_secs,
                global_rate_limit_rps: MAX_RPS,
                max_total_requests: MAX_TOTAL_REQUESTS,
                stats: self.engine_stats,
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
                egui::RichText::new("RED TEAM WORKBENCH")
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
            egui::RichText::new("Load Testing")
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
            ui.label("Load model");
            egui::ComboBox::from_id_salt("load_model")
                .selected_text(self.load_model.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.load_model,
                        LoadModel::ConstantArrivalRate,
                        "Constant Arrival Rate",
                    );
                    ui.selectable_value(&mut self.load_model, LoadModel::ClosedLoop, "Closed Loop");
                });
            ui.label(
                egui::RichText::new(self.load_model.description())
                    .small()
                    .color(egui::Color32::from_rgb(137, 157, 181)),
            );
        });

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
                ui.label("Arrival rate");
                ui.add_enabled(
                    self.running.is_none() && self.load_model == LoadModel::ConstantArrivalRate,
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

        if self.load_model == LoadModel::ClosedLoop {
            ui.label(
                egui::RichText::new(
                    "Closed Loop uses worker count as the concurrency limit; the arrival-rate slider is not used.",
                )
                .small()
                .color(egui::Color32::from_rgb(137, 157, 181)),
            );
        }

        ui.label(
            egui::RichText::new(format!(
                "Engine guardrails: max {MAX_RPS} request starts/s · max {MAX_TOTAL_REQUESTS} requests/run · max {} KiB POST JSON",
                MAX_REQUEST_BODY_BYTES / 1024
            ))
            .small()
            .color(egui::Color32::from_rgb(137, 157, 181)),
        );

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
            Self::metric_card(
                &mut columns[0],
                "COMPLETED REQUESTS",
                self.total_received,
                "",
            );
            Self::metric_card(
                &mut columns[1],
                "OBSERVED THROUGHPUT",
                format!("{rps:.2}"),
                "RPS",
            );
            Self::metric_card(&mut columns[2], "HTTP 2xx", self.total_success, "");
            Self::metric_card(&mut columns[3], "HTTP 4xx", self.total_http_4xx, "");
        });
        ui.add_space(8.0);
        ui.columns(4, |columns| {
            Self::metric_card(&mut columns[0], "HTTP 5xx", self.total_http_5xx, "");
            Self::metric_card(&mut columns[1], "HTTP 3xx", self.total_redirects, "");
            Self::metric_card(
                &mut columns[2],
                "NETWORK ERRORS",
                self.total_network_errors,
                "",
            );
            Self::metric_card(
                &mut columns[3],
                "AVERAGE LATENCY",
                format!("{avg:.1}"),
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

        ui.label(
            egui::RichText::new(format!(
                "Requests: scheduled {} · started {} · completed {} · queued {} · in-flight {} · cancelled {} · dropped {}",
                self.engine_stats.scheduled,
                self.engine_stats.started,
                self.engine_stats.completed,
                self.engine_stats.queued,
                self.engine_stats.in_flight,
                self.engine_stats.cancelled,
                self.engine_stats.dropped
            ))
            .small()
            .color(egui::Color32::from_rgb(156, 177, 201)),
        );
        if self.engine_stats.limit_reached {
            ui.colored_label(
                egui::Color32::from_rgb(238, 178, 82),
                "Maximum of 10,000 scheduled requests reached; no further requests were started.",
            );
        }

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.heading("Response time");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} samples · {} plotted",
                        self.recent_latencies.len(),
                        downsample_min_max(&self.recent_latencies, MAX_PLOT_POINTS).len()
                    ))
                    .small()
                    .color(egui::Color32::from_rgb(135, 153, 176)),
                );
            });
        });

        let points: PlotPoints = downsample_min_max(&self.recent_latencies, MAX_PLOT_POINTS).into();
        if self.recent_latencies.is_empty() {
            ui.label("Waiting for HTTP response samples…");
        } else {
            let plot = Plot::new("latency_plot")
                .height(205.0)
                // Keep the chart fixed: mouse wheel and pointer gestures must
                // scroll the surrounding dashboard, never alter plot bounds.
                .allow_zoom(false)
                .allow_scroll(false)
                .allow_drag(false)
                .allow_boxed_zoom(false)
                .allow_double_click_reset(false)
                .x_axis_label("Elapsed time (s)")
                .y_axis_label("Response latency (ms)")
                .include_x(0.0)
                .include_y(0.0);
            plot.show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new("Response latency", points)
                        .color(egui::Color32::from_rgb(83, 183, 246))
                        .width(2.0_f32),
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
                    let rate_description = run
                        .request_rate
                        .map(|rate| format!("{rate} req/s arrival target"))
                        .unwrap_or_else(|| "throughput determined by workers".into());
                    ui.label(format!(
                        "{} · {} · {} workers · {} s configured · {} · {} s timeout · {:.3} s measured",
                        run.method,
                        run.load_model.label(),
                        run.workers,
                        run.duration_secs,
                        rate_description,
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
            "{} completed requests · {} HTTP responses · {} HTTP 2xx · {} HTTP 3xx · {} HTTP 4xx · {} HTTP 5xx · {} network errors",
            self.total_received,
            self.http_response_count,
            self.total_success,
            self.total_redirects,
            self.total_http_4xx,
            self.total_http_5xx,
            self.total_network_errors
        ));
        ui.label(format!(
            "Scheduled {} · Started {} · Completed {} · Queued {} · In-flight {} · Cancelled {} · Dropped {}",
            self.engine_stats.scheduled,
            self.engine_stats.started,
            self.engine_stats.completed,
            self.engine_stats.queued,
            self.engine_stats.in_flight,
            self.engine_stats.cancelled,
            self.engine_stats.dropped
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
                        let color = match classify_status(sample.status) {
                            ResponseClass::NetworkError | ResponseClass::ServerError => {
                                egui::Color32::LIGHT_RED
                            }
                            ResponseClass::ClientError => egui::Color32::YELLOW,
                            ResponseClass::Redirect => egui::Color32::from_rgb(105, 174, 238),
                            ResponseClass::Success => egui::Color32::LIGHT_GREEN,
                            ResponseClass::Other => egui::Color32::GRAY,
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
        ui.label("A native Rust desktop red-team web assessment and HTTP benchmarking workbench.");
        ui.add_space(8.0);
        ui.label("Engine: Tokio + Reqwest + Rustls");
        ui.label("Interface: egui / eframe");
        ui.label("Recon: bounded HTTP surface inspection, header and cookie posture checks");
        ui.label("Load testing: Constant Arrival Rate and Closed Loop");
        ui.label("Metrics: response status, throughput and latency percentiles");
        ui.label("Reports: JSON and CSV for benchmark runs");
        ui.label("Recon profile: six sequential HTTP probes, no redirects, five-second timeout");
        ui.separator();
        ui.label(
            "Constant Arrival Rate schedules requests independently from HTTP worker completion.",
        );
        ui.label("Closed Loop lets each worker repeat only after its previous request completes.");
        ui.label("A bounded queue reports constant-rate overload as dropped arrivals instead of delaying future slots.");
        ui.label("Recon uses a fixed low-volume probe set: GET, HEAD, OPTIONS, security.txt, robots.txt and sitemap.xml.");
        ui.label("No credential guessing, exploit payloads, destructive HTTP methods or recursive wordlist scans are part of the recon profile.");
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
                    .selectable_label(self.tab == Tab::Recon, "◈  Recon")
                    .clicked()
                {
                    self.tab = Tab::Recon;
                }
                if ui
                    .selectable_label(self.tab == Tab::Tester, "◉  Load Test")
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
                Tab::Recon => self.recon.ui(ui),
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

#[cfg(test)]
mod response_class_tests {
    use super::{ResponseClass, classify_status};

    #[test]
    fn classifies_http_success_responses() {
        assert_eq!(classify_status(200), ResponseClass::Success);
        assert_eq!(classify_status(204), ResponseClass::Success);
    }

    #[test]
    fn classifies_redirect_responses_separately() {
        assert_eq!(classify_status(301), ResponseClass::Redirect);
        assert_eq!(classify_status(302), ResponseClass::Redirect);
        assert_eq!(classify_status(304), ResponseClass::Redirect);
    }

    #[test]
    fn classifies_client_and_server_errors() {
        assert_eq!(classify_status(405), ResponseClass::ClientError);
        assert_eq!(classify_status(429), ResponseClass::ClientError);
        assert_eq!(classify_status(500), ResponseClass::ServerError);
        assert_eq!(classify_status(503), ResponseClass::ServerError);
    }

    #[test]
    fn classifies_transport_errors_and_unusual_statuses() {
        assert_eq!(classify_status(0), ResponseClass::NetworkError);
        assert_eq!(classify_status(101), ResponseClass::Other);
    }
}

#[cfg(test)]
mod load_model_ui_tests {
    use crate::engine::LoadModel;

    #[test]
    fn load_models_have_clear_user_facing_names() {
        assert_eq!(
            LoadModel::ConstantArrivalRate.label(),
            "Constant Arrival Rate"
        );
        assert_eq!(LoadModel::ClosedLoop.label(), "Closed Loop");
    }
}

#[cfg(test)]
mod chart_downsampling_tests {
    use super::downsample_min_max;
    use std::collections::VecDeque;

    #[test]
    fn chart_downsampling_retains_full_time_range() {
        let points: VecDeque<(f64, f64)> = (0..10_000)
            .map(|index| (index as f64 / 100.0, 50.0))
            .collect();

        let reduced = downsample_min_max(&points, 2_000);
        assert!(reduced.len() <= 2_000);
        assert_eq!(reduced.first().expect("first point")[0], 0.0);
        assert_eq!(reduced.last().expect("last point")[0], 99.99);
        assert!(reduced.windows(2).all(|pair| pair[0][0] <= pair[1][0]));
    }

    #[test]
    fn chart_downsampling_preserves_minimum_and_maximum_spikes() {
        let mut points: VecDeque<(f64, f64)> =
            (0..10_000).map(|index| (index as f64, 50.0)).collect();
        points[4_321].1 = 900.0;
        points[7_654].1 = 1.0;

        let reduced = downsample_min_max(&points, 2_000);
        let values: Vec<f64> = reduced.iter().map(|point| point[1]).collect();
        assert!(values.contains(&900.0));
        assert!(values.contains(&1.0));
        assert!(reduced.len() <= 2_000);
    }

    #[test]
    fn short_chart_series_is_not_modified() {
        let points = VecDeque::from([(0.0, 10.0), (0.5, 15.0), (1.0, 8.0)]);
        assert_eq!(
            downsample_min_max(&points, 2_000),
            vec![[0.0, 10.0], [0.5, 15.0], [1.0, 8.0]]
        );
    }
}
