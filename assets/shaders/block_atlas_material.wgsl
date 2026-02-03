#import bevy_pbr::{
  pbr_fragment::pbr_input_from_standard_material,
  pbr_functions::alpha_discard,
}

#ifdef PREPASS_PIPELINE
  #import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
  }
#else
  #import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
  }
#endif

// Must match `BlockAtlasExtension` in Rust.
struct BlockAtlasExtension {
  tiles_per_row: u32,
  atlas_size: u32,
  _pad0: u32,
  _pad1: u32,
}

// Bevy 0.15: Standard/Extended material bind group is 2.
// (Bevy 0.15's shader processor does NOT support the newer `#{MATERIAL_BIND_GROUP}` placeholder.)
@group(2) @binding(100)
var<uniform> block_atlas: BlockAtlasExtension;

fn hash_u32(x: u32) -> u32 {
  // A tiny integer hash (fast, deterministic).
  var y: u32 = x;
  y ^= y >> 16u;
  y *= 0x7feb352du;
  y ^= y >> 15u;
  y *= 0x846ca68bu;
  y ^= y >> 16u;
  return y;
}

fn hash3(p: vec3<i32>, face_id: u32) -> u32 {
  let x = bitcast<u32>(p.x);
  let y = bitcast<u32>(p.y);
  let z = bitcast<u32>(p.z);
  return hash_u32(x ^ (y * 0x9e3779b9u) ^ (z * 0x85ebca6bu) ^ (face_id * 0xc2b2ae35u));
}

/// Check if a tile has directional texture that needs vertical orientation preserved.
/// Returns true for grass side (3,0), bark (7,0), sandstone (9,0), cactus side (15,0).
fn is_directional_tile(tile_xy: vec2<f32>) -> bool {
  let col = tile_xy.x;
  let row = tile_xy.y;
  // All directional tiles are in row 0
  if (row > 0.5) {
    return false;
  }
  // Grass side=3, Bark=7, Sandstone=9, Cactus side=15
  if (abs(col - 3.0) < 0.5 || abs(col - 7.0) < 0.5 || abs(col - 9.0) < 0.5 || abs(col - 15.0) < 0.5) {
    return true;
  }
  return false;
}

fn rotate_flip_uv(uv: vec2<f32>, variant: u32) -> vec2<f32> {
  // variant: 0..7 => 4 rotations * optional X flip
  var out = uv;
  if ((variant & 4u) != 0u) {
    out.x = 1.0 - out.x;
  }

  let rot = variant & 3u;
  if (rot == 1u) {
    // 90°
    out = vec2<f32>(out.y, 1.0 - out.x);
  } else if (rot == 2u) {
    // 180°
    out = vec2<f32>(1.0 - out.x, 1.0 - out.y);
  } else if (rot == 3u) {
    // 270°
    out = vec2<f32>(1.0 - out.y, out.x);
  }

  return out;
}

