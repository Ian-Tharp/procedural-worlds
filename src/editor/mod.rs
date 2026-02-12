//! Editor systems - UI panels, viewport, tools

pub mod block_highlight;
pub mod chunk_debug;
pub mod content_editor;
pub mod debug_console;
pub mod debug_overlay;
pub mod hud;
pub mod minimap;
pub mod performance;
pub mod worldgen_panel;

pub use block_highlight::BlockHighlightPlugin;
pub use chunk_debug::ChunkDebugPlugin;
pub use content_editor::ContentEditorPlugin;
pub use debug_console::DebugConsolePlugin;
pub use debug_overlay::DebugOverlayPlugin;
pub use hud::HudPlugin;
pub use minimap::MinimapPlugin;
pub use performance::PerformanceDashboardPlugin;
pub use worldgen_panel::WorldGenPanelPlugin;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy_egui::{egui, EguiContexts};

use crate::actors::{Movement, Player};
use crate::config::audio::AudioSettingsPanelState;
use crate::engine::input::ActionStates;
use crate::engine::lighting::DayNightCycle;
use crate::engine::profiler::ProfilerState;
use crate::engine::raycast::CurrentTarget;
use crate::world::ChunkLoadMetrics;

use worldgen_panel::{WorldGenPanelState, RegenerateWorldEvent};

/// System set for editor UI (runs in Update).
///
/// Other sets (e.g. `ControllerInputSet`) should declare `.after(EditorUiSet)`
/// so that egui has drawn all panels before any pointer-ownership checks run.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EditorUiSet;

/// Plugin for editor UI systems
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_plugins(BlockHighlightPlugin)
            .add_plugins(WorldGenPanelPlugin)
            .add_plugins(crate::rendering::ShaderHotReloadPlugin)
            .init_resource::<EditorState>()
            .add_systems(
                Update,
                editor_ui_system.in_set(EditorUiSet),
            );
    }
}

/// Editor state and settings
#[derive(Resource)]
pub struct EditorState {
    /// Show the inspector panel
    pub show_inspector: bool,
    /// Show the world settings panel
    pub show_world_settings: bool,
    /// Player world position for display (feet position)
    pub player_position: Vec3,
    /// Camera world position for display (eye position)
    pub camera_position: Vec3,
    /// Camera yaw angle in degrees (for compass)
    pub camera_yaw: f32,
    /// Number of loaded chunks
    pub chunk_count: usize,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            show_inspector: true,
            show_world_settings: false,
            player_position: Vec3::ZERO,
            camera_position: Vec3::ZERO,
            camera_yaw: 0.0,
            chunk_count: 0,
        }
    }
}

/// Convert yaw angle to cardinal direction string
fn yaw_to_cardinal(yaw: f32) -> &'static str {
    // Normalize yaw to 0-360
    let yaw = ((yaw % 360.0) + 360.0) % 360.0;

    // Cardinal directions (yaw 0 = looking towards -Z in Bevy)
    // Adjust based on how camera is oriented
    if yaw >= 337.5 || yaw < 22.5 {
        "N"
    } else if yaw >= 22.5 && yaw < 67.5 {
        "NE"
    } else if yaw >= 67.5 && yaw < 112.5 {
        "E"
    } else if yaw >= 112.5 && yaw < 157.5 {
        "SE"
    } else if yaw >= 157.5 && yaw < 202.5 {
        "S"
    } else if yaw >= 202.5 && yaw < 247.5 {
        "SW"
    } else if yaw >= 247.5 && yaw < 292.5 {
        "W"
    } else {
        "NW"
    }
}

