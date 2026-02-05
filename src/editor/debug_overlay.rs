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
use bevy_egui::egui;

use crate::config::EngineConfig;
use crate::engine::input::{ActionState, ActionStates, InputAction};
use crate::engine::lighting::{DayNightCycle, Sun};
use crate::engine::memory;
use crate::engine::raycast::CurrentTarget;
use crate::world::streaming::StreamingConfig;
use crate::world::unloading::UnloadConfig;
use crate::world::{ChunkLoadMetrics, ChunkMesh, CHUNK_SIZE, CHUNK_VOLUME};

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
    /// Show LOD distance configurator panel
    pub show_lod_config: bool,
    /// Cached FPS value (smoothed for stable display)
    pub cached_fps: f64,
    /// Cached frame time in ms
    pub cached_frame_time_ms: f64,
    /// Cached process memory snapshot
    pub cached_process_memory: Option<memory::ProcessMemory>,
    /// Cached entity count
    pub cached_entity_count: f64,
    /// Timer for periodic metric refresh (avoids per-frame OS queries)
    metrics_refresh_timer: f32,
    // ── Pre-computed render/shadow data (populated by update_debug_render_data) ──
    /// Shadow map resolution in pixels
    pub shadow_map_size: usize,
    /// Number of shadow cascade levels from config
    pub shadow_cascade_count: u32,
    /// Whether shadows are currently enabled on the sun
    pub shadows_enabled: bool,
    /// Number of chunks that ARE shadow casters
    pub shadow_caster_count: usize,
    /// Number of chunks culled from shadow casting
    pub shadow_culled_count: usize,
    /// Bloom enabled from config
    pub bloom_enabled: bool,
    /// Bloom intensity from config
    pub bloom_intensity: f32,
    /// Fog enabled from config
    pub fog_enabled: bool,
    /// Fog start distance
    pub fog_start: f32,
    /// Fog end distance
    pub fog_end: f32,

    // ── LOD distance configurator (runtime-adjustable via debug panel) ──

    /// Whether LOD config fields have been populated from resources.
    /// Set to `true` after first sync; prevents overwriting user edits.
    pub lod_config_initialized: bool,
    /// Render distance in chunks (how far chunks are visible)
    pub lod_render_distance: i32,
    /// Horizontal load distance in chunks (how far chunks are generated).
    /// When equal to render distance, `ChunkManager::load_distance` is `None`.
    pub lod_load_distance: i32,
    /// Vertical chunk layers loaded above the player
    pub lod_vertical_up: i32,
    /// Vertical chunk layers loaded below the player
    pub lod_vertical_down: i32,
    /// Distance at which chunks are unloaded. 0 = auto (render_distance + 2).
    pub lod_unload_distance: i32,
    /// Predictive streaming lookahead in chunks
    pub lod_lookahead: i32,
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
            show_lod_config: true,
            cached_fps: 0.0,
            cached_frame_time_ms: 0.0,
            cached_process_memory: None,
            cached_entity_count: 0.0,
            metrics_refresh_timer: 0.0,
            shadow_map_size: 0,
            shadow_cascade_count: 0,
            shadows_enabled: true,
            shadow_caster_count: 0,
            shadow_culled_count: 0,
            bloom_enabled: false,
            bloom_intensity: 0.0,
            fog_enabled: false,
            fog_start: 0.0,
            fog_end: 0.0,
            lod_config_initialized: false,
            lod_render_distance: 4,
            lod_load_distance: 4,
            lod_vertical_up: 4,
            lod_vertical_down: 2,
            lod_unload_distance: 0,
            lod_lookahead: 3,
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

