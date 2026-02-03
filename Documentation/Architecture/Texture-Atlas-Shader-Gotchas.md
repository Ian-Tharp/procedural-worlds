## Texture Atlas Shader Gotchas (Bevy 0.15)

This project uses a custom **ExtendedMaterial** shader to make voxel textures **tile per block**
even when geometry is greedy-meshed.

When this shader fails to compile, the world can appear to "render nothing" while UI still draws.
These are the most common causes and what to do next.

### Common "blank world" cause: shader compile failure

Symptoms in logs:
- `ERROR bevy_render::render_resource::pipeline_cache: failed to process shader`

If you see errors like "expected expression" or "reserved keyword", the WGSL did not parse.

### Bevy 0.15 shader preprocessor differences

Bevy 0.15 does **not** support newer WGSL placeholders like:
- `@group(#{MATERIAL_BIND_GROUP})`

For Bevy 0.15 `ExtendedMaterial<StandardMaterial, ...>` shaders, use:
- `@group(2)`

(Confirmed by Bevy's own `v0.15.3` `extended_material.wgsl`.)

### WGSL is not Rust

WGSL does **not** allow Rust syntax such as:
- `fn foo(mut x: u32) -> u32 { ... }`

Use:
- `fn foo(x: u32) -> u32 { var y = x; ... }`

### Why `cargo test` didn't catch it before

Rust unit tests compile Rust, but shader compilation happens at runtime when Bevy builds the render pipeline.

To prevent regressions, we added:
- A unit test that validates the embedded WGSL for common issues: `test_block_atlas_wgsl_validates`
- A startup-time guardrail: if WGSL validation fails, we **skip** creating the atlas material and
  fall back to `StandardMaterial` so the world still renders.

### Where the shader lives

- Shader source: `assets/shaders/block_atlas_material.wgsl`
- Embedded + registered at startup: `src/world/atlas_material.rs`

### Edge bleeding prevention (UV sampling)

Edge bleeding occurs when a fragment samples texels from a neighboring tile in the atlas.
This project prevents it at two levels:

1. **CPU-side inset** (`face_uvs_atlas` in `texture_atlas.rs`):
   UVs are inset by a **full texel** from each tile edge. This ensures that even with
   nearest filtering, the sampler never reaches an adjacent tile's pixels.

2. **Shader-side inset** (`remap_atlas_uv` in `block_atlas_material.wgsl`):
   The fragment shader computes per-block UVs from world position via `fract()`, then
   remaps them into the tile region with a full-texel inset and explicit clamping.
   This handles floating-point edge cases where `fract()` returns values at or near
   tile boundaries.

3. **UV1 tile coordinates**: The mesher emits tile grid coordinates in `ATTRIBUTE_UV_1`
   (Bevy's `uv_b`). The shader reads these via `#ifdef VERTEX_UVS_B` to know which
   tile region to remap into. When using `StandardMaterial` (no shader), UV1 is ignored.

### Debug checklist

If the world goes blank again:
- Check logs for `pipeline_cache: failed to process shader`
- Fix WGSL syntax or Bevy-version-specific annotations (`@group(2)`)
- Ensure the embedded shader is being registered (startup system in `BlockAtlasMaterialPlugin`)

If edge bleeding appears:
- Check that `face_uvs_atlas` uses full-texel inset (not half-texel)
- Check that `remap_atlas_uv` in the shader clamps input UVs
- Verify the atlas sampler uses `ClampToEdge` and `Nearest` filtering
- Ensure UV1 tile coordinates match the expected tile grid positions