/// Render the floating profiler overlay window showing per-scope timing data.
fn draw_profiler_overlay(ui_ctx: &mut egui::Context, profiler: &ProfilerState) {
    egui::Window::new("🔬 System Profiler")
        .default_pos([400.0, 120.0])
        .default_width(380.0)
        .resizable(true)
        .collapsible(true)
        .show(ui_ctx, |ui| {
            // ── Frame time headline ─────────────────────────
            ui.horizontal(|ui| {
                let frame_ms = profiler.current_frame_ms();
                let fps = if frame_ms > 0.0 { 1000.0 / frame_ms } else { 0.0 };
                let color = if fps >= 60.0 {
                    egui::Color32::from_rgb(100, 255, 100)
                } else if fps >= 30.0 {
                    egui::Color32::from_rgb(255, 255, 100)
                } else {
                    egui::Color32::from_rgb(255, 100, 100)
                };
                ui.colored_label(
                    color,
                    egui::RichText::new(format!("{:.1} FPS", fps))
                        .strong()
                        .size(18.0),
                );
                ui.monospace(format!(
                    "frame: {:.2} ms  avg: {:.2} ms",
                    frame_ms,
                    profiler.avg_frame_ms()
                ));
            });

            ui.horizontal(|ui| {
                ui.label("Frames profiled:");
                ui.monospace(format!("{}", profiler.total_frames));
                ui.label("Scopes:");
                ui.monospace(format!("{}", profiler.scopes.len()));
            });

            ui.separator();

            // ── Scope breakdown (sorted by avg time) ────────
            egui::CollapsingHeader::new("⏱ System Scopes")
                .default_open(true)
                .show(ui, |ui| {
                    let sorted = profiler.scopes_sorted_by_avg();
                    if sorted.is_empty() {
                        ui.colored_label(
                            egui::Color32::from_rgb(150, 150, 150),
                            "No scopes recorded yet",
                        );
                    } else {
                        egui::Grid::new("profiler_scope_grid")
                            .num_columns(5)
                            .spacing([12.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                // Header
                                ui.label(egui::RichText::new("Scope").strong());
                                ui.label(egui::RichText::new("Last").strong());
                                ui.label(egui::RichText::new("Avg").strong());
                                ui.label(egui::RichText::new("Max").strong());
                                ui.label(egui::RichText::new("Hits").strong());
                                ui.end_row();

                                for scope in &sorted {
                                    ui.label(&scope.name);

                                    // Format based on magnitude
                                    let fmt_us = |us: f64| -> String {
                                        if us >= 1000.0 {
                                            format!("{:.2} ms", us / 1000.0)
                                        } else {
                                            format!("{:.0} µs", us)
                                        }
                                    };

                                    let last_color = scope_time_color(scope.last_us);
                                    ui.colored_label(last_color, fmt_us(scope.last_us));

                                    let avg_color = scope_time_color(scope.avg_us);
                                    ui.colored_label(avg_color, fmt_us(scope.avg_us));

                                    let max_color = scope_time_color(scope.max_us);
                                    ui.colored_label(max_color, fmt_us(scope.max_us));

                                    ui.monospace(format!("{}", scope.total_hits));
                                    ui.end_row();
                                }
                            });
                    }
                });

            ui.separator();

            // ── Frame time mini-graph ────────────────────────
            egui::CollapsingHeader::new("📊 Frame Time Graph")
                .default_open(true)
                .show(ui, |ui| {
                    let history = profiler.ordered_frame_history();
                    if history.is_empty() {
                        ui.label("No data yet");
                        return;
                    }

                    let recent: Vec<f64> = history.iter().rev().take(120).copied().collect();
                    // Target 16.67ms = 16670us
                    let target_us = 16_670.0_f64;
                    let max_us = recent
                        .iter()
                        .copied()
                        .fold(target_us * 2.0, f64::max);

                    let graph_width = ui.available_width().min(360.0);
                    let graph_height = 40.0;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(graph_width, graph_height),
                        egui::Sense::hover(),
                    );

                    // Background
                    ui.painter()
                        .rect_filled(rect, 2.0, egui::Color32::from_rgb(20, 20, 30));

                    // 16.67ms target line
                    let target_y = rect.max.y - (target_us / max_us) as f32 * rect.height();
                    ui.painter().line_segment(
                        [
                            egui::pos2(rect.min.x, target_y),
                            egui::pos2(rect.max.x, target_y),
                        ],
                        egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_premultiplied(255, 255, 100, 80),
                        ),
                    );

                    // Bars
                    if !recent.is_empty() {
                        let bar_w = rect.width() / recent.len() as f32;
                        for (i, &ft) in recent.iter().rev().enumerate() {
                            let normalized = (ft / max_us).min(1.0) as f32;
                            let h = graph_height * normalized;
                            let x = rect.min.x + i as f32 * bar_w;
                            let bar_rect = egui::Rect::from_min_max(
                                egui::pos2(x, rect.max.y - h),
                                egui::pos2(x + bar_w - 0.5, rect.max.y),
                            );
                            let ft_ms = ft / 1000.0;
                            let color = if ft_ms <= 16.67 {
                                egui::Color32::from_rgb(100, 255, 100)
                            } else if ft_ms <= 33.33 {
                                egui::Color32::from_rgb(255, 255, 100)
                            } else {
                                egui::Color32::from_rgb(255, 100, 100)
                            };
                            ui.painter().rect_filled(bar_rect, 0.0, color);
                        }
                    }

                    ui.small("Yellow line = 16.67ms target | Green ≤60fps | Red >30fps");
                });

            ui.separator();

            // ── Chunk Loading Performance ────────────────────
            egui::CollapsingHeader::new("📦 Chunk Loading Metrics")
                .default_open(true)
                .show(ui, |ui| {
                    let cm = &profiler.chunk_metrics;

                    // Summary stats
                    egui::Grid::new("chunk_metrics_grid")
                        .num_columns(2)
                        .spacing([12.0, 2.0])
                        .show(ui, |ui| {
                            ui.label("Loaded:");
                            ui.monospace(format!("{} chunks", cm.loaded_chunk_count));
                            ui.end_row();

                            ui.label("Total loaded:");
                            ui.monospace(format!("{}", cm.total_chunks_loaded));
                            ui.end_row();

                            ui.label("Chunks/sec:");
                            let cps_color = if cm.chunks_per_second >= 10.0 {
                                egui::Color32::from_rgb(100, 255, 100)
                            } else if cm.chunks_per_second >= 2.0 {
                                egui::Color32::from_rgb(255, 255, 100)
                            } else {
                                egui::Color32::from_rgb(255, 100, 100)
                            };
                            ui.colored_label(cps_color, format!("{:.1}", cm.chunks_per_second));
                            ui.end_row();

                            ui.label("Avg load:");
                            let avg_color = if cm.avg_load_time_ms <= 20.0 {
                                egui::Color32::from_rgb(100, 255, 100)
                            } else if cm.avg_load_time_ms <= 100.0 {
                                egui::Color32::from_rgb(255, 255, 100)
                            } else {
                                egui::Color32::from_rgb(255, 100, 100)
                            };
                            ui.colored_label(avg_color, format!("{:.1} ms", cm.avg_load_time_ms));
                            ui.end_row();

                            ui.label("Peak load:");
                            let peak_color = if cm.peak_load_time_ms <= 50.0 {
                                egui::Color32::from_rgb(100, 255, 100)
                            } else if cm.peak_load_time_ms <= 200.0 {
                                egui::Color32::from_rgb(255, 255, 100)
                            } else {
                                egui::Color32::from_rgb(255, 100, 100)
                            };
                            ui.colored_label(peak_color, format!("{:.1} ms", cm.peak_load_time_ms));
                            ui.end_row();

                            ui.label("All-time peak:");
                            ui.monospace(format!("{:.1} ms", cm.all_time_peak_ms));
                            ui.end_row();
                        });

                    ui.add_space(4.0);

                    // Memory usage
                    ui.label(egui::RichText::new("💾 Memory").strong());
                    ui.horizontal(|ui| {
                        ui.label("Total:");
                        ui.monospace(format!("{:.2} MB", cm.memory_mb));
                        if cm.loaded_chunk_count > 0 {
                            let per_chunk_kb = cm.memory_per_chunk_bytes as f64 / 1024.0;
                            ui.colored_label(
                                egui::Color32::from_rgb(150, 150, 150),
                                format!("({:.1} KB/chunk)", per_chunk_kb),
                            );
                        }
                    });

                    ui.add_space(4.0);

                    // Cache hit/miss rates
                    ui.label(egui::RichText::new("🗄 Cache (Disk vs Generated)").strong());
                    let total_cache = cm.cache_hits + cm.cache_misses;
                    if total_cache > 0 {
                        ui.horizontal(|ui| {
                            ui.label("Hit rate:");
                            let rate_pct = cm.cache_hit_rate * 100.0;
                            let rate_color = if rate_pct >= 50.0 {
                                egui::Color32::from_rgb(100, 255, 100)
                            } else if rate_pct >= 20.0 {
                                egui::Color32::from_rgb(255, 255, 100)
                            } else {
                                egui::Color32::from_rgb(255, 100, 100)
                            };
                            ui.colored_label(rate_color, format!("{:.1}%", rate_pct));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Hits:");
                            ui.colored_label(
                                egui::Color32::from_rgb(100, 200, 100),
                                format!("{}", cm.cache_hits),
                            );
                            ui.label("Misses:");
                            ui.colored_label(
                                egui::Color32::from_rgb(200, 100, 100),
                                format!("{}", cm.cache_misses),
                            );
                        });

                        // Mini cache hit rate bar
                        let bar_width = ui.available_width().min(200.0);
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(bar_width, 10.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(
                            rect,
                            2.0,
                            egui::Color32::from_rgb(200, 60, 60),
                        );
                        let hit_width = rect.width() * cm.cache_hit_rate;
                        if hit_width > 0.0 {
                            let hit_rect = egui::Rect::from_min_max(
                                rect.min,
                                egui::pos2(rect.min.x + hit_width, rect.max.y),
                            );
                            ui.painter().rect_filled(
                                hit_rect,
                                2.0,
                                egui::Color32::from_rgb(60, 200, 60),
                            );
                        }
                        ui.small("Green = disk loaded | Red = generated");
                    } else {
                        ui.colored_label(
                            egui::Color32::from_rgb(150, 150, 150),
                            "No chunks loaded yet",
                        );
                    }

                    ui.add_space(4.0);

                    // Load time graph (recent individual chunk load times)
                    if !cm.recent_load_times_ms.is_empty() {
                        ui.label(egui::RichText::new("⏱ Load Time History").strong());

                        let times = &cm.recent_load_times_ms;
                        let max_time = times
                            .iter()
                            .copied()
                            .fold(50.0_f32, f32::max);

                        let graph_width = ui.available_width().min(360.0);
                        let graph_height = 35.0;
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(graph_width, graph_height),
                            egui::Sense::hover(),
                        );

                        // Background
                        ui.painter().rect_filled(
                            rect,
                            2.0,
                            egui::Color32::from_rgb(20, 20, 30),
                        );

                        // 50ms target line
                        let target_ms = 50.0_f32;
                        if target_ms < max_time {
                            let target_y = rect.max.y
                                - (target_ms / max_time) * rect.height();
                            ui.painter().line_segment(
                                [
                                    egui::pos2(rect.min.x, target_y),
                                    egui::pos2(rect.max.x, target_y),
                                ],
                                egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgba_premultiplied(
                                        255, 200, 100, 80,
                                    ),
                                ),
                            );
                        }

                        // Bars for each chunk load time (most recent on right)
                        let display_count = times.len().min(60);
                        let bar_w = rect.width() / display_count as f32;
                        let start_idx = times.len().saturating_sub(display_count);
                        for (i, &t) in times[start_idx..].iter().enumerate() {
                            let normalized = (t / max_time).min(1.0);
                            let h = graph_height * normalized;
                            let x = rect.min.x + i as f32 * bar_w;
                            let bar_rect = egui::Rect::from_min_max(
                                egui::pos2(x, rect.max.y - h),
                                egui::pos2(x + bar_w - 0.5, rect.max.y),
                            );
                            let color = if t <= 20.0 {
                                egui::Color32::from_rgb(100, 255, 100)
                            } else if t <= 100.0 {
                                egui::Color32::from_rgb(255, 255, 100)
                            } else {
                                egui::Color32::from_rgb(255, 100, 100)
                            };
                            ui.painter().rect_filled(bar_rect, 0.0, color);
                        }

                        ui.small("Per-chunk load time | Green ≤20ms | Yellow ≤100ms | Red >100ms");
                    }
                });

            ui.separator();
            ui.small("F4 toggle | Profiler tracks per-system execution times");
        });
}

