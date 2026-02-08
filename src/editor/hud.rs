//! Crosshair & Block Target HUD
//!
//! Renders a centered crosshair overlay and displays targeted block information
//! near the crosshair when the raycast system has a target.
//!
//! - Crosshair is only visible when the cursor is grabbed (FPS mode)
//! - Block info tooltip shows the block type display name and world position
//! - Chunk loading progress bar shows world generation status
//! - Uses egui for rendering, consistent with the rest of the editor UI

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use noise::Simplex;

use crate::engine::controller::CursorState;
use crate::engine::raycast::CurrentTarget;
use crate::generation::biome::{biome_at, BiomeType};
use crate::generation::TerrainConfig;
use crate::world::interaction::SelectedBlock;
use crate::world::{ChunkLoadMetrics, ChunkManager, PendingMesh, CHUNK_SIZE};

/// Tracks the chunk loading progress bar visibility and animation state.
///
/// The progress bar appears when chunks are pending and fades out
/// after loading completes, providing a smooth visual transition.
/// Includes a pulse animation timer for active loading indication.
#[derive(Resource)]
pub struct ChunkLoadingBarState {
    /// Opacity of the progress bar (0.0 = invisible, 1.0 = fully visible).
    /// Animates up when loading and down after completion.
    opacity: f32,
    /// Whether the bar was visible last frame (used to detect completion).
    was_loading: bool,
    /// Accumulated time for the pulse animation (wraps at 2π).
    pulse_timer: f32,
}

impl Default for ChunkLoadingBarState {
    fn default() -> Self {
        Self {
            opacity: 0.0,
            was_loading: false,
            pulse_timer: 0.0,
        }
    }
}

impl ChunkLoadingBarState {
    /// Current opacity of the progress bar (0.0–1.0).
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Whether the bar was in the loading state last frame.
    pub fn was_loading(&self) -> bool {
        self.was_loading
    }
}

// ============================================================================
// Biome Indicator
// ============================================================================

/// How long (seconds) the biome name stays fully visible after a change.
const BIOME_DISPLAY_DURATION: f32 = 3.0;

/// How fast the biome indicator fades in (opacity per second).
const BIOME_FADE_IN_SPEED: f32 = 4.0;

/// How fast the biome indicator fades out (opacity per second).
const BIOME_FADE_OUT_SPEED: f32 = 1.5;

/// Tracks the biome indicator HUD state.
///
/// When the player crosses a biome boundary, the new biome name fades in,
/// stays visible for [`BIOME_DISPLAY_DURATION`] seconds, then fades out.
/// This provides unobtrusive awareness of the current environment without
/// cluttering the screen during normal gameplay.
#[derive(Resource)]
pub struct BiomeIndicatorState {
    /// The biome the player is currently in.
    pub current_biome: Option<BiomeType>,
    /// Opacity of the indicator (0.0 = invisible, 1.0 = fully visible).
    pub opacity: f32,
    /// Timer counting down after a biome change (seconds remaining).
    pub display_timer: f32,
    /// Last checked player chunk (x, z) to avoid redundant noise queries.
    last_checked_chunk: Option<(i32, i32)>,
}

