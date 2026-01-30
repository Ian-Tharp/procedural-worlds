//! Debug Visualization Overlay
//!
//! A toggleable debug overlay showing real-time engine diagnostics:
//! - Frame time graph with history
//! - Player position and chunk coordinates
//! - Memory usage estimation
//! - Wireframe toggle
//! - Input state visualization

use bevy::prelude::*;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy_egui::{egui, EguiContexts};

use crate::actors::Player;
use crate::engine::input::{ActionState, ActionStates, InputAction};
use crate::world::{ChunkManager, CHUNK_SIZE, CHUNK_VOLUME};

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

/// System to update frame time history
pub fn update_frame_time_history(
    diagnostics: Res<DiagnosticsStore>,
    mut overlay_state: ResMut<DebugOverlayState>,
    time: Res<Time>,
) {
    overlay_state.update_timer += time.delta_secs();
    
    // Update history every frame for smooth graphing
    if overlay_state.update_timer >= 1.0 / 60.0 {
        overlay_state.update_timer = 0.0;
        
        if let Some(fps_diagnostic) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS) {
            if let Some(fps) = fps_diagnostic.value() {
                if fps > 0.0 {
                    let frame_time_ms = 1000.0 / fps;
                    overlay_state.record_frame_time(frame_time_ms as f32);
                }
            }
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

/// System to render the debug overlay window
pub fn debug_overlay_ui(
    mut contexts: EguiContexts,
    mut overlay_state: ResMut<DebugOverlayState>,
    player_query: Query<&GlobalTransform, With<Player>>,
    chunk_manager: Option<Res<ChunkManager>>,
    action_states: Option<Res<ActionStates>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    // Toggle overlay with F3
    if keyboard.just_pressed(KeyCode::F3) {
        overlay_state.visible = !overlay_state.visible;
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
            });
            
            ui.small("Press F3 to toggle overlay");
        });
}

/// Plugin to add debug overlay functionality
pub struct DebugOverlayPlugin;

impl Plugin for DebugOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugOverlayState>()
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
