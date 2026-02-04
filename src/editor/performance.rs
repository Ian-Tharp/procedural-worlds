//! Performance Metrics Dashboard
//!
//! A standalone floating performance overlay with comprehensive metrics:
//! - Real-time FPS counter with min/max/avg and percentile tracking
//! - Frame time graph with extended history
//! - Frame budget visualization (% of 16.67ms target)
//! - Chunk loading throughput and latency
//! - Process memory usage with trend detection
//!
//! Toggle with **F8**. Operates independently of the F3 debug overlay.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::egui;

use crate::engine::memory;
use crate::world::ChunkLoadMetrics;

// ============================================================================
// Constants
// ============================================================================

/// Number of frame-time samples kept for the dashboard graph.
const DASHBOARD_HISTORY_SIZE: usize = 240;

/// Number of samples used for percentile calculations.
const PERCENTILE_WINDOW_SIZE: usize = 600;

/// Target frame time in milliseconds (60 FPS).
const TARGET_FRAME_TIME_MS: f64 = 1000.0 / 60.0;

/// How often (in seconds) to snapshot memory for trend detection.
const MEMORY_TREND_INTERVAL: f32 = 2.0;

/// Number of memory snapshots kept for trend analysis.
const MEMORY_TREND_SAMPLES: usize = 30;

// ============================================================================
// Resource
// ============================================================================

/// Central resource tracking all dashboard-level performance metrics.
#[derive(Resource)]
pub struct PerformanceDashboard {
    // ── Visibility ──────────────────────────────────────────
    /// Whether the floating dashboard window is open.
    pub visible: bool,

    // ── Frame time history ──────────────────────────────────
    /// Circular buffer of recent frame times (ms) for the graph.
    graph_history: Vec<f32>,
    /// Write index into `graph_history`.
    graph_index: usize,
    /// Larger buffer used solely for percentile computation.
    percentile_buffer: Vec<f32>,
    /// Write index into `percentile_buffer`.
    percentile_index: usize,
    /// How many valid samples have been written to `percentile_buffer`.
    percentile_count: usize,

    // ── Derived FPS metrics (recomputed every frame) ────────
    /// Instantaneous FPS (smoothed by Bevy diagnostics).
    pub fps: f64,
    /// Current frame time in ms (smoothed).
    pub frame_time_ms: f64,
    /// Minimum frame time in the current graph window.
    pub frame_time_min_ms: f32,
    /// Maximum frame time in the current graph window.
    pub frame_time_max_ms: f32,
    /// Average frame time in the current graph window.
    pub frame_time_avg_ms: f32,
    /// 1st-percentile frame time (worst 1% of frames).
    pub p1_frame_time_ms: f32,
    /// 0.1th-percentile frame time (worst 0.1% of frames).
    pub p01_frame_time_ms: f32,
    /// 1% low FPS (derived from p1 frame time).
    pub fps_1_low: f32,
    /// 0.1% low FPS (derived from p01 frame time).
    pub fps_01_low: f32,
    /// Frame budget usage as fraction (1.0 = exactly 16.67ms).
    pub budget_usage: f64,
    /// Number of frames that exceeded the target this window.
    pub frames_over_budget: u32,

    // ── Memory trend ────────────────────────────────────────
    /// Recent RSS snapshots for trend detection.
    memory_snapshots: Vec<usize>,
    /// Timer controlling snapshot frequency.
    memory_trend_timer: f32,
    /// Computed trend in MB/s (positive = growing).
    pub memory_trend_mb_per_sec: f64,
    /// Latest RSS bytes (copied from `memory::get_process_memory`).
    pub current_rss_bytes: usize,
    /// Peak RSS bytes.
    pub peak_rss_bytes: Option<usize>,
}

