//! World Generation Configuration Panel
//!
//! Provides runtime-adjustable controls for terrain generation parameters.
//! Changes can be applied immediately by regenerating the world.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::generation::TerrainConfig;
use crate::world::{ChunkMesh, PendingChunk, PendingMesh};

/// State for the world generation configuration panel.
#[derive(Resource)]
pub struct WorldGenPanelState {
    /// Whether the panel section is expanded
    pub expanded: bool,
    /// Tracks if config has been modified since last apply
    pub dirty: bool,

    // ── Editable copies of TerrainConfig fields ──
    pub seed: u32,
    pub seed_text: String,
    pub base_height: f32,
    pub height_scale: f32,
    pub sea_level: i32,
    pub tree_density: f32,

    // ── Biome settings ──
    pub biome_scale: f32,
    pub blend_enabled: bool,
    pub blend_distance: f32,
    pub transition_noise_scale: f32,
    pub transition_noise_amplitude: f32,

    // ── Advanced noise settings ──
    pub frequency: f32,
    pub octaves: usize,
}

impl Default for WorldGenPanelState {
    fn default() -> Self {
        let config = TerrainConfig::default();
        Self {
            expanded: true,
            dirty: false,
            seed: config.seed,
            seed_text: config.seed.to_string(),
            base_height: config.base_height as f32,
            height_scale: config.height_scale as f32,
            sea_level: config.sea_level,
            tree_density: config.tree_density as f32,
            biome_scale: config.biome_scale as f32,
            blend_enabled: config.blend_enabled,
            blend_distance: config.blend_distance as f32,
            transition_noise_scale: config.transition_noise_scale as f32,
            transition_noise_amplitude: config.transition_noise_amplitude as f32,
            frequency: config.frequency as f32,
            octaves: config.octaves,
        }
    }
}

impl WorldGenPanelState {
    /// Sync panel state from an existing TerrainConfig resource
    pub fn sync_from_config(&mut self, config: &TerrainConfig) {
        self.seed = config.seed;
        self.seed_text = config.seed.to_string();
        self.base_height = config.base_height as f32;
        self.height_scale = config.height_scale as f32;
        self.sea_level = config.sea_level;
        self.tree_density = config.tree_density as f32;
        self.biome_scale = config.biome_scale as f32;
        self.blend_enabled = config.blend_enabled;
        self.blend_distance = config.blend_distance as f32;
        self.transition_noise_scale = config.transition_noise_scale as f32;
        self.transition_noise_amplitude = config.transition_noise_amplitude as f32;
        self.frequency = config.frequency as f32;
        self.octaves = config.octaves;
        self.dirty = false;
    }

    /// Apply panel state to TerrainConfig
    pub fn apply_to_config(&self, config: &mut TerrainConfig) {
        config.seed = self.seed;
        config.base_height = self.base_height as f64;
        config.height_scale = self.height_scale as f64;
        config.sea_level = self.sea_level;
        config.tree_density = self.tree_density as f64;
        config.biome_scale = self.biome_scale as f64;
        config.blend_enabled = self.blend_enabled;
        config.blend_distance = self.blend_distance as f64;
        config.transition_noise_scale = self.transition_noise_scale as f64;
        config.transition_noise_amplitude = self.transition_noise_amplitude as f64;
        config.frequency = self.frequency as f64;
        config.octaves = self.octaves;
    }
}

/// Event sent when the world should be regenerated with new config
#[derive(Event)]
pub struct RegenerateWorldEvent;

/// State for the regeneration loading screen
#[derive(Resource, Default)]
pub struct RegenerationState {
    /// Whether we're currently regenerating the world
    pub regenerating: bool,
    /// Timer for minimum loading screen display (prevents flicker)
    pub min_display_timer: f32,
    /// Fade-in/out animation progress (0.0 = invisible, 1.0 = fully visible)
    pub fade: f32,
}

