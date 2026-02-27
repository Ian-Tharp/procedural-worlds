//! World region export to portable schematic format (.schem)
//!
//! Enables exporting a player-defined axis-aligned bounding box (AABB) region
//! of the world to a self-contained binary file. Use cases include:
//!
//! - **Sharing builds** with other players
//! - **Backup/archiving** saves externally
//! - **Cross-save migration** and future import support
//!
//! # Binary Format
//!
//! The `.schem` file uses a simple binary layout:
//!
//! ```text
//! [ json_header_len (u32 LE) | json_header (UTF-8 JSON) | bincode_block_data ]
//! ```
//!
//! - **`json_header_len`**: 4 bytes, little-endian u32 indicating the byte length
//!   of the JSON header that follows. This allows readers to skip or parse the
//!   header without knowing its size upfront.
//! - **`json_header`**: UTF-8 encoded JSON containing a [`SchematicHeader`] with
//!   metadata (format version, dimensions, bounds, timestamp, author).
//! - **`bincode_block_data`**: The remaining bytes are a bincode-serialized
//!   [`SchematicBlockData`] containing block type IDs as `Vec<u16>` in
//!   row-major order (X varies fastest, then Y, then Z).
//!
//! # Block Indexing
//!
//! Blocks are stored in a flat `Vec<u16>` with dimensions `(width, height, depth)`.
//! The index for block at relative position `(x, y, z)` within the region is:
//!
//! ```text
//! index = x + y * width + z * width * height
//! ```
//!
//! where `(x, y, z)` are offsets from `bounds_min`.
//!
//! # Example
//!
//! ```ignore
//! use bevy::math::IVec3;
//! use procedural_worlds::world::schematic::*;
//!
//! // Export a region from loaded chunks
//! let bounds_min = IVec3::new(0, 0, 0);
//! let bounds_max = IVec3::new(31, 15, 31);
//! let bytes = export_region(&chunks, bounds_min, bounds_max, Some("PlayerName"))?;
//!
//! // Write to file
//! std::fs::write("my_build.schem", &bytes)?;
//! ```

use std::fmt;
use std::io;
use std::path::Path;

use bevy::prelude::IVec3;
use serde::{Deserialize, Serialize};

use super::{BlockType, Chunk, CHUNK_SIZE};

// ============================================================================
// FORMAT VERSION
// ============================================================================

/// Schematic format version enum.
///
/// Allows future format changes while maintaining backwards compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchematicVersion {
    /// Initial version: flat block array with JSON header.
    V1,
}

impl fmt::Display for SchematicVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchematicVersion::V1 => write!(f, "v1"),
        }
    }
}

// ============================================================================
// HEADER
// ============================================================================

/// Metadata header for a schematic file.
///
/// Serialized as JSON at the start of the `.schem` binary file.
/// Contains all information needed to interpret the block data that follows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchematicHeader {
    /// Format version for compatibility checking.
    pub version: SchematicVersion,
    /// Region dimensions in blocks: `[width, height, depth]`.
    pub dimensions: [u32; 3],
    /// Minimum corner of the exported AABB in world block coordinates: `[x, y, z]`.
    pub bounds_min: [i32; 3],
    /// Maximum corner of the exported AABB in world block coordinates: `[x, y, z]`.
    pub bounds_max: [i32; 3],
    /// Unix epoch timestamp (seconds) when the export was created.
    pub export_timestamp: u64,
    /// Optional author/player name.
    pub author: Option<String>,
    /// Total number of non-air blocks in the export.
    pub block_count: u64,
}

