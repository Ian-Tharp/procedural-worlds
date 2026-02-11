//! Chunk persistence — save/load chunk block data to/from disk
//!
//! Provides primitives for serializing chunk data to JSON files on disk.
//! Each chunk is saved as a separate file named by its chunk coordinates
//! (e.g., `chunk_0_2_-3.json`).
//!
//! # Formats
//!
//! - **JSON** (`.json`): Human-readable, ~100KB per chunk
//! - **Binary** (`.bin`): Compact via bincode, ~8KB per chunk
//! - **Compressed** (`.cbin`): Palette-based compression, ~4KB per chunk
//!
//! The compressed format uses a per-chunk palette mapping unique block types
//! to u8 indices. Since most chunks contain fewer than 256 unique block types,
//! this halves storage compared to raw u16 block IDs.
//!
//! # Usage
//!
//! ```ignore
//! let storage = ChunkStorage::new("world/saves");
//! save_chunk(&chunk, &storage)?;
//! let loaded = load_chunk(IVec3::new(0, 2, -3), &storage)?;
//! ```
//!
//! This module only provides the save/load primitives.
//! Integration with the chunk streaming system will come later.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{BlockType, Chunk, CHUNK_VOLUME};

// ============================================================================
// SAVE FORMAT
// ============================================================================

/// Serialization format for chunk data on disk.
///
/// The `world.json` metadata file is always JSON (human-readable).
/// This enum controls only the chunk data file format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveFormat {
    /// JSON — human-readable, larger files (~100KB per chunk).
    Json,
    /// Binary via bincode — compact (~8KB per chunk), faster I/O.
    Binary,
    /// Compressed binary with palette — most compact (~4KB per chunk).
    ///
    /// Uses a per-chunk palette mapping unique block types to u8 indices.
    /// Falls back to Binary if chunk has >256 unique block types.
    #[default]
    Compressed,
}

impl fmt::Display for SaveFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveFormat::Json => write!(f, "json"),
            SaveFormat::Binary => write!(f, "binary"),
            SaveFormat::Compressed => write!(f, "compressed"),
        }
    }
}

impl SaveFormat {
    /// Parse a format from a string (case-insensitive).
    ///
    /// Returns `Compressed` for unrecognised values (with a log warning).
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => SaveFormat::Json,
            "binary" | "bin" | "bincode" => SaveFormat::Binary,
            "compressed" | "cbin" | "palette" => SaveFormat::Compressed,
            other => {
                warn!("Unknown chunk_format '{}', defaulting to compressed", other);
                SaveFormat::Compressed
            }
        }
    }

    /// File extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            SaveFormat::Json => "json",
            SaveFormat::Binary => "bin",
            SaveFormat::Compressed => "cbin",
        }
    }
}

// ============================================================================
// DATA STRUCTURES
// ============================================================================

/// Serializable representation of a chunk's data
///
/// Stores block data as a `Vec<u16>` for compact JSON serialization
/// rather than serializing each `BlockType` enum variant by name.
#[derive(Serialize, Deserialize, Debug)]
pub struct SavedChunk {
    /// Chunk position in chunk coordinates `[x, y, z]`
    pub position: [i32; 3],
    /// Block data as u16 values (matches `BlockType` repr)
    pub blocks: Vec<u16>,
}

/// Palette-compressed representation of a chunk's data.
///
/// Uses a per-chunk palette to map unique block types to u8 indices,
/// reducing storage from 8KB (4096 × u16) to ~4KB (palette + 4096 × u8).
///
/// Most chunks contain only a handful of unique block types (air, stone,
/// dirt, grass, maybe a few ores), so the palette is typically <20 entries.
#[derive(Serialize, Deserialize, Debug)]
pub struct SavedChunkCompressed {
    /// Chunk position in chunk coordinates `[x, y, z]`
    pub position: [i32; 3],
    /// Palette mapping index → block type (as u16)
    ///
    /// The first entry (index 0) is typically Air for optimal compression
    /// of mostly-empty chunks.
    pub palette: Vec<u16>,
    /// Block data as palette indices (u8)
    ///
    /// Each value is an index into `palette`. Length is always `CHUNK_VOLUME`.
    pub indices: Vec<u8>,
}

/// Statistics from a palette compression operation.
///
/// Useful for debugging and monitoring compression effectiveness.
#[derive(Debug, Clone, Default)]
pub struct PaletteStats {
    /// Number of unique block types in the chunk
    pub unique_blocks: usize,
    /// Original size in bytes (uncompressed u16 per block)
    pub original_bytes: usize,
    /// Compressed size in bytes (palette + u8 indices)
    pub compressed_bytes: usize,
    /// Compression ratio (compressed / original), lower is better
    pub ratio: f32,
}

impl PaletteStats {
    /// Calculate stats for a given palette size.
    pub fn calculate(palette_size: usize) -> Self {
        let original_bytes = CHUNK_VOLUME * 2; // 4096 × u16
        // Palette (N × u16) + indices (4096 × u8)
        let compressed_bytes = (palette_size * 2) + CHUNK_VOLUME;
        let ratio = compressed_bytes as f32 / original_bytes as f32;
        
        Self {
            unique_blocks: palette_size,
            original_bytes,
            compressed_bytes,
            ratio,
        }
    }
}