/// Color for a scope timing value in microseconds.
fn scope_time_color(us: f64) -> egui::Color32 {
    let ms = us / 1000.0;
    if ms <= 1.0 {
        egui::Color32::from_rgb(100, 255, 100) // Fast: ≤1ms
    } else if ms <= 5.0 {
        egui::Color32::from_rgb(255, 255, 100) // Moderate: ≤5ms
    } else {
        egui::Color32::from_rgb(255, 100, 100) // Slow: >5ms
    }
}

/// Bundled panel state resources to reduce system parameter count
#[derive(SystemParam)]
pub struct EditorPanelStates<'w> {
    pub editor: ResMut<'w, EditorState>,
    pub overlay: ResMut<'w, debug_overlay::DebugOverlayState>,
    pub chunk_debug: ResMut<'w, chunk_debug::ChunkDebugState>,
    pub audio: ResMut<'w, AudioSettingsPanelState>,
    pub worldgen: ResMut<'w, WorldGenPanelState>,
    pub regenerate_events: EventWriter<'w, RegenerateWorldEvent>,
}

/// Bundled optional resources to reduce system parameter count
#[derive(SystemParam)]
pub struct EditorOptionalRes<'w> {
    pub profiler: Option<Res<'w, ProfilerState>>,
    pub physics: Option<Res<'w, crate::physics::PlayerPhysics>>,
    pub current_target: Option<Res<'w, CurrentTarget>>,
    pub day_night: Option<Res<'w, DayNightCycle>>,
    pub action_states: Option<Res<'w, ActionStates>>,
    pub load_metrics: Option<Res<'w, ChunkLoadMetrics>>,
}

