//! Chunk mesh caching — persist generated mesh data to disk
//!
//! Avoids re-meshing chunks on game restart by caching the mesh vertex data
//! (positions, normals, colors, UVs, indices) to `.mesh_cache/` within the
//! world save directory. Uses bincode for fast binary serialization.
//!
//! # Cache Invalidation
//!
//! Each cached mesh stores a `format_version` derived from the block palette
//! size and a compile-time constant. If the world format changes (new block
//! types, meshing algorithm updates), bumping `MESH_CACHE_VERSION` will
//! automatically invalidate all stale entries.
//!
//! # Usage
//!
//! The cache is optional — controlled by `SaveConfig::mesh_cache_enabled`.
//! When disabled, chunks are always re-meshed from block data (existing behavior).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use serde::{Deserialize, Serialize};

/// Bump this when the meshing algorithm changes to invalidate all cached meshes.
pub const MESH_CACHE_VERSION: u32 = 1;

/// Serializable representation of a chunk's mesh data (one mesh — opaque or water).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CachedMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    /// UV1 (atlas tile coords). Empty if atlas mode was not active.
    pub uv1s: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl CachedMeshData {
    /// Convert this cached data back into a Bevy `Mesh`.
    pub fn to_mesh(&self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.clone());
        if !self.uv1s.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.uv1s.clone());
        }
        mesh.insert_indices(Indices::U32(self.indices.clone()));
        mesh
    }

    /// Extract mesh data from a Bevy `Mesh` into a cacheable form.
    pub fn from_mesh(mesh: &Mesh) -> Option<Self> {
        use bevy::render::mesh::VertexAttributeValues;

        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION)? {
            VertexAttributeValues::Float32x3(v) => v.clone(),
            _ => return None,
        };
        let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL)? {
            VertexAttributeValues::Float32x3(v) => v.clone(),
            _ => return None,
        };
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR)? {
            VertexAttributeValues::Float32x4(v) => v.clone(),
            _ => return None,
        };
        let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0)? {
            VertexAttributeValues::Float32x2(v) => v.clone(),
            _ => return None,
        };
        let uv1s = match mesh.attribute(Mesh::ATTRIBUTE_UV_1) {
            Some(VertexAttributeValues::Float32x2(v)) => v.clone(),
            _ => Vec::new(),
        };
        let indices = match mesh.indices()? {
            Indices::U32(v) => v.clone(),
            Indices::U16(v) => v.iter().map(|&i| i as u32).collect(),
        };

        Some(Self {
            positions,
            normals,
            colors,
            uvs,
            uv1s,
            indices,
        })
    }

    /// Returns true if this mesh data is empty (no vertices).
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// On-disk envelope for a cached chunk mesh pair (opaque + optional water).
#[derive(Serialize, Deserialize, Debug)]
pub struct CachedChunkMesh {
    /// Cache format version — mismatches cause invalidation.
    pub format_version: u32,
    /// Number of block types at time of caching (palette change detection).
    pub block_type_count: u32,
    /// Chunk position `[x, y, z]` for validation.
    pub position: [i32; 3],
    /// Opaque mesh data.
    pub opaque: CachedMeshData,
    /// Water mesh data (empty positions = no water mesh).
    pub water: Option<CachedMeshData>,
}

/// Manages reading and writing cached chunk meshes to disk.
///
/// Insert as a Bevy `Resource` to enable mesh caching.
#[derive(Resource, Clone, Debug)]
pub struct ChunkMeshCache {
    /// Root directory for mesh cache files.
    pub cache_dir: PathBuf,
    /// Whether caching is enabled.
    pub enabled: bool,
}

impl Default for ChunkMeshCache {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("saves/default/.mesh_cache"),
            enabled: false,
        }
    }
}

impl ChunkMeshCache {
    /// Create a new cache rooted under the given save directory.
    pub fn new(save_dir: &Path, enabled: bool) -> Self {
        Self {
            cache_dir: save_dir.join(".mesh_cache"),
            enabled,
        }
    }

    /// File path for a chunk's cached mesh.
    fn mesh_path(&self, chunk_pos: IVec3) -> PathBuf {
        self.cache_dir.join(format!(
            "mesh_{}_{}_{}.bin",
            chunk_pos.x, chunk_pos.y, chunk_pos.z
        ))
    }