/// Compress chunk block data using palette encoding.
///
/// Returns `None` if the chunk has more than 256 unique block types
/// (which would require u16 indices, negating the compression benefit).
pub fn compress_chunk_data(blocks: &[BlockType; CHUNK_VOLUME]) -> Option<(Vec<u16>, Vec<u8>)> {
    // Build palette: map BlockType → palette index
    let mut type_to_index: HashMap<u16, u8> = HashMap::new();
    let mut palette: Vec<u16> = Vec::new();
    
    // First pass: build palette
    for block in blocks.iter() {
        let block_id = u16::from(*block);
        if !type_to_index.contains_key(&block_id) {
            if palette.len() >= 256 {
                // Too many unique types for u8 indices
                return None;
            }
            type_to_index.insert(block_id, palette.len() as u8);
            palette.push(block_id);
        }
    }
    
    // Second pass: convert to indices
    let indices: Vec<u8> = blocks
        .iter()
        .map(|block| {
            let block_id = u16::from(*block);
            *type_to_index.get(&block_id).unwrap()
        })
        .collect();
    
    Some((palette, indices))
}

/// Decompress palette-encoded chunk data back to block types.
///
/// # Errors
///
/// Returns an error if:
/// - The indices vector doesn't have exactly `CHUNK_VOLUME` entries
/// - Any index references a palette entry that doesn't exist
pub fn decompress_chunk_data(
    palette: &[u16],
    indices: &[u8],
) -> Result<[BlockType; CHUNK_VOLUME], String> {
    if indices.len() != CHUNK_VOLUME {
        return Err(format!(
            "Expected {} indices, got {}",
            CHUNK_VOLUME,
            indices.len()
        ));
    }
    
    let mut blocks = [BlockType::Air; CHUNK_VOLUME];
    
    for (i, &idx) in indices.iter().enumerate() {
        let block_id = *palette.get(idx as usize).ok_or_else(|| {
            format!(
                "Palette index {} out of bounds (palette size: {})",
                idx,
                palette.len()
            )
        })?;
        blocks[i] = BlockType::from(block_id);
    }
    
    Ok(blocks)
}

/// Resource for configuring chunk storage location on disk
///
/// Insert this as a Bevy resource to configure where chunks are saved.
/// Defaults to `"world/chunks"` relative to the working directory.
#[derive(Resource, Clone, Debug)]
pub struct ChunkStorage {
    /// Directory path where chunk files are saved
    pub save_dir: PathBuf,
}

impl Default for ChunkStorage {
    fn default() -> Self {
        Self {
            save_dir: PathBuf::from("world/chunks"),
        }
    }
}

impl ChunkStorage {
    /// Create a new `ChunkStorage` with a custom save directory
    pub fn new(save_dir: impl Into<PathBuf>) -> Self {
        Self {
            save_dir: save_dir.into(),
        }
    }
}

// ============================================================================
// FILE PATH HELPERS
// ============================================================================

/// Get the file path for a chunk at the given position (JSON format).
///
/// Returns a path like `<save_dir>/chunk_0_2_-3.json`.
pub fn chunk_file_path(position: IVec3, storage: &ChunkStorage) -> PathBuf {
    chunk_file_path_fmt(position, storage, SaveFormat::Json)
}

/// Get the file path for a chunk at the given position with a specific format.
///
/// Returns a path like `<save_dir>/chunk_0_2_-3.json` or `<save_dir>/chunk_0_2_-3.bin`.
pub fn chunk_file_path_fmt(position: IVec3, storage: &ChunkStorage, format: SaveFormat) -> PathBuf {
    storage.save_dir.join(format!(
        "chunk_{}_{}_{}.{}",
        position.x, position.y, position.z,
        format.extension()
    ))
}

// ============================================================================
// SAVE / LOAD
// ============================================================================

/// Save a chunk's block data to disk as a JSON file.
///
/// Creates the save directory if it doesn't exist. The chunk is serialized
/// to a file named by its position (e.g., `chunk_0_2_-3.json`).
///
/// This is the legacy JSON-only entry point. Prefer [`save_chunk_fmt`] for
/// format-aware saving.
///
/// # Errors
///
/// Returns `io::Error` if the directory can't be created or the file can't be written.
pub fn save_chunk(chunk: &Chunk, storage: &ChunkStorage) -> Result<(), io::Error> {
    save_chunk_fmt(chunk, storage, SaveFormat::Json)
}