impl Default for BiomeIndicatorState {
    fn default() -> Self {
        Self {
            current_biome: None,
            opacity: 0.0,
            display_timer: 0.0,
            last_checked_chunk: None,
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin that adds the crosshair overlay, block target HUD, and chunk
/// loading progress bar.
///
/// Requires:
/// - [`CursorState`] resource (from [`crate::engine::controller::ControllerPlugin`])
/// - [`CurrentTarget`] resource (from [`crate::engine::raycast::RaycastPlugin`])
/// - [`ChunkManager`] resource (from [`crate::world::WorldPlugin`])
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkLoadingBarState>()
            .init_resource::<BiomeIndicatorState>()
            .add_systems(
                Update,
                (hud_system, chunk_loading_progress_system, biome_indicator_system),
            );
    }
}

/// Combined crosshair and block info HUD system.
///
/// Draws:
/// 1. A crosshair (`+` shape with center gap) at screen center — only when
///    the cursor is grabbed.
/// 2. A small tooltip below the crosshair showing the targeted block type and
///    world position — only when [`CurrentTarget`] contains a hit.
fn hud_system(
    mut contexts: EguiContexts,
    cursor_state: Res<CursorState>,
    current_target: Option<Res<CurrentTarget>>,
    selected_block: Option<Res<SelectedBlock>>,
) {
    // Only show HUD elements when the cursor is grabbed (FPS mode)
    if !cursor_state.grabbed {
        return;
    }

    let ctx = contexts.ctx_mut();
    let screen_rect = ctx.screen_rect();
    let center = screen_rect.center();

    // ── Crosshair ──────────────────────────────────────────────────────
    let half_size = 10.0;
    let thickness = 2.0;
    let gap = 3.0; // Small center gap for aiming precision
    let crosshair_color = egui::Color32::from_rgba_premultiplied(255, 255, 255, 200);

    egui::Area::new(egui::Id::new("hud_crosshair"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            let painter = ui.painter();

            // Horizontal left segment
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(center.x - half_size, center.y - thickness / 2.0),
                    egui::vec2(half_size - gap, thickness),
                ),
                0.0,
                crosshair_color,
            );

            // Horizontal right segment
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(center.x + gap, center.y - thickness / 2.0),
                    egui::vec2(half_size - gap, thickness),
                ),
                0.0,
                crosshair_color,
            );

            // Vertical top segment
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(center.x - thickness / 2.0, center.y - half_size),
                    egui::vec2(thickness, half_size - gap),
                ),
                0.0,
                crosshair_color,
            );

            // Vertical bottom segment
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(center.x - thickness / 2.0, center.y + gap),
                    egui::vec2(thickness, half_size - gap),
                ),
                0.0,
                crosshair_color,
            );
        });

    // ── Block Info Tooltip ──────────────────────────────────────────────
    let target_res = match current_target {
        Some(ref t) => t.0.as_ref(),
        None => None,
    };

    if let Some(result) = target_res {
        let label_text = format!(
            "{} ({}, {}, {})",
            result.block_type.display_name(),
            result.block_pos.x,
            result.block_pos.y,
            result.block_pos.z,
        );

        // Position the tooltip just below the crosshair
        let tooltip_offset_y = half_size + 8.0;

        egui::Area::new(egui::Id::new("hud_block_info"))
            .fixed_pos(egui::pos2(center.x, center.y + tooltip_offset_y))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_premultiplied(0, 0, 0, 160))
                    .rounding(egui::Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                    .show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        ui.label(
                            egui::RichText::new(label_text)
                                .color(egui::Color32::from_rgb(220, 220, 220))
                                .size(13.0),
                        );
                    });
            });
    }

    // ── Selected Block Indicator ────────────────────────────────────────
    if let Some(ref sel) = selected_block {
        let sel_text = format!("Selected: {}", sel.block_type.display_name());

        egui::Area::new(egui::Id::new("hud_selected_block"))
            .fixed_pos(egui::pos2(center.x, screen_rect.max.y - 48.0))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_premultiplied(0, 0, 0, 180))
                    .rounding(egui::Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 8.0))
                    .show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        ui.label(
                            egui::RichText::new(sel_text)
                                .color(egui::Color32::from_rgb(255, 255, 200))
                                .size(15.0)
                                .strong(),
                        );
                    });
            });
    }
}

