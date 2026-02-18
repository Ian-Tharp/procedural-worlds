//! Unified Debug Console + Performance Overlay (F2)
//!
//! A single interactive overlay toggled with **F2** that displays real-time
//! performance metrics and debug information in collapsible panels:
//!
//! - **Performance**: FPS, frame time with color-coded warnings
//! - **World**: Loaded chunks, chunk load time, cache hit rate
//! - **Entities**: Total ECS entity count
//! - **System**: Process memory usage (RSS + peak)
//!
//! Uses the centralized [`PerformanceMetrics`] resource from `engine::metrics`
//! and reads entity counts from Bevy's `EntityCountDiagnosticsPlugin`.
//!
//! # Design Decisions
//!
//! - Merges concepts from `feature/debug-console-unified` (panel structure)
//!   and `feature/f2-minimal-perf-overlay` (F2 toggle + metric rendering).
//! - Renders as a floating egui `Window` (draggable, collapsible) rather than
//!   a fixed `Area`, so the user can reposition it.
//! - Each section is an `egui::CollapsingHeader` for selective detail.
//! - Memory queries are throttled in `engine::metrics`; we just read the value.

use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::engine::metrics::{PerformanceMetrics, PerformanceOverlay, PerformanceThresholds, WarningLevel};
use crate::world::ChunkLoadMetrics;

// ============================================================================
// Helper
// ============================================================================

/// Convert a [`WarningLevel`] to an egui color.
fn level_color(level: WarningLevel) -> egui::Color32 {
    let (r, g, b) = level.to_rgb();
    egui::Color32::from_rgb(r, g, b)
}

// ============================================================================
// Render System
// ============================================================================

/// Renders the unified F2 performance overlay with collapsible panels.
///
/// Only runs when [`PerformanceOverlay::visible`] is `true`.
pub fn render_unified_overlay(
    mut contexts: EguiContexts,
    overlay: Res<PerformanceOverlay>,
    metrics: Res<PerformanceMetrics>,
    thresholds: Res<PerformanceThresholds>,
    diagnostics: Res<DiagnosticsStore>,
    chunk_load_metrics: Option<Res<ChunkLoadMetrics>>,
) {
    if !overlay.visible {
        return;
    }

    let warnings = metrics.warning_levels(&thresholds);

    egui::Window::new("⚡ Performance Overlay")
        .id(egui::Id::new("unified_perf_overlay_f2"))
        .default_pos([overlay.x, overlay.y])
        .default_width(280.0)
        .resizable(true)
        .collapsible(true)
        .title_bar(true)
        .show(contexts.ctx_mut(), |ui| {
            // ── Performance ──────────────────────────────────────────
            egui::CollapsingHeader::new("📊 Performance")
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new("perf_grid")
                        .num_columns(2)
                        .spacing([12.0, 2.0])
                        .show(ui, |ui| {
                            ui.label("FPS:");
                            ui.colored_label(
                                level_color(warnings.fps),
                                format!("{:.1}", metrics.fps),
                            );
                            ui.end_row();

                            ui.label("Frame time:");
                            ui.colored_label(
                                level_color(warnings.frame_time),
                                format!("{:.2} ms", metrics.frame_time_ms),
                            );
                            ui.end_row();
                        });
                });

            ui.separator();

            // ── World ────────────────────────────────────────────────
            egui::CollapsingHeader::new("🌍 World")
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new("world_grid")
                        .num_columns(2)
                        .spacing([12.0, 2.0])
                        .show(ui, |ui| {
                            ui.label("Loaded chunks:");
                            ui.monospace(format!("{}", metrics.active_chunks));
                            ui.end_row();

                            ui.label("Avg chunk load:");
                            ui.colored_label(
                                level_color(warnings.chunk_load),
                                format!("{:.1} ms", metrics.avg_chunk_load_ms),
                            );
                            ui.end_row();

                            ui.label("Chunks/sec:");
                            ui.monospace(format!("{:.1}", metrics.chunks_per_second));
                            ui.end_row();

                            // Show cache stats if available
                            if let Some(ref clm) = chunk_load_metrics {
                                let total = clm.cache_hits + clm.cache_misses;
                                if total > 0 {
                                    let hit_pct = clm.cache_hit_rate * 100.0;
                                    let cache_color = if hit_pct >= 50.0 {
                                        egui::Color32::from_rgb(100, 255, 100)
                                    } else if hit_pct >= 20.0 {
                                        egui::Color32::from_rgb(255, 255, 100)
                                    } else {
                                        egui::Color32::from_rgb(255, 100, 100)
                                    };
                                    ui.label("Cache hit rate:");
                                    ui.colored_label(cache_color, format!("{:.1}%", hit_pct));
                                    ui.end_row();
                                }

                                // Chunk memory
                                if clm.chunk_memory_bytes > 0 {
                                    let chunk_mb = clm.chunk_memory_bytes as f64 / (1024.0 * 1024.0);
                                    ui.label("Chunk memory:");
                                    ui.monospace(format!("{:.1} MB", chunk_mb));
                                    ui.end_row();
                                }
                            }
                        });
                });

            ui.separator();

            // ── Entities ─────────────────────────────────────────────
            egui::CollapsingHeader::new("🧩 Entities")
                .default_open(true)
                .show(ui, |ui| {
                    let entity_count = diagnostics
                        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
                        .and_then(|d| d.smoothed())
                        .unwrap_or(0.0);

                    ui.horizontal(|ui| {
                        ui.label("Total:");
                        ui.monospace(format!("{:.0}", entity_count));
                    });
                });

            ui.separator();

            // ── System ───────────────────────────────────────────────
            egui::CollapsingHeader::new("💻 System")
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new("system_grid")
                        .num_columns(2)
                        .spacing([12.0, 2.0])
                        .show(ui, |ui| {
                            ui.label("Memory (RSS):");
                            ui.colored_label(
                                level_color(warnings.memory),
                                format!("{:.1} MB", metrics.memory_mb),
                            );
                            ui.end_row();

                            if let Some(peak) = metrics.peak_memory_mb {
                                ui.label("Peak memory:");
                                ui.monospace(format!("{:.1} MB", peak));
                                ui.end_row();
                            }
                        });
                });

            ui.separator();
            ui.small("F2 toggle | Drag to reposition");
        });
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for the unified F2 performance overlay.
///
/// Requires [`crate::engine::metrics::MetricsPlugin`] to be added first
/// (provides `PerformanceMetrics`, `PerformanceOverlay`, `PerformanceThresholds`).
pub struct UnifiedPerfOverlayPlugin;

impl Plugin for UnifiedPerfOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, render_unified_overlay);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_color_mapping() {
        // Ensure level_color returns distinct colors for each level
        let normal = level_color(WarningLevel::Normal);
        let warning = level_color(WarningLevel::Warning);
        let critical = level_color(WarningLevel::Critical);

        assert_ne!(normal, warning);
        assert_ne!(warning, critical);
        assert_ne!(normal, critical);
    }
}