/// Save a chunk's block data to disk in the specified format.
///
/// Creates the save directory if it doesn't exist. The chunk is serialized
/// to a file named by its position with the appropriate extension
/// (e.g., `chunk_0_2_-3.json`, `chunk_0_2_-3.bin`, or `chunk_0_2_-3.cbin`).
///
/// For `Compressed` format: if the chunk has >256 unique block types,
/// automatically falls back to `Binary` format.
///
/// # Errors
///
/// Returns `io::Error` if the directory can't be created or the file can't be written.
pub fn save_chunk_fmt(chunk: &Chunk, storage: &ChunkStorage, format: SaveFormat) -> Result<(), io::Error> {
    // Ensure the save directory exists
    fs::create_dir_all(&storage.save_dir)?;

    let position = [chunk.position.x, chunk.position.y, chunk.position.z];

    match format {
        SaveFormat::Json => {
            let saved = SavedChunk {
                position,
                blocks: chunk.blocks().iter().map(|&b| u16::from(b)).collect(),
            };
            let path = chunk_file_path_fmt(chunk.position, storage, format);
            let json = serde_json::to_string(&saved).map_err(io::Error::other)?;
            fs::write(&path, json)?;
            info!("Saved chunk at {:?} to {:?} ({})", chunk.position, path, format);
        }
        SaveFormat::Binary => {
            let saved = SavedChunk {
                position,
                blocks: chunk.blocks().iter().map(|&b| u16::from(b)).collect(),
            };
            let path = chunk_file_path_fmt(chunk.position, storage, format);
            let bytes = bincode::serialize(&saved)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            fs::write(&path, bytes)?;
            info!("Saved chunk at {:?} to {:?} ({})", chunk.position, path, format);
        }
        SaveFormat::Compressed => {
            // Try palette compression; fall back to binary if >256 unique types
            if let Some((palette, indices)) = compress_chunk_data(chunk.blocks()) {
                let saved = SavedChunkCompressed {
                    position,
                    palette,
                    indices,
                };
                let path = chunk_file_path_fmt(chunk.position, storage, format);
                let bytes = bincode::serialize(&saved)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                fs::write(&path, bytes)?;
                
                let stats = PaletteStats::calculate(saved.palette.len());
                debug!(
                    "Saved chunk at {:?} (compressed: {} unique blocks, {:.1}% of original)",
                    chunk.position,
                    stats.unique_blocks,
                    stats.ratio * 100.0
                );
            } else {
                // Fall back to binary format
                warn!(
                    "Chunk at {:?} has >256 unique block types, falling back to binary format",
                    chunk.position
                );
                return save_chunk_fmt(chunk, storage, SaveFormat::Binary);
            }
        }
    }

    Ok(())
}

/// Check if a saved chunk file exists on disk for the given position.
///
/// Checks for compressed, binary, and JSON formats. Returns `true` if any exists.
///
/// Useful for debugging and determining whether a chunk needs generation
/// or can be loaded from a previous save.
pub fn chunk_exists(position: IVec3, storage: &ChunkStorage) -> bool {
    chunk_file_path_fmt(position, storage, SaveFormat::Compressed).exists()
        || chunk_file_path_fmt(position, storage, SaveFormat::Binary).exists()
        || chunk_file_path_fmt(position, storage, SaveFormat::Json).exists()
}

/// Load a chunk's block data from disk (JSON format).
///
/// This is the legacy JSON-only entry point. For format-aware loading that
/// auto-detects binary or JSON files, use [`load_chunk_auto`].
///
/// # Errors
///
/// Returns `io::Error` if the file doesn't exist, is malformed, or has
/// an incorrect block count.
pub fn load_chunk(position: IVec3, storage: &ChunkStorage) -> Result<Chunk, io::Error> {
    // Try auto-detection first: binary, then JSON
    load_chunk_auto(position, storage)
}

/// Load a chunk from disk, auto-detecting the format.
///
/// Tries compressed (`.cbin`) first, then binary (`.bin`), then JSON (`.json`).
/// Returns `NotFound` if no file exists.
///
/// # Errors
///
/// Returns `io::Error` if:
/// - No file exists in any format (`NotFound`)
/// - The file is malformed (`InvalidData`)
/// - The block count doesn't match `CHUNK_VOLUME` (`InvalidData`)
pub fn load_chunk_auto(position: IVec3, storage: &ChunkStorage) -> Result<Chunk, io::Error> {
    // Try compressed first (new default, most compact)
    let cbin_path = chunk_file_path_fmt(position, storage, SaveFormat::Compressed);
    if cbin_path.exists() {
        return load_chunk_fmt(position, storage, SaveFormat::Compressed);
    }

    // Try binary (legacy default)
    let bin_path = chunk_file_path_fmt(position, storage, SaveFormat::Binary);
    if bin_path.exists() {
        return load_chunk_fmt(position, storage, SaveFormat::Binary);
    }

    // Fall back to JSON
    let json_path = chunk_file_path_fmt(position, storage, SaveFormat::Json);
    if json_path.exists() {
        return load_chunk_fmt(position, storage, SaveFormat::Json);
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("No saved chunk at {:?} (checked .cbin, .bin, and .json)", position),
    ))
}

