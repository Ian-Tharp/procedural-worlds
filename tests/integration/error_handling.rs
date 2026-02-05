//! Integration tests for error handling in the chunk loading pipeline.
//!
//! Verifies graceful behavior when persistence fails, data is corrupted,
//! or chunks are accessed at invalid positions.

use std::io;

use bevy::math::IVec3;

use procedural_worlds::world::persistence::{
    self, ChunkStorage, SaveFormat,
};
use procedural_worlds::world::{
    BlockType, Chunk, ChunkManager, CHUNK_SIZE,
};

// ============================================================================
// Helpers
// ============================================================================

use std::sync::atomic::{AtomicU64, Ordering};

static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_storage(prefix: &str) -> ChunkStorage {
    let unique = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "pw_error_test_{}_{}_{}",
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

// ============================================================================
// Tests: Loading from non-existent paths
// ============================================================================

/// Loading a chunk from a non-existent directory returns NotFound.
#[test]
fn load_nonexistent_directory_returns_not_found() {
    let storage = ChunkStorage::new("/tmp/pw_nonexistent_dir_12345_does_not_exist");
    let result = persistence::load_chunk(IVec3::ZERO, &storage);

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
}

/// Loading a chunk position that was never saved returns NotFound.
#[test]
fn load_unsaved_position_returns_not_found() {
    let storage = temp_storage("unsaved");
    std::fs::create_dir_all(&storage.save_dir).unwrap();

    let result = persistence::load_chunk(IVec3::new(99, 99, 99), &storage);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);

    cleanup(&storage);
}

// ============================================================================
// Tests: Corrupted data handling
// ============================================================================

/// Write invalid JSON to a chunk file and verify load fails gracefully.
#[test]
fn load_corrupted_json_returns_error() {
    let storage = temp_storage("corrupt_json");
    std::fs::create_dir_all(&storage.save_dir).unwrap();

    // Write garbage to the expected file path
    let path = storage.save_dir.join("chunk_0_0_0.json");
    std::fs::write(&path, "not valid json {{{").unwrap();

    let result = persistence::load_chunk_fmt(IVec3::ZERO, &storage, SaveFormat::Json);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);

    cleanup(&storage);
}

/// Write invalid binary to a chunk file and verify load fails gracefully.
#[test]
fn load_corrupted_binary_returns_error() {
    let storage = temp_storage("corrupt_bin");
    std::fs::create_dir_all(&storage.save_dir).unwrap();

    // Write random bytes to the expected file path
    let path = storage.save_dir.join("chunk_0_0_0.bin");
    std::fs::write(&path, &[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01]).unwrap();

    let result = persistence::load_chunk_fmt(IVec3::ZERO, &storage, SaveFormat::Binary);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);

    cleanup(&storage);
}

/// Write a JSON file with valid structure but wrong block count.
#[test]
fn load_wrong_block_count_returns_error() {
    let storage = temp_storage("wrong_count");
    std::fs::create_dir_all(&storage.save_dir).unwrap();

    // Create valid JSON with too few blocks
    let json = serde_json::json!({
        "position": [0, 0, 0],
        "blocks": [0, 1, 2, 3]  // Only 4 blocks, need CHUNK_VOLUME
    });
    let path = storage.save_dir.join("chunk_0_0_0.json");
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

    let result = persistence::load_chunk_fmt(IVec3::ZERO, &storage, SaveFormat::Json);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("Expected"),
        "Error should mention expected block count: {}",
        err
    );

    cleanup(&storage);
}

/// Write an empty file and verify it doesn't panic.
#[test]
fn load_empty_file_returns_error() {
    let storage = temp_storage("empty_file");
    std::fs::create_dir_all(&storage.save_dir).unwrap();

    let path = storage.save_dir.join("chunk_0_0_0.json");
    std::fs::write(&path, "").unwrap();

    let result = persistence::load_chunk_fmt(IVec3::ZERO, &storage, SaveFormat::Json);
    assert!(result.is_err());

    cleanup(&storage);
}

// ============================================================================
// Tests: Auto-detect fallback chain
// ============================================================================

/// When binary file is corrupted but JSON exists, auto-detect should fail
/// on the binary and not silently fall through to JSON.
#[test]
fn auto_detect_corrupted_binary_does_not_fallback() {
    let storage = temp_storage("auto_corrupt");
    std::fs::create_dir_all(&storage.save_dir).unwrap();

    // Save a valid JSON version
    let mut chunk = Chunk::new(IVec3::ZERO);
    chunk.set_block(0, 0, 0, BlockType::Stone);
    persistence::save_chunk_fmt(&chunk, &storage, SaveFormat::Json).unwrap();

    // Also create a corrupted binary file
    let bin_path = storage.save_dir.join("chunk_0_0_0.bin");
    std::fs::write(&bin_path, &[0xFF, 0xFF]).unwrap();

    // Auto-detect tries binary first; it exists but is corrupted → error
    let result = persistence::load_chunk(IVec3::ZERO, &storage);
    assert!(result.is_err(), "Should fail on corrupted binary, not silently fall through");

    cleanup(&storage);
}

// ============================================================================
// Tests: Edge case chunk positions
// ============================================================================

/// Verify chunks at extreme positions can be saved and loaded.
#[test]
fn extreme_chunk_positions_round_trip() {
    let storage = temp_storage("extreme_pos");

    let extreme_positions = vec![
        IVec3::new(i32::MAX / 2, 0, 0),
        IVec3::new(0, i32::MIN / 2, 0),
        IVec3::new(-1000, 1000, -1000),
        IVec3::new(0, 0, 0),
    ];

    for pos in &extreme_positions {
        let chunk = Chunk::new(*pos);
        persistence::save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).unwrap();
        let loaded = persistence::load_chunk(*pos, &storage).unwrap();
        assert_eq!(loaded.position, *pos, "Position mismatch for {:?}", pos);
    }

    cleanup(&storage);
}