/// Chunk loading progress bar system with phase-aware visuals.
///
/// Displays a translucent progress bar at the bottom center of the screen
/// showing chunk loading progress with distinct visual states for the
/// "generating" and "meshing" phases.
///
/// - **Generating** (blue/purple): Terrain, caves, and trees are being computed
///   on background threads (`PendingChunk` entities).
/// - **Meshing** (orange/amber): Block data is being converted into renderable
///   meshes on background threads (`PendingMesh` entities).
///
/// The bar includes a subtle pulse animation on the active portion to
/// communicate ongoing work, and fades in/out smoothly on state transitions.
///
/// # Calculation
///
/// Expected chunks = `(2 * load_distance + 1)² × vertical_levels`.
/// Loaded chunks = `chunk_manager.chunks.len()`.
/// Progress = loaded / expected, clamped to [0, 1].
fn chunk_loading_progress_system(
    mut contexts: EguiContexts,
    chunk_manager: Option<Res<ChunkManager>>,
    load_metrics: Option<Res<ChunkLoadMetrics>>,
    pending_mesh_query: Query<(), With<PendingMesh>>,
    mut bar_state: ResMut<ChunkLoadingBarState>,
    time: Res<Time>,
) {
    let Some(cm) = chunk_manager else { return };

    let ld = cm.effective_load_distance();
    let vertical_levels = (cm.vertical_load_down + cm.vertical_load_up + 1) as usize;
    let side = (2 * ld + 1) as usize;
    let expected = side * side * vertical_levels;
    let loaded = cm.chunks.len();
    let generating = cm.pending.len();
    let meshing = pending_mesh_query.iter().count();

    let is_loading = (generating > 0 || meshing > 0) && loaded < expected;

    // Animate opacity
    let dt = time.delta_secs();
    if is_loading {
        // Fade in quickly
        bar_state.opacity = (bar_state.opacity + dt * 4.0).min(1.0);
        bar_state.was_loading = true;
        // Advance pulse timer (wraps at 2π for smooth looping)
        bar_state.pulse_timer = (bar_state.pulse_timer + dt * 3.0) % std::f32::consts::TAU;
    } else if bar_state.was_loading {
        // Just finished loading — start fade out
        bar_state.was_loading = false;
        bar_state.pulse_timer = 0.0;
    }

    if !is_loading {
        // Fade out slowly
        bar_state.opacity = (bar_state.opacity - dt * 1.5).max(0.0);
    }

    // Don't render when fully invisible
    if bar_state.opacity < 0.01 {
        return;
    }

    let progress = if expected > 0 {
        (loaded as f32 / expected as f32).clamp(0.0, 1.0)
    } else {
        1.0
    };

    let alpha = (bar_state.opacity * 255.0) as u8;
    // Pulse factor: oscillates 0.0..1.0 for brightness modulation
    let pulse = (bar_state.pulse_timer.sin() * 0.5 + 0.5).clamp(0.0, 1.0);

    let ctx = contexts.ctx_mut();
    let screen_rect = ctx.screen_rect();

    // Bar dimensions
    let bar_width = (screen_rect.width() * 0.4).min(400.0);
    let bar_height = 20.0;
    let bar_x = (screen_rect.width() - bar_width) / 2.0;
    let bar_y = screen_rect.height() - 60.0;

    // Chunks per second text (if metrics available)
    let cps_text = load_metrics
        .as_ref()
        .filter(|m| m.chunks_per_second > 0.1)
        .map(|m| format!(" — {:.0} chunks/s", m.chunks_per_second))
        .unwrap_or_default();

    // Build phase-aware label
    let phase_text = if generating > 0 && meshing > 0 {
        format!("Generating: {}  Meshing: {}", generating, meshing)
    } else if generating > 0 {
        format!("Generating: {}", generating)
    } else if meshing > 0 {
        format!("Meshing: {}", meshing)
    } else {
        String::new()
    };

    let label_text = if phase_text.is_empty() {
        format!(
            "Loading World: {}/{} ({:.0}%){}",
            loaded, expected, progress * 100.0, cps_text,
        )
    } else {
        format!(
            "Loading World: {}/{} ({:.0}%){}  [{}]",
            loaded, expected, progress * 100.0, cps_text, phase_text,
        )
    };

    egui::Area::new(egui::Id::new("hud_chunk_progress"))
        .fixed_pos(egui::pos2(bar_x, bar_y))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            // Background frame
            egui::Frame::none()
                .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, alpha / 2))
                .rounding(egui::Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    // Label above bar
                    ui.label(
                        egui::RichText::new(&label_text)
                            .color(egui::Color32::from_rgba_unmultiplied(220, 220, 220, alpha))
                            .size(12.0),
                    );

                    ui.add_space(3.0);

                    // Progress bar track
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(bar_width - 20.0, bar_height),
                        egui::Sense::hover(),
                    );
                    let track_color = egui::Color32::from_rgba_unmultiplied(40, 40, 40, alpha);
                    ui.painter().rect_filled(rect, 4.0, track_color);

                    // Split the filled portion into generating and meshing segments.
                    // Generating portion (blue/purple) comes first, then meshing
                    // (orange/amber), then completed (green).
                    let total_active = generating + meshing + loaded;
                    let fill_width = rect.width() * progress;

                    if fill_width > 0.5 && total_active > 0 {
                        // Fraction of the filled bar that is "completed" vs active phases
                        let completed_chunks = if loaded > generating + meshing {
                            loaded - generating - meshing
                        } else {
                            loaded
                        };

                        // Calculate proportions within the filled area
                        let completed_frac = if expected > 0 {
                            (completed_chunks as f32 / expected as f32).clamp(0.0, progress)
                        } else {
                            progress
                        };
                        let generating_frac = if expected > 0 {
                            (generating as f32 / expected as f32).clamp(0.0, 1.0 - completed_frac)
                        } else {
                            0.0
                        };
                        let meshing_frac = if expected > 0 {
                            (meshing as f32 / expected as f32).clamp(0.0, 1.0 - completed_frac - generating_frac)
                        } else {
                            0.0
                        };

                        let completed_width = rect.width() * completed_frac;
                        let generating_width = rect.width() * generating_frac;
                        let meshing_width = rect.width() * meshing_frac;

                        // 1. Completed segment (green)
                        if completed_width > 0.5 {
                            let completed_rect = egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(completed_width, rect.height()),
                            );
                            let green = egui::Color32::from_rgba_unmultiplied(50, 200, 80, alpha);
                            ui.painter().rect_filled(completed_rect, 4.0, green);
                        }

                        // 2. Generating segment (blue/purple with pulse)
                        if generating_width > 0.5 {
                            let gen_rect = egui::Rect::from_min_size(
                                egui::pos2(rect.min.x + completed_width, rect.min.y),
                                egui::vec2(generating_width, rect.height()),
                            );
                            // Pulse between darker and brighter blue
                            let brightness = 140.0 + pulse * 60.0;
                            let gen_color = egui::Color32::from_rgba_unmultiplied(
                                (brightness * 0.3) as u8,
                                (brightness * 0.4) as u8,
                                brightness as u8,
                                alpha,
                            );
                            ui.painter().rect_filled(gen_rect, 4.0, gen_color);
                        }

                        // 3. Meshing segment (orange/amber with pulse)
                        if meshing_width > 0.5 {
                            let mesh_rect = egui::Rect::from_min_size(
                                egui::pos2(
                                    rect.min.x + completed_width + generating_width,
                                    rect.min.y,
                                ),
                                egui::vec2(meshing_width, rect.height()),
                            );
                            // Pulse between darker and brighter orange
                            let brightness = 160.0 + pulse * 60.0;
                            let mesh_color = egui::Color32::from_rgba_unmultiplied(
                                brightness as u8,
                                (brightness * 0.6) as u8,
                                (brightness * 0.15) as u8,
                                alpha,
                            );
                            ui.painter().rect_filled(mesh_rect, 4.0, mesh_color);
                        }
                    }
                });
        });
}