impl SchematicHeader {
    /// Serialize this header to a JSON byte vector.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, io::Error> {
        serde_json::to_vec(self).map_err(io::Error::other)
    }

    /// Deserialize a header from JSON bytes.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, io::Error> {
        serde_json::from_slice(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

// ============================================================================
// BLOCK DATA
// ============================================================================

/// Serializable block data for the schematic body.
///
/// Contains block type IDs as `u16` values in row-major order
/// (X varies fastest, then Y, then Z).
#[derive(Debug, Serialize, Deserialize)]
pub struct SchematicBlockData {
    /// Block type IDs in flat row-major order.
    /// Length = width * height * depth (from header dimensions).
    pub blocks: Vec<u16>,
}

// ============================================================================
// AABB VALIDATION
// ============================================================================

/// Error type for schematic operations.
#[derive(Debug)]
pub enum SchematicError {
    /// The AABB bounds are invalid (min >= max on some axis).
    InvalidBounds {
        min: IVec3,
        max: IVec3,
        reason: String,
    },
    /// IO error during file operations.
    Io(io::Error),
    /// Serialization/deserialization error.
    Serialization(String),
    /// The schematic data is malformed or corrupt.
    MalformedData(String),
}

impl fmt::Display for SchematicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchematicError::InvalidBounds { min, max, reason } => {
                write!(
                    f,
                    "Invalid AABB bounds: min={}, max={}: {}",
                    min, max, reason
                )
            }
            SchematicError::Io(e) => write!(f, "IO error: {}", e),
            SchematicError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            SchematicError::MalformedData(msg) => write!(f, "Malformed schematic data: {}", msg),
        }
    }
}

impl std::error::Error for SchematicError {}

impl From<io::Error> for SchematicError {
    fn from(e: io::Error) -> Self {
        SchematicError::Io(e)
    }
}

/// Validate AABB bounds: min must be strictly less than max on all axes.
///
/// Returns `(width, height, depth)` as `u32` on success.
fn validate_bounds(min: IVec3, max: IVec3) -> Result<(u32, u32, u32), SchematicError> {
    if min.x >= max.x {
        return Err(SchematicError::InvalidBounds {
            min,
            max,
            reason: format!("min.x ({}) must be less than max.x ({})", min.x, max.x),
        });
    }
    if min.y >= max.y {
        return Err(SchematicError::InvalidBounds {
            min,
            max,
            reason: format!("min.y ({}) must be less than max.y ({})", min.y, max.y),
        });
    }
    if min.z >= max.z {
        return Err(SchematicError::InvalidBounds {
            min,
            max,
            reason: format!("min.z ({}) must be less than max.z ({})", min.z, max.z),
        });
    }

    // Dimensions are inclusive on both ends: max - min + 1
    let width = (max.x - min.x + 1) as u32;
    let height = (max.y - min.y + 1) as u32;
    let depth = (max.z - min.z + 1) as u32;

    Ok((width, height, depth))
}

// ============================================================================
// BLOCK EXTRACTION
// ============================================================================

/// Look up the block at the given world-space block coordinate from loaded chunks.
///
/// Returns `BlockType::Air` if the chunk containing the position is not
/// in the provided chunk list.
fn get_block_from_chunks(world_pos: IVec3, chunks: &[&Chunk]) -> BlockType {
    // Determine which chunk contains this block
    let chunk_pos = IVec3::new(
        world_pos.x.div_euclid(CHUNK_SIZE as i32),
        world_pos.y.div_euclid(CHUNK_SIZE as i32),
        world_pos.z.div_euclid(CHUNK_SIZE as i32),
    );

    // Find the chunk in our list
    for chunk in chunks {
        if chunk.position == chunk_pos {
            let local_x = world_pos.x.rem_euclid(CHUNK_SIZE as i32) as usize;
            let local_y = world_pos.y.rem_euclid(CHUNK_SIZE as i32) as usize;
            let local_z = world_pos.z.rem_euclid(CHUNK_SIZE as i32) as usize;
            return chunk.get_block(local_x, local_y, local_z);
        }
    }

    BlockType::Air
}

// ============================================================================
// EXPORT FUNCTIONS
// ============================================================================