/// System to pre-compute render/shadow data for display
pub fn update_debug_render_data(
    mut overlay_state: ResMut<DebugOverlayState>,
    config: Res<EngineConfig>,
    shadow_map: Option<Res<DirectionalLightShadowMap>>,
    sun_query: Query<&DirectionalLight, With<Sun>>,
    not_shadow_caster_query: Query<(), (With<ChunkMesh>, With<NotShadowCaster>)>,
    shadow_caster_query: Query<(), (With<ChunkMesh>, Without<NotShadowCaster>)>,
) {
    overlay_state.shadow_map_size = shadow_map.as_ref().map(|sm| sm.size).unwrap_or(0);
    overlay_state.shadow_cascade_count = config.render.shadow_cascade_count;
    overlay_state.shadows_enabled = sun_query.iter().next().map(|l| l.shadows_enabled).unwrap_or(false);
    overlay_state.shadow_caster_count = shadow_caster_query.iter().count();
    overlay_state.shadow_culled_count = not_shadow_caster_query.iter().count();
    overlay_state.bloom_enabled = config.render.bloom_enabled;
    overlay_state.bloom_intensity = config.render.bloom_intensity;
    overlay_state.fog_enabled = config.render.fog_enabled;
    overlay_state.fog_start = config.render.fog_start;
    overlay_state.fog_end = config.render.fog_end;
}

/// System to synchronise LOD distance configurator state with engine resources.
///
/// On first run, populates `DebugOverlayState` LOD fields from the live
/// resources so sliders start at the correct values. On subsequent frames,
/// writes slider values back to the resources (one-directional: UI → engine),
/// following the same pattern as `toggle_wireframe`.
pub fn sync_lod_distances(
    mut overlay_state: ResMut<DebugOverlayState>,
    mut chunk_manager: Option<ResMut<crate::world::ChunkManager>>,
    mut unload_config: Option<ResMut<UnloadConfig>>,
    mut streaming_config: Option<ResMut<StreamingConfig>>,
) {
    // ── First-time initialisation: read from resources → overlay state ──
    if !overlay_state.lod_config_initialized {
        if let Some(ref cm) = chunk_manager {
            overlay_state.lod_render_distance = cm.render_distance;
            overlay_state.lod_load_distance = cm.effective_load_distance();
            overlay_state.lod_vertical_up = cm.vertical_load_up;
            overlay_state.lod_vertical_down = cm.vertical_load_down;
        }
        if let Some(ref uc) = unload_config {
            overlay_state.lod_unload_distance = uc.unload_distance.unwrap_or(0);
        }
        if let Some(ref sc) = streaming_config {
            overlay_state.lod_lookahead = sc.lookahead_chunks;
        }
        overlay_state.lod_config_initialized = true;
        return; // Skip write-back on the initialisation frame
    }

    // ── Apply overlay state → resources (UI is the source of truth) ──
    if let Some(ref mut cm) = chunk_manager {
        cm.render_distance = overlay_state.lod_render_distance;

        // If the user set load distance equal to render distance, use
        // the implicit default (`None`) so they stay locked together.
        if overlay_state.lod_load_distance == overlay_state.lod_render_distance {
            cm.load_distance = None;
        } else {
            cm.load_distance = Some(overlay_state.lod_load_distance);
        }

        cm.vertical_load_up = overlay_state.lod_vertical_up;
        cm.vertical_load_down = overlay_state.lod_vertical_down;
    }

    if let Some(ref mut uc) = unload_config {
        if overlay_state.lod_unload_distance <= 0 {
            uc.unload_distance = None; // auto: render_distance + 2
        } else {
            uc.unload_distance = Some(overlay_state.lod_unload_distance);
        }
    }

    if let Some(ref mut sc) = streaming_config {
        sc.lookahead_chunks = overlay_state.lod_lookahead;
    }
}