/// Load a chunk's block data from disk in a specific format.
///
/// Reads and deserializes a chunk file by position. The returned chunk
/// is marked dirty so its mesh will be rebuilt.
///
/// # Errors
///
/// Returns `io::Error` if:
/// - The file doesn't exist (`NotFound`)
/// - The data is malformed (`InvalidData`)
/// - The block count doesn't match `CHUNK_VOLUME` (`InvalidData`)
pub fn load_chunk_fmt(position: IVec3, storage: &ChunkStorage, format: SaveFormat) -> Result<Chunk, io::Error> {
    let path = chunk_file_path_fmt(position, storage, format);

    let (blocks, chunk_pos) = match format {
        SaveFormat::Json => {
            let json = fs::read_to_string(&path)?;
            let saved: SavedChunk = serde_json::from_str(&json)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            
            // Validate block data length
            if saved.blocks.len() != CHUNK_VOLUME {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Expected {} blocks, got {}", CHUNK_VOLUME, saved.blocks.len()),
                ));
            }
            
            // Convert u16 values back to BlockType array
            let mut blocks = [BlockType::Air; CHUNK_VOLUME];
            for (i, &val) in saved.blocks.iter().enumerate() {
                blocks[i] = BlockType::from(val);
            }
            
            let pos = IVec3::new(saved.position[0], saved.position[1], saved.position[2]);
            (blocks, pos)
        }
        SaveFormat::Binary => {
            let bytes = fs::read(&path)?;
            let saved: SavedChunk = bincode::deserialize(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            
            // Validate block data length
            if saved.blocks.len() != CHUNK_VOLUME {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Expected {} blocks, got {}", CHUNK_VOLUME, saved.blocks.len()),
                ));
            }
            
            // Convert u16 values back to BlockType array
            let mut blocks = [BlockType::Air; CHUNK_VOLUME];
            for (i, &val) in saved.blocks.iter().enumerate() {
                blocks[i] = BlockType::from(val);
            }
            
            let pos = IVec3::new(saved.position[0], saved.position[1], saved.position[2]);
            (blocks, pos)
        }
        SaveFormat::Compressed => {
            let bytes = fs::read(&path)?;
            let saved: SavedChunkCompressed = bincode::deserialize(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            
            // Decompress palette-encoded data
            let blocks = decompress_chunk_data(&saved.palette, &saved.indices)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            
            let pos = IVec3::new(saved.position[0], saved.position[1], saved.position[2]);
            
            debug!(
                "Loaded compressed chunk at {:?} ({} palette entries)",
                pos,
                saved.palette.len()
            );
            
            (blocks, pos)
        }
    };

    info!("Loaded chunk at {:?} from {:?} ({})", chunk_pos, path, format);
    Ok(Chunk::from_blocks(chunk_pos, blocks))
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Create a temporary storage directory for testing
    fn temp_storage() -> ChunkStorage {
        // Tests run in parallel; timestamps alone can collide on some platforms.
        // Add a monotonic counter to guarantee uniqueness.
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);

        let dir = std::env::temp_dir().join(format!(
            "procedural_worlds_test_{}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            unique,
        ));
        ChunkStorage::new(dir)
    }

    /// Clean up a test storage directory
    fn cleanup(storage: &ChunkStorage) {
        let _ = fs::remove_dir_all(&storage.save_dir);
    }

    #[test]
    fn test_chunk_file_path_positive() {
        let storage = ChunkStorage::new("saves");
        let path = chunk_file_path(IVec3::new(0, 2, 3), &storage);
        assert_eq!(path, Path::new("saves").join("chunk_0_2_3.json"));
    }

    #[test]
    fn test_chunk_file_path_negative() {
        let storage = ChunkStorage::new("saves");
        let path = chunk_file_path(IVec3::new(-1, -5, 3), &storage);
        assert_eq!(path, Path::new("saves").join("chunk_-1_-5_3.json"));
    }

    #[test]
    fn test_chunk_storage_default() {
        let storage = ChunkStorage::default();
        assert_eq!(storage.save_dir, PathBuf::from("world/chunks"));
    }

    #[test]
    fn test_round_trip_empty_chunk() {
        let storage = temp_storage();

        let chunk = Chunk::new(IVec3::new(1, 2, 3));
        save_chunk(&chunk, &storage).expect("save should succeed");

        let loaded = load_chunk(IVec3::new(1, 2, 3), &storage).expect("load should succeed");

        assert_eq!(loaded.position, chunk.position);
        for i in 0..CHUNK_VOLUME {
            assert_eq!(loaded.blocks()[i], chunk.blocks()[i]);
        }

        cleanup(&storage);
    }

    #[test]
    fn test_round_trip_with_blocks() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::new(-3, 0, 7));
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(5, 10, 15, BlockType::Grass);
        chunk.set_block(15, 15, 15, BlockType::Water);
        chunk.set_block(8, 4, 2, BlockType::Wood);
        chunk.set_block(1, 1, 1, BlockType::Leaves);
        chunk.set_block(0, 0, 1, BlockType::Sand);
        chunk.set_block(2, 3, 4, BlockType::Dirt);

        save_chunk(&chunk, &storage).expect("save should succeed");
        let loaded = load_chunk(IVec3::new(-3, 0, 7), &storage).expect("load should succeed");

        assert_eq!(loaded.position, IVec3::new(-3, 0, 7));
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Stone);
        assert_eq!(loaded.get_block(5, 10, 15), BlockType::Grass);
        assert_eq!(loaded.get_block(15, 15, 15), BlockType::Water);
        assert_eq!(loaded.get_block(8, 4, 2), BlockType::Wood);
        assert_eq!(loaded.get_block(1, 1, 1), BlockType::Leaves);
        assert_eq!(loaded.get_block(0, 0, 1), BlockType::Sand);
        assert_eq!(loaded.get_block(2, 3, 4), BlockType::Dirt);
        // Unset blocks should still be Air
        assert_eq!(loaded.get_block(7, 7, 7), BlockType::Air);

        cleanup(&storage);
    }

    #[test]
    fn test_round_trip_filled_chunk() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        save_chunk(&chunk, &storage).expect("save should succeed");
        let loaded = load_chunk(IVec3::ZERO, &storage).expect("load should succeed");

        for i in 0..CHUNK_VOLUME {
            assert_eq!(loaded.blocks()[i], BlockType::Stone, "block {} mismatch", i);
        }

        cleanup(&storage);
    }

    #[test]
    fn test_load_nonexistent_chunk_returns_error() {
        let storage = temp_storage();

        let result = load_chunk(IVec3::new(99, 99, 99), &storage);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);

        cleanup(&storage);
    }

    #[test]
    fn test_block_data_integrity_all_types() {
        let storage = temp_storage();

        let all_types = [
            BlockType::Air,
            BlockType::Stone,
            BlockType::Dirt,
            BlockType::Grass,
            BlockType::Sand,
            BlockType::Water,
            BlockType::Wood,
            BlockType::Leaves,
        ];

        let mut chunk = Chunk::new(IVec3::new(0, 0, 0));
        // Place each block type at a known position
        for (i, &block_type) in all_types.iter().enumerate() {
            chunk.set_block(i, 0, 0, block_type);
        }

        save_chunk(&chunk, &storage).expect("save should succeed");
        let loaded = load_chunk(IVec3::ZERO, &storage).expect("load should succeed");

        for (i, &block_type) in all_types.iter().enumerate() {
            assert_eq!(
                loaded.get_block(i, 0, 0),
                block_type,
                "Block type {:?} at index {} did not survive round-trip",
                block_type,
                i
            );
        }

        cleanup(&storage);
    }

    #[test]
    fn test_overwrite_existing_chunk() {
        let storage = temp_storage();

        // Save a chunk with stone
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);
        save_chunk(&chunk, &storage).expect("first save should succeed");

        // Overwrite with dirt
        let mut chunk2 = Chunk::new(IVec3::ZERO);
        chunk2.fill(BlockType::Dirt);
        save_chunk(&chunk2, &storage).expect("overwrite save should succeed");

        // Load should return the overwritten data
        let loaded = load_chunk(IVec3::ZERO, &storage).expect("load should succeed");
        assert_eq!(loaded.blocks()[0], BlockType::Dirt);

        cleanup(&storage);
    }

    #[test]
    fn test_saved_chunk_serialization_format() {
        // Verify the JSON structure is what we expect
        let saved = SavedChunk {
            position: [1, 2, 3],
            blocks: vec![0, 1, 2, 3],
        };

        let json = serde_json::to_string(&saved).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["position"], serde_json::json!([1, 2, 3]));
        assert_eq!(parsed["blocks"], serde_json::json!([0, 1, 2, 3]));
    }

    #[test]
    fn test_blocktype_u16_conversion_roundtrip() {
        let types = [
            BlockType::Air,
            BlockType::Stone,
            BlockType::Dirt,
            BlockType::Grass,
            BlockType::Sand,
            BlockType::Water,
            BlockType::Wood,
            BlockType::Leaves,
        ];

        for block in types {
            let val: u16 = block.into();
            let back: BlockType = val.into();
            assert_eq!(back, block, "u16 round-trip failed for {:?} (u16={})", block, val);
        }
    }

    #[test]
    fn test_unknown_u16_defaults_to_air() {
        let block: BlockType = 255u16.into();
        assert_eq!(block, BlockType::Air);

        let block: BlockType = 100u16.into();
        assert_eq!(block, BlockType::Air);
    }

    #[test]
    fn test_chunk_exists() {
        let storage = temp_storage();

        assert!(!chunk_exists(IVec3::ZERO, &storage));

        let chunk = Chunk::new(IVec3::ZERO);
        save_chunk(&chunk, &storage).expect("save should succeed");

        assert!(chunk_exists(IVec3::ZERO, &storage));
        assert!(!chunk_exists(IVec3::ONE, &storage));

        cleanup(&storage);
    }

    #[test]
    fn test_chunk_exists_binary() {
        let storage = temp_storage();

        assert!(!chunk_exists(IVec3::ZERO, &storage));

        let chunk = Chunk::new(IVec3::ZERO);
        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("save should succeed");

        assert!(chunk_exists(IVec3::ZERO, &storage));
        assert!(!chunk_exists(IVec3::ONE, &storage));

        cleanup(&storage);
    }

    #[test]
    fn test_loaded_chunk_is_dirty() {
        let storage = temp_storage();

        let chunk = Chunk::new(IVec3::ZERO);
        save_chunk(&chunk, &storage).expect("save should succeed");

        let loaded = load_chunk(IVec3::ZERO, &storage).expect("load should succeed");
        assert!(loaded.dirty, "Loaded chunks should be marked dirty for mesh rebuild");

        cleanup(&storage);
    }

    // ========================================================================
    // Binary format tests
    // ========================================================================

    #[test]
    fn test_binary_round_trip_empty_chunk() {
        let storage = temp_storage();

        let chunk = Chunk::new(IVec3::new(1, 2, 3));
        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("save should succeed");

        let loaded = load_chunk_fmt(IVec3::new(1, 2, 3), &storage, SaveFormat::Binary)
            .expect("load should succeed");

        assert_eq!(loaded.position, chunk.position);
        for i in 0..CHUNK_VOLUME {
            assert_eq!(loaded.blocks()[i], chunk.blocks()[i]);
        }

        cleanup(&storage);
    }

    #[test]
    fn test_binary_round_trip_with_blocks() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::new(-3, 0, 7));
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(5, 10, 15, BlockType::Grass);
        chunk.set_block(15, 15, 15, BlockType::Water);
        chunk.set_block(8, 4, 2, BlockType::Wood);

        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("save should succeed");
        let loaded = load_chunk_fmt(IVec3::new(-3, 0, 7), &storage, SaveFormat::Binary)
            .expect("load should succeed");

        assert_eq!(loaded.position, IVec3::new(-3, 0, 7));
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Stone);
        assert_eq!(loaded.get_block(5, 10, 15), BlockType::Grass);
        assert_eq!(loaded.get_block(15, 15, 15), BlockType::Water);
        assert_eq!(loaded.get_block(8, 4, 2), BlockType::Wood);
        assert_eq!(loaded.get_block(7, 7, 7), BlockType::Air);

        cleanup(&storage);
    }

    #[test]
    fn test_binary_round_trip_filled_chunk() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("save should succeed");
        let loaded = load_chunk_fmt(IVec3::ZERO, &storage, SaveFormat::Binary)
            .expect("load should succeed");

        for i in 0..CHUNK_VOLUME {
            assert_eq!(loaded.blocks()[i], BlockType::Stone, "block {} mismatch", i);
        }

        cleanup(&storage);
    }

    #[test]
    fn test_binary_is_smaller_than_json() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::ZERO);
        // Fill with a variety of blocks for a realistic comparison
        for x in 0..16 {
            for z in 0..16 {
                chunk.set_block(x, 0, z, BlockType::Stone);
                chunk.set_block(x, 1, z, BlockType::Dirt);
                chunk.set_block(x, 2, z, BlockType::Grass);
            }
        }

        // Save in both formats
        save_chunk_fmt(&chunk, &storage, SaveFormat::Json).expect("json save");
        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("binary save");

        let json_path = chunk_file_path_fmt(IVec3::ZERO, &storage, SaveFormat::Json);
        let bin_path = chunk_file_path_fmt(IVec3::ZERO, &storage, SaveFormat::Binary);

        let json_size = fs::metadata(&json_path).unwrap().len();
        let bin_size = fs::metadata(&bin_path).unwrap().len();

        assert!(
            bin_size < json_size,
            "Binary ({} bytes) should be smaller than JSON ({} bytes)",
            bin_size,
            json_size
        );

        cleanup(&storage);
    }

    #[test]
    fn test_auto_detect_loads_binary_first() {
        let storage = temp_storage();

        // Save in binary format
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Obsidian);
        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("binary save");

        // load_chunk_auto should find the .bin file
        let loaded = load_chunk_auto(IVec3::ZERO, &storage).expect("auto load");
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Obsidian);

        cleanup(&storage);
    }

    #[test]
    fn test_auto_detect_falls_back_to_json() {
        let storage = temp_storage();

        // Save in JSON format only
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Wood);
        save_chunk_fmt(&chunk, &storage, SaveFormat::Json).expect("json save");

        // load_chunk_auto should find the .json file
        let loaded = load_chunk_auto(IVec3::ZERO, &storage).expect("auto load");
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Wood);

        cleanup(&storage);
    }

    #[test]
    fn test_auto_detect_returns_not_found() {
        let storage = temp_storage();

        let result = load_chunk_auto(IVec3::new(99, 99, 99), &storage);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);

        cleanup(&storage);
    }

    #[test]
    fn test_save_format_from_str_lossy() {
        assert_eq!(SaveFormat::from_str_lossy("json"), SaveFormat::Json);
        assert_eq!(SaveFormat::from_str_lossy("JSON"), SaveFormat::Json);
        assert_eq!(SaveFormat::from_str_lossy("binary"), SaveFormat::Binary);
        assert_eq!(SaveFormat::from_str_lossy("bin"), SaveFormat::Binary);
        assert_eq!(SaveFormat::from_str_lossy("bincode"), SaveFormat::Binary);
        assert_eq!(SaveFormat::from_str_lossy("BINARY"), SaveFormat::Binary);
        assert_eq!(SaveFormat::from_str_lossy("compressed"), SaveFormat::Compressed);
        assert_eq!(SaveFormat::from_str_lossy("cbin"), SaveFormat::Compressed);
        assert_eq!(SaveFormat::from_str_lossy("palette"), SaveFormat::Compressed);
        // Unknown defaults to compressed (new default)
        assert_eq!(SaveFormat::from_str_lossy("unknown"), SaveFormat::Compressed);
    }

    #[test]
    fn test_save_format_extension() {
        assert_eq!(SaveFormat::Json.extension(), "json");
        assert_eq!(SaveFormat::Binary.extension(), "bin");
        assert_eq!(SaveFormat::Compressed.extension(), "cbin");
    }

    #[test]
    fn test_save_format_display() {
        assert_eq!(format!("{}", SaveFormat::Json), "json");
        assert_eq!(format!("{}", SaveFormat::Binary), "binary");
        assert_eq!(format!("{}", SaveFormat::Compressed), "compressed");
    }

    #[test]
    fn test_binary_all_block_types() {
        let storage = temp_storage();

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

        let mut chunk = Chunk::new(IVec3::new(0, 0, 0));
        for (i, &block_type) in all_types.iter().enumerate() {
            chunk.set_block(i, 0, 0, block_type);
        }

        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("save should succeed");
        let loaded = load_chunk_fmt(IVec3::ZERO, &storage, SaveFormat::Binary)
            .expect("load should succeed");

        for (i, &block_type) in all_types.iter().enumerate() {
            assert_eq!(
                loaded.get_block(i, 0, 0),
                block_type,
                "Block type {:?} at index {} did not survive binary round-trip",
                block_type,
                i
            );
        }

        cleanup(&storage);
    }

    // ========================================================================
    // Compressed (palette) format tests
    // ========================================================================

    #[test]
    fn test_compress_chunk_data_simple() {
        // Test compression with a few block types
        let mut blocks = [BlockType::Air; CHUNK_VOLUME];
        blocks[0] = BlockType::Stone;
        blocks[1] = BlockType::Dirt;
        blocks[2] = BlockType::Grass;

        let result = compress_chunk_data(&blocks);
        assert!(result.is_some());

        let (palette, indices) = result.unwrap();
        assert_eq!(palette.len(), 4); // Air, Stone, Dirt, Grass
        assert_eq!(indices.len(), CHUNK_VOLUME);

        // Verify indices are valid
        for idx in &indices {
            assert!((*idx as usize) < palette.len());
        }
    }

    #[test]
    fn test_compress_decompress_roundtrip() {
        let mut blocks = [BlockType::Air; CHUNK_VOLUME];
        blocks[0] = BlockType::Stone;
        blocks[100] = BlockType::Dirt;
        blocks[1000] = BlockType::Water;
        blocks[4000] = BlockType::Obsidian;

        let (palette, indices) = compress_chunk_data(&blocks).expect("compression should succeed");
        let decompressed = decompress_chunk_data(&palette, &indices).expect("decompression should succeed");

        for i in 0..CHUNK_VOLUME {
            assert_eq!(decompressed[i], blocks[i], "block {} mismatch", i);
        }
    }

    #[test]
    fn test_palette_stats_calculation() {
        let stats = PaletteStats::calculate(5);
        assert_eq!(stats.unique_blocks, 5);
        assert_eq!(stats.original_bytes, CHUNK_VOLUME * 2); // 8192 bytes
        assert_eq!(stats.compressed_bytes, (5 * 2) + CHUNK_VOLUME); // 4106 bytes
        assert!(stats.ratio < 1.0); // Should be compressed
    }

    #[test]
    fn test_compressed_round_trip_empty_chunk() {
        let storage = temp_storage();

        let chunk = Chunk::new(IVec3::new(1, 2, 3));
        save_chunk_fmt(&chunk, &storage, SaveFormat::Compressed).expect("save should succeed");

        let loaded = load_chunk_fmt(IVec3::new(1, 2, 3), &storage, SaveFormat::Compressed)
            .expect("load should succeed");

        assert_eq!(loaded.position, chunk.position);
        for i in 0..CHUNK_VOLUME {
            assert_eq!(loaded.blocks()[i], chunk.blocks()[i]);
        }

        cleanup(&storage);
    }

    #[test]
    fn test_compressed_round_trip_with_blocks() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::new(-3, 0, 7));
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(5, 10, 15, BlockType::Grass);
        chunk.set_block(15, 15, 15, BlockType::Water);
        chunk.set_block(8, 4, 2, BlockType::Wood);

        save_chunk_fmt(&chunk, &storage, SaveFormat::Compressed).expect("save should succeed");
        let loaded = load_chunk_fmt(IVec3::new(-3, 0, 7), &storage, SaveFormat::Compressed)
            .expect("load should succeed");

        assert_eq!(loaded.position, IVec3::new(-3, 0, 7));
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Stone);
        assert_eq!(loaded.get_block(5, 10, 15), BlockType::Grass);
        assert_eq!(loaded.get_block(15, 15, 15), BlockType::Water);
        assert_eq!(loaded.get_block(8, 4, 2), BlockType::Wood);
        assert_eq!(loaded.get_block(7, 7, 7), BlockType::Air);

        cleanup(&storage);
    }

    #[test]
    fn test_compressed_round_trip_filled_chunk() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.fill(BlockType::Stone);

        save_chunk_fmt(&chunk, &storage, SaveFormat::Compressed).expect("save should succeed");
        let loaded = load_chunk_fmt(IVec3::ZERO, &storage, SaveFormat::Compressed)
            .expect("load should succeed");

        for i in 0..CHUNK_VOLUME {
            assert_eq!(loaded.blocks()[i], BlockType::Stone, "block {} mismatch", i);
        }

        cleanup(&storage);
    }

    #[test]
    fn test_compressed_is_smaller_than_binary() {
        let storage = temp_storage();

        let mut chunk = Chunk::new(IVec3::ZERO);
        // Fill with a variety of blocks for a realistic comparison
        for x in 0..16 {
            for z in 0..16 {
                chunk.set_block(x, 0, z, BlockType::Stone);
                chunk.set_block(x, 1, z, BlockType::Dirt);
                chunk.set_block(x, 2, z, BlockType::Grass);
            }
        }

        // Save in both formats
        save_chunk_fmt(&chunk, &storage, SaveFormat::Binary).expect("binary save");
        save_chunk_fmt(&chunk, &storage, SaveFormat::Compressed).expect("compressed save");

        let bin_path = chunk_file_path_fmt(IVec3::ZERO, &storage, SaveFormat::Binary);
        let cbin_path = chunk_file_path_fmt(IVec3::ZERO, &storage, SaveFormat::Compressed);

        let bin_size = fs::metadata(&bin_path).unwrap().len();
        let cbin_size = fs::metadata(&cbin_path).unwrap().len();

        assert!(
            cbin_size < bin_size,
            "Compressed ({} bytes) should be smaller than Binary ({} bytes)",
            cbin_size,
            bin_size
        );

        // Verify compression ratio is reasonable (should be ~50%)
        let ratio = cbin_size as f64 / bin_size as f64;
        assert!(
            ratio < 0.65,
            "Compression ratio ({:.1}%) should be better than 65%",
            ratio * 100.0
        );

        cleanup(&storage);
    }

    #[test]
    fn test_compressed_all_block_types() {
        let storage = temp_storage();

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
            BlockType::CopperOre,
            BlockType::IronOre,
            BlockType::SilverOre,
            BlockType::GoldOre,
        ];

        let mut chunk = Chunk::new(IVec3::new(0, 0, 0));
        // Spread blocks across x and y to avoid x >= CHUNK_SIZE (16)
        for (i, &block_type) in all_types.iter().enumerate() {
            let x = i % 16;
            let y = i / 16;
            chunk.set_block(x, y, 0, block_type);
        }

        save_chunk_fmt(&chunk, &storage, SaveFormat::Compressed).expect("save should succeed");
        let loaded = load_chunk_fmt(IVec3::ZERO, &storage, SaveFormat::Compressed)
            .expect("load should succeed");

        for (i, &block_type) in all_types.iter().enumerate() {
            let x = i % 16;
            let y = i / 16;
            assert_eq!(
                loaded.get_block(x, y, 0),
                block_type,
                "Block type {:?} at ({}, {}, 0) did not survive compressed round-trip",
                block_type,
                x,
                y
            );
        }

        cleanup(&storage);
    }

    #[test]
    fn test_auto_detect_loads_compressed_first() {
        let storage = temp_storage();

        // Save in compressed format
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::GoldOre);
        save_chunk_fmt(&chunk, &storage, SaveFormat::Compressed).expect("compressed save");

        // load_chunk_auto should find the .cbin file
        let loaded = load_chunk_auto(IVec3::ZERO, &storage).expect("auto load");
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::GoldOre);

        cleanup(&storage);
    }

    #[test]
    fn test_chunk_exists_compressed() {
        let storage = temp_storage();

        assert!(!chunk_exists(IVec3::ZERO, &storage));

        let chunk = Chunk::new(IVec3::ZERO);
        save_chunk_fmt(&chunk, &storage, SaveFormat::Compressed).expect("save should succeed");

        assert!(chunk_exists(IVec3::ZERO, &storage));
        assert!(!chunk_exists(IVec3::ONE, &storage));

        cleanup(&storage);
    }

    #[test]
    fn test_decompress_invalid_index() {
        let palette = vec![0, 1, 2]; // 3 entries
        let mut indices = vec![0u8; CHUNK_VOLUME];
        indices[100] = 5; // Invalid index (out of bounds)

        let result = decompress_chunk_data(&palette, &indices);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("out of bounds"));
    }

    #[test]
    fn test_decompress_wrong_length() {
        let palette = vec![0, 1];
        let indices = vec![0u8; 100]; // Wrong length

        let result = decompress_chunk_data(&palette, &indices);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Expected"));
    }

    #[test]
    fn test_single_block_type_chunk() {
        // Chunk with only one block type should have palette size 1
        let blocks = [BlockType::Stone; CHUNK_VOLUME];
        let (palette, _indices) = compress_chunk_data(&blocks).expect("compression should succeed");
        
        assert_eq!(palette.len(), 1);
        assert_eq!(palette[0], BlockType::Stone as u16);
    }

    #[test]
    fn test_default_save_format_is_compressed() {
        assert_eq!(SaveFormat::default(), SaveFormat::Compressed);
    }
}