impl Default for PerformanceDashboard {
    fn default() -> Self {
        Self {
            visible: false,

            graph_history: vec![0.0; DASHBOARD_HISTORY_SIZE],
            graph_index: 0,
            percentile_buffer: vec![0.0; PERCENTILE_WINDOW_SIZE],
            percentile_index: 0,
            percentile_count: 0,

            fps: 0.0,
            frame_time_ms: 0.0,
            frame_time_min_ms: 0.0,
            frame_time_max_ms: 0.0,
            frame_time_avg_ms: 0.0,
            p1_frame_time_ms: 0.0,
            p01_frame_time_ms: 0.0,
            fps_1_low: 0.0,
            fps_01_low: 0.0,
            budget_usage: 0.0,
            frames_over_budget: 0,

            memory_snapshots: Vec::with_capacity(MEMORY_TREND_SAMPLES),
            memory_trend_timer: 0.0,
            memory_trend_mb_per_sec: 0.0,
            current_rss_bytes: 0,
            peak_rss_bytes: None,
        }
    }
}

impl PerformanceDashboard {
    /// Record a single frame time sample.
    pub fn record_frame_time(&mut self, frame_time_ms: f32) {
        // Graph history
        self.graph_history[self.graph_index] = frame_time_ms;
        self.graph_index = (self.graph_index + 1) % DASHBOARD_HISTORY_SIZE;

        // Percentile buffer
        self.percentile_buffer[self.percentile_index] = frame_time_ms;
        self.percentile_index = (self.percentile_index + 1) % PERCENTILE_WINDOW_SIZE;
        if self.percentile_count < PERCENTILE_WINDOW_SIZE {
            self.percentile_count += 1;
        }
    }

    /// Get graph history in chronological order (oldest → newest).
    pub fn ordered_graph_history(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(DASHBOARD_HISTORY_SIZE);
        for i in 0..DASHBOARD_HISTORY_SIZE {
            let idx = (self.graph_index + i) % DASHBOARD_HISTORY_SIZE;
            out.push(self.graph_history[idx]);
        }
        out
    }

    /// Recompute all derived statistics from the current buffers.
    pub fn recompute_stats(&mut self) {
        // ── Graph-window stats ──────────────────────────────
        let valid: Vec<f32> = self
            .graph_history
            .iter()
            .copied()
            .filter(|&t| t > 0.0)
            .collect();

        if valid.is_empty() {
            self.frame_time_min_ms = 0.0;
            self.frame_time_max_ms = 0.0;
            self.frame_time_avg_ms = 0.0;
            self.frames_over_budget = 0;
        } else {
            self.frame_time_min_ms = valid.iter().copied().fold(f32::INFINITY, f32::min);
            self.frame_time_max_ms = valid.iter().copied().fold(0.0_f32, f32::max);
            self.frame_time_avg_ms = valid.iter().sum::<f32>() / valid.len() as f32;
            self.frames_over_budget = valid
                .iter()
                .filter(|&&t| t > TARGET_FRAME_TIME_MS as f32)
                .count() as u32;
        }

        // Budget usage (from smoothed frame time)
        self.budget_usage = self.frame_time_ms / TARGET_FRAME_TIME_MS;

        // ── Percentile computation ──────────────────────────
        if self.percentile_count >= 10 {
            let mut sorted: Vec<f32> = self.percentile_buffer[..self.percentile_count]
                .iter()
                .copied()
                .filter(|&t| t > 0.0)
                .collect();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            if !sorted.is_empty() {
                let len = sorted.len();
                // P99 of frame times = worst 1%
                let p99_idx = ((len as f64) * 0.99).ceil() as usize;
                self.p1_frame_time_ms = sorted[p99_idx.min(len - 1)];
                // P99.9 = worst 0.1%
                let p999_idx = ((len as f64) * 0.999).ceil() as usize;
                self.p01_frame_time_ms = sorted[p999_idx.min(len - 1)];

                self.fps_1_low = if self.p1_frame_time_ms > 0.0 {
                    1000.0 / self.p1_frame_time_ms
                } else {
                    0.0
                };
                self.fps_01_low = if self.p01_frame_time_ms > 0.0 {
                    1000.0 / self.p01_frame_time_ms
                } else {
                    0.0
                };
            }
        }
    }

