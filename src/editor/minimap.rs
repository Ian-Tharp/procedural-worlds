//! Minimap System - Real-time overhead terrain view
//!
//! Renders a top-down view of the terrain around the player,
//! showing actual block colors from loaded chunks, with biome-based
//! fallback for unloaded areas. Similar to Xaero's Minimap.
//!
//! Toggle: M key
//! Zoom: Mouse wheel when hovering (or +/- keys)

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::generation::biome::{biome_at, BiomeType};
use crate::generation::TerrainConfig;
use crate::world::CHUNK_SIZE;

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Minimap display settings
#[derive(Resource)]
pub struct MinimapConfig {
    /// Is the minimap visible
    pub visible: bool,
    /// Size in pixels (width = height)
    pub size: f32,
    /// Zoom level (blocks per pixel, lower = more zoomed in)
    pub blocks_per_pixel: f32,
    /// Minimum zoom (most zoomed in)
    pub min_zoom: f32,
    /// Maximum zoom (most zoomed out)
    pub max_zoom: f32,
    /// Position from top-right corner
    pub margin: f32,
    /// Opacity (0.0 - 1.0)
    pub opacity: f32,
    /// Show chunk grid overlay
    pub show_chunk_grid: bool,
    /// Show compass directions
    pub show_compass: bool,
}

impl Default for MinimapConfig {
    fn default() -> Self {
        Self {
            visible: true,
            size: 180.0,
            blocks_per_pixel: 1.0,  // 1 block = 1 pixel
            min_zoom: 0.25,         // 4 pixels per block (zoomed in)
            max_zoom: 4.0,          // 0.25 pixels per block (zoomed out)
            margin: 28.0,
            opacity: 0.9,
            show_chunk_grid: false,
            show_compass: true,
        }
    }
}

/// Cached minimap texture data
#[derive(Resource, Default)]
pub struct MinimapTexture {
    /// RGBA pixel data
    pub pixels: Vec<u8>,
    /// Texture size (width = height)
    pub texture_size: usize,
    /// Center world position when texture was generated
    pub center_x: i32,
    pub center_z: i32,
    /// Blocks per pixel when generated
    pub generated_zoom: f32,
    /// Whether texture needs regeneration
    pub dirty: bool,
    /// egui texture handle (must keep alive to prevent texture from being freed)
    pub texture_handle: Option<egui::TextureHandle>,
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin for the minimap system
pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MinimapConfig>()
            .init_resource::<MinimapTexture>()
            .add_systems(Update, (
                minimap_toggle_system,
                minimap_update_system,
                minimap_render_system,
            ).chain());
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Toggle minimap visibility with M key
fn minimap_toggle_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut config: ResMut<MinimapConfig>,
    mut egui_contexts: EguiContexts,
) {
    // Don't toggle if typing in UI
    if egui_contexts.ctx_mut().wants_keyboard_input() {
        return;
    }
    
    if keyboard.just_pressed(KeyCode::KeyM) {
        config.visible = !config.visible;
    }
    
    // Zoom controls (when not typing)
    if keyboard.just_pressed(KeyCode::Equal) || keyboard.just_pressed(KeyCode::NumpadAdd) {
        config.blocks_per_pixel = (config.blocks_per_pixel / 1.5).max(config.min_zoom);
    }
    if keyboard.just_pressed(KeyCode::Minus) || keyboard.just_pressed(KeyCode::NumpadSubtract) {
        config.blocks_per_pixel = (config.blocks_per_pixel * 1.5).min(config.max_zoom);
    }
}

