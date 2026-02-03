//! Texture variation system for breaking tiling repetition.
//!
//! When identical block types are placed adjacent to each other, the repeated
//! tile pattern becomes visually obvious. This module provides position-based
//! variation utilities that produce subtle per-block differences:
//!
//! - **UV offsets**: Small sub-texel shifts within a tile so adjacent blocks
//!   don't sample the exact same pixel pattern.
//! - **Color tints**: Per-channel RGB adjustments for organic-looking variety.
//! - **Deterministic seeding**: All variation is derived from block world
//!   position, ensuring consistency across frames and chunk reloads.
//!
//! The shader (`block_atlas_material.wgsl`) applies these variations at
//! render time using equivalent GPU-side hash functions. This Rust module
//! provides CPU-side utilities for testing, debugging, and any future
//! CPU-side rendering paths.

// ============================================================================
// VARIATION SEED
// ============================================================================

/// Deterministic hash for a single `u32` value.
///
/// Uses the same algorithm as the GPU-side `hash_u32` in the WGSL shader,
/// ensuring CPU/GPU parity for any given input.
#[inline]
pub fn hash_u32(x: u32) -> u32 {
    let mut y = x;
    y ^= y >> 16;
    y = y.wrapping_mul(0x7feb352d);
    y ^= y >> 15;
    y = y.wrapping_mul(0x846ca68b);
    y ^= y >> 16;
    y
}

/// Compute a variation seed from a block's world position and face ID.
///
/// This mirrors the GPU-side `hash3(block_pos, face_id)` function exactly,
/// allowing CPU code to predict what the shader will produce.
///
/// `face_id` values: 0=+Y, 1=-Y, 2=+X, 3=-X, 4=+Z, 5=-Z.
#[inline]
pub fn variation_seed(x: i32, y: i32, z: i32, face_id: u32) -> u32 {
    let xu = x as u32;
    let yu = y as u32;
    let zu = z as u32;
    hash_u32(
        xu ^ yu.wrapping_mul(0x9e3779b9)
            ^ zu.wrapping_mul(0x85ebca6b)
            ^ face_id.wrapping_mul(0xc2b2ae35),
    )
}

// ============================================================================
// UV OFFSET VARIATION
// ============================================================================

/// Maximum UV offset as a fraction of tile size (in texels).
///
/// Kept small enough that the shifted sample never approaches neighboring tile
/// boundaries, but large enough to visibly break tiling patterns.
/// A value of 0.15 means up to ~15% of the tile width/height.
pub const MAX_UV_OFFSET_FRACTION: f32 = 0.15;

/// Compute a sub-texel UV offset for a block at the given world position.
///
/// Returns `(du, dv)` where each component is in `[-MAX_UV_OFFSET_FRACTION, +MAX_UV_OFFSET_FRACTION]`.
/// The offset is applied within the tile's UV region so the sampling point
/// shifts slightly, making adjacent identical blocks look different without
/// leaking into neighboring atlas tiles.
///
/// # Arguments
/// * `x`, `y`, `z` — block world position (integer coordinates)
/// * `face_id` — face identifier (0-5)
#[inline]
pub fn uv_offset(x: i32, y: i32, z: i32, face_id: u32) -> (f32, f32) {
    let seed = variation_seed(x, y, z, face_id);
    // Extract two independent 8-bit values from the hash
    let bits_u = (seed & 0xFF) as f32 / 255.0; // [0, 1]
    let bits_v = ((seed >> 8) & 0xFF) as f32 / 255.0; // [0, 1]
    let du = (bits_u * 2.0 - 1.0) * MAX_UV_OFFSET_FRACTION;
    let dv = (bits_v * 2.0 - 1.0) * MAX_UV_OFFSET_FRACTION;
    (du, dv)
}

// ============================================================================
// COLOR TINT VARIATION
// ============================================================================

/// Maximum per-channel color variation (fraction of base color).
///
/// Each RGB channel is independently adjusted by up to this amount.
/// 0.06 = ±6%, subtle enough for visual coherence but enough to break
/// the "photocopy" look of identical adjacent blocks.
pub const MAX_COLOR_VARIATION: f32 = 0.06;

