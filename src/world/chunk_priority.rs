//! Chunk loading priority system — distance + direction-based ordering
//!
//! Replaces the simple distance-ring iteration in `chunk_streaming_system`
//! with a priority queue that factors in both distance from the player
//! and alignment with the camera's forward direction.
//!
//! Chunks the player is looking toward are prioritised for loading,
//! giving the perception of faster world generation in the direction
//! that matters most.
//!
//! # Priority Score
//!
//! ```text
//! score = distance - direction_bonus
//!
//! distance        = Chebyshev distance (max of |dx|, |dz|) from player chunk
//! direction_bonus = dot(to_chunk_xz, forward_xz) * weight * distance_factor
//! ```
//!
//! Lower scores are loaded first. The direction bonus reduces the score
//! for chunks in the forward direction and increases it for chunks behind,
//! effectively reordering same-distance chunks by facing.

use bevy::prelude::*;

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Configuration for chunk loading priority.
///
/// Controls how strongly the camera's forward direction biases the
/// loading order. Insert as a Bevy resource alongside `ChunkManager`.
#[derive(Resource, Clone, Debug)]
pub struct ChunkPriorityConfig {
    /// How much the forward direction influences loading order.
    ///
    /// At `0.0`, priority is purely distance-based (same as the old system).
    /// At `1.0`, chunks directly in front of the player can jump ahead
    /// of chunks at the same distance behind. Higher values make the
    /// directional bias even more aggressive.
    ///
    /// Default: `0.5`.
    pub direction_weight: f32,

    /// Whether priority sorting is enabled.
    ///
    /// When `false`, falls back to the simple distance-ring iteration.
    /// Useful for profiling or disabling the feature at runtime.
    ///
    /// Default: `true`.
    pub enabled: bool,
}

impl Default for ChunkPriorityConfig {
    fn default() -> Self {
        Self {
            direction_weight: 0.5,
            enabled: true,
        }
    }
}

// ============================================================================
// PRIORITY CALCULATION
// ============================================================================

/// A chunk position paired with its computed priority score.
///
/// Lower `score` means higher priority (loaded first).
#[derive(Debug, Clone)]
pub struct ScoredChunk {
    /// Chunk-coordinate position.
    pub position: IVec3,
    /// Priority score — lower values load first.
    pub score: f32,
}

/// Compute the priority score for a single chunk position.
///
/// The score is based on Chebyshev distance (matching the existing
/// distance-ring logic) with an optional directional bias that reduces
/// the score for chunks aligned with `forward_dir`.
///
/// # Arguments
///
/// * `chunk_pos` — Target chunk position in chunk coordinates.
/// * `player_chunk` — Player's current chunk position.
/// * `forward_dir` — Camera's horizontal forward direction (normalized XZ).
///   Pass `None` to use pure distance-based priority.
/// * `direction_weight` — Strength of the directional bias (see [`ChunkPriorityConfig`]).
///
/// # Returns
///
/// A priority score where lower values should be loaded first.
pub fn compute_priority_score(
    chunk_pos: IVec3,
    player_chunk: IVec3,
    forward_dir: Option<Vec3>,
    direction_weight: f32,
) -> f32 {
    let diff = chunk_pos - player_chunk;

    // Chebyshev distance on the XZ plane (matches existing ring-based iteration)
    let distance = diff.x.abs().max(diff.z.abs()) as f32;

    // No direction bias: pure distance
    let Some(forward) = forward_dir else {
        return distance;
    };

    // Compute horizontal direction from player to chunk
    let to_chunk = Vec3::new(diff.x as f32, 0.0, diff.z as f32);
    let to_chunk_len = to_chunk.length();

    if to_chunk_len < 0.001 {
        // Player's own chunk — highest priority
        return 0.0;
    }

    let to_chunk_norm = to_chunk / to_chunk_len;
    let forward_xz = Vec3::new(forward.x, 0.0, forward.z);
    let forward_len = forward_xz.length();

    if forward_len < 0.001 {
        // Degenerate forward vector (looking straight up/down)
        return distance;
    }

    let forward_norm = forward_xz / forward_len;

    // Dot product: +1 = directly in front, -1 = directly behind
    let dot = to_chunk_norm.dot(forward_norm);

    // Direction bonus: positive for forward chunks (reduces score),
    // negative for behind chunks (increases score).
    // Scaled by distance so the effect is proportional.
    let direction_bonus = dot * direction_weight * distance;

    distance - direction_bonus
}