/// Draw the world generation config UI section.
///
/// Returns `true` if the "Regenerate" button was clicked.
pub fn draw_worldgen_panel(
    ui: &mut egui::Ui,
    state: &mut WorldGenPanelState,
) -> bool {
    let mut regenerate_clicked = false;

    egui::CollapsingHeader::new("🌍 World Generation")
        .default_open(state.expanded)
        .show(ui, |ui| {
            state.expanded = true;

            // ── Seed ──
            ui.horizontal(|ui| {
                ui.label("Seed:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.seed_text)
                        .desired_width(100.0)
                        .hint_text("12345"),
                );
                if response.changed() {
                    if let Ok(parsed) = state.seed_text.parse::<u32>() {
                        state.seed = parsed;
                        state.dirty = true;
                    }
                }
                if ui.button("🎲").on_hover_text("Random seed").clicked() {
                    state.seed = rand::random();
                    state.seed_text = state.seed.to_string();
                    state.dirty = true;
                }
            });

            ui.add_space(4.0);

            // ── Terrain Shape ──
            ui.label(egui::RichText::new("Terrain Shape").strong());

            ui.horizontal(|ui| {
                ui.label("Base Height:");
                if ui.add(egui::Slider::new(&mut state.base_height, 16.0..=128.0)).changed() {
                    state.dirty = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Height Scale:");
                if ui.add(egui::Slider::new(&mut state.height_scale, 4.0..=64.0)).changed() {
                    state.dirty = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Sea Level:");
                if ui.add(egui::Slider::new(&mut state.sea_level, 0..=64)).changed() {
                    state.dirty = true;
                }
            });

            ui.add_space(4.0);

            // ── Biomes ──
            ui.label(egui::RichText::new("Biomes").strong());

            ui.horizontal(|ui| {
                ui.label("Biome Size:");
                // Convert scale to a more intuitive "size" (inverse relationship)
                // biome_scale 0.002 = huge biomes, 0.02 = tiny biomes
                let mut biome_size = 1.0 / (state.biome_scale * 100.0);
                if ui.add(
                    egui::Slider::new(&mut biome_size, 0.5..=10.0)
                        .logarithmic(true)
                        .suffix("x")
                ).changed() {
                    state.biome_scale = 1.0 / (biome_size * 100.0);
                    state.dirty = true;
                }
            });
            ui.small("Larger = bigger biomes");

            ui.horizontal(|ui| {
                if ui.checkbox(&mut state.blend_enabled, "Blend Boundaries").changed() {
                    state.dirty = true;
                }
            });

            if state.blend_enabled {
                ui.horizontal(|ui| {
                    ui.label("  Blend Distance:");
                    if ui.add(egui::Slider::new(&mut state.blend_distance, 8.0..=128.0)).changed() {
                        state.dirty = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("  Edge Noise:");
                    if ui.add(egui::Slider::new(&mut state.transition_noise_amplitude, 0.0..=1.0)).changed() {
                        state.dirty = true;
                    }
                });
            }

            ui.add_space(4.0);

            // ── Vegetation ──
            ui.label(egui::RichText::new("Vegetation").strong());

            ui.horizontal(|ui| {
                ui.label("Tree Density:");
                // Show as percentage
                let mut density_pct = state.tree_density * 100.0;
                if ui.add(
                    egui::Slider::new(&mut density_pct, 0.0..=20.0)
                        .suffix("%")
                ).changed() {
                    state.tree_density = density_pct / 100.0;
                    state.dirty = true;
                }
            });

            ui.add_space(4.0);

            // ── Advanced (collapsed by default) ──
            egui::CollapsingHeader::new("⚙ Advanced")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Noise Frequency:");
                        if ui.add(
                            egui::Slider::new(&mut state.frequency, 0.005..=0.1)
                                .logarithmic(true)
                        ).changed() {
                            state.dirty = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Octaves:");
                        let mut octaves_i32 = state.octaves as i32;
                        if ui.add(egui::Slider::new(&mut octaves_i32, 1..=8)).changed() {
                            state.octaves = octaves_i32 as usize;
                            state.dirty = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Transition Noise Scale:");
                        if ui.add(
                            egui::Slider::new(&mut state.transition_noise_scale, 0.01..=0.2)
                        ).changed() {
                            state.dirty = true;
                        }
                    });
                });

            ui.add_space(8.0);

            // ── Action Buttons ──
            ui.horizontal(|ui| {
                let regen_text = if state.dirty {
                    egui::RichText::new("🔄 Regenerate World")
                        .color(egui::Color32::from_rgb(255, 200, 100))
                        .strong()
                } else {
                    egui::RichText::new("🔄 Regenerate World")
                };

                if ui.button(regen_text).clicked() {
                    regenerate_clicked = true;
                    state.dirty = false;
                }

                if ui.button("↺ Reset").on_hover_text("Reset to defaults").clicked() {
                    *state = WorldGenPanelState::default();
                    regenerate_clicked = true;
                }
            });

            if state.dirty {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 200, 100),
                    "⚠ Changes pending - click Regenerate to apply",
                );
            }
        });

    regenerate_clicked
}