/// System to handle debug keyboard shortcuts (F3 overlay toggle, F5/F6 load dist, F7 shadow toggle)
pub fn debug_keyboard_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut overlay_state: ResMut<DebugOverlayState>,
    mut sun_query: Query<&mut DirectionalLight, With<Sun>>,
    mut chunk_manager: Option<ResMut<crate::world::ChunkManager>>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        overlay_state.visible = !overlay_state.visible;
    }
    // Adjust chunk loading distance with F5 (decrease) / F6 (increase).
    // Also update the overlay state so the LOD configurator sliders stay in sync.
    if let Some(ref mut cm) = chunk_manager {
        if keyboard.just_pressed(KeyCode::F5) {
            let current = cm.effective_load_distance();
            let new_dist = (current - 1).max(1);
            cm.load_distance = Some(new_dist);
            overlay_state.lod_load_distance = new_dist;
            info!("Chunk load distance decreased to {}", new_dist);
        }
        if keyboard.just_pressed(KeyCode::F6) {
            let current = cm.effective_load_distance();
            let new_dist = current + 1;
            cm.load_distance = Some(new_dist);
            overlay_state.lod_load_distance = new_dist;
            info!("Chunk load distance increased to {}", new_dist);
        }
    }
    if keyboard.just_pressed(KeyCode::F7) {
        for mut light in &mut sun_query {
            light.shadows_enabled = !light.shadows_enabled;
            info!("Shadows toggled: {}", light.shadows_enabled);
        }
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

/// Draw debug sections into the given UI container (called from the inspector panel).
///
/// This is NOT a Bevy system — it's a plain function that the inspector system
/// calls, passing all the data it needs.
#[allow(clippy::too_many_arguments)]
pub fn draw_debug_ui(
    ui: &mut egui::Ui,
    overlay_state: &mut DebugOverlayState,
    player_pos: Vec3,
    chunk_count: usize,
    render_distance: u32,
    load_distance: i32,
    vertical_up: i32,
    vertical_down: i32,
    current_target: Option<&CurrentTarget>,
    day_night: Option<&DayNightCycle>,
    action_states: Option<&ActionStates>,
    load_metrics: Option<&ChunkLoadMetrics>,
) {
    // ── Performance summary (always visible at top) ─────────
    {
        let fps = overlay_state.cached_fps;
        let frame_ms = overlay_state.cached_frame_time_ms;

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
                ui.colored_label(egui::Color32::from_rgb(150, 150, 150), "N/A");
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
    let chunk_coords = world_to_chunk_coords(player_pos);
    ui.collapsing("📍 Position", |ui| {
        ui.horizontal(|ui| {
            ui.label("World:");
            ui.monospace(format!("X:{:.1} Y:{:.1} Z:{:.1}", player_pos.x, player_pos.y, player_pos.z));
        });
        ui.horizontal(|ui| {
            ui.label("Chunk:");
            ui.monospace(format!("({}, {}, {})", chunk_coords.x, chunk_coords.y, chunk_coords.z));
        });
        ui.horizontal(|ui| {
            ui.label("Local:");
            let local_x = ((player_pos.x % CHUNK_SIZE as f32) + CHUNK_SIZE as f32) % CHUNK_SIZE as f32;
            let local_y = ((player_pos.y % CHUNK_SIZE as f32) + CHUNK_SIZE as f32) % CHUNK_SIZE as f32;
            let local_z = ((player_pos.z % CHUNK_SIZE as f32) + CHUNK_SIZE as f32) % CHUNK_SIZE as f32;
            ui.monospace(format!("({:.1}, {:.1}, {:.1})", local_x, local_y, local_z));
        });
    });

    ui.separator();

    // Target block section
    ui.collapsing("🎯 Target Block", |ui| {
        if let Some(target_res) = current_target {
            if let Some(ref result) = target_res.0 {
                ui.horizontal(|ui| {
                    ui.label("Block:");
                    ui.monospace(format!("({}, {}, {})", result.block_pos.x, result.block_pos.y, result.block_pos.z));
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
                    ui.monospace(format!("({}, {}, {})", result.adjacent_pos.x, result.adjacent_pos.y, result.adjacent_pos.z));
                });
            } else {
                ui.colored_label(egui::Color32::from_rgb(150, 150, 150), "No block in range");
            }
        } else {
            ui.label("Raycast not available");
        }
    });

    ui.separator();

    // Day/night cycle section
    if let Some(cycle) = day_night {
        ui.collapsing("🌅 Time of Day", |ui| {
            ui.horizontal(|ui| { ui.label("Clock:"); ui.monospace(cycle.clock_display()); });
            ui.horizontal(|ui| { ui.label("Phase:"); ui.monospace(cycle.phase_name()); });
            ui.horizontal(|ui| { ui.label("Raw:"); ui.monospace(format!("{:.4}", cycle.time_of_day)); });
            ui.horizontal(|ui| {
                ui.label("Cycle:");
                ui.monospace(format!("{:.0}s", cycle.cycle_duration));
                if cycle.paused {
                    ui.colored_label(egui::Color32::from_rgb(255, 200, 100), " ⏸ PAUSED");
                }
            });

            // Visual time-of-day bar
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(20, 20, 40));
            let marker_x = rect.min.x + rect.width() * cycle.time_of_day;
            let marker_center = egui::pos2(marker_x, rect.center().y);
            ui.painter().circle_filled(marker_center, 5.0, egui::Color32::from_rgb(255, 220, 80));
        });
        ui.separator();
    }

    // Frame time graph
    ui.collapsing("📊 Frame Time", |ui| {
        let history = overlay_state.get_ordered_history();
        let (avg, min, max) = overlay_state.frame_time_stats();

        ui.horizontal(|ui| {
            ui.label("Avg:"); ui.monospace(format!("{:.2}ms", avg));
            ui.label("Min:"); ui.monospace(format!("{:.2}ms", min));
            ui.label("Max:"); ui.monospace(format!("{:.2}ms", max));
        });

        let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
        let fps_color = if fps >= 60.0 {
            egui::Color32::from_rgb(100, 255, 100)
        } else if fps >= 30.0 {
            egui::Color32::from_rgb(255, 255, 100)
        } else {
            egui::Color32::from_rgb(255, 100, 100)
        };
        ui.horizontal(|ui| { ui.label("FPS:"); ui.colored_label(fps_color, format!("{:.0}", fps)); });

        ui.add_space(4.0);
        ui.label("Recent frame times:");

        let recent: Vec<f32> = history.iter().rev().take(30).copied().collect();
        let max_frame_time = recent.iter().copied().fold(33.33_f32, f32::max);

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
            for &frame_time in recent.iter().rev() {
                let normalized = (frame_time / max_frame_time).min(1.0);
                let color = if frame_time <= 16.67 {
                    egui::Color32::from_rgb(100, 255, 100)
                } else if frame_time <= 33.33 {
                    egui::Color32::from_rgb(255, 255, 100)
                } else {
                    egui::Color32::from_rgb(255, 100, 100)
                };
                let height = 20.0 * normalized;
                let (rect, _response) = ui.allocate_exact_size(egui::vec2(4.0, 20.0), egui::Sense::hover());
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
            ui.horizontal(|ui| { ui.label("Chunks loaded:"); ui.monospace(format!("{}", chunk_count)); });
            ui.horizontal(|ui| { ui.label("Block data:"); ui.monospace(format!("{:.1} MB", block_mb)); });
            ui.horizontal(|ui| { ui.label("Mesh data:"); ui.monospace(format!("~{:.1} MB", mesh_mb)); });
            ui.horizontal(|ui| { ui.label("Total (est):"); ui.strong(format!("~{:.1} MB", total_mb)); });
        });
    }

    ui.separator();

    // Chunk statistics
    if overlay_state.show_chunks {
        ui.collapsing("📦 Chunk Statistics", |ui| {
            let vertical_levels = (vertical_down + vertical_up + 1) as usize;
            let side = (2 * load_distance + 1) as usize;
            let expected_chunks = side * side * vertical_levels;

            let loaded_pct = if expected_chunks > 0 {
                (chunk_count as f64 / expected_chunks as f64 * 100.0).min(100.0)
            } else {
                0.0
            };
            let pending = expected_chunks.saturating_sub(chunk_count);
            let block_bytes = chunk_count * CHUNK_VOLUME * std::mem::size_of::<crate::world::BlockType>();
            let block_mb = block_bytes as f64 / (1024.0 * 1024.0);

            ui.horizontal(|ui| { ui.label("Loaded:"); ui.monospace(format!("{}", chunk_count)); });
            ui.horizontal(|ui| { ui.label("Expected:"); ui.monospace(format!("{}", expected_chunks)); });
            ui.horizontal(|ui| { ui.label("Pending:"); ui.monospace(format!("{}", pending)); });
            ui.horizontal(|ui| { ui.label("Loaded %:"); ui.monospace(format!("{:.1}%", loaded_pct)); });
            ui.horizontal(|ui| {
                ui.label("Render dist:");
                ui.monospace(format!("{}", render_distance));
            });
            ui.horizontal(|ui| {
                ui.label("Load dist:");
                ui.monospace(format!("{}", load_distance));
                ui.small("(F5↓ F6↑)");
            });
            ui.horizontal(|ui| {
                ui.label("Vertical:");
                ui.monospace(format!("-{}..+{}", vertical_down, vertical_up));
            });
            ui.horizontal(|ui| { ui.label("Block memory:"); ui.monospace(format!("{:.1} MB", block_mb)); });

            // Performance metrics sub-section
            if let Some(metrics) = load_metrics {
                ui.separator();
                ui.label(egui::RichText::new("⏱ Load Performance").strong());
                ui.horizontal(|ui| {
                    ui.label("Chunks/sec:");
                    let cps = metrics.chunks_per_second;
                    let cps_color = if cps >= 10.0 {
                        egui::Color32::from_rgb(100, 255, 100)
                    } else if cps >= 2.0 {
                        egui::Color32::from_rgb(255, 255, 100)
                    } else {
                        egui::Color32::from_rgb(255, 100, 100)
                    };
                    ui.colored_label(cps_color, format!("{:.1}", cps));
                });
                ui.horizontal(|ui| {
                    ui.label("Avg load time:");
                    let avg = metrics.avg_load_time_ms;
                    let avg_color = if avg <= 20.0 {
                        egui::Color32::from_rgb(100, 255, 100)
                    } else if avg <= 100.0 {
                        egui::Color32::from_rgb(255, 255, 100)
                    } else {
                        egui::Color32::from_rgb(255, 100, 100)
                    };
                    ui.colored_label(avg_color, format!("{:.1} ms", avg));
                });
                ui.horizontal(|ui| {
                    ui.label("Peak load time:");
                    let peak = metrics.peak_load_time_ms;
                    let peak_color = if peak <= 50.0 {
                        egui::Color32::from_rgb(100, 255, 100)
                    } else if peak <= 200.0 {
                        egui::Color32::from_rgb(255, 255, 100)
                    } else {
                        egui::Color32::from_rgb(255, 100, 100)
                    };
                    ui.colored_label(peak_color, format!("{:.1} ms", peak));
                    ui.colored_label(
                        egui::Color32::from_rgb(150, 150, 150),
                        format!("(all-time: {:.1} ms)", metrics.all_time_peak_load_time_ms),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Total loaded:");
                    ui.monospace(format!("{}", metrics.total_chunks_loaded));
                });
                ui.horizontal(|ui| {
                    ui.label("Chunk data:");
                    ui.monospace(format!("{:.1} MB", metrics.chunk_memory_mb()));
                });
            }
        });
    }

    ui.separator();

    // Rendering debug panel (uses pre-computed data from overlay_state)
    if overlay_state.show_render {
        ui.collapsing("🌟 Rendering", |ui| {
            ui.horizontal(|ui| { ui.label("Shadow map:"); ui.monospace(format!("{}px", overlay_state.shadow_map_size)); });
            ui.horizontal(|ui| { ui.label("Cascades:"); ui.monospace(format!("{}", overlay_state.shadow_cascade_count)); });
            ui.horizontal(|ui| {
                ui.label("Shadows:");
                if overlay_state.shadows_enabled {
                    ui.colored_label(egui::Color32::from_rgb(100, 255, 100), "ON");
                } else {
                    ui.colored_label(egui::Color32::from_rgb(255, 100, 100), "OFF");
                }
                ui.small("(F7 toggle)");
            });
            ui.horizontal(|ui| { ui.label("Shadow casters:"); ui.monospace(format!("{}", overlay_state.shadow_caster_count)); });
            ui.horizontal(|ui| { ui.label("Shadow culled:"); ui.monospace(format!("{}", overlay_state.shadow_culled_count)); });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Bloom:");
                if overlay_state.bloom_enabled {
                    ui.colored_label(egui::Color32::from_rgb(100, 255, 100), format!("ON ({:.2})", overlay_state.bloom_intensity));
                } else {
                    ui.colored_label(egui::Color32::from_rgb(150, 150, 150), "OFF");
                }
            });
            ui.horizontal(|ui| {
                ui.label("Fog:");
                if overlay_state.fog_enabled {
                    ui.colored_label(egui::Color32::from_rgb(100, 255, 100), format!("ON ({:.0}..{:.0})", overlay_state.fog_start, overlay_state.fog_end));
                } else {
                    ui.colored_label(egui::Color32::from_rgb(150, 150, 150), "OFF");
                }
            });
            ui.horizontal(|ui| { ui.label("Tonemapping:"); ui.monospace("ACES Fitted"); });
        });
        ui.separator();
    }

    // LOD distance configurator — runtime adjustment of chunk loading/rendering distances
    if overlay_state.show_lod_config {
        ui.collapsing("🔭 LOD Distances", |ui| {
            ui.label(
                egui::RichText::new("Adjust chunk level-of-detail distances at runtime.")
                    .weak()
                    .italics(),
            );
            ui.add_space(4.0);

            // ── Render distance: controls how far chunks are visible ──
            ui.horizontal(|ui| {
                ui.label("Render dist:");
                if ui
                    .add(
                        egui::Slider::new(&mut overlay_state.lod_render_distance, 1..=24)
                            .suffix(" chunks"),
                    )
                    .changed()
                {
                    // Keep load distance >= render distance
                    if overlay_state.lod_load_distance < overlay_state.lod_render_distance {
                        overlay_state.lod_load_distance = overlay_state.lod_render_distance;
                    }
                }
            });

            // ── Load distance: how far chunks are generated (>= render dist) ──
            ui.horizontal(|ui| {
                ui.label("Load dist:");
                let min_load = overlay_state.lod_render_distance;
                ui.add(
                    egui::Slider::new(&mut overlay_state.lod_load_distance, min_load..=32)
                        .suffix(" chunks"),
                )
                .on_hover_text("How far ahead chunks are generated. Must be ≥ render distance.");
            });

            ui.add_space(2.0);

            // ── Vertical range: chunk layers above/below the player ──
            ui.horizontal(|ui| {
                ui.label("Vertical ↑:");
                ui.add(
                    egui::Slider::new(&mut overlay_state.lod_vertical_up, 0..=16)
                        .suffix(" layers"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Vertical ↓:");
                ui.add(
                    egui::Slider::new(&mut overlay_state.lod_vertical_down, 0..=16)
                        .suffix(" layers"),
                );
            });

            ui.add_space(2.0);

            // ── Unload distance: when chunks are removed from memory ──
            // Pre-compute hover text before the mutable borrow in Slider::new
            let unload_hover = if overlay_state.lod_unload_distance <= 0 {
                format!("auto ({})", overlay_state.lod_render_distance + 2)
            } else {
                format!("{} chunks", overlay_state.lod_unload_distance)
            };
            ui.horizontal(|ui| {
                ui.label("Unload dist:");
                ui.add(
                    egui::Slider::new(&mut overlay_state.lod_unload_distance, 0..=32)
                        .custom_formatter(|val, _| {
                            if val <= 0.0 {
                                "auto".to_string()
                            } else {
                                format!("{}", val as i32)
                            }
                        }),
                )
                .on_hover_text(unload_hover);
            });

            // ── Streaming lookahead: predictive loading distance ──
            ui.horizontal(|ui| {
                ui.label("Lookahead:");
                ui.add(
                    egui::Slider::new(&mut overlay_state.lod_lookahead, 0..=8)
                        .suffix(" chunks"),
                )
                .on_hover_text("Predictive streaming: how many chunks ahead of movement to pre-load");
            });

            ui.add_space(4.0);

            // ── Summary of effective distances ──
            let eff_load = overlay_state.lod_load_distance;
            let eff_unload = if overlay_state.lod_unload_distance <= 0 {
                overlay_state.lod_render_distance + 2
            } else {
                overlay_state.lod_unload_distance
            };
            let total_vert = overlay_state.lod_vertical_down + overlay_state.lod_vertical_up + 1;
            let side = (2 * eff_load + 1) as usize;
            let max_chunks = side * side * total_vert as usize;

            ui.separator();
            ui.label(egui::RichText::new("Summary").strong());
            ui.horizontal(|ui| {
                ui.label("Effective unload:");
                ui.monospace(format!("{} chunks", eff_unload));
            });
            ui.horizontal(|ui| {
                ui.label("Max chunks:");
                ui.monospace(format!("~{}", max_chunks));
            });
            ui.horizontal(|ui| {
                ui.label("Vert layers:");
                ui.monospace(format!("-{}..+{} ({})", overlay_state.lod_vertical_down, overlay_state.lod_vertical_up, total_vert));
            });

            // ── Reset button ──
            ui.add_space(4.0);
            if ui
                .button("↺ Reset to Defaults")
                .on_hover_text("Restore default LOD distances")
                .clicked()
            {
                overlay_state.lod_render_distance = 4;
                overlay_state.lod_load_distance = 4;
                overlay_state.lod_vertical_up = 4;
                overlay_state.lod_vertical_down = 2;
                overlay_state.lod_unload_distance = 0;
                overlay_state.lod_lookahead = 3;
            }
        });
        ui.separator();
    }

    // Render options
    ui.collapsing("🎨 Render Options", |ui| {
        ui.checkbox(&mut overlay_state.wireframe_enabled, "Wireframe mode");
        ui.small("Note: Requires WireframePlugin");
    });

    ui.separator();

    // Input state visualization
    if overlay_state.show_input_state {
        ui.collapsing("🎮 Input State", |ui| {
            if let Some(states) = action_states {
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

    // Section toggle buttons at bottom
    ui.horizontal(|ui| {
        ui.checkbox(&mut overlay_state.show_memory, "Memory");
        ui.checkbox(&mut overlay_state.show_input_state, "Input");
        ui.checkbox(&mut overlay_state.show_chunks, "Chunks");
        ui.checkbox(&mut overlay_state.show_render, "Render");
        ui.checkbox(&mut overlay_state.show_lod_config, "LOD");
    });

    ui.small("F3 overlay | F4 profiler | F5/F6 load dist | F7 shadows | F8 perf");
}

/// Plugin to add debug overlay functionality.
///
/// The debug UI itself is rendered by the inspector panel in [`super::EditorPlugin`].
/// This plugin only registers the data-gathering systems and keyboard shortcuts.
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
                    update_debug_render_data,
                    sync_lod_distances,
                    debug_keyboard_input,
                    toggle_wireframe,
                )
                    .chain(),
            );
    }
}