    /// Record a memory snapshot and recompute trend.
    pub fn record_memory_snapshot(&mut self, rss_bytes: usize) {
        self.current_rss_bytes = rss_bytes;

        if self.memory_snapshots.len() >= MEMORY_TREND_SAMPLES {
            self.memory_snapshots.remove(0);
        }
        self.memory_snapshots.push(rss_bytes);

        // Compute trend (simple linear: compare first half avg to second half avg)
        if self.memory_snapshots.len() >= 4 {
            let mid = self.memory_snapshots.len() / 2;
            let first_avg =
                self.memory_snapshots[..mid].iter().sum::<usize>() as f64 / mid as f64;
            let second_avg =
                self.memory_snapshots[mid..].iter().sum::<usize>() as f64
                    / (self.memory_snapshots.len() - mid) as f64;
            let delta_bytes = second_avg - first_avg;
            let time_span = (self.memory_snapshots.len() as f64 / 2.0) * MEMORY_TREND_INTERVAL as f64;
            if time_span > 0.0 {
                self.memory_trend_mb_per_sec = delta_bytes / (1024.0 * 1024.0) / time_span;
            }
        }
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Gather frame-time diagnostics and update the dashboard resource.
pub fn update_performance_dashboard(
    diagnostics: Res<DiagnosticsStore>,
    mut dashboard: ResMut<PerformanceDashboard>,
    time: Res<Time>,
) {
    // FPS / frame time from Bevy diagnostics
    if let Some(fps_diag) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS) {
        if let Some(fps) = fps_diag.smoothed() {
            dashboard.fps = fps;
            if fps > 0.0 {
                dashboard.frame_time_ms = 1000.0 / fps;
            }
        }

        if let Some(raw_fps) = fps_diag.value()
            && raw_fps > 0.0
        {
            let frame_ms = (1000.0 / raw_fps) as f32;
            dashboard.record_frame_time(frame_ms);
        }
    }

    // Recompute derived stats every frame (cheap)
    dashboard.recompute_stats();

    // Memory snapshots at a lower frequency
    let dt = time.delta_secs();
    dashboard.memory_trend_timer += dt;
    if dashboard.memory_trend_timer >= MEMORY_TREND_INTERVAL {
        dashboard.memory_trend_timer = 0.0;
        if let Some(mem) = memory::get_process_memory() {
            dashboard.peak_rss_bytes = mem.peak_rss_bytes;
            dashboard.record_memory_snapshot(mem.rss_bytes);
        }
    }
}

/// Toggle dashboard visibility with F4.
pub fn dashboard_keyboard_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut dashboard: ResMut<PerformanceDashboard>,
) {
    if keyboard.just_pressed(KeyCode::F8) {
        dashboard.visible = !dashboard.visible;
    }
}

