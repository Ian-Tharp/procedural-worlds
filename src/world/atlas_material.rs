//! Custom material for voxel texture atlas tiling.
//!
//! Problem:
//! - With a packed atlas + `StandardMaterial`, scaling UVs to "tile" across a
//!   greedy-merged quad causes the UVs to walk into neighboring tiles, producing
//!   stripes and magenta debug artifacts.
//!
//! Solution:
//! - Store **local** (repeatable) UVs in `uv` (Mesh UV0): 0..quad_w / 0..quad_h
//! - Store **tile coordinates** in `uv_b` (Mesh UV1): (tile_x, tile_y)
//! - In the fragment shader, compute:
//!   `atlas_uv = tile_min + inset + fract(local_uv) * (tile_uv_size - 2*inset)`
//!   so repetition happens *within the tile bounds*.
//!
//! ## Edge Bleeding Prevention
//!
//! UV sampling at tile boundaries is the primary cause of edge bleeding in
//! texture atlases. This module addresses it at two levels:
//!
//! 1. **CPU-side (mesh UV0):** `face_uvs_atlas` in `texture_atlas.rs` insets
//!    UVs by a full texel from tile edges, ensuring StandardMaterial never
//!    samples neighboring tiles.
//!
//! 2. **Shader-side (UV1 + `remap_atlas_uv`):** The fragment shader computes
//!    per-block UVs via `fract()` on world position, then remaps into tile
//!    bounds with a full-texel inset and explicit clamping. This prevents
//!    floating-point edge cases from causing bleed even under magnification.

use bevy::{
    pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin},
    prelude::*,
    render::render_resource::AsBindGroup,
    render::render_resource::ShaderRef,
    render::render_resource::Shader,
};

use super::texture_atlas;

/// Embedded WGSL for the atlas tiling material.
///
/// Embedding avoids runtime "assets folder" issues (common when launching an exe
/// directly or from an IDE with a different working directory).
const BLOCK_ATLAS_SHADER_WGSL: &str =
    include_str!("../../assets/shaders/block_atlas_material.wgsl");

/// Stable handle for the embedded shader asset.
pub const BLOCK_ATLAS_SHADER_HANDLE: Handle<Shader> =
    Handle::weak_from_u128(0x6b9d_8d19_9a36_4a77_9a7a_3b2f_5f6b_aa1c);

/// Plugin that registers the embedded shader and atlas material pipeline.
///
/// This plugin:
/// 1. Registers `MaterialPlugin::<BlockAtlasMaterial>` so Bevy knows how to
///    render entities using this material type.
/// 2. Embeds and registers the WGSL shader asset at startup.
/// 3. Creates a `BlockAtlasChunkMaterial` resource holding the material handle
///    (when textures are enabled and the atlas exists).
pub struct BlockAtlasMaterialPlugin;

impl Plugin for BlockAtlasMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<BlockAtlasMaterial>::default())
            .init_resource::<BlockAtlasChunkMaterial>()
            .add_systems(Startup, register_block_atlas_shader);
    }
}

fn register_block_atlas_shader(mut shaders: ResMut<Assets<Shader>>) {
    // Insert/overwrite is fine; handle is stable.
    shaders.insert(
        &BLOCK_ATLAS_SHADER_HANDLE,
        Shader::from_wgsl(
            BLOCK_ATLAS_SHADER_WGSL,
            "embedded://shaders/block_atlas_material.wgsl",
        ),
    );
}