/// Plugin for the world generation config panel
pub struct WorldGenPanelPlugin;

impl Plugin for WorldGenPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldGenPanelState>()
            .init_resource::<RegenerationState>()
            .add_event::<RegenerateWorldEvent>()
            .add_systems(Startup, sync_panel_from_config)
            .add_systems(Update, (
                handle_regenerate_event,
                update_regeneration_state,
                draw_loading_screen,
            ).chain());
    }
}

/// Sync panel state from config on startup
fn sync_panel_from_config(
    mut panel_state: ResMut<WorldGenPanelState>,
    config: Option<Res<TerrainConfig>>,
) {
    if let Some(config) = config {
        panel_state.sync_from_config(&config);
    }
}

/// Handle regenerate world events - despawn all chunk entities and clear manager
fn handle_regenerate_event(
    mut commands: Commands,
    mut events: EventReader<RegenerateWorldEvent>,
    panel_state: Res<WorldGenPanelState>,
    mut terrain_config: Option<ResMut<TerrainConfig>>,
    mut chunk_manager: Option<ResMut<crate::world::ChunkManager>>,
    mut regen_state: ResMut<RegenerationState>,
    // Query all chunk-related entities to despawn
    chunk_mesh_query: Query<Entity, With<ChunkMesh>>,
    pending_chunk_query: Query<Entity, With<PendingChunk>>,
    pending_mesh_query: Query<Entity, With<PendingMesh>>,
) {
    for _ in events.read() {
        // Start regeneration state
        regen_state.regenerating = true;
        regen_state.min_display_timer = 0.5; // Minimum display time to prevent flicker
        regen_state.fade = 0.0;

        // Apply panel config to terrain config
        if let Some(ref mut config) = terrain_config {
            panel_state.apply_to_config(config);
            info!(
                "Applied new terrain config: seed={}, biome_scale={:.4}, sea_level={}",
                config.seed, config.biome_scale, config.sea_level
            );
        }

        // Despawn all chunk mesh entities
        let mut despawned = 0;
        for entity in chunk_mesh_query.iter() {
            commands.entity(entity).despawn_recursive();
            despawned += 1;
        }
        
        // Despawn pending chunk generation tasks
        for entity in pending_chunk_query.iter() {
            commands.entity(entity).despawn_recursive();
        }
        
        // Despawn pending mesh tasks
        for entity in pending_mesh_query.iter() {
            commands.entity(entity).despawn_recursive();
        }

        // Clear chunk manager data structures
        if let Some(ref mut cm) = chunk_manager {
            cm.chunks.clear();
            cm.pending.clear();
            info!("Despawned {} chunk entities, cleared manager for regeneration", despawned);
        }
    }
}