/// Export a world region defined by an AABB to a schematic byte vector.
///
/// Collects all blocks within the inclusive bounds `[min, max]` from the
/// provided chunks and serializes them into the `.schem` binary format.
///
/// # Binary Format
///
/// ```text
/// [ json_header_len (u32 LE) | json_header (UTF-8) | bincode_block_data ]
/// ```
///
/// # Arguments
///
/// * `chunks` — Slice of chunk references to read blocks from. Chunks outside
///   the bounds are harmlessly ignored; positions not covered by any chunk
///   default to `BlockType::Air`.
/// * `bounds_min` — Inclusive minimum corner in world block coordinates.
/// * `bounds_max` — Inclusive maximum corner in world block coordinates.
/// * `author` — Optional author/player name to embed in the header.
///
/// # Errors
///
/// Returns [`SchematicError::InvalidBounds`] if `min >= max` on any axis.
/// Returns [`SchematicError::Serialization`] if bincode encoding fails.
pub fn export_region(
    chunks: &[&Chunk],
    bounds_min: IVec3,
    bounds_max: IVec3,
    author: Option<&str>,
) -> Result<Vec<u8>, SchematicError> {
    let (width, height, depth) = validate_bounds(bounds_min, bounds_max)?;

    // Extract blocks in row-major order: x varies fastest, then y, then z
    let total_blocks = width as usize * height as usize * depth as usize;
    let mut block_ids: Vec<u16> = Vec::with_capacity(total_blocks);
    let mut non_air_count: u64 = 0;

    for z in bounds_min.z..=bounds_max.z {
        for y in bounds_min.y..=bounds_max.y {
            for x in bounds_min.x..=bounds_max.x {
                let block = get_block_from_chunks(IVec3::new(x, y, z), chunks);
                if block != BlockType::Air {
                    non_air_count += 1;
                }
                block_ids.push(u16::from(block));
            }
        }
    }

    // Build header
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let header = SchematicHeader {
        version: SchematicVersion::V1,
        dimensions: [width, height, depth],
        bounds_min: [bounds_min.x, bounds_min.y, bounds_min.z],
        bounds_max: [bounds_max.x, bounds_max.y, bounds_max.z],
        export_timestamp: timestamp,
        author: author.map(|s| s.to_string()),
        block_count: non_air_count,
    };

    let block_data = SchematicBlockData { blocks: block_ids };

    // Serialize header to JSON
    let header_json = header.to_json_bytes()?;
    let header_len = header_json.len() as u32;

    // Serialize block data with bincode
    let block_bytes = bincode::serialize(&block_data)
        .map_err(|e| SchematicError::Serialization(e.to_string()))?;

    // Assemble: [header_len (u32 LE) | header_json | block_bytes]
    let mut output =
        Vec::with_capacity(4 + header_json.len() + block_bytes.len());
    output.extend_from_slice(&header_len.to_le_bytes());
    output.extend_from_slice(&header_json);
    output.extend_from_slice(&block_bytes);

    Ok(output)
}

/// Export a world region to a `.schem` file on disk.
///
/// This is a convenience wrapper around [`export_region`] that writes
/// the result to the specified file path. Parent directories are created
/// if they don't exist.
///
/// # Arguments
///
/// * `chunks` — Slice of chunk references to read blocks from.
/// * `bounds_min` — Inclusive minimum corner in world block coordinates.
/// * `bounds_max` — Inclusive maximum corner in world block coordinates.
/// * `author` — Optional author/player name.
/// * `path` — Output file path (typically ending in `.schem`).
///
/// # Errors
///
/// Returns [`SchematicError`] on validation, serialization, or IO failure.
pub fn export_region_to_file(
    chunks: &[&Chunk],
    bounds_min: IVec3,
    bounds_max: IVec3,
    author: Option<&str>,
    path: &Path,
) -> Result<(), SchematicError> {
    let bytes = export_region(chunks, bounds_min, bounds_max, author)?;

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(path, bytes)?;
    Ok(())
}

// ============================================================================
// PARSING / READING (for tests and future import)
// ============================================================================