/// Validate the embedded WGSL for common issues.
///
/// This catches the "blank world" failures documented in
/// `Documentation/Architecture/Texture-Atlas-Shader-Gotchas.md` by checking
/// for known bad patterns. Full pipeline validation happens at runtime when
/// Bevy compiles the shader — this is a best-effort static check.
///
/// We intentionally avoid depending on `naga` directly because the shader
/// uses Bevy preprocessor directives (`#import`, `#ifdef`) that raw WGSL
/// parsers cannot handle.
pub fn validate_block_atlas_wgsl() -> Result<(), String> {
    let src = BLOCK_ATLAS_SHADER_WGSL;

    if src.is_empty() {
        return Err("Shader source is empty".into());
    }

    // Required functions must exist
    for func in &["fn remap_atlas_uv", "fn fragment", "fn rotate_flip_uv", "fn hash_u32"] {
        if !src.contains(func) {
            return Err(format!("Missing required function: {func}"));
        }
    }

    // Required struct must exist
    if !src.contains("struct BlockAtlasExtension") {
        return Err("Missing BlockAtlasExtension struct definition".into());
    }

    // Bevy 0.15 compatibility: must NOT use the newer placeholder syntax
    // Only check non-comment lines (the shader may mention it in comments as
    // documentation of what NOT to do).
    for line in src.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("//") && trimmed.contains("#{MATERIAL_BIND_GROUP}") {
            return Err(
                "Bevy 0.15 does not support #{MATERIAL_BIND_GROUP}; use @group(2) instead"
                    .into(),
            );
        }
    }

    // WGSL is not Rust: must NOT use `mut` in function parameters
    // (common mistake when writing WGSL in a Rust project)
    for line in src.lines() {
        let trimmed = line.trim();
        // Check for `fn name(mut param:` pattern
        if trimmed.starts_with("fn ")
            && trimmed.contains("(mut ")
        {
            return Err(format!(
                "WGSL does not support 'mut' in function parameters: {trimmed}"
            ));
        }
    }

    // Must use correct bind group for Bevy 0.15 ExtendedMaterial
    if !src.contains("@group(2)") {
        return Err("Shader must use @group(2) for Bevy 0.15 ExtendedMaterial bindings".into());
    }

    Ok(())
}

/// Extension data for [`BlockAtlasMaterial`].
///
/// Bindings must not conflict with `StandardMaterial`, so we start at 100.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct BlockAtlasExtension {
    /// Tiles per row in the atlas grid (e.g., 16).
    #[uniform(100)]
    pub tiles_per_row: u32,
    /// Atlas size in pixels (e.g., 256 for 16×16 tiles of 16px).
    #[uniform(100)]
    pub atlas_size: u32,
    /// Padding to keep the uniform 16-byte aligned across backends.
    #[uniform(100)]
    pub _pad0: u32,
    #[uniform(100)]
    pub _pad1: u32,
}

impl BlockAtlasExtension {
    pub fn new(tiles_per_row: u32, atlas_size: u32) -> Self {
        Self {
            tiles_per_row,
            atlas_size,
            _pad0: 0,
            _pad1: 0,
        }
    }
}

impl MaterialExtension for BlockAtlasExtension {
    fn fragment_shader() -> ShaderRef {
        BLOCK_ATLAS_SHADER_HANDLE.clone().into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        BLOCK_ATLAS_SHADER_HANDLE.clone().into()
    }
}

/// PBR material with atlas tiling logic injected via [`BlockAtlasExtension`].
pub type BlockAtlasMaterial = ExtendedMaterial<StandardMaterial, BlockAtlasExtension>;

/// Resource holding the shared [`BlockAtlasMaterial`] handle for chunk rendering.
///
/// Populated at startup when `use_textures` is enabled. Systems that spawn
/// chunk meshes can check this resource and use the atlas material instead of
/// the plain `StandardMaterial` for improved per-block tiling.
#[derive(Resource, Default)]
pub struct BlockAtlasChunkMaterial {
    pub handle: Option<Handle<BlockAtlasMaterial>>,
}

/// Create the [`BlockAtlasMaterial`] from the atlas texture and config.
///
/// Call this after both the atlas texture and the shader have been registered.
/// Returns `None` if textures are disabled, the atlas is missing, or shader
/// validation fails (in which case a warning is logged and the caller should
/// fall back to `StandardMaterial`).
pub fn create_block_atlas_material(
    materials: &mut Assets<BlockAtlasMaterial>,
    atlas: &texture_atlas::BlockTextureAtlas,
) -> Option<Handle<BlockAtlasMaterial>> {
    // Validate shader before creating material
    if let Err(e) = validate_block_atlas_wgsl() {
        warn!(
            "Atlas shader validation failed: {e}. \
             Falling back to StandardMaterial for chunks."
        );
        return None;
    }

    let handle = materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(atlas.texture.clone()),
            perceptual_roughness: 0.9,
            metallic: 0.0,
            ..default()
        },
        extension: BlockAtlasExtension::new(atlas.tiles_per_row, atlas.atlas_size),
    });

    info!(
        "Block atlas material created ({}×{} atlas, {} tiles/row)",
        atlas.atlas_size, atlas.atlas_size, atlas.tiles_per_row
    );

    Some(handle)
}

