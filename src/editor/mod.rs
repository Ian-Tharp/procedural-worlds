//! Editor systems - UI panels, viewport, tools

pub mod block_highlight;
pub mod chunk_debug;
pub mod debug_overlay;
pub mod hud;
pub mod performance;

pub use block_highlight::BlockHighlightPlugin;
pub use chunk_debug::ChunkDebugPlugin;
pub use debug_overlay::DebugOverlayPlugin;
pub use hud::HudPlugin;
pub use performance::PerformanceDashboardPlugin;

use bevy::prelude::*;
use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy_egui::{egui, EguiContexts};

use crate::actors::{Movement, Player};
use crate::config::audio::AudioSettingsPanelState;
use crate::engine::input::ActionStates;
use crate::engine::lighting::DayNightCycle;
use crate::engine::raycast::CurrentTarget;
use crate::world::ChunkLoadMetrics;

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

/// Main editor UI system
#[allow(clippy::too_many_arguments)]
fn editor_ui_system(
    mut contexts: EguiContexts,
    mut editor_state: ResMut<EditorState>,
    mut overlay_state: ResMut<debug_overlay::DebugOverlayState>,
    mut audio_panel_state: ResMut<AudioSettingsPanelState>,
    perf_dashboard: Option<Res<performance::PerformanceDashboard>>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut chunk_manager: Option<ResMut<crate::world::ChunkManager>>,
    physics: Option<Res<crate::physics::PlayerPhysics>>,
    player_transform_query: Query<&GlobalTransform, With<Player>>,
    mut player_query: Query<&mut Movement, With<Player>>,
    current_target: Option<Res<CurrentTarget>>,
    day_night: Option<Res<DayNightCycle>>,
    action_states: Option<Res<ActionStates>>,
    load_metrics: Option<Res<ChunkLoadMetrics>>,
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
                ui.separator();
                ui.checkbox(&mut audio_panel_state.visible, "Audio Settings (F9)");
                ui.separator();
                ui.checkbox(&mut overlay_state.visible, "Debug (F3)");
            });

            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    info!("Procedural Worlds Engine v{}", env!("CARGO_PKG_VERSION"));
                    ui.close_menu();
                }
            });

            // Right-aligned FPS counter (smoothed, from debug overlay state)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let fps = overlay_state.cached_fps;
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
    if editor_state.show_inspector {
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

                    // ── Debug sections (toggled via F3 or View menu) ──
                    if overlay_state.visible {
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
                            &mut overlay_state,
                            editor_state.player_position,
                            chunk_count,
                            render_distance,
                            ld,
                            vert_up,
                            vert_down,
                            current_target.as_deref(),
                            day_night.as_deref(),
                            action_states.as_deref(),
                            load_metrics.as_deref(),
                        );
                    }
                });
            });
    }

    // Right panel - World Settings (no bottom debug bar — all debug info is in the inspector now)
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

    // ── Floating Performance Dashboard (F8) ──
    if let Some(ref dashboard) = perf_dashboard
        && dashboard.visible
    {
        let chunk_count = chunk_manager.as_ref().map(|cm| cm.chunks.len()).unwrap_or(0);
        performance::draw_performance_dashboard(
            contexts.ctx_mut(),
            dashboard,
            chunk_count,
            load_metrics.as_deref(),
        );
    }
}
