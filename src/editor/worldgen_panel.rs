//! World Generation Configuration Panel
//!
//! Provides runtime-adjustable controls for terrain generation parameters.
//! Changes can be applied immediately by regenerating the world.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::actors::Player;
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

    // ── Cave settings ──
    pub caves_enabled: bool,
    pub cave_threshold: f32,
    pub cave_surface_protection: i32,
    pub cave_frequency: f32,

    // ── Additional vegetation ──
    pub cactus_density_multiplier: f32,
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
            caves_enabled: config.caves_enabled,
            cave_threshold: config.cave_threshold as f32,
            cave_surface_protection: config.cave_surface_protection,
            cave_frequency: config.cave_frequency as f32,
            cactus_density_multiplier: config.cactus_density_multiplier as f32,
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
        self.caves_enabled = config.caves_enabled;
        self.cave_threshold = config.cave_threshold as f32;
        self.cave_surface_protection = config.cave_surface_protection;
        self.cave_frequency = config.cave_frequency as f32;
        self.cactus_density_multiplier = config.cactus_density_multiplier as f32;
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
        config.caves_enabled = self.caves_enabled;
        config.cave_threshold = self.cave_threshold as f64;
        config.cave_surface_protection = self.cave_surface_protection;
        config.cave_frequency = self.cave_frequency as f64;
        config.cactus_density_multiplier = self.cactus_density_multiplier as f64;
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
pub fn draw_worldgen_panel(ui: &mut egui::Ui, state: &mut WorldGenPanelState) -> bool {
    let mut regenerate_clicked = false;

    egui::CollapsingHeader::new("🌍 World Generation")
        .default_open(state.expanded)
        .show(ui, |ui| {
            state.expanded = true;

            // ── Seed ──
            ui.horizontal(|ui| {
                ui.label("Seed:").on_hover_text(
                    "The world seed determines all terrain generation.\n\
                    Same seed = same world every time.",
                );
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
                if ui
                    .button("🎲")
                    .on_hover_text("Generate a random seed")
                    .clicked()
                {
                    state.seed = rand::random();
                    state.seed_text = state.seed.to_string();
                    state.dirty = true;
                }
            });

            ui.add_space(4.0);

            // ── Terrain Shape ──
            ui.label(egui::RichText::new("Terrain Shape").strong());

            ui.horizontal(|ui| {
                ui.label("Base Height:").on_hover_text(format!(
                    "The average ground level in blocks.\n\
                    Higher values raise the entire world.\n\
                    Default: 32 | Current: {:.0}",
                    state.base_height
                ));
                if ui
                    .add(egui::Slider::new(&mut state.base_height, 16.0..=128.0))
                    .on_hover_text(format!("Current: {:.0} blocks", state.base_height))
                    .changed()
                {
                    state.dirty = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Height Scale:").on_hover_text(format!(
                    "How much terrain varies from the base height.\n\
                    Low = flat plains, High = dramatic mountains.\n\
                    Default: 16 | Current: {:.0}",
                    state.height_scale
                ));
                if ui
                    .add(egui::Slider::new(&mut state.height_scale, 4.0..=64.0))
                    .on_hover_text(format!(
                        "Current: {:.0} blocks variation",
                        state.height_scale
                    ))
                    .changed()
                {
                    state.dirty = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Sea Level:").on_hover_text(format!(
                    "Blocks below this Y-level become water.\n\
                    Set lower than Base Height for oceans.\n\
                    Default: 28 | Current: {}",
                    state.sea_level
                ));
                if ui
                    .add(egui::Slider::new(&mut state.sea_level, 0..=64))
                    .on_hover_text(format!("Current: Y={}", state.sea_level))
                    .changed()
                {
                    state.dirty = true;
                }
            });

            ui.add_space(4.0);

            // ── Biomes ──
            ui.label(egui::RichText::new("Biomes").strong());

            ui.horizontal(|ui| {
                // Convert scale to a more intuitive "size" (inverse relationship)
                // biome_scale 0.002 = huge biomes, 0.02 = tiny biomes
                let biome_size = 1.0 / (state.biome_scale * 100.0);
                ui.label("Biome Size:").on_hover_text(format!(
                    "How large biomes are across the world.\n\
                    Small = frequent biome changes.\n\
                    Large = vast continuous regions.\n\
                    Default: 2.0x | Current: {:.1}x",
                    biome_size
                ));
                let mut biome_size_mut = biome_size;
                if ui
                    .add(
                        egui::Slider::new(&mut biome_size_mut, 0.5..=10.0)
                            .logarithmic(true)
                            .suffix("x"),
                    )
                    .on_hover_text(format!("Current: {:.1}x", biome_size_mut))
                    .changed()
                {
                    state.biome_scale = 1.0 / (biome_size_mut * 100.0);
                    state.dirty = true;
                }
            });

            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut state.blend_enabled, "Blend Boundaries")
                    .on_hover_text(format!(
                        "Smoothly blend terrain between biomes.\n\
                        Creates gradual transitions instead of\n\
                        hard edges at biome borders.\n\
                        Current: {}",
                        if state.blend_enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        }
                    ))
                    .changed()
                {
                    state.dirty = true;
                }
            });

            if state.blend_enabled {
                ui.horizontal(|ui| {
                    ui.label("  Blend Distance:").on_hover_text(format!(
                        "Width of the transition zone between biomes.\n\
                        Larger = smoother, wider gradients.\n\
                        Default: 32 | Current: {:.0} blocks",
                        state.blend_distance
                    ));
                    if ui
                        .add(egui::Slider::new(&mut state.blend_distance, 8.0..=128.0))
                        .on_hover_text(format!("Current: {:.0} blocks", state.blend_distance))
                        .changed()
                    {
                        state.dirty = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("  Edge Noise:").on_hover_text(format!(
                        "How irregular biome boundaries are.\n\
                        0 = smooth geometric edges.\n\
                        1 = jagged, organic-looking borders.\n\
                        Default: 0.45 | Current: {:.2}",
                        state.transition_noise_amplitude
                    ));
                    if ui
                        .add(egui::Slider::new(
                            &mut state.transition_noise_amplitude,
                            0.0..=1.0,
                        ))
                        .on_hover_text(format!("Current: {:.2}", state.transition_noise_amplitude))
                        .changed()
                    {
                        state.dirty = true;
                    }
                });
            }

            ui.add_space(4.0);

            // ── Caves ──
            ui.label(egui::RichText::new("Caves").strong());

            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut state.caves_enabled, "Enable Caves")
                    .on_hover_text(format!(
                        "Generate underground cave systems.\n\
                        Caves carve through stone below the surface.\n\
                        Current: {}",
                        if state.caves_enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        }
                    ))
                    .changed()
                {
                    state.dirty = true;
                }
            });

            if state.caves_enabled {
                ui.horizontal(|ui| {
                    ui.label("  Cave Density:").on_hover_text(format!(
                        "How common caves are underground.\n\
                        Lower threshold = more caves.\n\
                        Higher threshold = fewer, isolated caves.\n\
                        Default: 0.70 | Current: {:.2}",
                        state.cave_threshold
                    ));
                    if ui
                        .add(egui::Slider::new(&mut state.cave_threshold, 0.5..=0.9))
                        .on_hover_text(format!("Current: {:.2}", state.cave_threshold))
                        .changed()
                    {
                        state.dirty = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("  Cave Size:").on_hover_text(format!(
                        "Size of cave tunnels and chambers.\n\
                        Lower = larger cave systems.\n\
                        Higher = smaller, tighter tunnels.\n\
                        Default: 0.05 | Current: {:.3}",
                        state.cave_frequency
                    ));
                    if ui
                        .add(
                            egui::Slider::new(&mut state.cave_frequency, 0.02..=0.1)
                                .logarithmic(true),
                        )
                        .on_hover_text(format!("Current: {:.3}", state.cave_frequency))
                        .changed()
                    {
                        state.dirty = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("  Surface Protection:").on_hover_text(format!(
                        "Blocks below surface protected from caves.\n\
                        Prevents caves from breaking through grass/dirt.\n\
                        Default: 5 | Current: {}",
                        state.cave_surface_protection
                    ));
                    if ui
                        .add(egui::Slider::new(
                            &mut state.cave_surface_protection,
                            0..=15,
                        ))
                        .on_hover_text(format!("Current: {} blocks", state.cave_surface_protection))
                        .changed()
                    {
                        state.dirty = true;
                    }
                });
            }

            ui.add_space(4.0);

            // ── Vegetation ──
            ui.label(egui::RichText::new("Vegetation").strong());

            ui.horizontal(|ui| {
                ui.label("Tree Density:").on_hover_text(format!(
                    "Probability of trees spawning on valid surfaces.\n\
                    Affects forests, plains, and other tree-supporting biomes.\n\
                    0% = no trees, 20% = dense forest.\n\
                    Default: 2% | Current: {:.1}%",
                    state.tree_density * 100.0
                ));
                // Show as percentage
                let mut density_pct = state.tree_density * 100.0;
                if ui
                    .add(egui::Slider::new(&mut density_pct, 0.0..=20.0).suffix("%"))
                    .on_hover_text(format!("Current: {:.1}%", density_pct))
                    .changed()
                {
                    state.tree_density = density_pct / 100.0;
                    state.dirty = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Cactus Density:").on_hover_text(format!(
                    "Multiplier for cactus spawning in deserts.\n\
                    0x = no cacti, 2x = double normal density.\n\
                    Default: 1.0x | Current: {:.1}x",
                    state.cactus_density_multiplier
                ));
                if ui
                    .add(
                        egui::Slider::new(&mut state.cactus_density_multiplier, 0.0..=5.0)
                            .suffix("x"),
                    )
                    .on_hover_text(format!("Current: {:.1}x", state.cactus_density_multiplier))
                    .changed()
                {
                    state.dirty = true;
                }
            });

            ui.add_space(4.0);

            // ── Advanced (collapsed by default) ──
            egui::CollapsingHeader::new("⚙ Advanced")
                .default_open(false)
                .show(ui, |ui| {
                    ui.small(
                        "⚠ These settings require understanding of noise-based terrain generation.",
                    );
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label("Noise Frequency:").on_hover_text(format!(
                            "Base frequency of the terrain noise.\n\
                            Lower = larger, smoother features.\n\
                            Higher = smaller, more detailed terrain.\n\
                            Default: 0.02 | Current: {:.3}",
                            state.frequency
                        ));
                        if ui
                            .add(
                                egui::Slider::new(&mut state.frequency, 0.005..=0.1)
                                    .logarithmic(true),
                            )
                            .on_hover_text(format!("Current: {:.3}", state.frequency))
                            .changed()
                        {
                            state.dirty = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Octaves:").on_hover_text(format!(
                            "Number of noise layers combined.\n\
                            More octaves = more detail at multiple scales.\n\
                            Higher values are more expensive to compute.\n\
                            Default: 4 | Current: {}",
                            state.octaves
                        ));
                        let mut octaves_i32 = state.octaves as i32;
                        if ui
                            .add(egui::Slider::new(&mut octaves_i32, 1..=8))
                            .on_hover_text(format!("Current: {}", octaves_i32))
                            .changed()
                        {
                            state.octaves = octaves_i32 as usize;
                            state.dirty = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Transition Noise Scale:").on_hover_text(format!(
                            "Frequency of the noise used to warp biome edges.\n\
                            Higher = more jagged, detailed borders.\n\
                            Lower = smoother, broader edge variations.\n\
                            Default: 0.08 | Current: {:.2}",
                            state.transition_noise_scale
                        ));
                        if ui
                            .add(egui::Slider::new(
                                &mut state.transition_noise_scale,
                                0.01..=0.2,
                            ))
                            .on_hover_text(format!("Current: {:.2}", state.transition_noise_scale))
                            .changed()
                        {
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

                if ui
                    .button("↺ Reset")
                    .on_hover_text("Reset to defaults")
                    .clicked()
                {
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
            .add_systems(
                Update,
                (
                    handle_regenerate_event,
                    update_regeneration_state,
                    draw_loading_screen,
                )
                    .chain(),
            );
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
    // Query player to teleport
    mut player_query: Query<&mut Transform, With<Player>>,
) {
    for _ in events.read() {
        // Start regeneration state
        regen_state.regenerating = true;
        regen_state.min_display_timer = 0.5; // Minimum display time to prevent flicker
        regen_state.fade = 0.0;

        // Apply panel config to terrain config
        let new_base_height = panel_state.base_height;
        if let Some(ref mut config) = terrain_config {
            panel_state.apply_to_config(config);
            info!(
                "Applied new terrain config: seed={}, biome_scale={:.4}, sea_level={}, base_height={}",
                config.seed, config.biome_scale, config.sea_level, config.base_height
            );
        }

        // Teleport player to new spawn height (base_height + buffer for hills + player height)
        // This ensures the player is above the new terrain surface
        let spawn_y = new_base_height + 30.0; // 30 blocks above base to clear hills
        if let Ok(mut player_transform) = player_query.get_single_mut() {
            let old_y = player_transform.translation.y;
            player_transform.translation.y = spawn_y;
            // Keep X/Z position, or reset to origin for fresh start
            player_transform.translation.x = 0.0;
            player_transform.translation.z = 0.0;
            info!(
                "Teleported player from Y={:.1} to Y={:.1} (spawn at origin)",
                old_y, spawn_y
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
            info!(
                "Despawned {} chunk entities, cleared manager for regeneration",
                despawned
            );
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
                info!(
                    "World regeneration complete: {} chunks loaded",
                    cm.chunks.len()
                );
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
                                .color(egui::Color32::from_rgba_unmultiplied(
                                    255, 255, 255, text_alpha,
                                ))
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
                                    .color(egui::Color32::from_rgba_unmultiplied(
                                        180, 180, 180, text_alpha,
                                    ))
                                    .size(14.0),
                            );

                            if pending_gen > 0 || pending_mesh > 0 {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Generating: {}  Meshing: {}",
                                        pending_gen, pending_mesh
                                    ))
                                    .color(egui::Color32::from_rgba_unmultiplied(
                                        150, 150, 150, text_alpha,
                                    ))
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
                                .color(egui::Color32::from_rgba_unmultiplied(
                                    120, 120, 120, text_alpha,
                                ))
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