/// Compute tile grid coordinates (column, row) for a given tile index.
///
/// Used by the mesher to populate UV1 (tile coordinates) for the shader.
/// The shader's `remap_atlas_uv` expects `(tile_x, tile_y)` where
/// `tile_min = tile_xy * (1.0 / tiles_per_row)`.
#[inline]
pub fn tile_grid_coords(tile_index: u32, tiles_per_row: u32) -> [f32; 2] {
    let tile_x = tile_index % tiles_per_row;
    let tile_y = tile_index / tiles_per_row;
    [tile_x as f32, tile_y as f32]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_atlas_wgsl_validates() {
        validate_block_atlas_wgsl().expect("block_atlas_material.wgsl must pass validation");
    }

    #[test]
    fn test_shader_contains_edge_bleeding_prevention() {
        let src = BLOCK_ATLAS_SHADER_WGSL;

        // The shader must clamp UVs to prevent edge bleeding
        assert!(
            src.contains("clamp"),
            "Shader should use clamp() for edge bleeding prevention"
        );

        // The shader must compute an inset to avoid sampling tile borders
        assert!(
            src.contains("inset"),
            "Shader should compute an inset from tile edges"
        );
    }

    #[test]
    fn test_shader_uses_correct_bind_group() {
        assert!(
            BLOCK_ATLAS_SHADER_WGSL.contains("@group(2) @binding(100)"),
            "Shader must bind to group 2, binding 100 for Bevy 0.15 ExtendedMaterial"
        );
    }

    #[test]
    fn test_shader_struct_matches_rust() {
        // The WGSL struct must have the same fields as BlockAtlasExtension
        let src = BLOCK_ATLAS_SHADER_WGSL;
        assert!(src.contains("tiles_per_row: u32"));
        assert!(src.contains("atlas_size: u32"));
        assert!(src.contains("_pad0: u32"));
        assert!(src.contains("_pad1: u32"));
    }

    #[test]
    fn test_extension_new() {
        let ext = BlockAtlasExtension::new(16, 256);
        assert_eq!(ext.tiles_per_row, 16);
        assert_eq!(ext.atlas_size, 256);
        assert_eq!(ext._pad0, 0);
        assert_eq!(ext._pad1, 0);
    }

    #[test]
    fn test_extension_default() {
        let ext = BlockAtlasExtension::default();
        assert_eq!(ext.tiles_per_row, 0);
        assert_eq!(ext.atlas_size, 0);
    }

    #[test]
    fn test_tile_grid_coords() {
        // Tile 0 in a 16-wide grid → (0, 0)
        assert_eq!(tile_grid_coords(0, 16), [0.0, 0.0]);

        // Tile 1 → (1, 0)
        assert_eq!(tile_grid_coords(1, 16), [1.0, 0.0]);

        // Tile 15 → (15, 0) (last in first row)
        assert_eq!(tile_grid_coords(15, 16), [15.0, 0.0]);

        // Tile 16 → (0, 1) (first in second row)
        assert_eq!(tile_grid_coords(16, 16), [0.0, 1.0]);

        // Tile 17 → (1, 1)
        assert_eq!(tile_grid_coords(17, 16), [1.0, 1.0]);

        // Tile 255 → (15, 15) (last tile in 16×16 grid)
        assert_eq!(tile_grid_coords(255, 16), [15.0, 15.0]);
    }

    #[test]
    fn test_tile_grid_coords_different_grid_sizes() {
        // 4-wide grid
        assert_eq!(tile_grid_coords(0, 4), [0.0, 0.0]);
        assert_eq!(tile_grid_coords(3, 4), [3.0, 0.0]);
        assert_eq!(tile_grid_coords(4, 4), [0.0, 1.0]);
        assert_eq!(tile_grid_coords(7, 4), [3.0, 1.0]);
    }

    #[test]
    fn test_validate_passes_on_real_shader() {
        // Verify the validation function accepts the actual embedded shader.
        assert!(validate_block_atlas_wgsl().is_ok());
    }

    #[test]
    fn test_shader_has_uv_safety_clamping() {
        // The fragment function should clamp fract() results for safety
        let src = BLOCK_ATLAS_SHADER_WGSL;
        // Check that the remap function or fragment applies clamping
        assert!(
            src.contains("safe_uv") || src.contains("clamp(uv01"),
            "Shader should have UV safety clamping to prevent edge bleeding"
        );
    }
}
