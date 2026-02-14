//! Integration tests for the chunk loading pipeline.
//!
//! Verifies the end-to-end flow: chunk creation → terrain generation →
//! persistence (save/load) → data integrity, including async task pool
//! execution and ChunkManager bookkeeping.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::math::IVec3;
use bevy::tasks::{AsyncComputeTaskPool, block_on};

use procedural_worlds::generation::{
    TerrainConfig, generate_cacti, generate_caves, generate_chunk_terrain, generate_trees,
};
use procedural_worlds::world::persistence::{self, ChunkStorage, SaveFormat};
use procedural_worlds::world::streaming::{PlayerChunkVelocity, compute_predicted_positions};
use procedural_worlds::world::{CHUNK_SIZE, Chunk, ChunkLoadMetrics, ChunkManager};

// ============================================================================
// Helpers
// ============================================================================

/// Monotonic counter for unique temp directory names across parallel tests.
static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Create a unique temporary `ChunkStorage` for a single test.
fn temp_storage(prefix: &str) -> ChunkStorage {
    let unique = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "pw_integration_{}_{}_{}",
        prefix,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        unique,
    ));
    ChunkStorage::new(dir)
}

fn cleanup(storage: &ChunkStorage) {
    let _ = std::fs::remove_dir_all(&storage.save_dir);
}

/// Ensure `AsyncComputeTaskPool` is initialised (safe to call multiple times).
fn init_task_pool() {
    AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
}

/// Run the full terrain generation pipeline on a chunk.
fn generate_full(chunk: &mut Chunk, config: &TerrainConfig) {
    generate_chunk_terrain(chunk, config);
    generate_caves(chunk, config);
    generate_trees(chunk, config);
    generate_cacti(chunk, config);
}

/// Assert two chunks have identical block data.
fn assert_chunks_equal(a: &Chunk, b: &Chunk) {
    assert_eq!(a.position, b.position, "Chunk positions differ");
    for x in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                assert_eq!(
                    a.get_block(x, y, z),
                    b.get_block(x, y, z),
                    "Block mismatch at ({x}, {y}, {z})"
                );
            }
        }
    }
}

// ============================================================================
// Tests: Full pipeline round-trip (generate → save → load → verify)
// ============================================================================

/// Generate terrain, save to disk, load back, and verify block-for-block match.
#[test]
fn full_pipeline_generate_save_load_json() {
    let storage = temp_storage("full_json");
    let config = TerrainConfig::default();
    let pos = IVec3::new(0, 2, 0);

    let mut original = Chunk::new(pos);
    generate_full(&mut original, &config);

    persistence::save_chunk_fmt(&original, &storage, SaveFormat::Json).unwrap();
    let loaded = persistence::load_chunk_fmt(pos, &storage, SaveFormat::Json).unwrap();

    assert_chunks_equal(&original, &loaded);
    assert!(
        loaded.dirty,
        "Loaded chunks must be marked dirty for mesh rebuild"
    );

    cleanup(&storage);
}

/// Same pipeline round-trip using binary format.
#[test]
fn full_pipeline_generate_save_load_binary() {
    let storage = temp_storage("full_bin");
    let config = TerrainConfig::default();
    let pos = IVec3::new(-2, 0, 3);

    let mut original = Chunk::new(pos);
    generate_full(&mut original, &config);

    persistence::save_chunk_fmt(&original, &storage, SaveFormat::Binary).unwrap();
    let loaded = persistence::load_chunk_fmt(pos, &storage, SaveFormat::Binary).unwrap();

    assert_chunks_equal(&original, &loaded);

    cleanup(&storage);
}

/// Verify auto-detection loads the binary file when both formats exist.
#[test]
fn auto_detect_prefers_binary_over_json() {
    use procedural_worlds::world::BlockType;

    let storage = temp_storage("auto_pref");
    let pos = IVec3::ZERO;

    // Save a JSON version with Stone
    let mut json_chunk = Chunk::new(pos);
    json_chunk.set_block(0, 0, 0, BlockType::Stone);
    persistence::save_chunk_fmt(&json_chunk, &storage, SaveFormat::Json).unwrap();

    // Save a Binary version with Obsidian
    let mut bin_chunk = Chunk::new(pos);
    bin_chunk.set_block(0, 0, 0, BlockType::Obsidian);
    persistence::save_chunk_fmt(&bin_chunk, &storage, SaveFormat::Binary).unwrap();

    // Auto-load should pick binary (Obsidian)
    let loaded = persistence::load_chunk(pos, &storage).unwrap();
    assert_eq!(loaded.get_block(0, 0, 0), BlockType::Obsidian);

    cleanup(&storage);
}

