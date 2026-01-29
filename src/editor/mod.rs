//! Editor systems - UI panels, viewport, tools

pub mod debug_overlay;

pub use debug_overlay::DebugOverlayPlugin;

use bevy::prelude::*;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy_egui::{egui, EguiContexts};

use crate::actors::{Movement, Player};

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
            .init_resource::<EditorState>()
            .init_resource::<PerformanceMetrics>()
            .add_systems(
                Update,
                (update_performance_metrics, editor_ui_system)
                    .chain()
                    .in_set(EditorUiSet),
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
    /// Show debug info
    pub show_debug: bool,
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
            show_debug: true,
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

/// Smoothed performance metrics for stable display
#[derive(Resource)]
pub struct PerformanceMetrics {
    /// Smoothed FPS value
    fps: f64,
    /// Smoothed frame time in ms
    frame_time_ms: f64,
    /// Frame count since start
    frame_count: u64,
    /// Time since last metrics update
    update_timer: f32,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            fps: 0.0,
            frame_time_ms: 0.0,
            frame_count: 0,
            update_timer: 0.0,
        }
    }
}

/// Update performance metrics using Bevy's built-in diagnostics
fn update_performance_metrics(
    diagnostics: Res<DiagnosticsStore>,
    mut metrics: ResMut<PerformanceMetrics>,
    time: Res<Time>,
) {
    metrics.frame_count += 1;
    metrics.update_timer += time.delta_secs();

    // Update metrics every 0.1 seconds for stable display
    if metrics.update_timer >= 0.1 {
        metrics.update_timer = 0.0;

        // Get smoothed FPS from Bevy's diagnostics
        if let Some(fps_diagnostic) = diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS) {
            if let Some(fps) = fps_diagnostic.smoothed() {
                metrics.fps = fps;
            }
        }

        // Calculate frame time from FPS (more reliable than the diagnostic)
        if metrics.fps > 0.0 {
            metrics.frame_time_ms = 1000.0 / metrics.fps;
        }
    }
}

