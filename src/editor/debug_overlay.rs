//! Debug Visualization Overlay
//!
//! A toggleable debug overlay showing real-time engine diagnostics:
//! - Frame time graph with history
//! - Player position and chunk coordinates
//! - Memory usage estimation
//! - Wireframe toggle
//! - Input state visualization

use bevy::pbr::{DirectionalLightShadowMap, NotShadowCaster};
use bevy::prelude::*;
use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy_egui::{egui, EguiContexts};

use crate::actors::Player;
use crate::config::EngineConfig;
use crate::engine::input::{ActionState, ActionStates, InputAction};
use crate::engine::lighting::{DayNightCycle, Sun};
use crate::engine::memory;
use crate::engine::raycast::CurrentTarget;
use crate::world::{ChunkManager, ChunkMesh, CHUNK_SIZE, CHUNK_VOLUME};

/// Number of frame time samples to keep for the graph
const FRAME_TIME_HISTORY_SIZE: usize = 120;

/// Estimated bytes per block in a chunk
const BYTES_PER_BLOCK: usize = 2; // u16 BlockType

/// Estimated bytes per chunk mesh vertex (position + normal + uv)
const BYTES_PER_VERTEX: usize = 32;

/// Average vertices per non-empty chunk (rough estimate)
const AVG_VERTICES_PER_CHUNK: usize = 2000;

/// Debug overlay configuration and state
#[derive(Resource)]
pub struct DebugOverlayState {
    /// Whether the debug overlay is visible
    pub visible: bool,
    /// Frame time history for graphing (in milliseconds)
    frame_time_history: Vec<f32>,
    /// Current index in the circular buffer
    history_index: usize,
    /// Time since last history update
    update_timer: f32,
    /// Whether wireframe mode is enabled
    pub wireframe_enabled: bool,
    /// Show input state panel
    pub show_input_state: bool,
    /// Show memory panel
    pub show_memory: bool,
    /// Show chunk statistics panel
    pub show_chunks: bool,
    /// Show rendering settings panel
    pub show_render: bool,
    /// Cached FPS value (smoothed for stable display)
    cached_fps: f64,
    /// Cached frame time in ms
    cached_frame_time_ms: f64,
    /// Cached process memory snapshot
    cached_process_memory: Option<memory::ProcessMemory>,
    /// Cached entity count
    cached_entity_count: f64,
    /// Timer for periodic metric refresh (avoids per-frame OS queries)
    metrics_refresh_timer: f32,
}

impl Default for DebugOverlayState {
    fn default() -> Self {
        Self {
            visible: true,
            frame_time_history: vec![0.0; FRAME_TIME_HISTORY_SIZE],
            history_index: 0,
            update_timer: 0.0,
            wireframe_enabled: false,
            show_input_state: false,
            show_memory: true,
            show_chunks: true,
            show_render: true,
            cached_fps: 0.0,
            cached_frame_time_ms: 0.0,
            cached_process_memory: None,
            cached_entity_count: 0.0,
            metrics_refresh_timer: 0.0,
        }
    }
}

impl DebugOverlayState {
    /// Record a frame time sample
    pub fn record_frame_time(&mut self, frame_time_ms: f32) {
        self.frame_time_history[self.history_index] = frame_time_ms;
        self.history_index = (self.history_index + 1) % FRAME_TIME_HISTORY_SIZE;
    }

    /// Get frame time history in order (oldest to newest)
    pub fn get_ordered_history(&self) -> Vec<f32> {
        let mut result = Vec::with_capacity(FRAME_TIME_HISTORY_SIZE);
        for i in 0..FRAME_TIME_HISTORY_SIZE {
            let idx = (self.history_index + i) % FRAME_TIME_HISTORY_SIZE;
            result.push(self.frame_time_history[idx]);
        }
        result
    }

    /// Calculate statistics from frame time history
    pub fn frame_time_stats(&self) -> (f32, f32, f32) {
        let history = &self.frame_time_history;
        let valid: Vec<f32> = history.iter().copied().filter(|&t| t > 0.0).collect();
        
        if valid.is_empty() {
            return (0.0, 0.0, 0.0);
        }

        let avg = valid.iter().sum::<f32>() / valid.len() as f32;
        let min = valid.iter().copied().fold(f32::INFINITY, f32::min);
        let max = valid.iter().copied().fold(0.0, f32::max);
        
        (avg, min, max)
    }
}