/// Main editor UI system
#[allow(clippy::too_many_arguments)]
fn editor_ui_system(
    mut contexts: EguiContexts,
    mut panels: EditorPanelStates,
    optional: EditorOptionalRes,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut chunk_manager: Option<ResMut<crate::world::ChunkManager>>,
    player_transform_query: Query<&GlobalTransform, With<Player>>,
    mut player_query: Query<&mut Movement, With<Player>>,
) {
    // Update player position for display (world-space, robust to parenting)
    if let Ok(player_global) = player_transform_query.get_single() {
        panels.editor.player_position = player_global.translation();
    }

    // Update camera position and rotation for display (world-space)
    if let Ok(camera_global) = camera_query.get_single() {
        panels.editor.camera_position = camera_global.translation();
        // Extract yaw from camera rotation
        let camera_transform = camera_global.compute_transform();
        let (yaw, _pitch, _roll) = camera_transform
            .rotation
            .to_euler(bevy::math::EulerRot::YXZ);
        panels.editor.camera_yaw = yaw.to_degrees();
    }

    // Update chunk count
    if let Some(ref cm) = chunk_manager {
        panels.editor.chunk_count = cm.chunks.len();
    }

    // Top menu bar
    egui::TopBottomPanel::top("menu_bar").show(contexts.ctx_mut(), |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New World").clicked() {
                    info!("New World requested");
                    ui.close_menu();
                }
                if ui.button("Open World...").clicked() {
                    info!("Open World requested");
                    ui.close_menu();
                }
                if ui.button("Save World").clicked() {
                    info!("Save World requested");
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    std::process::exit(0);
                }
            });

            ui.menu_button("View", |ui| {
                ui.checkbox(&mut panels.editor.show_inspector, "Inspector");
                ui.checkbox(&mut panels.editor.show_world_settings, "World Settings");
                ui.separator();
                ui.checkbox(&mut panels.audio.visible, "Audio Settings (F9)");
                ui.separator();
                ui.checkbox(&mut panels.overlay.visible, "Debug (F3)");
            });

            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    info!("Procedural Worlds Engine v{}", env!("CARGO_PKG_VERSION"));
                    ui.close_menu();
                }
            });

            // Right-aligned FPS counter (smoothed, from debug overlay state)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let fps = panels.overlay.cached_fps;
                let fps_color = if fps >= 60.0 {
                    egui::Color32::from_rgb(100, 255, 100)
                } else if fps >= 30.0 {
                    egui::Color32::from_rgb(255, 255, 100)
                } else {
                    egui::Color32::from_rgb(255, 100, 100)
                };
                ui.colored_label(fps_color, format!("{:.0} FPS", fps));
            });
        });
    });

    // Left panel - Inspector (includes debug sections when enabled)
    if panels.editor.show_inspector {
        egui::SidePanel::left("inspector")
            .default_width(280.0)
            .show(contexts.ctx_mut(), |ui| {
                // Use a scroll area so the panel is scrollable when content overflows
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Inspector");
                    ui.separator();

                    egui::CollapsingHeader::new("Player")
                        .default_open(true)
                        .show(ui, |ui| {
                            let pos = panels.editor.player_position;
                            ui.label(format!(
                                "Feet: ({:.1}, {:.1}, {:.1})",
                                pos.x, pos.y, pos.z
                            ));
                            let cam = panels.editor.camera_position;
                            ui.label(format!(
                                "Camera: ({:.1}, {:.1}, {:.1})",
                                cam.x, cam.y, cam.z
                            ));

                            // Compass direction
                            let cardinal = yaw_to_cardinal(panels.editor.camera_yaw);
                            ui.horizontal(|ui| {
                                ui.label("Facing:");
                                ui.label(
                                    egui::RichText::new(cardinal)
                                        .strong()
                                        .color(egui::Color32::from_rgb(100, 200, 255))
                                );
                                ui.label(format!("({:.0}°)", panels.editor.camera_yaw));
                            });

                            ui.separator();

                            // Physics mode toggles - modify Movement component directly
                            if let Ok(mut movement) = player_query.get_single_mut() {
                                ui.horizontal(|ui| {
                                    let flying_text = if movement.flying {
                                        egui::RichText::new("Flying").color(egui::Color32::from_rgb(100, 255, 100))
                                    } else {
                                        egui::RichText::new("Walking").color(egui::Color32::from_rgb(255, 200, 100))
                                    };
                                    ui.checkbox(&mut movement.flying, flying_text);
                                });
                                ui.horizontal(|ui| {
                                    let noclip_text = if movement.noclip {
                                        egui::RichText::new("Noclip").color(egui::Color32::from_rgb(255, 100, 100))
                                    } else {
                                        egui::RichText::new("Collision").color(egui::Color32::from_rgb(150, 150, 150))
                                    };
                                    ui.checkbox(&mut movement.noclip, noclip_text);
                                });
                                ui.separator();
                            }

                            ui.label("Controls:");
                            ui.label("  WASD - Move");
                            if let Some(ref phys) = optional.physics {
                                if phys.flying {
                                    ui.label("  Space/Ctrl - Up/Down");
                                } else {
                                    ui.label("  Space - Jump");
                                }
                            } else if let Ok(movement) = player_query.get_single() {
                                if movement.flying {
                                    ui.label("  Space/Ctrl - Up/Down");
                                } else {
                                    ui.label("  Space - Jump");
                                }
                            }
                            ui.label("  Right-click + Mouse - Look");
                            ui.label("  Shift - Sprint");
                            ui.separator();
                            ui.label("  F - Toggle flying");
                            ui.label("  N - Toggle noclip");
                        });

                    egui::CollapsingHeader::new("Selection")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.label("No selection");
                        });

                    ui.separator();

                    // ── World Generation Config Panel ──
                    if worldgen_panel::draw_worldgen_panel(ui, &mut panels.worldgen) {
                        panels.regenerate_events.send(RegenerateWorldEvent);
                    }

                    // ── Debug sections (toggled via F3 or View menu) ──
                    if panels.overlay.visible {
                        ui.separator();
                        ui.heading("Debug");
                        ui.separator();

                        let chunk_count = chunk_manager.as_ref().map(|cm| cm.chunks.len()).unwrap_or(0);
                        let render_distance = chunk_manager.as_ref().map(|cm| cm.render_distance as u32).unwrap_or(0);
                        let ld = chunk_manager.as_ref().map(|cm| cm.effective_load_distance()).unwrap_or(render_distance as i32);
                        let vert_up = chunk_manager.as_ref().map(|cm| cm.vertical_load_up).unwrap_or(4);
                        let vert_down = chunk_manager.as_ref().map(|cm| cm.vertical_load_down).unwrap_or(2);

                        debug_overlay::draw_debug_ui(
                            ui,
                            &mut panels.overlay,
                            panels.editor.player_position,
                            chunk_count,
                            render_distance,
                            ld,
                            vert_up,
                            vert_down,
                            optional.current_target.as_deref(),
                            optional.day_night.as_deref(),
                            optional.action_states.as_deref(),
                            optional.load_metrics.as_deref(),
                        );

                        // Chunk border legend (shows when F4 overlay is active)
                        if panels.chunk_debug.visible {
                            ui.separator();
                            chunk_debug::draw_chunk_state_legend(
                                ui,
                                &mut panels.chunk_debug,
                            );
                        }
                    }
                });
            });
    }

    // Right panel - World Settings (no bottom debug bar — all debug info is in the inspector now)
    if panels.editor.show_world_settings {
        egui::SidePanel::right("world_settings")
            .default_width(250.0)
            .show(contexts.ctx_mut(), |ui| {
                ui.heading("World Settings");
                ui.separator();

                egui::CollapsingHeader::new("Terrain")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.label("Chunk Size: 16x16x16");

                        // Render distance slider
                        if let Some(ref mut cm) = chunk_manager {
                            ui.horizontal(|ui| {
                                ui.label("View Distance:");
                                if ui.add(egui::Slider::new(&mut cm.render_distance, 2..=16).suffix(" chunks")).changed() {
                                    // Slider changed - chunks will update automatically via streaming system
                                }
                            });
                            ui.label(format!("  ~{} chunks loaded", cm.chunks.len()));
                        } else {
                            ui.label("Render Distance: N/A");
                        }

                        ui.separator();
                        ui.label("Generation:");
                        ui.label("  Seed: 12345");
                        ui.label("  Noise: Simplex");
                    });

                egui::CollapsingHeader::new("Lighting")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.label("Sun Intensity: 15000");
                        ui.label("Ambient: 200");
                        ui.label("Shadows: Enabled");
                    });
            });
    }

    // ── Floating Performance Dashboard (F8) ──
    // Note: Dashboard rendering is now handled by render_dashboard_with_exports
    // in the performance module to support export controls without hitting
    // the 16-parameter system limit.

    // ── Floating Profiler Overlay (F4) ──
    if let Some(ref profiler) = optional.profiler
        && profiler.overlay_visible
    {
        draw_profiler_overlay(contexts.ctx_mut(), profiler);
    }
}
