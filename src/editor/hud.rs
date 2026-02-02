//! Crosshair & Block Target HUD
//!
//! Renders a centered crosshair overlay and displays targeted block information
//! near the crosshair when the raycast system has a target.
//!
//! - Crosshair is only visible when the cursor is grabbed (FPS mode)
//! - Block info tooltip shows the block type display name and world position
//! - Uses egui for rendering, consistent with the rest of the editor UI

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::engine::controller::CursorState;
use crate::engine::raycast::CurrentTarget;

/// Plugin that adds the crosshair overlay and block target HUD.
///
/// Requires:
/// - [`CursorState`] resource (from [`crate::engine::controller::ControllerPlugin`])
/// - [`CurrentTarget`] resource (from [`crate::engine::raycast::RaycastPlugin`])
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, hud_system);
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

    let Some(result) = target_res else {
        return;
    };

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
}