/// Render the floating performance dashboard window.
pub fn draw_performance_dashboard(
    ui_ctx: &mut egui::Context,
    dashboard: &PerformanceDashboard,
    chunk_count: usize,
    load_metrics: Option<&ChunkLoadMetrics>,
) {
    egui::Window::new("⚡ Performance Dashboard")
        .default_pos([400.0, 20.0])
        .default_width(360.0)
        .resizable(true)
        .collapsible(true)
        .show(ui_ctx, |ui| {
            // ── FPS headline ────────────────────────────────
            ui.horizontal(|ui| {
                let fps = dashboard.fps;
                let color = fps_color(fps);
                ui.colored_label(
                    color,
                    egui::RichText::new(format!("{:.0} FPS", fps))
                        .strong()
                        .size(22.0),
                );
                ui.monospace(format!("({:.2} ms)", dashboard.frame_time_ms));
            });

            // ── Frame budget bar ────────────────────────────
            let budget = dashboard.budget_usage.clamp(0.0, 2.0) as f32;
            let budget_color = if budget <= 0.8 {
                egui::Color32::from_rgb(100, 255, 100)
            } else if budget <= 1.0 {
                egui::Color32::from_rgb(255, 255, 100)
            } else {
                egui::Color32::from_rgb(255, 100, 100)
            };

            ui.horizontal(|ui| {
                ui.label("Budget:");
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(180.0, 14.0), egui::Sense::hover());
                // Background
                ui.painter()
                    .rect_filled(rect, 2.0, egui::Color32::from_rgb(40, 40, 40));
                // Fill
                let fill_width = rect.width() * (budget / 2.0).min(1.0);
                let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_width, rect.height()));
                ui.painter().rect_filled(fill_rect, 2.0, budget_color);
                // Target line at 50% (= 100% budget)
                let target_x = rect.min.x + rect.width() * 0.5;
                ui.painter().line_segment(
                    [
                        egui::pos2(target_x, rect.min.y),
                        egui::pos2(target_x, rect.max.y),
                    ],
                    egui::Stroke::new(1.0, egui::Color32::WHITE),
                );
                ui.monospace(format!("{:.0}%", budget * 100.0));
            });

            ui.separator();

            // ── FPS breakdown ───────────────────────────────
            egui::CollapsingHeader::new("📈 FPS Breakdown")
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new("fps_grid")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Average:");
                            let avg_fps = if dashboard.frame_time_avg_ms > 0.0 {
                                1000.0 / dashboard.frame_time_avg_ms
                            } else {
                                0.0
                            };
                            ui.colored_label(fps_color(avg_fps as f64), format!("{:.1} FPS", avg_fps));
                            ui.end_row();

                            ui.label("1% Low:");
                            ui.colored_label(
                                fps_color(dashboard.fps_1_low as f64),
                                format!("{:.1} FPS", dashboard.fps_1_low),
                            );
                            ui.end_row();

                            ui.label("0.1% Low:");
                            ui.colored_label(
                                fps_color(dashboard.fps_01_low as f64),
                                format!("{:.1} FPS", dashboard.fps_01_low),
                            );
                            ui.end_row();

                            ui.label("Over budget:");
                            let ob = dashboard.frames_over_budget;
                            let ob_color = if ob == 0 {
                                egui::Color32::from_rgb(100, 255, 100)
                            } else if ob < 10 {
                                egui::Color32::from_rgb(255, 255, 100)
                            } else {
                                egui::Color32::from_rgb(255, 100, 100)
                            };
                            ui.colored_label(
                                ob_color,
                                format!(
                                    "{} / {}",
                                    ob, DASHBOARD_HISTORY_SIZE
                                ),
                            );
                            ui.end_row();
                        });
                });

            ui.separator();

            // ── Frame time stats ────────────────────────────
            egui::CollapsingHeader::new("⏱ Frame Times")
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new("ft_grid")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Current:");
                            ui.monospace(format!("{:.2} ms", dashboard.frame_time_ms));
                            ui.end_row();

                            ui.label("Average:");
                            ui.monospace(format!("{:.2} ms", dashboard.frame_time_avg_ms));
                            ui.end_row();

                            ui.label("Min:");
                            ui.monospace(format!("{:.2} ms", dashboard.frame_time_min_ms));
                            ui.end_row();

                            ui.label("Max:");
                            ui.monospace(format!("{:.2} ms", dashboard.frame_time_max_ms));
                            ui.end_row();

                            ui.label("P99 (1% low):");
                            ui.monospace(format!("{:.2} ms", dashboard.p1_frame_time_ms));
                            ui.end_row();

                            ui.label("P99.9 (0.1% low):");
                            ui.monospace(format!("{:.2} ms", dashboard.p01_frame_time_ms));
                            ui.end_row();
                        });

                    // Mini graph
                    ui.add_space(4.0);
                    ui.label("Frame time history:");
                    let history = dashboard.ordered_graph_history();
                    let recent: Vec<f32> = history.iter().rev().take(120).copied().collect();
                    let max_ft = recent
                        .iter()
                        .copied()
                        .fold(TARGET_FRAME_TIME_MS as f32 * 2.0, f32::max);

                    let graph_width = ui.available_width().min(340.0);
                    let graph_height = 50.0;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(graph_width, graph_height),
                        egui::Sense::hover(),
                    );

                    // Background
                    ui.painter()
                        .rect_filled(rect, 2.0, egui::Color32::from_rgb(20, 20, 30));

                    // 16.67ms target line
                    let target_y = rect.max.y
                        - (TARGET_FRAME_TIME_MS as f32 / max_ft) * rect.height();
                    ui.painter().line_segment(
                        [
                            egui::pos2(rect.min.x, target_y),
                            egui::pos2(rect.max.x, target_y),
                        ],
                        egui::Stroke::new(1.0, egui::Color32::from_rgba_premultiplied(255, 255, 100, 80)),
                    );

                    // Bars
                    if !recent.is_empty() {
                        let bar_w = rect.width() / recent.len() as f32;
                        for (i, &ft) in recent.iter().rev().enumerate() {
                            let normalized = (ft / max_ft).min(1.0);
                            let h = graph_height * normalized;
                            let x = rect.min.x + i as f32 * bar_w;
                            let bar_rect = egui::Rect::from_min_max(
                                egui::pos2(x, rect.max.y - h),
                                egui::pos2(x + bar_w - 0.5, rect.max.y),
                            );
                            let color = frame_time_color(ft);
                            ui.painter().rect_filled(bar_rect, 0.0, color);
                        }
                    }

                    ui.small("Yellow line = 16.67ms (60 FPS target)");
                });

            ui.separator();

            // ── Chunk loading ───────────────────────────────
            egui::CollapsingHeader::new("📦 Chunk Loading")
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new("chunk_grid")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Active chunks:");
                            ui.monospace(format!("{}", chunk_count));
                            ui.end_row();

                            if let Some(metrics) = load_metrics {
                                ui.label("Chunks/sec:");
                                let cps = metrics.chunks_per_second;
                                ui.colored_label(
                                    if cps >= 10.0 {
                                        egui::Color32::from_rgb(100, 255, 100)
                                    } else if cps >= 2.0 {
                                        egui::Color32::from_rgb(255, 255, 100)
                                    } else {
                                        egui::Color32::from_rgb(255, 100, 100)
                                    },
                                    format!("{:.1}", cps),
                                );
                                ui.end_row();

                                ui.label("Avg load time:");
                                let avg = metrics.avg_load_time_ms;
                                ui.colored_label(
                                    if avg <= 20.0 {
                                        egui::Color32::from_rgb(100, 255, 100)
                                    } else if avg <= 100.0 {
                                        egui::Color32::from_rgb(255, 255, 100)
                                    } else {
                                        egui::Color32::from_rgb(255, 100, 100)
                                    },
                                    format!("{:.1} ms", avg),
                                );
                                ui.end_row();

                                ui.label("Total loaded:");
                                ui.monospace(format!("{}", metrics.total_chunks_loaded));
                                ui.end_row();
                            }
                        });
                });

            ui.separator();

            // ── Memory ──────────────────────────────────────
            egui::CollapsingHeader::new("💾 Memory")
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new("mem_grid")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("RSS:");
                            if dashboard.current_rss_bytes > 0 {
                                ui.monospace(memory::format_bytes(dashboard.current_rss_bytes));
                            } else {
                                ui.colored_label(
                                    egui::Color32::from_rgb(150, 150, 150),
                                    "N/A",
                                );
                            }
                            ui.end_row();

                            if let Some(peak) = dashboard.peak_rss_bytes {
                                ui.label("Peak RSS:");
                                ui.monospace(memory::format_bytes(peak));
                                ui.end_row();
                            }

                            ui.label("Trend:");
                            let trend = dashboard.memory_trend_mb_per_sec;
                            let (trend_text, trend_color) = if trend.abs() < 0.01 {
                                ("stable".to_string(), egui::Color32::from_rgb(150, 150, 150))
                            } else if trend > 0.0 {
                                (
                                    format!("+{:.2} MB/s", trend),
                                    egui::Color32::from_rgb(255, 200, 100),
                                )
                            } else {
                                (
                                    format!("{:.2} MB/s", trend),
                                    egui::Color32::from_rgb(100, 200, 255),
                                )
                            };
                            ui.colored_label(trend_color, trend_text);
                            ui.end_row();
                        });
                });

            ui.separator();
            ui.small("F8 toggle | Updates every frame");
        });
}