// ============================================================================
// Tests: Async task pool integration
// ============================================================================

/// Simulate the runtime chunk streaming pattern: spawn an async task that
/// tries disk first, then generates. Verify the result matches inline generation.
#[test]
fn async_task_disk_then_generate_fallback() {
    init_task_pool();
    let storage = temp_storage("async_fallback");
    let config = TerrainConfig::default();
    let pos = IVec3::new(1, 2, 1);

    // No file on disk — should fall through to generation
    let config_clone = config.clone();
    let storage_clone = ChunkStorage::new(storage.save_dir.clone());

    let task = AsyncComputeTaskPool::get().spawn(async move {
        if let Ok(chunk) = persistence::load_chunk(pos, &storage_clone) {
            return chunk;
        }
        let mut chunk = Chunk::new(pos);
        generate_full(&mut chunk, &config_clone);
        chunk
    });

    let result = block_on(task);

    // Reference
    let mut reference = Chunk::new(pos);
    generate_full(&mut reference, &config);

    assert_chunks_equal(&result, &reference);

    cleanup(&storage);
}

/// Simulate the runtime pattern where a saved chunk exists on disk and
/// should be loaded instead of regenerated.
#[test]
fn async_task_loads_from_disk_when_available() {
    init_task_pool();
    let storage = temp_storage("async_disk");
    let config = TerrainConfig::default();
    let pos = IVec3::new(3, 0, 3);

    // Save a chunk with a distinctive marker block
    let mut saved_chunk = Chunk::new(pos);
    saved_chunk.set_block(0, 0, 0, BlockType::Obsidian);
    persistence::save_chunk(&saved_chunk, &storage).unwrap();

    let config_clone = config.clone();
    let storage_clone = ChunkStorage::new(storage.save_dir.clone());

    let task = AsyncComputeTaskPool::get().spawn(async move {
        if let Ok(chunk) = persistence::load_chunk(pos, &storage_clone) {
            return chunk;
        }
        let mut chunk = Chunk::new(pos);
        generate_full(&mut chunk, &config_clone);
        chunk
    });

    let result = block_on(task);
    assert_eq!(result.get_block(0, 0, 0), BlockType::Obsidian);

    cleanup(&storage);
}

/// Spawn many concurrent chunk generation tasks and verify all complete
/// with correct positions and deterministic content.
#[test]
fn concurrent_chunk_generation_deterministic() {
    init_task_pool();
    let config = TerrainConfig::default();
    let task_pool = AsyncComputeTaskPool::get();

    let positions: Vec<IVec3> = (-2..=2)
        .flat_map(|x| (-1..=1).map(move |z| IVec3::new(x, 2, z)))
        .collect();

    // Spawn all tasks concurrently
    let tasks: Vec<_> = positions
        .iter()
        .map(|&pos| {
            let cfg = config.clone();
            task_pool.spawn(async move {
                let mut chunk = Chunk::new(pos);
                generate_full(&mut chunk, &cfg);
                chunk
            })
        })
        .collect();

    // Collect results and verify against inline generation
    for (task, &expected_pos) in tasks.into_iter().zip(&positions) {
        let result = block_on(task);
        assert_eq!(result.position, expected_pos);

        let mut reference = Chunk::new(expected_pos);
        generate_full(&mut reference, &config);
        assert_chunks_equal(&result, &reference);
    }
}

// ============================================================================
// Tests: ChunkManager bookkeeping
// ============================================================================