/// Update regeneration state - track when loading is complete
fn update_regeneration_state(
    mut regen_state: ResMut<RegenerationState>,
    chunk_manager: Option<Res<crate::world::ChunkManager>>,
    pending_mesh_query: Query<(), With<PendingMesh>>,
    time: Res<Time>,
) {
    if !regen_state.regenerating {
        // Fade out
        regen_state.fade = (regen_state.fade - time.delta_secs() * 3.0).max(0.0);
        return;
    }

    // Fade in
    regen_state.fade = (regen_state.fade + time.delta_secs() * 5.0).min(1.0);

    // Update minimum display timer
    regen_state.min_display_timer -= time.delta_secs();

    // Check if loading is complete
    if regen_state.min_display_timer <= 0.0 {
        if let Some(ref cm) = chunk_manager {
            let pending_chunks = cm.pending.len();
            let pending_meshes = pending_mesh_query.iter().count();
            
            // Consider loading "mostly done" when pending work is low
            // and we have some chunks loaded
            let has_chunks = cm.chunks.len() > 0;
            let low_pending = pending_chunks < 5 && pending_meshes < 3;
            
            if has_chunks && low_pending {
                regen_state.regenerating = false;
                info!("World regeneration complete: {} chunks loaded", cm.chunks.len());
            }
        }
    }
}

/// Draw the loading screen overlay during regeneration
fn draw_loading_screen(
    mut contexts: EguiContexts,
    regen_state: Res<RegenerationState>,
    chunk_manager: Option<Res<crate::world::ChunkManager>>,
    pending_mesh_query: Query<(), With<PendingMesh>>,
) {
    // Don't draw if fully faded out
    if regen_state.fade < 0.01 {
        return;
    }

    let ctx = contexts.ctx_mut();
    let screen_rect = ctx.screen_rect();
    let alpha = (regen_state.fade * 220.0) as u8;

    // Full-screen overlay
    egui::Area::new(egui::Id::new("worldgen_loading_screen"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            // Dark background overlay
            let painter = ui.painter();
            painter.rect_filled(
                screen_rect,
                0.0,
                egui::Color32::from_rgba_unmultiplied(10, 10, 20, alpha),
            );
        });

    // Centered loading panel
    egui::Area::new(egui::Id::new("worldgen_loading_panel"))
        .fixed_pos(screen_rect.center())
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            let text_alpha = (regen_state.fade * 255.0) as u8;

            egui::Frame::none()
                .fill(egui::Color32::from_rgba_unmultiplied(20, 20, 30, alpha))
                .rounding(egui::Rounding::same(12.0))
                .inner_margin(egui::Margin::symmetric(40.0, 30.0))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("🌍 Regenerating World...")
                                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, text_alpha))
                                .size(24.0)
                                .strong(),
                        );

                        ui.add_space(16.0);

                        // Progress info
                        if let Some(ref cm) = chunk_manager {
                            let loaded = cm.chunks.len();
                            let pending_gen = cm.pending.len();
                            let pending_mesh = pending_mesh_query.iter().count();

                            ui.label(
                                egui::RichText::new(format!("Chunks loaded: {}", loaded))
                                    .color(egui::Color32::from_rgba_unmultiplied(180, 180, 180, text_alpha))
                                    .size(14.0),
                            );

                            if pending_gen > 0 || pending_mesh > 0 {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Generating: {}  Meshing: {}",
                                        pending_gen, pending_mesh
                                    ))
                                    .color(egui::Color32::from_rgba_unmultiplied(150, 150, 150, text_alpha))
                                    .size(12.0),
                                );
                            }
                        }

                        ui.add_space(12.0);

                        // Animated dots
                        let dots = match ((ui.ctx().input(|i| i.time) * 2.0) as usize) % 4 {
                            0 => "",
                            1 => ".",
                            2 => "..",
                            _ => "...",
                        };
                        ui.label(
                            egui::RichText::new(format!("Please wait{}", dots))
                                .color(egui::Color32::from_rgba_unmultiplied(120, 120, 120, text_alpha))
                                .size(12.0)
                                .italics(),
                        );
                    });
                });
        });

    // Request continuous repaints during loading for animation
    if regen_state.regenerating || regen_state.fade > 0.01 {
        ctx.request_repaint();
    }
}