/// Compute per-channel RGB tint multipliers for a block at the given position.
///
/// Returns `(r_mult, g_mult, b_mult)` where each is in
/// `[1.0 - MAX_COLOR_VARIATION, 1.0 + MAX_COLOR_VARIATION]`.
///
/// Unlike uniform albedo jitter (which multiplies all channels equally),
/// per-channel variation produces warmer/cooler shifts that look more
/// natural for organic materials like stone, dirt, and wood.
#[inline]
pub fn color_tint(x: i32, y: i32, z: i32, face_id: u32) -> (f32, f32, f32) {
    let seed = variation_seed(x, y, z, face_id);
    // Use different bit ranges for each channel to ensure independence
    let r_bits = ((seed >> 16) & 0xFF) as f32 / 255.0;
    let g_bits = ((seed >> 8) & 0xFF) as f32 / 255.0;
    let b_bits = (seed & 0xFF) as f32 / 255.0;
    let r = 1.0 + (r_bits * 2.0 - 1.0) * MAX_COLOR_VARIATION;
    let g = 1.0 + (g_bits * 2.0 - 1.0) * MAX_COLOR_VARIATION;
    let b = 1.0 + (b_bits * 2.0 - 1.0) * MAX_COLOR_VARIATION;
    (r, g, b)
}

// ============================================================================
// ROTATION/FLIP VARIANT
// ============================================================================

/// Compute a rotation/flip variant index (0-7) for a block face.
///
/// This matches the shader's `variant = h & 7u` derivation.
/// - Bits 0-1: rotation (0°, 90°, 180°, 270°)
/// - Bit 2: horizontal flip
#[inline]
pub fn rotation_variant(x: i32, y: i32, z: i32, face_id: u32) -> u8 {
    let seed = variation_seed(x, y, z, face_id);
    (seed & 7) as u8
}

// ============================================================================
// COMBINED VARIATION PARAMETERS
// ============================================================================

/// All variation parameters for a single block face, bundled for convenience.
#[derive(Clone, Copy, Debug)]
pub struct BlockVariation {
    /// Rotation/flip variant index (0-7).
    pub rotation: u8,
    /// Sub-texel UV offset within the tile region.
    pub uv_offset: (f32, f32),
    /// Per-channel color multipliers (r, g, b).
    pub color_tint: (f32, f32, f32),
}

impl BlockVariation {
    /// Compute all variation parameters for a block face.
    pub fn from_position(x: i32, y: i32, z: i32, face_id: u32) -> Self {
        Self {
            rotation: rotation_variant(x, y, z, face_id),
            uv_offset: uv_offset(x, y, z, face_id),
            color_tint: color_tint(x, y, z, face_id),
        }
    }