/// Biome indicator system — shows the biome name when crossing boundaries.
///
/// Detects biome changes by sampling the biome noise at the player's current
/// chunk position. When the biome changes, the name fades in with an icon,
/// stays visible for [`BIOME_DISPLAY_DURATION`] seconds, then fades out.
///
/// Only visible when the cursor is grabbed (FPS mode), consistent with
/// the crosshair and other HUD elements.
fn biome_indicator_system(
    mut contexts: EguiContexts,
    cursor_state: Res<CursorState>,
    chunk_manager: Option<Res<ChunkManager>>,
    terrain_config: Option<Res<TerrainConfig>>,
    mut indicator: ResMut<BiomeIndicatorState>,
    time: Res<Time>,
) {
    // ── Biome detection ────────────────────────────────────────────────
    if let (Some(cm), Some(config)) = (&chunk_manager, &terrain_config) {
        let chunk_xz = (cm.player_chunk.x, cm.player_chunk.z);

        if indicator.last_checked_chunk != Some(chunk_xz) {
            indicator.last_checked_chunk = Some(chunk_xz);

            // Sample biome at the center of the player's chunk
            let world_x = cm.player_chunk.x * CHUNK_SIZE as i32 + CHUNK_SIZE as i32 / 2;
            let world_z = cm.player_chunk.z * CHUNK_SIZE as i32 + CHUNK_SIZE as i32 / 2;
            let biome_noise =
                Simplex::new(config.seed.wrapping_add(config.biome_seed_offset));
            let biome = biome_at(world_x, world_z, &biome_noise, config.biome_scale);

            if indicator.current_biome != Some(biome) {
                indicator.current_biome = Some(biome);
                // Trigger fade-in animation
                indicator.display_timer = BIOME_DISPLAY_DURATION;
                indicator.opacity = 0.0;
            }
        }
    }

    // ── Animation update ───────────────────────────────────────────────
    let dt = time.delta_secs();
    if indicator.display_timer > 0.0 {
        // Fade in while the timer is active
        indicator.opacity = (indicator.opacity + dt * BIOME_FADE_IN_SPEED).min(1.0);
        indicator.display_timer -= dt;
    } else {
        // Fade out after the timer expires
        indicator.opacity = (indicator.opacity - dt * BIOME_FADE_OUT_SPEED).max(0.0);
    }

    // ── Rendering ──────────────────────────────────────────────────────
    if indicator.opacity < 0.01 || !cursor_state.grabbed {
        return;
    }

    let Some(biome) = indicator.current_biome else {
        return;
    };

    let ctx = contexts.ctx_mut();
    let screen_rect = ctx.screen_rect();
    let alpha = (indicator.opacity * 255.0) as u8;

    let display_text = format!("{} {}", biome.icon(), biome.display_name());

    egui::Area::new(egui::Id::new("hud_biome_indicator"))
        .fixed_pos(egui::pos2(screen_rect.center().x, 24.0))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 0.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, alpha / 3))
                .rounding(egui::Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(20.0, 10.0))
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.label(
                        egui::RichText::new(display_text)
                            .color(egui::Color32::from_rgba_unmultiplied(
                                255, 255, 240, alpha,
                            ))
                            .size(20.0)
                            .strong(),
                    );
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hud_plugin_builds() {
        // Verify the plugin can be added to an app without panicking.
        // We can't fully run Update (no egui backend) but we can confirm registration.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<CursorState>();
        app.init_resource::<CurrentTarget>();
        app.init_resource::<ChunkLoadingBarState>();
        // HudPlugin only adds systems; building should not panic.
        app.add_plugins(HudPlugin);
    }

    #[test]
    fn test_block_type_display_names() {
        use crate::world::BlockType;

        assert_eq!(BlockType::Air.display_name(), "Air");
        assert_eq!(BlockType::Stone.display_name(), "Stone");
        assert_eq!(BlockType::Dirt.display_name(), "Dirt");
        assert_eq!(BlockType::Grass.display_name(), "Grass");
        assert_eq!(BlockType::Sand.display_name(), "Sand");
        assert_eq!(BlockType::Water.display_name(), "Water");
        assert_eq!(BlockType::Wood.display_name(), "Wood");
        assert_eq!(BlockType::Leaves.display_name(), "Leaves");
    }

    #[test]
    fn test_chunk_loading_bar_state_defaults() {
        let state = ChunkLoadingBarState::default();
        assert_eq!(state.opacity, 0.0);
        assert!(!state.was_loading);
        assert_eq!(state.pulse_timer, 0.0);
    }

    #[test]
    fn test_chunk_loading_bar_opacity_clamp() {
        let mut state = ChunkLoadingBarState::default();

        // Simulate rapid fade-in
        state.opacity = (state.opacity + 10.0).min(1.0);
        assert_eq!(state.opacity, 1.0);

        // Simulate rapid fade-out
        state.opacity = (state.opacity - 10.0).max(0.0);
        assert_eq!(state.opacity, 0.0);
    }

    // ── Biome indicator tests ──

    #[test]
    fn test_biome_indicator_state_defaults() {
        let state = BiomeIndicatorState::default();
        assert!(state.current_biome.is_none());
        assert_eq!(state.opacity, 0.0);
        assert_eq!(state.display_timer, 0.0);
        assert!(state.last_checked_chunk.is_none());
    }

    #[test]
    fn test_biome_display_names() {
        use crate::generation::biome::BiomeType;

        assert_eq!(BiomeType::Plains.display_name(), "Plains");
        assert_eq!(BiomeType::Desert.display_name(), "Desert");
        assert_eq!(BiomeType::Forest.display_name(), "Forest");
        assert_eq!(BiomeType::Mountains.display_name(), "Mountains");
        assert_eq!(BiomeType::Tundra.display_name(), "Tundra");
        assert_eq!(BiomeType::Volcanic.display_name(), "Volcanic Wastes");
    }

    #[test]
    fn test_biome_icons_non_empty() {
        use crate::generation::biome::BiomeType;

        for biome in BiomeType::all() {
            assert!(
                !biome.icon().is_empty(),
                "{:?} should have a non-empty icon",
                biome
            );
        }
    }

    #[test]
    fn test_biome_indicator_display_constants() {
        assert!(BIOME_DISPLAY_DURATION > 0.0);
        assert!(BIOME_FADE_IN_SPEED > 0.0);
        assert!(BIOME_FADE_OUT_SPEED > 0.0);
        // Fade-in should be faster than fade-out for snappy feel
        assert!(
            BIOME_FADE_IN_SPEED > BIOME_FADE_OUT_SPEED,
            "Fade-in should be faster than fade-out"
        );
    }
}