/// Collect all chunk positions that need loading and return them sorted by priority.
///
/// This replaces the nested distance-ring loop in the old `chunk_streaming_system`.
/// Positions that are already loaded or pending are excluded.
///
/// # Arguments
///
/// * `player_chunk` — Player's current chunk position.
/// * `load_distance` — Horizontal load distance in chunks.
/// * `vert_down` — Vertical layers to load below the player.
/// * `vert_up` — Vertical layers to load above the player.
/// * `forward_dir` — Camera's horizontal forward direction (normalized), or `None`.
/// * `direction_weight` — Strength of the directional bias.
/// * `loaded` — Set of already-loaded chunk positions.
/// * `pending` — Set of already-pending chunk positions.
///
/// # Returns
///
/// A `Vec<ScoredChunk>` sorted by ascending score (highest priority first).
#[allow(clippy::too_many_arguments)]
pub fn collect_needed_chunks_sorted(
    player_chunk: IVec3,
    load_distance: i32,
    vert_down: i32,
    vert_up: i32,
    forward_dir: Option<Vec3>,
    direction_weight: f32,
    loaded: &std::collections::HashSet<IVec3>,
    pending: &std::collections::HashSet<IVec3>,
) -> Vec<ScoredChunk> {
    let mut scored = Vec::new();

    for x in (player_chunk.x - load_distance)..=(player_chunk.x + load_distance) {
        for z in (player_chunk.z - load_distance)..=(player_chunk.z + load_distance) {
            for y in -vert_down..=vert_up {
                let chunk_pos = IVec3::new(x, y, z);

                // Skip already loaded or pending
                if loaded.contains(&chunk_pos) || pending.contains(&chunk_pos) {
                    continue;
                }

                let score =
                    compute_priority_score(chunk_pos, player_chunk, forward_dir, direction_weight);

                scored.push(ScoredChunk {
                    position: chunk_pos,
                    score,
                });
            }
        }
    }

    // Sort by score ascending (lowest = highest priority)
    scored.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    scored
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // --- compute_priority_score tests ---

    #[test]
    fn test_score_at_player_chunk_is_zero() {
        let player = IVec3::new(5, 0, 5);
        let score = compute_priority_score(player, player, Some(Vec3::NEG_Z), 0.5);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_score_increases_with_distance() {
        let player = IVec3::ZERO;
        let forward = Some(Vec3::X);

        let near = compute_priority_score(IVec3::new(1, 0, 0), player, forward, 0.5);
        let far = compute_priority_score(IVec3::new(5, 0, 0), player, forward, 0.5);

        assert!(
            near < far,
            "Nearer chunks should have lower scores: near={near}, far={far}"
        );
    }

    #[test]
    fn test_forward_chunks_prioritised_over_behind() {
        let player = IVec3::ZERO;
        let forward = Some(Vec3::X); // Looking +X

        // Chunk directly in front
        let front_score = compute_priority_score(IVec3::new(3, 0, 0), player, forward, 0.5);
        // Chunk directly behind
        let back_score = compute_priority_score(IVec3::new(-3, 0, 0), player, forward, 0.5);

        assert!(
            front_score < back_score,
            "Forward chunk should have lower score: front={front_score}, back={back_score}"
        );
    }

    #[test]
    fn test_side_chunks_neutral_priority() {
        let player = IVec3::ZERO;
        let forward = Some(Vec3::X); // Looking +X

        // Chunk to the side (perpendicular)
        let side_score = compute_priority_score(IVec3::new(0, 0, 3), player, forward, 0.5);
        // Should be roughly equal to base distance (3.0), no significant bonus
        assert!(
            (side_score - 3.0).abs() < 0.01,
            "Side chunk score should be ~3.0: got {side_score}"
        );
    }

    #[test]
    fn test_no_direction_gives_pure_distance() {
        let player = IVec3::ZERO;
        let score = compute_priority_score(IVec3::new(4, 0, 3), player, None, 0.5);
        // Chebyshev distance = max(4, 3) = 4.0
        assert_eq!(score, 4.0);
    }

    #[test]
    fn test_zero_weight_gives_pure_distance() {
        let player = IVec3::ZERO;
        let forward = Some(Vec3::X);
        let score = compute_priority_score(IVec3::new(3, 0, 0), player, forward, 0.0);
        assert_eq!(score, 3.0);
    }

    #[test]
    fn test_higher_weight_increases_direction_effect() {
        let player = IVec3::ZERO;
        let forward = Some(Vec3::X);
        let pos = IVec3::new(3, 0, 0);

        let low_weight = compute_priority_score(pos, player, forward, 0.3);
        let high_weight = compute_priority_score(pos, player, forward, 0.8);

        // Higher weight = lower score for forward chunks (more bonus)
        assert!(
            high_weight < low_weight,
            "Higher weight should give lower score for forward chunk: high={high_weight}, low={low_weight}"
        );
    }

    #[test]
    fn test_diagonal_forward() {
        let player = IVec3::ZERO;
        let forward = Some(Vec3::new(1.0, 0.0, 1.0).normalize());

        // Chunk in the diagonal forward direction
        let diag_score = compute_priority_score(IVec3::new(3, 0, 3), player, forward, 0.5);
        // Chunk in the opposite diagonal
        let opp_score = compute_priority_score(IVec3::new(-3, 0, -3), player, forward, 0.5);

        assert!(
            diag_score < opp_score,
            "Diagonal-forward should beat diagonal-behind: diag={diag_score}, opp={opp_score}"
        );
    }

    #[test]
    fn test_score_with_degenerate_forward_falls_back_to_distance() {
        let player = IVec3::ZERO;
        // Forward vector is straight up (no XZ component)
        let forward = Some(Vec3::Y);
        let score = compute_priority_score(IVec3::new(3, 0, 0), player, forward, 0.5);
        // Should fall back to pure distance
        assert_eq!(score, 3.0);
    }

    #[test]
    fn test_score_negative_coordinates() {
        let player = IVec3::new(-5, 0, -5);
        let forward = Some(Vec3::NEG_X);

        // Chunk in front (-X direction)
        let front = compute_priority_score(IVec3::new(-8, 0, -5), player, forward, 0.5);
        // Chunk behind (+X direction)
        let behind = compute_priority_score(IVec3::new(-2, 0, -5), player, forward, 0.5);

        assert!(
            front < behind,
            "Forward chunk should score lower in negative coords"
        );
    }

    // --- collect_needed_chunks_sorted tests ---

    #[test]
    fn test_collect_excludes_loaded_and_pending() {
        let player = IVec3::ZERO;
        let mut loaded = HashSet::new();
        let mut pending = HashSet::new();

        loaded.insert(IVec3::new(1, 0, 0));
        pending.insert(IVec3::new(-1, 0, 0));

        let sorted = collect_needed_chunks_sorted(
            player, 1,    // load_distance
            0,    // vert_down
            0,    // vert_up
            None, // no direction
            0.5, &loaded, &pending,
        );

        let positions: HashSet<IVec3> = sorted.iter().map(|s| s.position).collect();
        assert!(
            !positions.contains(&IVec3::new(1, 0, 0)),
            "Should exclude loaded"
        );
        assert!(
            !positions.contains(&IVec3::new(-1, 0, 0)),
            "Should exclude pending"
        );
    }

    #[test]
    fn test_collect_sorted_by_score() {
        let player = IVec3::ZERO;
        let loaded = HashSet::new();
        let pending = HashSet::new();

        let sorted = collect_needed_chunks_sorted(
            player,
            3, // load_distance
            0,
            0,
            Some(Vec3::X), // looking +X
            0.5,
            &loaded,
            &pending,
        );

        // Verify scores are non-decreasing (sorted ascending)
        for window in sorted.windows(2) {
            assert!(
                window[0].score <= window[1].score,
                "Scores must be non-decreasing: {} > {} at {:?} vs {:?}",
                window[0].score,
                window[1].score,
                window[0].position,
                window[1].position,
            );
        }
    }

    #[test]
    fn test_collect_forward_chunks_appear_first() {
        let player = IVec3::ZERO;
        let loaded = HashSet::new();
        let pending = HashSet::new();

        let sorted = collect_needed_chunks_sorted(
            player,
            3,
            0,
            0,
            Some(Vec3::X), // looking +X
            0.5,
            &loaded,
            &pending,
        );

        // Among distance-3 chunks, the one at (3,0,0) should appear before (-3,0,0)
        let front_idx = sorted
            .iter()
            .position(|s| s.position == IVec3::new(3, 0, 0))
            .expect("(3,0,0) should be in results");
        let back_idx = sorted
            .iter()
            .position(|s| s.position == IVec3::new(-3, 0, 0))
            .expect("(-3,0,0) should be in results");

        assert!(
            front_idx < back_idx,
            "Forward chunk should appear before behind chunk: front_idx={front_idx}, back_idx={back_idx}"
        );
    }

    #[test]
    fn test_collect_player_chunk_first() {
        let player = IVec3::new(2, 0, 2);
        let loaded = HashSet::new();
        let pending = HashSet::new();

        let sorted =
            collect_needed_chunks_sorted(player, 2, 0, 0, Some(Vec3::X), 0.5, &loaded, &pending);

        assert!(!sorted.is_empty());
        assert_eq!(
            sorted[0].position, player,
            "Player's own chunk should be first (score 0.0)"
        );
        assert_eq!(sorted[0].score, 0.0);
    }

    #[test]
    fn test_collect_with_vertical_range() {
        let player = IVec3::ZERO;
        let loaded = HashSet::new();
        let pending = HashSet::new();

        let sorted = collect_needed_chunks_sorted(
            player, 1, 2, // vert_down
            3, // vert_up
            None, 0.5, &loaded, &pending,
        );

        // Horizontal: 3x3 = 9 positions, vertical: -2..=3 = 6 layers → 54 total
        assert_eq!(sorted.len(), 9 * 6);

        // All y values should be in range
        for s in &sorted {
            assert!(s.position.y >= -2 && s.position.y <= 3);
        }
    }

    #[test]
    fn test_collect_empty_when_all_loaded() {
        let player = IVec3::ZERO;
        let mut loaded = HashSet::new();
        let pending = HashSet::new();

        // Mark everything within load_distance=1 as loaded
        for x in -1..=1 {
            for z in -1..=1 {
                loaded.insert(IVec3::new(x, 0, z));
            }
        }

        let sorted = collect_needed_chunks_sorted(player, 1, 0, 0, None, 0.5, &loaded, &pending);

        assert!(sorted.is_empty(), "All chunks loaded — nothing to collect");
    }

    #[test]
    fn test_collect_count_matches_expected() {
        let player = IVec3::ZERO;
        let loaded = HashSet::new();
        let pending = HashSet::new();

        let sorted = collect_needed_chunks_sorted(player, 2, 0, 0, None, 0.5, &loaded, &pending);

        // 5x5 grid * 1 vertical layer = 25
        assert_eq!(sorted.len(), 25);
    }

    #[test]
    fn test_no_direction_gives_distance_only_ordering() {
        let player = IVec3::ZERO;
        let loaded = HashSet::new();
        let pending = HashSet::new();

        let sorted = collect_needed_chunks_sorted(
            player, 3, 0, 0, None, // no direction bias
            0.5, &loaded, &pending,
        );

        // First chunk should be the player chunk (distance 0)
        assert_eq!(sorted[0].position, player);

        // All scores should equal Chebyshev distance
        for s in &sorted {
            let diff = s.position - player;
            let expected = diff.x.abs().max(diff.z.abs()) as f32;
            assert_eq!(
                s.score, expected,
                "Without direction, score should be Chebyshev distance: pos={:?}",
                s.position
            );
        }
    }
}