// ============================================================================
// Helpers
// ============================================================================

/// Color for an FPS value (green ≥ 60, yellow ≥ 30, red < 30).
fn fps_color(fps: f64) -> egui::Color32 {
    if fps >= 60.0 {
        egui::Color32::from_rgb(100, 255, 100)
    } else if fps >= 30.0 {
        egui::Color32::from_rgb(255, 255, 100)
    } else {
        egui::Color32::from_rgb(255, 100, 100)
    }
}

/// Color for a frame-time value in ms.
fn frame_time_color(ft_ms: f32) -> egui::Color32 {
    if ft_ms <= 16.67 {
        egui::Color32::from_rgb(100, 255, 100)
    } else if ft_ms <= 33.33 {
        egui::Color32::from_rgb(255, 255, 100)
    } else {
        egui::Color32::from_rgb(255, 100, 100)
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin that registers the performance dashboard resource and update systems.
///
/// The actual egui rendering is driven by the editor UI system in [`super::EditorPlugin`]
/// because it needs access to `EguiContexts`.
pub struct PerformanceDashboardPlugin;

impl Plugin for PerformanceDashboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PerformanceDashboard>()
            .add_systems(
                Update,
                (update_performance_dashboard, dashboard_keyboard_input),
            );
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_dashboard() {
        let d = PerformanceDashboard::default();
        assert!(!d.visible);
        assert_eq!(d.fps, 0.0);
        assert_eq!(d.frame_time_ms, 0.0);
        assert_eq!(d.graph_history.len(), DASHBOARD_HISTORY_SIZE);
        assert_eq!(d.percentile_buffer.len(), PERCENTILE_WINDOW_SIZE);
        assert_eq!(d.percentile_count, 0);
        assert_eq!(d.memory_snapshots.len(), 0);
    }

    #[test]
    fn test_record_frame_time_wraps_graph() {
        let mut d = PerformanceDashboard::default();

        // Fill entire graph buffer + 10 extra to test wrapping
        for i in 0..(DASHBOARD_HISTORY_SIZE + 10) {
            d.record_frame_time(i as f32);
        }

        // Index should have wrapped
        assert_eq!(d.graph_index, 10);
        // The buffer should still be the same size
        assert_eq!(d.graph_history.len(), DASHBOARD_HISTORY_SIZE);
        // Last written value should be at index 9
        assert_eq!(d.graph_history[9], (DASHBOARD_HISTORY_SIZE + 9) as f32);
    }

    #[test]
    fn test_record_frame_time_wraps_percentile() {
        let mut d = PerformanceDashboard::default();

        for i in 0..(PERCENTILE_WINDOW_SIZE + 20) {
            d.record_frame_time(i as f32);
        }

        assert_eq!(d.percentile_index, 20);
        assert_eq!(d.percentile_count, PERCENTILE_WINDOW_SIZE);
    }

    #[test]
    fn test_ordered_graph_history() {
        let mut d = PerformanceDashboard::default();

        // Write 5 samples
        for i in 1..=5 {
            d.record_frame_time(i as f32);
        }

        let history = d.ordered_graph_history();
        assert_eq!(history.len(), DASHBOARD_HISTORY_SIZE);
        // First (DASHBOARD_HISTORY_SIZE - 5) entries should be 0.0
        for &v in &history[..(DASHBOARD_HISTORY_SIZE - 5)] {
            assert_eq!(v, 0.0);
        }
        // Last 5 should be 1..=5
        let tail = &history[(DASHBOARD_HISTORY_SIZE - 5)..];
        assert_eq!(tail, &[1.0, 2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn test_recompute_stats_empty() {
        let mut d = PerformanceDashboard::default();
        d.recompute_stats();
        assert_eq!(d.frame_time_min_ms, 0.0);
        assert_eq!(d.frame_time_max_ms, 0.0);
        assert_eq!(d.frame_time_avg_ms, 0.0);
        assert_eq!(d.frames_over_budget, 0);
    }

    #[test]
    fn test_recompute_stats_with_data() {
        let mut d = PerformanceDashboard::default();

        // Record a mix of frame times
        let samples = [10.0, 12.0, 14.0, 16.0, 20.0, 30.0, 50.0];
        for &s in &samples {
            d.record_frame_time(s);
        }

        d.frame_time_ms = 16.0; // simulate smoothed value
        d.recompute_stats();

        assert!((d.frame_time_min_ms - 10.0).abs() < 0.01);
        assert!((d.frame_time_max_ms - 50.0).abs() < 0.01);

        let expected_avg = samples.iter().sum::<f32>() / samples.len() as f32;
        assert!((d.frame_time_avg_ms - expected_avg).abs() < 0.01);

        // Budget usage: 16.0 / 16.67 ≈ 0.96
        assert!((d.budget_usage - 16.0 / TARGET_FRAME_TIME_MS).abs() < 0.01);

        // Frames over budget: 20, 30, 50 → 3
        assert_eq!(d.frames_over_budget, 3);
    }

    #[test]
    fn test_percentile_computation() {
        let mut d = PerformanceDashboard::default();

        // Record 100 samples: 1..=100 ms
        for i in 1..=100 {
            d.record_frame_time(i as f32);
        }

        d.recompute_stats();

        // P99 should be high (99th or 100th value)
        assert!(d.p1_frame_time_ms >= 99.0);
        // P99.9 should be 100
        assert!(d.p01_frame_time_ms >= 100.0);

        // 1% low FPS ~ 1000 / 99 ≈ 10.1
        assert!(d.fps_1_low > 0.0);
        assert!(d.fps_1_low < 12.0);
    }

    #[test]
    fn test_memory_trend_stable() {
        let mut d = PerformanceDashboard::default();

        // Record same memory 10 times
        for _ in 0..10 {
            d.record_memory_snapshot(100_000_000);
        }

        assert!(
            d.memory_trend_mb_per_sec.abs() < 0.01,
            "Trend should be ~0 for stable memory, got {}",
            d.memory_trend_mb_per_sec
        );
    }

    #[test]
    fn test_memory_trend_growing() {
        let mut d = PerformanceDashboard::default();

        // Simulate growing memory: each snapshot 1MB larger
        for i in 0..10 {
            d.record_memory_snapshot(100_000_000 + i * 1_048_576);
        }

        assert!(
            d.memory_trend_mb_per_sec > 0.0,
            "Trend should be positive for growing memory, got {}",
            d.memory_trend_mb_per_sec
        );
    }

    #[test]
    fn test_memory_trend_capped_samples() {
        let mut d = PerformanceDashboard::default();

        // Record more than MEMORY_TREND_SAMPLES
        for i in 0..(MEMORY_TREND_SAMPLES + 20) {
            d.record_memory_snapshot(100_000_000 + i * 1000);
        }

        assert!(
            d.memory_snapshots.len() <= MEMORY_TREND_SAMPLES,
            "Should not exceed max samples"
        );
    }

    #[test]
    fn test_fps_color_thresholds() {
        let green = fps_color(60.0);
        let yellow = fps_color(45.0);
        let red = fps_color(20.0);

        assert_eq!(green, egui::Color32::from_rgb(100, 255, 100));
        assert_eq!(yellow, egui::Color32::from_rgb(255, 255, 100));
        assert_eq!(red, egui::Color32::from_rgb(255, 100, 100));
    }

    #[test]
    fn test_frame_time_color_thresholds() {
        let green = frame_time_color(10.0);
        let yellow = frame_time_color(25.0);
        let red = frame_time_color(40.0);

        assert_eq!(green, egui::Color32::from_rgb(100, 255, 100));
        assert_eq!(yellow, egui::Color32::from_rgb(255, 255, 100));
        assert_eq!(red, egui::Color32::from_rgb(255, 100, 100));
    }
}