// ============================================================================
// Tests: ChunkManager error resilience
// ============================================================================

/// ChunkManager should handle repeated operations on the same position
/// without panicking or producing inconsistent state.
#[test]
fn chunk_manager_repeated_insert_remove() {
    let mut cm = ChunkManager::default();
    let pos = IVec3::new(1, 0, 1);

    // Insert and remove multiple times
    for _ in 0..100 {
        cm.pending.insert(pos);
        cm.pending.remove(&pos);
    }

    assert!(!cm.pending.contains(&pos));
    assert!(cm.pending.is_empty());
}

/// ChunkManager should not consider unloaded positions as loaded.
#[test]
fn chunk_manager_unloaded_position_not_found() {
    let cm = ChunkManager::default();

    // Random positions that were never loaded
    let test_positions = [
        IVec3::new(100, 0, 100),
        IVec3::new(-50, 5, -50),
        IVec3::ZERO,
    ];

    for pos in &test_positions {
        assert!(
            !cm.chunks.contains_key(pos),
            "Position {:?} should not be in chunks map",
            pos
        );
    }
}

/// Verify that chunk_exists returns false for missing chunks and true
/// after saving (integration between persistence and manager logic).
#[test]
fn chunk_exists_reflects_disk_state() {
    let storage = temp_storage("exists_disk");

    let pos = IVec3::new(5, 0, 5);
    assert!(!persistence::chunk_exists(pos, &storage));

    let chunk = Chunk::new(pos);
    persistence::save_chunk(&chunk, &storage).unwrap();
    assert!(persistence::chunk_exists(pos, &storage));

    // Different position should still not exist
    assert!(!persistence::chunk_exists(IVec3::new(6, 0, 5), &storage));

    cleanup(&storage);
}

// ============================================================================
// Tests: Block access out-of-bounds safety
// ============================================================================

/// Out-of-bounds block access returns Air without panicking.
#[test]
fn out_of_bounds_block_access_is_safe() {
    let chunk = Chunk::new(IVec3::ZERO);

    // Various out-of-bounds accesses
    assert_eq!(chunk.get_block(CHUNK_SIZE, 0, 0), BlockType::Air);
    assert_eq!(chunk.get_block(0, CHUNK_SIZE, 0), BlockType::Air);
    assert_eq!(chunk.get_block(0, 0, CHUNK_SIZE), BlockType::Air);
    assert_eq!(chunk.get_block(usize::MAX, usize::MAX, usize::MAX), BlockType::Air);
}

/// Out-of-bounds set_block is silently ignored.
#[test]
fn out_of_bounds_set_block_is_noop() {
    let mut chunk = Chunk::new(IVec3::ZERO);
    chunk.set_block(0, 0, 0, BlockType::Stone);

    // These should be no-ops
    chunk.set_block(CHUNK_SIZE, 0, 0, BlockType::Dirt);
    chunk.set_block(0, CHUNK_SIZE, 0, BlockType::Dirt);
    chunk.set_block(0, 0, CHUNK_SIZE, BlockType::Dirt);

    // Original block should be unchanged
    assert_eq!(chunk.get_block(0, 0, 0), BlockType::Stone);
}

// ============================================================================
// Tests: Overwrite and data integrity
// ============================================================================

/// Saving the same position twice overwrites cleanly.
#[test]
fn overwrite_preserves_latest_data() {
    let storage = temp_storage("overwrite");

    let pos = IVec3::ZERO;

    // First save: all stone
    let mut chunk1 = Chunk::new(pos);
    chunk1.fill(BlockType::Stone);
    persistence::save_chunk_fmt(&chunk1, &storage, SaveFormat::Binary).unwrap();

    // Second save: all dirt
    let mut chunk2 = Chunk::new(pos);
    chunk2.fill(BlockType::Dirt);
    persistence::save_chunk_fmt(&chunk2, &storage, SaveFormat::Binary).unwrap();

    // Load should return dirt
    let loaded = persistence::load_chunk(pos, &storage).unwrap();
    for x in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                assert_eq!(
                    loaded.get_block(x, y, z),
                    BlockType::Dirt,
                    "Overwrite failed at ({x}, {y}, {z})"
                );
            }
        }
    }

    cleanup(&storage);
}

/// Verify all block types survive a full round-trip through binary persistence.
#[test]
fn all_block_types_survive_round_trip() {
    let storage = temp_storage("all_types");

    let all_types = [
        BlockType::Air,
        BlockType::Stone,
        BlockType::Dirt,
        BlockType::Grass,
        BlockType::Sand,
        BlockType::Water,
        BlockType::Wood,
        BlockType::Leaves,
        BlockType::Sandstone,
        BlockType::Snow,
        BlockType::Ice,
        BlockType::Obsidian,
        BlockType::VolcanicRock,
        BlockType::Cactus,
        BlockType::SandDunes,
    ];

    let mut chunk = Chunk::new(IVec3::ZERO);
    for (i, &block) in all_types.iter().enumerate() {
        chunk.set_block(i % CHUNK_SIZE, i / CHUNK_SIZE, 0, block);
    }

    persistence::save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).unwrap();
    let loaded = persistence::load_chunk(IVec3::ZERO, &storage).unwrap();

    for (i, &block) in all_types.iter().enumerate() {
        let x = i % CHUNK_SIZE;
        let y = i / CHUNK_SIZE;
        assert_eq!(
            loaded.get_block(x, y, 0),
            block,
            "Block type {:?} lost at ({}, {}, 0)",
            block, x, y
        );
    }

    cleanup(&storage);
}