/// Parse a schematic binary blob, returning the header and block data.
///
/// Useful for verification and will serve as the basis for a future
/// import feature.
///
/// # Errors
///
/// Returns [`SchematicError::MalformedData`] if the data is too short,
/// the header length is invalid, or deserialization fails.
pub fn parse_schematic(data: &[u8]) -> Result<(SchematicHeader, SchematicBlockData), SchematicError> {
    // Need at least 4 bytes for the header length
    if data.len() < 4 {
        return Err(SchematicError::MalformedData(
            "Data too short: missing header length".to_string(),
        ));
    }

    // Read header length (u32 LE)
    let header_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

    // Validate we have enough bytes
    let header_end = 4 + header_len;
    if data.len() < header_end {
        return Err(SchematicError::MalformedData(format!(
            "Data too short: header claims {} bytes but only {} available after length field",
            header_len,
            data.len() - 4
        )));
    }

    // Parse JSON header
    let header = SchematicHeader::from_json_bytes(&data[4..header_end])?;

    // Parse bincode block data
    let block_data: SchematicBlockData = bincode::deserialize(&data[header_end..])
        .map_err(|e| SchematicError::Serialization(e.to_string()))?;

    // Validate block count matches dimensions
    let expected_count =
        header.dimensions[0] as usize * header.dimensions[1] as usize * header.dimensions[2] as usize;
    if block_data.blocks.len() != expected_count {
        return Err(SchematicError::MalformedData(format!(
            "Block count mismatch: header says {}x{}x{} = {} blocks, but data has {}",
            header.dimensions[0],
            header.dimensions[1],
            header.dimensions[2],
            expected_count,
            block_data.blocks.len()
        )));
    }

    Ok((header, block_data))
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a chunk at a given chunk-coordinate position with all Air.
    fn empty_chunk(pos: IVec3) -> Chunk {
        Chunk::new(pos)
    }

    /// Helper: create a chunk with terrain-like blocks.
    /// Bottom half (y < 8) is Stone, top half is Air. Surface row (y=7) is Grass.
    fn terrain_chunk(pos: IVec3) -> Chunk {
        let mut chunk = Chunk::new(pos);
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                for y in 0..8 {
                    if y == 7 {
                        chunk.set_block(x, y, z, BlockType::Grass);
                    } else {
                        chunk.set_block(x, y, z, BlockType::Stone);
                    }
                }
            }
        }
        chunk
    }

    // ========================================================================
    // Test 1: Export an empty region (valid bounds, no blocks / all air)
    // ========================================================================

    #[test]
    fn test_export_empty_region() {
        // Region within a single chunk that is all air
        let chunk = empty_chunk(IVec3::ZERO);
        let chunks: Vec<&Chunk> = vec![&chunk];

        let bounds_min = IVec3::new(0, 0, 0);
        let bounds_max = IVec3::new(7, 7, 7);

        let result = export_region(&chunks, bounds_min, bounds_max, None);
        assert!(result.is_ok(), "Export should succeed for valid empty region");

        let bytes = result.unwrap();
        assert!(!bytes.is_empty(), "Output should not be empty");

        // Parse back and verify
        let (header, block_data) = parse_schematic(&bytes).expect("Should parse successfully");

        assert_eq!(header.version, SchematicVersion::V1);
        assert_eq!(header.dimensions, [8, 8, 8]);
        assert_eq!(header.bounds_min, [0, 0, 0]);
        assert_eq!(header.bounds_max, [7, 7, 7]);
        assert_eq!(header.block_count, 0); // All air
        assert_eq!(header.author, None);

        // All blocks should be air (BlockType::Air = 0)
        assert_eq!(block_data.blocks.len(), 8 * 8 * 8);
        assert!(
            block_data.blocks.iter().all(|&b| b == 0),
            "All blocks should be Air (0)"
        );
    }

    // ========================================================================
    // Test 2: Export a populated region spanning multiple chunks
    // ========================================================================

    #[test]
    fn test_export_populated_region() {
        // Create a 2x1x2 grid of terrain chunks (each 16x16x16)
        let chunk_00 = terrain_chunk(IVec3::new(0, 0, 0));
        let chunk_10 = terrain_chunk(IVec3::new(1, 0, 0));
        let chunk_01 = terrain_chunk(IVec3::new(0, 0, 1));
        let chunk_11 = terrain_chunk(IVec3::new(1, 0, 1));

        let chunks: Vec<&Chunk> = vec![&chunk_00, &chunk_10, &chunk_01, &chunk_11];

        // Export a region that spans across chunk boundaries:
        // x: 8..23 (crosses from chunk 0 to chunk 1 in X)
        // y: 0..9 (includes stone + grass + some air above)
        // z: 8..23 (crosses from chunk 0 to chunk 1 in Z)
        let bounds_min = IVec3::new(8, 0, 8);
        let bounds_max = IVec3::new(23, 9, 23);

        let result = export_region(&chunks, bounds_min, bounds_max, Some("TestPlayer"));
        assert!(result.is_ok(), "Export should succeed for multi-chunk region");

        let bytes = result.unwrap();
        let (header, block_data) = parse_schematic(&bytes).expect("Should parse successfully");

        // Dimensions: 16 x 10 x 16
        assert_eq!(header.dimensions, [16, 10, 16]);
        assert_eq!(header.bounds_min, [8, 0, 8]);
        assert_eq!(header.bounds_max, [23, 9, 23]);
        assert_eq!(header.author, Some("TestPlayer".to_string()));

        // Should have blocks (stone + grass layers)
        assert!(header.block_count > 0, "Should have non-air blocks");

        // Expected non-air: 16*8*16 = 2048 blocks (y=0..7 are stone/grass)
        // (y=0..6 is stone = 7 layers, y=7 is grass = 1 layer, y=8..9 is air)
        let expected_non_air = 16 * 8 * 16;
        assert_eq!(
            header.block_count, expected_non_air,
            "Should have exactly {} non-air blocks",
            expected_non_air
        );

        // Verify some specific blocks:
        // Block at relative (0, 0, 0) = world (8, 0, 8) should be Stone
        let width = header.dimensions[0] as usize;
        let height = header.dimensions[1] as usize;

        let idx_stone = 0 + 0 * width + 0 * width * height; // (0,0,0)
        assert_eq!(
            BlockType::from(block_data.blocks[idx_stone]),
            BlockType::Stone
        );

        // Block at relative (0, 7, 0) = world (8, 7, 8) should be Grass
        let idx_grass = 0 + 7 * width + 0 * width * height;
        assert_eq!(
            BlockType::from(block_data.blocks[idx_grass]),
            BlockType::Grass
        );

        // Block at relative (0, 9, 0) = world (8, 9, 8) should be Air
        let idx_air = 0 + 9 * width + 0 * width * height;
        assert_eq!(
            BlockType::from(block_data.blocks[idx_air]),
            BlockType::Air
        );
    }

    // ========================================================================
    // Test 3: Roundtrip — export then parse, verify structural integrity
    // ========================================================================

    #[test]
    fn test_schematic_roundtrip() {
        // Create a chunk with a distinctive block pattern
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(1, 0, 0, BlockType::Dirt);
        chunk.set_block(0, 1, 0, BlockType::Grass);
        chunk.set_block(0, 0, 1, BlockType::Sand);
        chunk.set_block(1, 1, 1, BlockType::Obsidian);
        chunk.set_block(3, 3, 3, BlockType::GoldOre);
        chunk.set_block(7, 7, 7, BlockType::Water);

        let chunks: Vec<&Chunk> = vec![&chunk];
        let bounds_min = IVec3::new(0, 0, 0);
        let bounds_max = IVec3::new(7, 7, 7);

        // Export
        let bytes = export_region(&chunks, bounds_min, bounds_max, Some("Roundtripper"))
            .expect("Export should succeed");

        // Parse back
        let (header, block_data) =
            parse_schematic(&bytes).expect("Parse should succeed");

        // Verify header
        assert_eq!(header.version, SchematicVersion::V1);
        assert_eq!(header.dimensions, [8, 8, 8]);
        assert_eq!(header.author, Some("Roundtripper".to_string()));
        assert!(header.export_timestamp > 0);

        // Verify block data length
        assert_eq!(block_data.blocks.len(), 8 * 8 * 8);

        // Helper to compute flat index
        let idx = |x: usize, y: usize, z: usize| -> usize {
            x + y * 8 + z * 8 * 8
        };

        // Verify specific blocks survived the roundtrip
        assert_eq!(
            BlockType::from(block_data.blocks[idx(0, 0, 0)]),
            BlockType::Stone
        );
        assert_eq!(
            BlockType::from(block_data.blocks[idx(1, 0, 0)]),
            BlockType::Dirt
        );
        assert_eq!(
            BlockType::from(block_data.blocks[idx(0, 1, 0)]),
            BlockType::Grass
        );
        assert_eq!(
            BlockType::from(block_data.blocks[idx(0, 0, 1)]),
            BlockType::Sand
        );
        assert_eq!(
            BlockType::from(block_data.blocks[idx(1, 1, 1)]),
            BlockType::Obsidian
        );
        assert_eq!(
            BlockType::from(block_data.blocks[idx(3, 3, 3)]),
            BlockType::GoldOre
        );
        assert_eq!(
            BlockType::from(block_data.blocks[idx(7, 7, 7)]),
            BlockType::Water
        );

        // Non-air block count: 7 blocks placed
        assert_eq!(header.block_count, 7);

        // Verify an unset position is Air
        assert_eq!(
            BlockType::from(block_data.blocks[idx(4, 4, 4)]),
            BlockType::Air
        );
    }

    // ========================================================================
    // Test 4: Invalid bounds should error gracefully
    // ========================================================================

    #[test]
    fn test_invalid_bounds() {
        let chunk = empty_chunk(IVec3::ZERO);
        let chunks: Vec<&Chunk> = vec![&chunk];

        // min.x >= max.x
        let result = export_region(
            &chunks,
            IVec3::new(10, 0, 0),
            IVec3::new(5, 10, 10),
            None,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            SchematicError::InvalidBounds { reason, .. } => {
                assert!(reason.contains("min.x"));
            }
            other => panic!("Expected InvalidBounds, got: {}", other),
        }

        // min.y >= max.y
        let result = export_region(
            &chunks,
            IVec3::new(0, 10, 0),
            IVec3::new(10, 10, 10),
            None,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            SchematicError::InvalidBounds { reason, .. } => {
                assert!(reason.contains("min.y"));
            }
            other => panic!("Expected InvalidBounds, got: {}", other),
        }

        // min.z >= max.z
        let result = export_region(
            &chunks,
            IVec3::new(0, 0, 10),
            IVec3::new(10, 10, 5),
            None,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            SchematicError::InvalidBounds { reason, .. } => {
                assert!(reason.contains("min.z"));
            }
            other => panic!("Expected InvalidBounds, got: {}", other),
        }

        // All equal (min == max is invalid since min must be strictly less)
        let result = export_region(
            &chunks,
            IVec3::new(5, 5, 5),
            IVec3::new(5, 5, 5),
            None,
        );
        assert!(result.is_err());
    }

    // ========================================================================
    // Test 5: File export roundtrip
    // ========================================================================

    #[test]
    fn test_export_to_file_roundtrip() {
        let mut chunk = Chunk::new(IVec3::ZERO);
        chunk.set_block(2, 3, 4, BlockType::IronOre);
        chunk.set_block(5, 6, 7, BlockType::CopperOre);

        let chunks: Vec<&Chunk> = vec![&chunk];
        let bounds_min = IVec3::new(0, 0, 0);
        let bounds_max = IVec3::new(15, 15, 15);

        // Write to a temp file
        let dir = std::env::temp_dir().join(format!(
            "pw_schematic_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file_path = dir.join("test.schem");

        export_region_to_file(&chunks, bounds_min, bounds_max, Some("FileTest"), &file_path)
            .expect("File export should succeed");

        assert!(file_path.exists(), "Schematic file should exist");

        // Read back and verify
        let data = std::fs::read(&file_path).expect("Should read file");
        let (header, block_data) = parse_schematic(&data).expect("Should parse file");

        assert_eq!(header.dimensions, [16, 16, 16]);
        assert_eq!(header.author, Some("FileTest".to_string()));
        assert_eq!(header.block_count, 2);

        // Verify blocks
        let width = 16;
        let height = 16;
        let idx = |x: usize, y: usize, z: usize| x + y * width + z * width * height;

        assert_eq!(
            BlockType::from(block_data.blocks[idx(2, 3, 4)]),
            BlockType::IronOre
        );
        assert_eq!(
            BlockType::from(block_data.blocks[idx(5, 6, 7)]),
            BlockType::CopperOre
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ========================================================================
    // Test 6: Header serialization roundtrip
    // ========================================================================

    #[test]
    fn test_header_json_roundtrip() {
        let header = SchematicHeader {
            version: SchematicVersion::V1,
            dimensions: [32, 16, 32],
            bounds_min: [-10, 0, -10],
            bounds_max: [21, 15, 21],
            export_timestamp: 1700000000,
            author: Some("TestAuthor".to_string()),
            block_count: 42,
        };

        let json_bytes = header.to_json_bytes().expect("Serialization should succeed");
        let parsed = SchematicHeader::from_json_bytes(&json_bytes)
            .expect("Deserialization should succeed");

        assert_eq!(header, parsed);
    }

    // ========================================================================
    // Test 7: Malformed data detection
    // ========================================================================

    #[test]
    fn test_parse_malformed_data() {
        // Too short (less than 4 bytes)
        assert!(parse_schematic(&[0, 1]).is_err());

        // Header length claims more than available
        let mut data = vec![0u8; 10];
        data[0..4].copy_from_slice(&1000u32.to_le_bytes());
        assert!(parse_schematic(&data).is_err());
    }

    // ========================================================================
    // Test 8: Negative coordinate region export
    // ========================================================================

    #[test]
    fn test_export_negative_coordinates() {
        // Chunk at (-1, -1, -1) covers world blocks (-16..-1, -16..-1, -16..-1)
        let mut chunk = Chunk::new(IVec3::new(-1, -1, -1));
        chunk.set_block(0, 0, 0, BlockType::Dirt); // world (-16, -16, -16)
        chunk.set_block(15, 15, 15, BlockType::Stone); // world (-1, -1, -1)

        let chunks: Vec<&Chunk> = vec![&chunk];
        let bounds_min = IVec3::new(-16, -16, -16);
        let bounds_max = IVec3::new(-1, -1, -1);

        let bytes = export_region(&chunks, bounds_min, bounds_max, None)
            .expect("Should handle negative coordinates");

        let (header, block_data) = parse_schematic(&bytes).expect("Should parse");

        assert_eq!(header.dimensions, [16, 16, 16]);
        assert_eq!(header.bounds_min, [-16, -16, -16]);
        assert_eq!(header.bounds_max, [-1, -1, -1]);
        assert_eq!(header.block_count, 2);

        // Verify blocks at expected positions
        let idx = |x: usize, y: usize, z: usize| x + y * 16 + z * 16 * 16;

        // (0,0,0) relative = world (-16,-16,-16) = chunk local (0,0,0) = Dirt
        assert_eq!(
            BlockType::from(block_data.blocks[idx(0, 0, 0)]),
            BlockType::Dirt
        );
        // (15,15,15) relative = world (-1,-1,-1) = chunk local (15,15,15) = Stone
        assert_eq!(
            BlockType::from(block_data.blocks[idx(15, 15, 15)]),
            BlockType::Stone
        );
    }
}