/// Convert world position to chunk coordinates
pub fn world_to_chunk_coords(world_pos: Vec3) -> IVec3 {
    let chunk_size = CHUNK_SIZE as f32;
    IVec3::new(
        (world_pos.x / chunk_size).floor() as i32,
        (world_pos.y / chunk_size).floor() as i32,
        (world_pos.z / chunk_size).floor() as i32,
    )
}

/// Estimate memory usage from chunks
fn estimate_memory_usage(chunk_count: usize) -> (f64, f64, f64) {
    // Block data memory
    let block_data_bytes = chunk_count * CHUNK_VOLUME * BYTES_PER_BLOCK;
    
    // Mesh memory (rough estimate)
    let mesh_bytes = chunk_count * AVG_VERTICES_PER_CHUNK * BYTES_PER_VERTEX;
    
    // Total
    let total_bytes = block_data_bytes + mesh_bytes;
    
    // Convert to MB
    let block_mb = block_data_bytes as f64 / (1024.0 * 1024.0);
    let mesh_mb = mesh_bytes as f64 / (1024.0 * 1024.0);
    let total_mb = total_bytes as f64 / (1024.0 * 1024.0);
    
    (block_mb, mesh_mb, total_mb)
}

/// System to update frame time history and cached performance metrics
pub fn update_frame_time_history(
    diagnostics: Res<DiagnosticsStore>,
    mut overlay_state: ResMut<DebugOverlayState>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    overlay_state.update_timer += dt;
    overlay_state.metrics_refresh_timer += dt;

    // Update history every frame for smooth graphing
    if overlay_state.update_timer >= 1.0 / 60.0 {
        overlay_state.update_timer = 0.0;

        if let Some(fps_diagnostic) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS) {
            if let Some(fps) = fps_diagnostic.value()
                && fps > 0.0
            {
                let frame_time_ms = 1000.0 / fps;
                overlay_state.record_frame_time(frame_time_ms as f32);
            }

            // Cache smoothed FPS for the performance panel
            if let Some(fps_smoothed) = fps_diagnostic.smoothed() {
                overlay_state.cached_fps = fps_smoothed;
                if fps_smoothed > 0.0 {
                    overlay_state.cached_frame_time_ms = 1000.0 / fps_smoothed;
                }
            }
        }

        // Cache entity count from diagnostics
        if let Some(entity_diag) =
            diagnostics.get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
            && let Some(count) = entity_diag.value()
        {
            overlay_state.cached_entity_count = count;
        }
    }

    // Refresh OS memory query at a lower frequency (every 0.5s) to avoid overhead
    if overlay_state.metrics_refresh_timer >= 0.5 {
        overlay_state.metrics_refresh_timer = 0.0;
        overlay_state.cached_process_memory = memory::get_process_memory();
    }
}

/// System to toggle wireframe mode
pub fn toggle_wireframe(
    overlay_state: Res<DebugOverlayState>,
    mut wireframe_config: ResMut<WireframeConfig>,
) {
    if wireframe_config.global != overlay_state.wireframe_enabled {
        wireframe_config.global = overlay_state.wireframe_enabled;
    }
}

/// Wireframe configuration resource (if not using bevy's built-in)
#[derive(Resource, Default)]
pub struct WireframeConfig {
    pub global: bool,
}