/// Remap a normalized [0,1] UV into the atlas tile region with edge-bleeding prevention.
///
/// Edge bleeding occurs when UV sampling hits the boundary between two adjacent
/// tiles in the atlas. Even with nearest filtering, floating-point imprecision
/// can cause the sampler to read from the wrong tile. This function prevents
/// that with two techniques:
///
/// 1. **Full-texel inset:** UVs are inset by one full texel (not half) from
///    each tile edge. This provides a safety margin even under magnification,
///    mip-mapping, or driver-level filtering quirks.
///
/// 2. **Input clamping:** The input UV is clamped to [0,1] before remapping.
///    Since `fract()` can produce values extremely close to 1.0 due to
///    floating-point precision, this prevents the remapped UV from escaping
///    the tile region.
fn remap_atlas_uv(uv01: vec2<f32>, tile_xy: vec2<f32>) -> vec2<f32> {
  let tpr: u32 = max(block_atlas.tiles_per_row, 1u);
  let tile_uv_size: f32 = 1.0 / f32(tpr);

  // Full-texel inset from each tile edge.
  // Half-texel is the theoretical minimum for nearest filtering, but a full
  // texel provides robustness against:
  //   - Bilinear filtering bleed (if filtering mode changes)
  //   - Mip-map level sampling that straddles tile boundaries
  //   - GPU driver rounding differences
  let atlas_px: u32 = max(block_atlas.atlas_size, 1u);
  let texel: f32 = 1.0 / f32(atlas_px);
  let inset: f32 = min(texel, 0.25 * tile_uv_size);

  // Clamp input UV to [0, 1] to guard against fract() edge cases.
  // fract(x) returns x - floor(x), which is in [0, 1) mathematically,
  // but floating-point can produce values at or beyond 1.0 for inputs
  // near integer boundaries. Without this clamp, such values would
  // push the remapped UV into the neighboring tile.
  let safe_uv: vec2<f32> = clamp(uv01, vec2<f32>(0.0), vec2<f32>(1.0));

  let tile_min: vec2<f32> = tile_xy * tile_uv_size;
  let inner_size: f32 = tile_uv_size - 2.0 * inset;

  // Final atlas UV: tile origin + inset + scaled local UV
  return tile_min + vec2<f32>(inset) + safe_uv * inner_size;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
  // Override the UVs *before* StandardMaterial sampling happens.
  var vin = in;

  // Compute a stable per-block variation using world position + face.
  // This breaks obvious tiling repetition without needing extra textures.
  let n = normalize(in.world_normal);
  let an = abs(n);
  let pos = in.world_position.xyz - n * 0.001; // push inside the block for stable floor()
  let block_pos = vec3<i32>(floor(pos));

  // Face id for hashing (0..5)
  var face_id: u32 = 0u;
  if (an.y >= an.x && an.y >= an.z) {
    face_id = select(0u, 1u, n.y < 0.0);
  } else if (an.x >= an.z) {
    face_id = select(2u, 3u, n.x < 0.0);
  } else {
    face_id = select(4u, 5u, n.z < 0.0);
  }

  let h = hash3(block_pos, face_id);
  let variant = h & 7u;

  // Determine if this is a side face (not top/bottom)
  let is_side_face: bool = !(an.y >= an.x && an.y >= an.z);

  // Local UV in the face plane from world position, ensuring world-stable mapping.
  // Top/bottom:  (x,z), North/south: (x,y), East/west: (z,y)
  var uv_raw: vec2<f32> = vec2<f32>(0.0, 0.0);
  if (an.y >= an.x && an.y >= an.z) {
    uv_raw = vec2<f32>(fract(pos.x), fract(pos.z));
  } else if (an.x >= an.z) {
    uv_raw = vec2<f32>(fract(pos.z), fract(pos.y));
  } else {
    uv_raw = vec2<f32>(fract(pos.x), fract(pos.y));
  }

  // Flip V for side faces: fract(pos.y) goes 0→1 from block bottom to top,
  // but texture V=0 is the image top. Flipping ensures that world Y=1 (block top)
  // maps to texture V=0 (top of image, e.g. the green strip on grass side).
  if (is_side_face) {
    uv_raw.y = 1.0 - uv_raw.y;
  }

  // Clamp fract() results to a safe interior range [epsilon, 1-epsilon].
  // This prevents sampling at exact tile edges where fract() returns 0.0
  // (which maps to the tile's left/bottom border) or values extremely
  // close to 1.0 (which approach the next tile's border).
  let eps: f32 = 0.001;
  var uv01: vec2<f32> = clamp(uv_raw, vec2<f32>(eps), vec2<f32>(1.0 - eps));

  // For directional textures, restrict the variant to avoid 90°/270° rotations
  // which would rotate vertically-oriented features (grass strips, bark lines)
  // sideways. Only allow no rotation (0) or horizontal flip (4).
  var effective_variant: u32 = variant;
  #ifdef VERTEX_UVS
    #ifdef VERTEX_UVS_B
      if (is_directional_tile(in.uv_b)) {
        effective_variant = variant & 4u; // keep flip bit, zero rotation bits
      }
    #endif
  #endif

  uv01 = rotate_flip_uv(uv01, effective_variant);

  #ifdef VERTEX_UVS
    #ifdef VERTEX_UVS_B
      vin.uv = remap_atlas_uv(uv01, in.uv_b);
    #endif
  #endif

  // Generate PbrInput from StandardMaterial bindings (now sampling with remapped UVs).
  var pbr_input = pbr_input_from_standard_material(vin, is_front);

  // Subtle per-block albedo variation to reduce flat/plasticky look.
  // Keep it tight so it doesn't look like noise.
  let jitter = (f32((h >> 8u) & 255u) / 255.0) * 0.10 - 0.05; // [-0.05, +0.05]
  pbr_input.material.base_color.rgb *= (1.0 + jitter);

  // Alpha discard (for alpha-cutout textures, if configured in StandardMaterial).
  pbr_input.material.base_color =
    alpha_discard(pbr_input.material, pbr_input.material.base_color);

  #ifdef PREPASS_PIPELINE
    let out = deferred_output(vin, pbr_input);
  #else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
  #endif

  return out;
}