/// Verify that ChunkManager correctly tracks pending → loaded transitions.
#[test]
fn chunk_manager_pending_to_loaded_transition() {
    let mut cm = ChunkManager::default();
    let pos = IVec3::new(1, 0, 1);

    // Phase 1: Mark as pending
    assert!(!cm.pending.contains(&pos));
    assert!(!cm.chunks.contains_key(&pos));
    cm.pending.insert(pos);
    assert!(cm.pending.contains(&pos));

    // Phase 2: Simulate completion — move from pending to loaded
    cm.pending.remove(&pos);
    cm.chunks
        .insert(pos, bevy::ecs::entity::Entity::PLACEHOLDER);

    assert!(!cm.pending.contains(&pos));
    assert!(cm.chunks.contains_key(&pos));
}

/// Verify that concurrent loading doesn't produce duplicate entries.
#[test]
fn chunk_manager_no_duplicate_loading() {
    let mut cm = ChunkManager::default();

    let positions: Vec<IVec3> = (0..10).map(|i| IVec3::new(i, 0, 0)).collect();

    // Insert all as pending
    for &pos in &positions {
        let inserted = cm.pending.insert(pos);
        assert!(inserted, "First insert should succeed");
    }

    // Try inserting again — should not create duplicates
    for &pos in &positions {
        let inserted = cm.pending.insert(pos);
        assert!(!inserted, "Duplicate insert should return false");
    }

    assert_eq!(cm.pending.len(), 10);
}

/// Verify effective_load_distance interacts correctly with vertical range
/// to compute the expected total chunk count.
#[test]
fn expected_chunk_count_calculation() {
    let mut cm = ChunkManager::default();
    cm.render_distance = 4;
    cm.load_distance = Some(6);
    cm.vertical_load_up = 4;
    cm.vertical_load_down = 2;
    let cm = cm;

    let ld = cm.effective_load_distance();
    assert_eq!(ld, 6);

    let side = (2 * ld + 1) as usize; // 13
    let vert = (cm.vertical_load_up + cm.vertical_load_down + 1) as usize; // 7
    let expected = side * side * vert;
    assert_eq!(expected, 13 * 13 * 7);
    assert_eq!(expected, 1183);
}

// ============================================================================
// Tests: ChunkLoadMetrics integration
// ============================================================================

/// Simulate a realistic loading session: record loads over time, refresh,
/// and verify derived metrics are consistent.
#[test]
fn metrics_realistic_loading_session() {
    let mut metrics = ChunkLoadMetrics::default();

    // Simulate 50 chunks loading over 2 seconds
    for i in 0..50 {
        let t = 1.0 + (i as f64) * 0.04; // 0.04s apart
        let duration = 0.02 + (i as f32) * 0.001; // 20ms + slight increase
        metrics.record_load(duration, t);
    }

    metrics.refresh(3.0);

    assert_eq!(metrics.total_chunks_loaded, 50);
    assert!(metrics.avg_load_time_ms > 0.0);
    assert!(metrics.peak_load_time_ms >= metrics.avg_load_time_ms);
    assert!(metrics.chunks_per_second > 0.0);

    // Memory estimate
    metrics.update_memory_estimate(50);
    assert!(metrics.chunk_memory_bytes > 0);
    assert!(metrics.chunk_memory_mb() > 0.0);
}

/// Verify metrics correctly handle the transition from loading to idle.
#[test]
fn metrics_loading_to_idle_transition() {
    let mut metrics = ChunkLoadMetrics::default();

    // Active loading at t=1.0
    for i in 0..20 {
        metrics.record_load(0.05, 1.0 + i as f64 * 0.01);
    }
    metrics.refresh(1.5);
    assert!(
        metrics.chunks_per_second > 0.0,
        "Should show active loading"
    );

    // No more loads, advance time past the 2s window
    metrics.refresh(5.0);
    assert_eq!(
        metrics.chunks_per_second, 0.0,
        "Should show idle after window expires"
    );

    // Total should still be preserved
    assert_eq!(metrics.total_chunks_loaded, 20);
}

// ============================================================================
// Tests: Predictive streaming integration
// ============================================================================

/// Verify that predicted positions from streaming are beyond the load distance
/// and don't overlap with standard loading zone.
#[test]
fn predictive_positions_beyond_load_distance() {
    let center = IVec3::new(5, 0, 5);
    let direction = bevy::math::Vec3::new(1.0, 0.0, 0.0); // Moving +X
    let load_dist = 4;
    let lookahead = 3;

    let positions = compute_predicted_positions(center, direction, load_dist, lookahead, 0, 0);

    // All predicted positions should be beyond load_dist from center
    for pos in &positions {
        let dx = (pos.x - center.x).abs();
        let dz = (pos.z - center.z).abs();
        let dist = dx.max(dz);
        assert!(
            dist > load_dist,
            "Predicted position {:?} should be beyond load_dist {} (dist={})",
            pos,
            load_dist,
            dist
        );
    }
}