/// Update minimap texture when player moves or zoom changes
/// Uses biome colors from terrain seed - instant, no chunk loading needed
fn minimap_update_system(
    config: Res<MinimapConfig>,
    mut texture: ResMut<MinimapTexture>,
    terrain_config: Res<TerrainConfig>,
    player_query: Query<&GlobalTransform, With<Camera3d>>,
) {
    if !config.visible {
        return;
    }
    
    let Ok(player_transform) = player_query.get_single() else {
        return;
    };
    
    let player_pos = player_transform.translation();
    let center_x = player_pos.x.floor() as i32;
    let center_z = player_pos.z.floor() as i32;
    
    // Check if we need to regenerate
    let size = config.size as usize;
    let needs_regen = texture.dirty
        || texture.texture_size != size
        || texture.generated_zoom != config.blocks_per_pixel
        || (texture.center_x - center_x).abs() > 16
        || (texture.center_z - center_z).abs() > 16;
    
    if !needs_regen {
        return;
    }
    
    // Pre-create biome noise (uses seed - deterministic)
    let biome_noise = noise::Simplex::new(
        terrain_config.seed.wrapping_add(terrain_config.biome_seed_offset)
    );
    
    // Generate texture using biome colors (instant - just noise sampling)
    let blocks_per_pixel = config.blocks_per_pixel;
    let mut pixels = vec![0u8; size * size * 4];
    
    // Track biome counts for debugging
    let mut biome_counts = [0u32; 12];
    
    for py in 0..size {
        for px in 0..size {
            // Map pixel to world position
            let offset_x = (px as f32 - size as f32 / 2.0) * blocks_per_pixel;
            let offset_z = (py as f32 - size as f32 / 2.0) * blocks_per_pixel;
            let world_x = center_x + offset_x as i32;
            let world_z = center_z + offset_z as i32;
            
            // Get biome color (instant - just noise lookup)
            let biome = biome_at(world_x, world_z, &biome_noise, terrain_config.biome_scale);
            biome_counts[biome as usize] += 1;
            let (r, g, b) = biome_to_color(biome);
            
            let idx = (py * size + px) * 4;
            pixels[idx] = r;
            pixels[idx + 1] = g;
            pixels[idx + 2] = b;
            pixels[idx + 3] = 255;
        }
    }
    
    // Log biome distribution for debugging
    info!(
        "Minimap generated at ({}, {}): Plains={}, Forest={}, Desert={}, Mountains={}, Tundra={}, Volcanic={}",
        center_x, center_z,
        biome_counts[0], biome_counts[1], biome_counts[2], 
        biome_counts[3], biome_counts[4], biome_counts[5]
    );
    
    texture.pixels = pixels;
    texture.texture_size = size;
    texture.center_x = center_x;
    texture.center_z = center_z;
    texture.generated_zoom = blocks_per_pixel;
    texture.dirty = false;
    texture.texture_handle = None; // Will be recreated in render
}

/// Map biome type to minimap color
fn biome_to_color(biome: BiomeType) -> (u8, u8, u8) {
    match biome {
        BiomeType::Plains => (100, 200, 80),      // Bright green
        BiomeType::Forest => (40, 140, 40),       // Dark green
        BiomeType::Desert => (240, 220, 160),     // Bright sand
        BiomeType::Mountains => (150, 150, 160),  // Light gray
        BiomeType::Tundra => (255, 255, 255),     // Pure white
        BiomeType::Volcanic => (180, 60, 40),
        BiomeType::Swamp => (85, 107, 47),
        BiomeType::Savanna => (218, 190, 80),
        BiomeType::Taiga => (30, 90, 80),
        BiomeType::Jungle => (50, 200, 50),
        BiomeType::Badlands => (170, 90, 50),
        BiomeType::Mushroom => (140, 70, 160),     // Bright red-brown
    }
}