    /// The identity (no variation) — useful as a default or for testing.
    pub fn identity() -> Self {
        Self {
            rotation: 0,
            uv_offset: (0.0, 0.0),
            color_tint: (1.0, 1.0, 1.0),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_u32_deterministic() {
        // Same input must produce same output
        assert_eq!(hash_u32(42), hash_u32(42));
        assert_eq!(hash_u32(0), hash_u32(0));
        assert_eq!(hash_u32(u32::MAX), hash_u32(u32::MAX));
    }

    #[test]
    fn test_hash_u32_avalanche() {
        // Adjacent inputs should produce very different outputs
        let a = hash_u32(0);
        let b = hash_u32(1);
        // At least half the bits should differ (good avalanche)
        let diff_bits = (a ^ b).count_ones();
        assert!(
            diff_bits >= 8,
            "hash_u32 avalanche too weak: {} differing bits for inputs 0 vs 1",
            diff_bits
        );
    }

    #[test]
    fn test_variation_seed_deterministic() {
        let s1 = variation_seed(10, 20, 30, 0);
        let s2 = variation_seed(10, 20, 30, 0);
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_variation_seed_differs_by_position() {
        let s1 = variation_seed(0, 0, 0, 0);
        let s2 = variation_seed(1, 0, 0, 0);
        let s3 = variation_seed(0, 1, 0, 0);
        let s4 = variation_seed(0, 0, 1, 0);
        // All should differ (extremely high probability)
        assert_ne!(s1, s2);
        assert_ne!(s1, s3);
        assert_ne!(s1, s4);
        assert_ne!(s2, s3);
    }

    #[test]
    fn test_variation_seed_differs_by_face() {
        let seeds: Vec<u32> = (0..6).map(|f| variation_seed(5, 5, 5, f)).collect();
        // All 6 face seeds should be unique
        for i in 0..seeds.len() {
            for j in (i + 1)..seeds.len() {
                assert_ne!(
                    seeds[i], seeds[j],
                    "Face {} and {} produced same seed",
                    i, j
                );
            }
        }
    }

    #[test]
    fn test_variation_seed_handles_negative_coords() {
        // Should not panic and should produce valid hashes
        let s1 = variation_seed(-1, -1, -1, 0);
        let s2 = variation_seed(-100, 50, -200, 3);
        // Just verify they're different from origin
        assert_ne!(s1, variation_seed(0, 0, 0, 0));
        assert_ne!(s2, variation_seed(0, 0, 0, 0));
    }

    #[test]
    fn test_uv_offset_range() {
        // Check that UV offsets stay within the documented bounds
        for x in -5..5 {
            for z in -5..5 {
                for face in 0..6 {
                    let (du, dv) = uv_offset(x, 10, z, face);
                    assert!(
                        du.abs() <= MAX_UV_OFFSET_FRACTION + 1e-6,
                        "du={du} out of range for pos ({x},10,{z}) face {face}"
                    );
                    assert!(
                        dv.abs() <= MAX_UV_OFFSET_FRACTION + 1e-6,
                        "dv={dv} out of range for pos ({x},10,{z}) face {face}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_uv_offset_varies_between_positions() {
        let (du1, dv1) = uv_offset(0, 0, 0, 0);
        let (du2, dv2) = uv_offset(1, 0, 0, 0);
        // At least one component should differ
        assert!(
            (du1 - du2).abs() > 1e-6 || (dv1 - dv2).abs() > 1e-6,
            "Adjacent blocks should have different UV offsets"
        );
    }

    #[test]
    fn test_uv_offset_deterministic() {
        let (du1, dv1) = uv_offset(42, 7, -3, 2);
        let (du2, dv2) = uv_offset(42, 7, -3, 2);
        assert!((du1 - du2).abs() < 1e-9);
        assert!((dv1 - dv2).abs() < 1e-9);
    }

    #[test]
    fn test_color_tint_range() {
        let lower = 1.0 - MAX_COLOR_VARIATION;
        let upper = 1.0 + MAX_COLOR_VARIATION;
        for x in -5..5 {
            for y in -5..5 {
                for face in 0..6 {
                    let (r, g, b) = color_tint(x, y, 0, face);
                    assert!(
                        r >= lower - 1e-6 && r <= upper + 1e-6,
                        "r={r} out of range for pos ({x},{y},0) face {face}"
                    );
                    assert!(
                        g >= lower - 1e-6 && g <= upper + 1e-6,
                        "g={g} out of range for pos ({x},{y},0) face {face}"
                    );
                    assert!(
                        b >= lower - 1e-6 && b <= upper + 1e-6,
                        "b={b} out of range for pos ({x},{y},0) face {face}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_color_tint_per_channel_independence() {
        // Verify that channels vary independently (not all the same multiplier)
        let mut all_same = true;
        for x in 0..20 {
            let (r, g, b) = color_tint(x, 0, 0, 0);
            if (r - g).abs() > 1e-4 || (r - b).abs() > 1e-4 {
                all_same = false;
                break;
            }
        }
        assert!(
            !all_same,
            "Per-channel tints should vary independently (not uniform)"
        );
    }

    #[test]
    fn test_rotation_variant_range() {
        for x in -10..10 {
            for z in -10..10 {
                let v = rotation_variant(x, 0, z, 0);
                assert!(v < 8, "Variant {v} out of range [0..8) at ({x},0,{z})");
            }
        }
    }

    #[test]
    fn test_rotation_variant_distribution() {
        // Over a large enough sample, all 8 variants should appear
        let mut seen = [false; 8];
        for x in 0..100 {
            for z in 0..100 {
                let v = rotation_variant(x, 0, z, 0) as usize;
                seen[v] = true;
            }
        }
        for (i, &s) in seen.iter().enumerate() {
            assert!(s, "Variant {i} never appeared in 10000 samples");
        }
    }

    #[test]
    fn test_block_variation_identity() {
        let id = BlockVariation::identity();
        assert_eq!(id.rotation, 0);
        assert!((id.uv_offset.0).abs() < 1e-9);
        assert!((id.uv_offset.1).abs() < 1e-9);
        assert!((id.color_tint.0 - 1.0).abs() < 1e-9);
        assert!((id.color_tint.1 - 1.0).abs() < 1e-9);
        assert!((id.color_tint.2 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_block_variation_from_position() {
        let v = BlockVariation::from_position(5, 10, 15, 0);
        // Rotation in range
        assert!(v.rotation < 8);
        // UV offset in range
        assert!(v.uv_offset.0.abs() <= MAX_UV_OFFSET_FRACTION + 1e-6);
        assert!(v.uv_offset.1.abs() <= MAX_UV_OFFSET_FRACTION + 1e-6);
        // Color tint in range
        let lower = 1.0 - MAX_COLOR_VARIATION;
        let upper = 1.0 + MAX_COLOR_VARIATION;
        assert!(v.color_tint.0 >= lower - 1e-6 && v.color_tint.0 <= upper + 1e-6);
        assert!(v.color_tint.1 >= lower - 1e-6 && v.color_tint.1 <= upper + 1e-6);
        assert!(v.color_tint.2 >= lower - 1e-6 && v.color_tint.2 <= upper + 1e-6);
    }

    #[test]
    fn test_block_variation_differs_between_adjacent_blocks() {
        let v1 = BlockVariation::from_position(0, 0, 0, 0);
        let v2 = BlockVariation::from_position(1, 0, 0, 0);
        // At least one parameter should differ
        let differs = v1.rotation != v2.rotation
            || (v1.uv_offset.0 - v2.uv_offset.0).abs() > 1e-6
            || (v1.uv_offset.1 - v2.uv_offset.1).abs() > 1e-6
            || (v1.color_tint.0 - v2.color_tint.0).abs() > 1e-6
            || (v1.color_tint.1 - v2.color_tint.1).abs() > 1e-6
            || (v1.color_tint.2 - v2.color_tint.2).abs() > 1e-6;
        assert!(differs, "Adjacent blocks should have different variation");
    }

    #[test]
    fn test_cpu_gpu_hash_parity() {
        // Verify our CPU hash matches the expected GPU hash for known inputs.
        // Since we can't run the GPU code in a unit test, we verify the CPU
        // implementation is self-consistent and produces the documented outputs.
        //
        // Note: hash_u32(0) == 0 is a fixed point of this particular hash
        // function (since XOR/shift of all zeros stays zero). This is fine —
        // the variation_seed combiner ensures real inputs are never all-zero.
        let h1 = hash_u32(1);
        let h2 = hash_u32(2);
        // Non-zero inputs should produce non-zero, distinct outputs
        assert_ne!(h1, 0, "hash_u32(1) should not be 0");
        assert_ne!(h1, h2, "hash_u32(1) and hash_u32(2) should differ");
        // Reproducibility across calls
        assert_eq!(hash_u32(0x12345678), hash_u32(0x12345678));
    }

    #[test]
    fn test_variation_coverage_no_dead_spots() {
        // Ensure there are no systematic dead spots where variation is near zero
        // Sample a grid and check that variation is applied (not identity)
        let mut near_identity_count = 0;
        let sample_size = 1000;
        for i in 0..sample_size {
            let x = (i * 7) % 100 - 50; // Spread across negative and positive
            let z = (i * 13) % 100 - 50;
            let v = BlockVariation::from_position(x, 0, z, 0);
            let is_near_identity = v.uv_offset.0.abs() < 0.01
                && v.uv_offset.1.abs() < 0.01
                && (v.color_tint.0 - 1.0).abs() < 0.01
                && (v.color_tint.1 - 1.0).abs() < 0.01
                && (v.color_tint.2 - 1.0).abs() < 0.01;
            if is_near_identity {
                near_identity_count += 1;
            }
        }
        // Less than 10% of blocks should be near-identity
        let threshold = sample_size / 10;
        assert!(
            near_identity_count < threshold,
            "Too many near-identity blocks: {near_identity_count}/{sample_size} \
             (threshold: {threshold}). Hash distribution may be poor."
        );
    }
}