    /// Cache a chunk's mesh data to disk.
    ///
    /// `block_type_count` is the current number of block type variants,
    /// used for invalidation when the palette changes.
    pub fn cache_mesh(
        &self,
        chunk_pos: IVec3,
        opaque: &Mesh,
        water: Option<&Mesh>,
        block_type_count: u32,
    ) -> Result<(), io::Error> {
        if !self.enabled {
            return Ok(());
        }

        let opaque_data = CachedMeshData::from_mesh(opaque).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Failed to extract opaque mesh data")
        })?;

        let water_data = water
            .and_then(CachedMeshData::from_mesh);

        let cached = CachedChunkMesh {
            format_version: MESH_CACHE_VERSION,
            block_type_count,
            position: [chunk_pos.x, chunk_pos.y, chunk_pos.z],
            opaque: opaque_data,
            water: water_data,
        };

        fs::create_dir_all(&self.cache_dir)?;

        let bytes = bincode::serialize(&cached)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        fs::write(self.mesh_path(chunk_pos), bytes)
    }

    /// Load a cached mesh for the given chunk position.
    ///
    /// Returns `None` if:
    /// - Cache is disabled
    /// - No cached file exists
    /// - Format version or block type count doesn't match (stale cache)
    /// - Deserialization fails
    pub fn load_cached_mesh(
        &self,
        chunk_pos: IVec3,
        expected_block_type_count: u32,
    ) -> Option<(Mesh, Option<Mesh>)> {
        if !self.enabled {
            return None;
        }

        let path = self.mesh_path(chunk_pos);
        let bytes = fs::read(&path).ok()?;

        let cached: CachedChunkMesh = bincode::deserialize(&bytes).ok()?;

        // Validate version and palette
        if cached.format_version != MESH_CACHE_VERSION {
            debug!(
                "Mesh cache version mismatch for {:?}: expected {}, got {}",
                chunk_pos, MESH_CACHE_VERSION, cached.format_version
            );
            // Remove stale file
            let _ = fs::remove_file(&path);
            return None;
        }

        if cached.block_type_count != expected_block_type_count {
            debug!(
                "Mesh cache block type count mismatch for {:?}: expected {}, got {}",
                chunk_pos, expected_block_type_count, cached.block_type_count
            );
            let _ = fs::remove_file(&path);
            return None;
        }

        let opaque = cached.opaque.to_mesh();
        let water = cached.water
            .filter(|w| !w.is_empty())
            .map(|w| w.to_mesh());

        Some((opaque, water))
    }

    /// Invalidate (remove) the cached mesh for a specific chunk.
    pub fn invalidate_cache(&self, chunk_pos: IVec3) {
        let path = self.mesh_path(chunk_pos);
        let _ = fs::remove_file(path);
    }

    /// Invalidate all cached meshes (e.g., after a world format update).
    pub fn invalidate_all(&self) -> io::Result<()> {
        if self.cache_dir.exists() {
            fs::remove_dir_all(&self.cache_dir)?;
        }
        Ok(())
    }
}

