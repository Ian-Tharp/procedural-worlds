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

use crate::engine::controller::CursorState;
use crate::engine::raycast::CurrentTarget;
use crate::world::interaction::SelectedBlock;
use crate::world::{ChunkLoadMetrics, ChunkManager};

/// Tracks the chunk loading progress bar visibility state.
///
/// The progress bar appears when chunks are pending and fades out
/// after loading completes, providing a smooth visual transition.
#[derive(Resource)]
pub struct ChunkLoadingBarState {
    /// Opacity of the progress bar (0.0 = invisible, 1.0 = fully visible).
    /// Animates up when loading and down after completion.
    opacity: f32,
    /// Whether the bar was visible last frame (used to detect completion).
    was_loading: bool,
}

impl Default for ChunkLoadingBarState {
    fn default() -> Self {
        Self {
            opacity: 0.0,
            was_loading: false,
        }
    }
}

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
            .add_systems(Update, (hud_system, chunk_loading_progress_system));
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
                    .inner_margin(egui::Margin::symmetric(6.0, 3.0))
                    .show(ui, |ui| {
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
            .fixed_pos(egui::pos2(center.x, screen_rect.max.y - 40.0))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_premultiplied(0, 0, 0, 180))
                    .rounding(egui::Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                    .show(ui, |ui| {
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

/// Chunk loading progress bar system.
///
/// Displays a translucent progress bar at the bottom center of the screen
/// showing how many chunks have been loaded versus the expected total.
/// The bar fades in when chunks are pending and fades out smoothly after
/// loading completes.
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
    mut bar_state: ResMut<ChunkLoadingBarState>,
    time: Res<Time>,
) {
    let Some(cm) = chunk_manager else { return };

    let ld = cm.effective_load_distance();
    let vertical_levels = (cm.vertical_load_down + cm.vertical_load_up + 1) as usize;
    let side = (2 * ld + 1) as usize;
    let expected = side * side * vertical_levels;
    let loaded = cm.chunks.len();
    let pending = cm.pending.len();

    let is_loading = pending > 0 && loaded < expected;

    // Animate opacity
    let dt = time.delta_secs();
    if is_loading {
        // Fade in quickly
        bar_state.opacity = (bar_state.opacity + dt * 4.0).min(1.0);
        bar_state.was_loading = true;
    } else if bar_state.was_loading {
        // Just finished loading — start fade out
        bar_state.was_loading = false;
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
        .map(|m| format!(" ({:.0} chunks/s)", m.chunks_per_second))
        .unwrap_or_default();

    let label_text = format!(
        "Loading World: {}/{} ({:.0}%){}",
        loaded,
        expected,
        progress * 100.0,
        cps_text,
    );

    egui::Area::new(egui::Id::new("hud_chunk_progress"))
        .fixed_pos(egui::pos2(bar_x, bar_y))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            // Background frame
            egui::Frame::none()
                .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, alpha / 2))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                .show(ui, |ui| {
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

                    // Progress bar fill
                    let fill_width = rect.width() * progress;
                    if fill_width > 0.5 {
                        let fill_rect = egui::Rect::from_min_size(
                            rect.min,
                            egui::vec2(fill_width, rect.height()),
                        );

                        // Color gradient: blue → green as progress increases
                        let r = ((1.0 - progress) * 60.0) as u8;
                        let g = (100.0 + progress * 155.0) as u8;
                        let b = ((1.0 - progress) * 200.0 + 55.0) as u8;
                        let fill_color = egui::Color32::from_rgba_unmultiplied(r, g, b, alpha);
                        ui.painter().rect_filled(fill_rect, 4.0, fill_color);
                    }
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
}