/// Main editor UI system
fn editor_ui_system(
    mut contexts: EguiContexts,
    mut editor_state: ResMut<EditorState>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    metrics: Res<PerformanceMetrics>,
    mut chunk_manager: Option<ResMut<crate::world::ChunkManager>>,
    physics: Option<Res<crate::physics::PlayerPhysics>>,
    player_transform_query: Query<&GlobalTransform, With<Player>>,
    mut player_query: Query<&mut Movement, With<Player>>,
) {
    // Update player position for display (world-space, robust to parenting)
    if let Ok(player_global) = player_transform_query.get_single() {
        editor_state.player_position = player_global.translation();
    }

    // Update camera position and rotation for display (world-space)
    if let Ok(camera_global) = camera_query.get_single() {
        editor_state.camera_position = camera_global.translation();
        // Extract yaw from camera rotation
        let camera_transform = camera_global.compute_transform();
        let (yaw, _pitch, _roll) = camera_transform
            .rotation
            .to_euler(bevy::math::EulerRot::YXZ);
        editor_state.camera_yaw = yaw.to_degrees();
    }

    // Update chunk count
    if let Some(ref cm) = chunk_manager {
        editor_state.chunk_count = cm.chunks.len();
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
                ui.checkbox(&mut editor_state.show_inspector, "Inspector");
                ui.checkbox(&mut editor_state.show_world_settings, "World Settings");
                ui.checkbox(&mut editor_state.show_debug, "Debug Info");
            });

            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    info!("Procedural Worlds Engine v{}", env!("CARGO_PKG_VERSION"));
                    ui.close_menu();
                }
            });

            // Right-aligned FPS counter (smoothed)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Color code FPS: green >= 60, yellow >= 30, red < 30
                let fps_color = if metrics.fps >= 60.0 {
                    egui::Color32::from_rgb(100, 255, 100)
                } else if metrics.fps >= 30.0 {
                    egui::Color32::from_rgb(255, 255, 100)
                } else {
                    egui::Color32::from_rgb(255, 100, 100)
                };
                ui.colored_label(fps_color, format!("{:.0} FPS", metrics.fps));
            });
        });
    });

    // Left panel - Inspector
    if editor_state.show_inspector {
        egui::SidePanel::left("inspector")
            .default_width(250.0)
            .show(contexts.ctx_mut(), |ui| {
                ui.heading("Inspector");
                ui.separator();

                egui::CollapsingHeader::new("Player")
                    .default_open(true)
                    .show(ui, |ui| {
                        let pos = editor_state.player_position;
                        ui.label(format!(
                            "Feet: ({:.1}, {:.1}, {:.1})",
                            pos.x, pos.y, pos.z
                        ));
                        let cam = editor_state.camera_position;
                        ui.label(format!(
                            "Camera: ({:.1}, {:.1}, {:.1})",
                            cam.x, cam.y, cam.z
                        ));

                        // Compass direction
                        let cardinal = yaw_to_cardinal(editor_state.camera_yaw);
                        ui.horizontal(|ui| {
                            ui.label("Facing:");
                            ui.label(
                                egui::RichText::new(cardinal)
                                    .strong()
                                    .color(egui::Color32::from_rgb(100, 200, 255))
                            );
                            ui.label(format!("({:.0}°)", editor_state.camera_yaw));
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
                        if let Some(ref phys) = physics {
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
            });
    }

    // Right panel - World Settings
    if editor_state.show_world_settings {
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

    // Bottom panel - Debug info
    if editor_state.show_debug {
        egui::TopBottomPanel::bottom("debug_panel")
            .default_height(80.0)
            .show(contexts.ctx_mut(), |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Performance")
                            .strong()
                            .color(egui::Color32::from_rgb(150, 150, 255))
                    );
                    ui.separator();
                    ui.label(format!("FPS: {:.1}", metrics.fps));
                    ui.label(format!("Frame: {:.2}ms", metrics.frame_time_ms));
                    ui.label(format!("Frames: {}", metrics.frame_count));

                    ui.separator();

                    ui.label(
                        egui::RichText::new("Player")
                            .strong()
                            .color(egui::Color32::from_rgb(150, 255, 150))
                    );
                    let pos = editor_state.player_position;
                    ui.label(format!(
                        "X:{:.1} Y:{:.1} Z:{:.1}",
                        pos.x, pos.y, pos.z
                    ));
                    let cardinal = yaw_to_cardinal(editor_state.camera_yaw);
                    ui.label(format!("Facing: {} ({:.0}°)", cardinal, editor_state.camera_yaw));

                    ui.separator();

                    ui.label(
                        egui::RichText::new("Mode")
                            .strong()
                            .color(egui::Color32::from_rgb(255, 150, 200))
                    );
                    if let Some(ref phys) = physics {
                        let mode = if phys.noclip {
                            "Noclip"
                        } else if phys.flying {
                            "Flying"
                        } else if phys.grounded {
                            "Grounded"
                        } else {
                            "Falling"
                        };
                        ui.label(mode);
                    }

                    ui.separator();

                    ui.label(
                        egui::RichText::new("World")
                            .strong()
                            .color(egui::Color32::from_rgb(255, 200, 100))
                    );

                    // Show loading progress
                    if let Some(ref cm) = chunk_manager {
                        let rd = cm.render_distance;
                        // Horizontal: (2*rd+1)^2, Vertical: 7 layers (-2 to +4)
                        let expected = ((2 * rd + 1) * (2 * rd + 1) * 7) as usize;
                        let loaded = cm.chunks.len();

                        if loaded < expected {
                            // Still loading
                            let pct = (loaded as f32 / expected as f32 * 100.0) as u32;
                            ui.label(
                                egui::RichText::new(format!("Loading {}% ({}/{})", pct, loaded, expected))
                                    .color(egui::Color32::from_rgb(255, 255, 100))
                            );
                        } else {
                            ui.label(format!("Chunks: {}", loaded));
                        }
                    } else {
                        ui.label(format!("Chunks: {}", editor_state.chunk_count));
                    }

                    ui.separator();

                    ui.label(
                        egui::RichText::new("Engine")
                            .strong()
                            .color(egui::Color32::from_rgb(200, 150, 255))
                    );
                    ui.label("Bevy 0.15 | wgpu | Vulkan");
                });
            });
    }
}