/// Verify that the standard loading zone and predictive zone together
/// provide coverage in the movement direction.
#[test]
fn predictive_plus_standard_covers_movement_direction() {
    let center = IVec3::ZERO;
    let direction = bevy::math::Vec3::new(0.0, 0.0, 1.0); // Moving +Z
    let load_dist = 4;
    let lookahead = 2;

    let predicted: HashSet<IVec3> =
        compute_predicted_positions(center, direction, load_dist, lookahead, 0, 0)
            .into_iter()
            .collect();

    // Standard zone: all positions within load_dist
    let mut standard = HashSet::new();
    for x in -load_dist..=load_dist {
        for z in -load_dist..=load_dist {
            standard.insert(IVec3::new(x, 0, z));
        }
    }

    // Combined coverage
    let combined: HashSet<IVec3> = standard.union(&predicted).copied().collect();

    // Should have coverage beyond load_dist in the +Z direction
    let max_z = combined.iter().map(|p| p.z).max().unwrap();
    assert!(
        max_z > load_dist,
        "Combined coverage should extend beyond load_dist in movement direction (max_z={})",
        max_z
    );
}

/// Verify velocity tracking produces correct direction from chunk deltas.
#[test]
fn player_velocity_direction_from_movement() {
    let vel = PlayerChunkVelocity {
        velocity: bevy::math::Vec3::new(3.0, 0.0, 4.0),
        prev_chunk: IVec3::ZERO,
        initialized: true,
    };

    let dir = vel
        .direction_xz(0.5)
        .expect("Speed should exceed threshold");

    // Should be normalised
    let len = (dir.x * dir.x + dir.z * dir.z).sqrt();
    assert!((len - 1.0).abs() < 0.01, "Direction should be unit length");

    // Direction should point in +X, +Z quadrant
    assert!(dir.x > 0.0);
    assert!(dir.z > 0.0);
}

// ============================================================================
// Tests: Multi-format persistence pipeline
// ============================================================================

/// Save a generated chunk in both formats, load each, verify they match.
#[test]
fn cross_format_consistency() {
    let storage = temp_storage("cross_fmt");
    let config = TerrainConfig::default();
    let pos = IVec3::new(0, 1, 0);

    let mut original = Chunk::new(pos);
    generate_full(&mut original, &config);

    persistence::save_chunk_fmt(&original, &storage, SaveFormat::Json).unwrap();
    persistence::save_chunk_fmt(&original, &storage, SaveFormat::Binary).unwrap();

    let from_json = persistence::load_chunk_fmt(pos, &storage, SaveFormat::Json).unwrap();
    let from_binary = persistence::load_chunk_fmt(pos, &storage, SaveFormat::Binary).unwrap();

    assert_chunks_equal(&from_json, &from_binary);
    assert_chunks_equal(&original, &from_json);

    cleanup(&storage);
}

/// Save multiple chunks from a grid, load them all, verify none are corrupted.
#[test]
fn batch_save_load_grid() {
    let storage = temp_storage("batch_grid");
    let config = TerrainConfig::default();

    let positions: Vec<IVec3> = (-1..=1)
        .flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| IVec3::new(x, y, z))))
        .collect();

    // Generate and save all chunks
    let mut originals: Vec<Chunk> = Vec::new();
    for &pos in &positions {
        let mut chunk = Chunk::new(pos);
        generate_full(&mut chunk, &config);
        persistence::save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).unwrap();
        originals.push(chunk);
    }

    // Load all and verify
    for (original, &pos) in originals.iter().zip(&positions) {
        let loaded = persistence::load_chunk(pos, &storage).unwrap();
        assert_chunks_equal(original, &loaded);
    }

    assert_eq!(originals.len(), 27); // 3³

    cleanup(&storage);
}

use procedural_worlds::world::BlockType;