/// System to render the debug overlay window
pub fn debug_overlay_ui(
    mut contexts: EguiContexts,
    mut overlay_state: ResMut<DebugOverlayState>,
    player_query: Query<&GlobalTransform, With<Player>>,
    chunk_manager: Option<Res<ChunkManager>>,
    day_night: Option<Res<DayNightCycle>>,
    action_states: Option<Res<ActionStates>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    current_target: Option<Res<CurrentTarget>>,
    config: Res<EngineConfig>,
    shadow_map: Option<Res<DirectionalLightShadowMap>>,
    mut sun_query: Query<&mut DirectionalLight, With<Sun>>,
    not_shadow_caster_query: Query<(), (With<ChunkMesh>, With<NotShadowCaster>)>,
    shadow_caster_query: Query<(), (With<ChunkMesh>, Without<NotShadowCaster>)>,
) {
    // Toggle overlay with F3
    if keyboard.just_pressed(KeyCode::F3) {
        overlay_state.visible = !overlay_state.visible;
    }

    // Toggle shadows with F7
    if keyboard.just_pressed(KeyCode::F7) {
        for mut light in &mut sun_query {
            light.shadows_enabled = !light.shadows_enabled;
            info!("Shadows toggled: {}", light.shadows_enabled);
        }
    }

    if !overlay_state.visible {
        return;
    }

    // Get player world position (feet). This is the canonical "where am I?" location.
    let player_pos = player_query
        .get_single()
        .map(|t| t.translation())
        .unwrap_or(Vec3::ZERO);
    let chunk_coords = world_to_chunk_coords(player_pos);
    
    // Get chunk count
    let chunk_count = chunk_manager.as_ref().map(|cm| cm.chunks.len()).unwrap_or(0);

    // Main debug overlay window (top-left corner)
    egui::Window::new("🔧 Debug Overlay")
        .default_pos([10.0, 40.0])
        .default_width(280.0)
        .collapsible(true)
        .resizable(true)
        .show(contexts.ctx_mut(), |ui| {
            // ── Performance summary (always visible at top) ─────────
            {
                let fps = overlay_state.cached_fps;
                let frame_ms = overlay_state.cached_frame_time_ms;

                // FPS color: green ≥60, yellow ≥30, red <30
                let fps_color = if fps >= 60.0 {
                    egui::Color32::from_rgb(100, 255, 100)
                } else if fps >= 30.0 {
                    egui::Color32::from_rgb(255, 255, 100)
                } else {
                    egui::Color32::from_rgb(255, 100, 100)
                };

                ui.horizontal(|ui| {
                    ui.label("⚡");
                    ui.colored_label(
                        fps_color,
                        egui::RichText::new(format!("{:.0} FPS", fps))
                            .strong()
                            .size(16.0),
                    );
                    ui.monospace(format!("({:.2} ms)", frame_ms));
                });

                ui.horizontal(|ui| {
                    // Process memory
                    if let Some(ref mem) = overlay_state.cached_process_memory {
                        ui.label("💾");
                        ui.monospace(memory::format_bytes(mem.rss_bytes));
                        if let Some(peak) = mem.peak_rss_bytes {
                            ui.colored_label(
                                egui::Color32::from_rgb(150, 150, 150),
                                format!("(peak {})", memory::format_bytes(peak)),
                            );
                        }
                    } else {
                        ui.label("💾");
                        ui.colored_label(
                            egui::Color32::from_rgb(150, 150, 150),
                            "N/A",
                        );
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("📦");
                    ui.monospace(format!("{} chunks", chunk_count));
                    ui.label("  🧩");
                    ui.monospace(format!("{} entities", overlay_state.cached_entity_count as u64));
                });
            }

            ui.separator();

            // Coordinates section
            ui.collapsing("📍 Position", |ui| {
                ui.horizontal(|ui| {
                    ui.label("World:");
                    ui.monospace(format!(
                        "X:{:.1} Y:{:.1} Z:{:.1}",
                        player_pos.x, player_pos.y, player_pos.z
                    ));
                });
                ui.horizontal(|ui| {
                    ui.label("Chunk:");
                    ui.monospace(format!(
                        "({}, {}, {})",
                        chunk_coords.x, chunk_coords.y, chunk_coords.z
                    ));
                });
                ui.horizontal(|ui| {
                    ui.label("Local:");
                    let local_x =
                        ((player_pos.x % CHUNK_SIZE as f32) + CHUNK_SIZE as f32) % CHUNK_SIZE as f32;
                    let local_y =
                        ((player_pos.y % CHUNK_SIZE as f32) + CHUNK_SIZE as f32) % CHUNK_SIZE as f32;
                    let local_z =
                        ((player_pos.z % CHUNK_SIZE as f32) + CHUNK_SIZE as f32) % CHUNK_SIZE as f32;
                    ui.monospace(format!("({:.1}, {:.1}, {:.1})", local_x, local_y, local_z));
                });
            });

            ui.separator();

            // Target block section
            ui.collapsing("🎯 Target Block", |ui| {
                if let Some(ref target_res) = current_target {
                    if let Some(ref result) = target_res.0 {
                        ui.horizontal(|ui| {
                            ui.label("Block:");
                            ui.monospace(format!(
                                "({}, {}, {})",
                                result.block_pos.x, result.block_pos.y, result.block_pos.z
                            ));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Type:");
                            ui.monospace(format!("{:?}", result.block_type));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Face:");
                            let face_name = match (result.face_normal.x, result.face_normal.y, result.face_normal.z) {
                                (1, 0, 0) => "+X (East)",
                                (-1, 0, 0) => "-X (West)",
                                (0, 1, 0) => "+Y (Top)",
                                (0, -1, 0) => "-Y (Bottom)",
                                (0, 0, 1) => "+Z (South)",
                                (0, 0, -1) => "-Z (North)",
                                _ => "Unknown",
                            };
                            ui.monospace(face_name);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Distance:");
                            ui.monospace(format!("{:.2} blocks", result.distance));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Place at:");
                            ui.monospace(format!(
                                "({}, {}, {})",
                                result.adjacent_pos.x, result.adjacent_pos.y, result.adjacent_pos.z
                            ));
                        });
                    } else {
                        ui.colored_label(
                            egui::Color32::from_rgb(150, 150, 150),
                            "No block in range",
                        );
                    }
                } else {
                    ui.label("Raycast not available");
                }
            });

            ui.separator();

            // Day/night cycle section
            if let Some(ref cycle) = day_night {
                ui.collapsing("🌅 Time of Day", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Clock:");
                        ui.monospace(cycle.clock_display());
                    });
                    ui.horizontal(|ui| {
                        ui.label("Phase:");
                        ui.monospace(cycle.phase_name());
                    });
                    ui.horizontal(|ui| {
                        ui.label("Raw:");
                        ui.monospace(format!("{:.4}", cycle.time_of_day));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Cycle:");
                        ui.monospace(format!("{:.0}s", cycle.cycle_duration));
                        if cycle.paused {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 200, 100),
                                " ⏸ PAUSED",
                            );
                        }
                    });

                    // Visual time-of-day bar
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 12.0),
                        egui::Sense::hover(),
                    );
                    // Background gradient hint (night-dawn-day-dusk-night)
                    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(20, 20, 40));
                    // Sun position marker
                    let marker_x = rect.min.x + rect.width() * cycle.time_of_day;
                    let marker_center = egui::pos2(marker_x, rect.center().y);
                    ui.painter().circle_filled(
                        marker_center,
                        5.0,
                        egui::Color32::from_rgb(255, 220, 80),
                    );
                });

                ui.separator();
            }

            // Frame time graph
            ui.collapsing("📊 Frame Time", |ui| {
                let history = overlay_state.get_ordered_history();
                let (avg, min, max) = overlay_state.frame_time_stats();
                
                ui.horizontal(|ui| {
                    ui.label("Avg:");
                    ui.monospace(format!("{:.2}ms", avg));
                    ui.label("Min:");
                    ui.monospace(format!("{:.2}ms", min));
                    ui.label("Max:");
                    ui.monospace(format!("{:.2}ms", max));
                });
                
                // Calculate FPS from average frame time
                let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
                let fps_color = if fps >= 60.0 {
                    egui::Color32::from_rgb(100, 255, 100)
                } else if fps >= 30.0 {
                    egui::Color32::from_rgb(255, 255, 100)
                } else {
                    egui::Color32::from_rgb(255, 100, 100)
                };
                ui.horizontal(|ui| {
                    ui.label("FPS:");
                    ui.colored_label(fps_color, format!("{:.0}", fps));
                });
                
                // Simple bar graph using progress bars for last 30 frames
                ui.add_space(4.0);
                ui.label("Recent frame times:");
                
                // Take last 30 samples for visualization
                let recent: Vec<f32> = history.iter().rev().take(30).copied().collect();
                let max_frame_time = recent.iter().copied().fold(33.33_f32, f32::max);
                
                // Draw a simple horizontal bar chart
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
                    for &frame_time in recent.iter().rev() {
                        let normalized = (frame_time / max_frame_time).min(1.0);
                        let color = if frame_time <= 16.67 {
                            egui::Color32::from_rgb(100, 255, 100) // Green: good
                        } else if frame_time <= 33.33 {
                            egui::Color32::from_rgb(255, 255, 100) // Yellow: ok
                        } else {
                            egui::Color32::from_rgb(255, 100, 100) // Red: bad
                        };
                        
                        let height = 20.0 * normalized;
                        let (rect, _response) = ui.allocate_exact_size(
                            egui::vec2(4.0, 20.0),
                            egui::Sense::hover(),
                        );
                        
                        // Draw the bar from bottom
                        let bar_rect = egui::Rect::from_min_max(
                            egui::pos2(rect.min.x, rect.max.y - height),
                            rect.max,
                        );
                        ui.painter().rect_filled(bar_rect, 0.0, color);
                    }
                });
                
                ui.small("Green ≤16.7ms (60fps) | Yellow ≤33.3ms (30fps) | Red >33.3ms");
            });

            ui.separator();

            // Memory estimation
            if overlay_state.show_memory {
                ui.collapsing("💾 Memory (estimated)", |ui| {
                    let (block_mb, mesh_mb, total_mb) = estimate_memory_usage(chunk_count);
                    
                    ui.horizontal(|ui| {
                        ui.label("Chunks loaded:");
                        ui.monospace(format!("{}", chunk_count));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Block data:");
                        ui.monospace(format!("{:.1} MB", block_mb));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Mesh data:");
                        ui.monospace(format!("~{:.1} MB", mesh_mb));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Total (est):");
                        ui.strong(format!("~{:.1} MB", total_mb));
                    });
                });
            }

            ui.separator();

            // Chunk statistics
            if overlay_state.show_chunks {
                ui.collapsing("📦 Chunk Statistics", |ui| {
                    let render_distance = chunk_manager
                        .as_ref()
                        .map(|cm| cm.render_distance)
                        .unwrap_or(0);

                    // Vertical range matches chunk_streaming_system: y in -2..=4 (7 levels)
                    let vertical_levels = 7;
                    let side = (2 * render_distance + 1) as usize;
                    let expected_chunks = side * side * vertical_levels;

                    let loaded_pct = if expected_chunks > 0 {
                        (chunk_count as f64 / expected_chunks as f64 * 100.0).min(100.0)
                    } else {
                        0.0
                    };

                    let pending = expected_chunks.saturating_sub(chunk_count);

                    // Memory estimate: block data only (CHUNK_VOLUME × size_of BlockType per chunk)
                    let block_bytes = chunk_count * CHUNK_VOLUME * std::mem::size_of::<crate::world::BlockType>();
                    let block_mb = block_bytes as f64 / (1024.0 * 1024.0);

                    ui.horizontal(|ui| {
                        ui.label("Loaded:");
                        ui.monospace(format!("{}", chunk_count));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Expected:");
                        ui.monospace(format!("{}", expected_chunks));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Pending:");
                        ui.monospace(format!("{}", pending));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Loaded %:");
                        ui.monospace(format!("{:.1}%", loaded_pct));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Render dist:");
                        ui.monospace(format!("{}", render_distance));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Block memory:");
                        ui.monospace(format!("{:.1} MB", block_mb));
                    });
                });
            }

            ui.separator();

            // Rendering debug panel
            if overlay_state.show_render {
                ui.collapsing("🌟 Rendering", |ui| {
                    // Shadow map
                    let shadow_size = shadow_map
                        .as_ref()
                        .map(|sm| sm.size)
                        .unwrap_or(0);
                    ui.horizontal(|ui| {
                        ui.label("Shadow map:");
                        ui.monospace(format!("{}px", shadow_size));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Cascades:");
                        ui.monospace(format!("{}", config.render.shadow_cascade_count));
                    });

                    // Shadow state from sun
                    for light in sun_query.iter() {
                        ui.horizontal(|ui| {
                            ui.label("Shadows:");
                            if light.shadows_enabled {
                                ui.colored_label(
                                    egui::Color32::from_rgb(100, 255, 100),
                                    "ON",
                                );
                            } else {
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 100, 100),
                                    "OFF",
                                );
                            }
                            ui.small("(F7 toggle)");
                        });
                    }

                    // Shadow caster counts
                    let casters = shadow_caster_query.iter().count();
                    let culled = not_shadow_caster_query.iter().count();
                    ui.horizontal(|ui| {
                        ui.label("Shadow casters:");
                        ui.monospace(format!("{}", casters));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Shadow culled:");
                        ui.monospace(format!("{}", culled));
                    });

                    ui.separator();

                    // Bloom
                    ui.horizontal(|ui| {
                        ui.label("Bloom:");
                        if config.render.bloom_enabled {
                            ui.colored_label(
                                egui::Color32::from_rgb(100, 255, 100),
                                format!("ON ({:.2})", config.render.bloom_intensity),
                            );
                        } else {
                            ui.colored_label(
                                egui::Color32::from_rgb(150, 150, 150),
                                "OFF",
                            );
                        }
                    });

                    // Fog
                    ui.horizontal(|ui| {
                        ui.label("Fog:");
                        if config.render.fog_enabled {
                            ui.colored_label(
                                egui::Color32::from_rgb(100, 255, 100),
                                format!("ON ({:.0}..{:.0})", config.render.fog_start, config.render.fog_end),
                            );
                        } else {
                            ui.colored_label(
                                egui::Color32::from_rgb(150, 150, 150),
                                "OFF",
                            );
                        }
                    });

                    // Tonemapping
                    ui.horizontal(|ui| {
                        ui.label("Tonemapping:");
                        ui.monospace("ACES Fitted");
                    });
                });

                ui.separator();
            }

            // Render settings
            ui.collapsing("🎨 Render", |ui| {
                ui.checkbox(&mut overlay_state.wireframe_enabled, "Wireframe mode");
                ui.small("Note: Requires WireframePlugin");
            });

            ui.separator();

            // Input state visualization
            if overlay_state.show_input_state {
                ui.collapsing("🎮 Input State", |ui| {
                    if let Some(ref states) = action_states {
                        let actions = [
                            ("Forward", InputAction::MoveForward),
                            ("Backward", InputAction::MoveBackward),
                            ("Left", InputAction::MoveLeft),
                            ("Right", InputAction::MoveRight),
                            ("Jump", InputAction::Jump),
                            ("Crouch", InputAction::Crouch),
                            ("Sprint", InputAction::Sprint),
                        ];
                        
                        ui.horizontal_wrapped(|ui| {
                            for (name, action) in actions {
                                let state = states.get(action);
                                let (color, symbol) = match state {
                                    ActionState::Pressed | ActionState::JustPressed => {
                                        (egui::Color32::from_rgb(100, 255, 100), "●")
                                    }
                                    ActionState::JustReleased => {
                                        (egui::Color32::from_rgb(255, 200, 100), "○")
                                    }
                                    ActionState::Released => {
                                        (egui::Color32::from_rgb(100, 100, 100), "○")
                                    }
                                };
                                ui.colored_label(color, format!("{} {}", symbol, name));
                            }
                        });
                    } else {
                        ui.label("Input system not available");
                    }
                });
            }

            ui.separator();

            // Toggle buttons at bottom
            ui.horizontal(|ui| {
                ui.checkbox(&mut overlay_state.show_memory, "Memory");
                ui.checkbox(&mut overlay_state.show_input_state, "Input");
                ui.checkbox(&mut overlay_state.show_chunks, "Chunks");
                ui.checkbox(&mut overlay_state.show_render, "Render");
            });
            
            ui.small("F3 overlay | F7 shadows");
        });
}

/// Plugin to add debug overlay functionality
pub struct DebugOverlayPlugin;

impl Plugin for DebugOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EntityCountDiagnosticsPlugin)
            .init_resource::<DebugOverlayState>()
            .init_resource::<WireframeConfig>()
            .add_systems(
                Update,
                (
                    update_frame_time_history,
                    debug_overlay_ui,
                    toggle_wireframe,
                )
                    .chain(),
            );
    }
}