/// The current number of BlockType variants. Used for cache invalidation
/// when new block types are added.
pub const BLOCK_TYPE_COUNT: u32 = 19; // Air through GoldOre

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_cache() -> ChunkMeshCache {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "pw_mesh_cache_test_{}_{}", 
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            unique,
        ));
        ChunkMeshCache {
            cache_dir: dir,
            enabled: true,
        }
    }

    fn cleanup(cache: &ChunkMeshCache) {
        let _ = fs::remove_dir_all(&cache.cache_dir);
    }

    /// Create a simple test mesh with known data.
    fn make_test_mesh(with_uv1: bool) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![
            [0.0_f32, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![
            [0.0_f32, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![
            [1.0_f32, 1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
        ]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![
            [0.0_f32, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ]);
        if with_uv1 {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![
                [0.0_f32, 0.0],
                [1.0, 0.0],
                [1.0, 1.0],
                [0.0, 1.0],
            ]);
        }
        mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
        mesh
    }

    #[test]
    fn test_cache_disabled_noop() {
        let cache = ChunkMeshCache {
            cache_dir: PathBuf::from("/tmp/disabled"),
            enabled: false,
        };
        let mesh = make_test_mesh(false);
        // Should succeed (no-op) when disabled
        assert!(cache.cache_mesh(IVec3::ZERO, &mesh, None, BLOCK_TYPE_COUNT).is_ok());
        // Should return None when disabled
        assert!(cache.load_cached_mesh(IVec3::ZERO, BLOCK_TYPE_COUNT).is_none());
    }

    #[test]
    fn test_cache_round_trip_opaque_only() {
        let cache = temp_cache();
        let mesh = make_test_mesh(false);

        cache.cache_mesh(IVec3::new(1, 2, 3), &mesh, None, BLOCK_TYPE_COUNT)
            .expect("cache should succeed");

        let result = cache.load_cached_mesh(IVec3::new(1, 2, 3), BLOCK_TYPE_COUNT);
        assert!(result.is_some());

        let (opaque, water) = result.unwrap();
        assert!(water.is_none());
        assert_eq!(
            opaque.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().len(),
            4
        );

        cleanup(&cache);
    }

    #[test]
    fn test_cache_round_trip_with_water() {
        let cache = temp_cache();
        let opaque = make_test_mesh(false);
        let water = make_test_mesh(false);

        cache.cache_mesh(IVec3::ZERO, &opaque, Some(&water), BLOCK_TYPE_COUNT)
            .expect("cache should succeed");

        let result = cache.load_cached_mesh(IVec3::ZERO, BLOCK_TYPE_COUNT);
        assert!(result.is_some());

        let (_, water_mesh) = result.unwrap();
        assert!(water_mesh.is_some());

        cleanup(&cache);
    }

    #[test]
    fn test_cache_round_trip_with_uv1() {
        let cache = temp_cache();
        let mesh = make_test_mesh(true);

        cache.cache_mesh(IVec3::ZERO, &mesh, None, BLOCK_TYPE_COUNT)
            .expect("cache should succeed");

        let (loaded, _) = cache.load_cached_mesh(IVec3::ZERO, BLOCK_TYPE_COUNT).unwrap();
        assert!(loaded.attribute(Mesh::ATTRIBUTE_UV_1).is_some());

        cleanup(&cache);
    }

    #[test]
    fn test_cache_miss_nonexistent() {
        let cache = temp_cache();
        assert!(cache.load_cached_mesh(IVec3::new(99, 99, 99), BLOCK_TYPE_COUNT).is_none());
        cleanup(&cache);
    }

    #[test]
    fn test_cache_invalidation_version_mismatch() {
        let cache = temp_cache();
        let mesh = make_test_mesh(false);

        cache.cache_mesh(IVec3::ZERO, &mesh, None, BLOCK_TYPE_COUNT)
            .expect("cache should succeed");

        // Loading with different block type count should invalidate
        assert!(cache.load_cached_mesh(IVec3::ZERO, BLOCK_TYPE_COUNT + 1).is_none());
        // File should have been removed
        assert!(!cache.mesh_path(IVec3::ZERO).exists());

        cleanup(&cache);
    }

    #[test]
    fn test_invalidate_single_chunk() {
        let cache = temp_cache();
        let mesh = make_test_mesh(false);

        cache.cache_mesh(IVec3::ZERO, &mesh, None, BLOCK_TYPE_COUNT)
            .expect("cache should succeed");
        assert!(cache.mesh_path(IVec3::ZERO).exists());

        cache.invalidate_cache(IVec3::ZERO);
        assert!(!cache.mesh_path(IVec3::ZERO).exists());

        cleanup(&cache);
    }

    #[test]
    fn test_invalidate_all() {
        let cache = temp_cache();
        let mesh = make_test_mesh(false);

        cache.cache_mesh(IVec3::ZERO, &mesh, None, BLOCK_TYPE_COUNT).unwrap();
        cache.cache_mesh(IVec3::ONE, &mesh, None, BLOCK_TYPE_COUNT).unwrap();

        cache.invalidate_all().expect("invalidate_all should succeed");
        assert!(!cache.cache_dir.exists());

        cleanup(&cache);
    }

    #[test]
    fn test_cached_mesh_data_from_mesh_extraction() {
        let mesh = make_test_mesh(true);
        let data = CachedMeshData::from_mesh(&mesh).expect("extraction should succeed");
        assert_eq!(data.positions.len(), 4);
        assert_eq!(data.normals.len(), 4);
        assert_eq!(data.colors.len(), 4);
        assert_eq!(data.uvs.len(), 4);
        assert_eq!(data.uv1s.len(), 4);
        assert_eq!(data.indices.len(), 6);
    }
}