/// Render the minimap UI
fn minimap_render_system(
    config: Res<MinimapConfig>,
    mut texture: ResMut<MinimapTexture>,
    mut contexts: EguiContexts,
    camera_controller: Query<&crate::engine::controller::CameraController, With<Camera3d>>,
) {
    if !config.visible || texture.pixels.is_empty() {
        return;
    }
    
    let ctx = contexts.ctx_mut();
    let size = texture.texture_size;
    
    // Create or update texture (must keep TextureHandle alive!)
    if texture.texture_handle.is_none() {
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [size, size],
            &texture.pixels,
        );
        texture.texture_handle = Some(ctx.load_texture(
            "minimap",
            color_image,
            egui::TextureOptions::NEAREST,
        ));
    }
    
    let Some(ref tex_handle) = texture.texture_handle else {
        return;
    };
    let tex_id = tex_handle.id();
    
    // Position in top-right corner
    let screen = ctx.screen_rect();
    let map_size = egui::vec2(config.size, config.size);
    let pos = egui::pos2(
        screen.max.x - config.size - config.margin,
        config.margin,
    );
    
    egui::Area::new(egui::Id::new("minimap"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // Background frame
            let frame = egui::Frame::none()
                .fill(egui::Color32::from_rgba_unmultiplied(20, 20, 30, (config.opacity * 255.0) as u8))
                .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgb(60, 60, 80)))
                .rounding(egui::Rounding::same(4.0))
                .inner_margin(egui::Margin::same(4.0));
            
            frame.show(ui, |ui| {
                // Minimap texture
                let img = egui::Image::new(egui::load::SizedTexture::new(tex_id, map_size))
                    .rounding(egui::Rounding::same(2.0));
                let response = ui.add(img);
                
                let rect = response.rect;
                let painter = ui.painter_at(rect);
                
                // Draw player indicator (center)
                let center = rect.center();
                
                // Player direction arrow
                if let Ok(controller) = camera_controller.get_single() {
                    // target_yaw is in degrees, convert to radians for trig
                    let yaw_rad = controller.target_yaw.to_radians();
                    let arrow_len = 12.0;
                    let arrow_width = 6.0;
                    
                    // Arrow points in look direction (north = -Z = up on map)
                    // Camera forward = (-sin(yaw), 0, -cos(yaw)) in world space
                    // Minimap: world +X = screen right, world +Z = screen down
                    let dir_x = -yaw_rad.sin();
                    let dir_z = -yaw_rad.cos();
                    
                    let tip = center + egui::vec2(dir_x * arrow_len, dir_z * arrow_len);
                    let left = center + egui::vec2(
                        -dir_z * arrow_width - dir_x * arrow_len * 0.3,
                        dir_x * arrow_width - dir_z * arrow_len * 0.3,
                    );
                    let right = center + egui::vec2(
                        dir_z * arrow_width - dir_x * arrow_len * 0.3,
                        -dir_x * arrow_width - dir_z * arrow_len * 0.3,
                    );
                    
                    // Draw arrow
                    painter.add(egui::Shape::convex_polygon(
                        vec![tip, left, right],
                        egui::Color32::from_rgb(255, 100, 100),
                        egui::Stroke::new(1.5, egui::Color32::WHITE),
                    ));
                }
                
                // Compass directions
                if config.show_compass {
                    let font = egui::FontId::proportional(10.0);
                    let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 180);
                    
                    // N at top
                    painter.text(
                        egui::pos2(rect.center().x, rect.min.y + 8.0),
                        egui::Align2::CENTER_CENTER,
                        "N",
                        font.clone(),
                        text_color,
                    );
                    // S at bottom
                    painter.text(
                        egui::pos2(rect.center().x, rect.max.y - 8.0),
                        egui::Align2::CENTER_CENTER,
                        "S",
                        font.clone(),
                        text_color,
                    );
                    // E at right
                    painter.text(
                        egui::pos2(rect.max.x - 8.0, rect.center().y),
                        egui::Align2::CENTER_CENTER,
                        "E",
                        font.clone(),
                        text_color,
                    );
                    // W at left
                    painter.text(
                        egui::pos2(rect.min.x + 8.0, rect.center().y),
                        egui::Align2::CENTER_CENTER,
                        "W",
                        font,
                        text_color,
                    );
                }
                
                // Chunk grid overlay
                if config.show_chunk_grid {
                    let grid_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 40);
                    let pixels_per_chunk = CHUNK_SIZE as f32 / config.blocks_per_pixel;
                    
                    // Calculate grid offset based on player position
                    let offset_x = (texture.center_x as f32 % CHUNK_SIZE as f32) / config.blocks_per_pixel;
                    let offset_z = (texture.center_z as f32 % CHUNK_SIZE as f32) / config.blocks_per_pixel;
                    
                    // Vertical lines
                    let mut x = rect.min.x + (config.size / 2.0) - offset_x;
                    while x > rect.min.x {
                        x -= pixels_per_chunk;
                    }
                    while x < rect.max.x {
                        painter.line_segment(
                            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                            egui::Stroke::new(1.0, grid_color),
                        );
                        x += pixels_per_chunk;
                    }
                    
                    // Horizontal lines
                    let mut z = rect.min.y + (config.size / 2.0) - offset_z;
                    while z > rect.min.y {
                        z -= pixels_per_chunk;
                    }
                    while z < rect.max.y {
                        painter.line_segment(
                            [egui::pos2(rect.min.x, z), egui::pos2(rect.max.x, z)],
                            egui::Stroke::new(1.0, grid_color),
                        );
                        z += pixels_per_chunk;
                    }
                }
                
                // Zoom indicator
                let zoom_text = format!("{}x", (1.0 / config.blocks_per_pixel).round());
                painter.text(
                    egui::pos2(rect.max.x - 4.0, rect.max.y - 4.0),
                    egui::Align2::RIGHT_BOTTOM,
                    zoom_text,
                    egui::FontId::proportional(9.0),
                    egui::Color32::from_rgba_unmultiplied(200, 200, 200, 180),
                );
            });
        });
}
