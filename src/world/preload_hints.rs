//! Chunk preload hint system — explicit preload requests from gameplay code
//!
//! Provides an API for gameplay systems (camera movement, world generation
//! config panels, teleportation, etc.) to request chunk preloading in a
//! given region. The system integrates with the existing chunk manager
//! and generation pipeline, prioritising hint-requested chunks alongside
//! the standard distance-based and predictive loading.
//!
//! # Usage
//!
//! ```ignore
//! fn my_system(mut hints: ResMut<PreloadHints>) {
//!     // Preload chunks around a teleport destination
//!     hints.request(PreloadRequest {
//!         center: Vec3::new(500.0, 0.0, 300.0),
//!         radius: 64.0,
//!         priority: 10,
//!     });
//! }
//! ```
//!
//! # Integration
//!
//! The `process_preload_hints` system runs in `WorldSystems::ChunkLoading`
//! after the standard streaming systems. It drains the request queue each
//! frame, converts world-space requests into chunk positions, and spawns
//! async generation tasks for any chunks not already loaded or pending.

use bevy::prelude::*;
use std::collections::BinaryHeap;
use std::cmp::Ordering;

use super::{world_to_chunk_pos, CHUNK_SIZE};

// ============================================================================
// REQUEST TYPES
// ============================================================================

/// A request to preload chunks in a spherical region.
///
/// Gameplay code pushes these into [`PreloadHints`]. The preload system
/// converts them into chunk-coordinate positions and spawns generation
/// tasks for missing chunks.
#[derive(Debug, Clone)]
pub struct PreloadRequest {
    /// Center of the preload region in world coordinates.
    pub center: Vec3,
    /// Radius in world units (blocks). Chunks overlapping this sphere
    /// will be queued for generation.
    pub radius: f32,
    /// Priority — higher values are processed first. Requests with the
    /// same priority are processed in FIFO order.
    pub priority: u32,
}

/// Internal wrapper for priority queue ordering.
#[derive(Debug, Clone)]
struct PrioritisedRequest {
    request: PreloadRequest,
    /// Insertion sequence number for stable FIFO within same priority.
    sequence: u64,
}

impl PartialEq for PrioritisedRequest {
    fn eq(&self, other: &Self) -> bool {
        self.request.priority == other.request.priority && self.sequence == other.sequence
    }
}

impl Eq for PrioritisedRequest {}

impl PartialOrd for PrioritisedRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritisedRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first, then lower sequence (FIFO) first
        self.request
            .priority
            .cmp(&other.request.priority)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

// ============================================================================
// RESOURCE
// ============================================================================

/// Resource holding pending preload requests.
///
/// Gameplay systems push [`PreloadRequest`]s here; the `process_preload_hints`
/// system drains them each frame.
#[derive(Resource)]
pub struct PreloadHints {
    queue: BinaryHeap<PrioritisedRequest>,
    sequence_counter: u64,
    /// Maximum chunk-generation tasks the hint system may spawn per frame.
    /// Independent of `ChunkManager::max_chunks_per_frame`.
    pub max_tasks_per_frame: u32,
}

impl Default for PreloadHints {
    fn default() -> Self {
        Self {
            queue: BinaryHeap::new(),
            sequence_counter: 0,
            max_tasks_per_frame: 4,
        }
    }
}

impl PreloadHints {
    /// Submit a preload request.
    ///
    /// The request will be processed on the next frame. Higher-priority
    /// requests are handled first.
    pub fn request(&mut self, req: PreloadRequest) {
        let seq = self.sequence_counter;
        self.sequence_counter += 1;
        self.queue.push(PrioritisedRequest {
            request: req,
            sequence: seq,
        });
    }

    /// Number of pending requests.
    pub fn pending_count(&self) -> usize {
        self.queue.len()
    }

    /// Returns `true` if there are no pending requests.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Drain all pending requests in priority order (highest first).
    pub fn drain_sorted(&mut self) -> Vec<PreloadRequest> {
        let mut out = Vec::with_capacity(self.queue.len());
        while let Some(entry) = self.queue.pop() {
            out.push(entry.request);
        }
        out
    }

    /// Clear all pending requests without processing them.
    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

// ============================================================================
// CHUNK POSITION EXPANSION
// ============================================================================

/// Expand a [`PreloadRequest`] into the set of chunk positions it covers.
///
/// Computes the axis-aligned bounding box of the sphere in chunk coordinates,
/// then filters to chunks whose centers fall within the radius.
pub fn expand_request_to_chunks(request: &PreloadRequest) -> Vec<IVec3> {
    let radius = request.radius.max(0.0);
    let chunk_size_f = CHUNK_SIZE as f32;

    // Bounding box in chunk coordinates
    let min_chunk = world_to_chunk_pos(request.center - Vec3::splat(radius));
    let max_chunk = world_to_chunk_pos(request.center + Vec3::splat(radius));

    let radius_in_chunks = radius / chunk_size_f;
    let radius_sq = radius_in_chunks * radius_in_chunks;

    let center_chunk = Vec3::new(
        request.center.x / chunk_size_f,
        request.center.y / chunk_size_f,
        request.center.z / chunk_size_f,
    );

    let mut positions = Vec::new();

    for x in min_chunk.x..=max_chunk.x {
        for y in min_chunk.y..=max_chunk.y {
            for z in min_chunk.z..=max_chunk.z {
                let chunk_center = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                let diff = chunk_center - center_chunk;
                if diff.length_squared() <= radius_sq {
                    positions.push(IVec3::new(x, y, z));
                }
            }
        }
    }

    positions
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preload_request_creation() {
        let req = PreloadRequest {
            center: Vec3::new(100.0, 0.0, 200.0),
            radius: 48.0,
            priority: 5,
        };
        assert_eq!(req.priority, 5);
        assert_eq!(req.radius, 48.0);
    }

    #[test]
    fn test_hints_default_empty() {
        let hints = PreloadHints::default();
        assert!(hints.is_empty());
        assert_eq!(hints.pending_count(), 0);
    }

    #[test]
    fn test_hints_push_and_count() {
        let mut hints = PreloadHints::default();
        hints.request(PreloadRequest {
            center: Vec3::ZERO,
            radius: 32.0,
            priority: 1,
        });
        hints.request(PreloadRequest {
            center: Vec3::X * 100.0,
            radius: 16.0,
            priority: 2,
        });
        assert_eq!(hints.pending_count(), 2);
        assert!(!hints.is_empty());
    }

    #[test]
    fn test_hints_drain_priority_order() {
        let mut hints = PreloadHints::default();

        hints.request(PreloadRequest {
            center: Vec3::ZERO,
            radius: 16.0,
            priority: 1, // low
        });
        hints.request(PreloadRequest {
            center: Vec3::X * 100.0,
            radius: 16.0,
            priority: 10, // high
        });
        hints.request(PreloadRequest {
            center: Vec3::Z * 50.0,
            radius: 16.0,
            priority: 5, // medium
        });

        let drained = hints.drain_sorted();
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].priority, 10);
        assert_eq!(drained[1].priority, 5);
        assert_eq!(drained[2].priority, 1);
        assert!(hints.is_empty());
    }

    #[test]
    fn test_hints_fifo_within_same_priority() {
        let mut hints = PreloadHints::default();

        let c1 = Vec3::new(1.0, 0.0, 0.0);
        let c2 = Vec3::new(2.0, 0.0, 0.0);
        let c3 = Vec3::new(3.0, 0.0, 0.0);

        hints.request(PreloadRequest { center: c1, radius: 16.0, priority: 5 });
        hints.request(PreloadRequest { center: c2, radius: 16.0, priority: 5 });
        hints.request(PreloadRequest { center: c3, radius: 16.0, priority: 5 });

        let drained = hints.drain_sorted();
        assert_eq!(drained[0].center, c1);
        assert_eq!(drained[1].center, c2);
        assert_eq!(drained[2].center, c3);
    }

    #[test]
    fn test_hints_clear() {
        let mut hints = PreloadHints::default();
        hints.request(PreloadRequest {
            center: Vec3::ZERO,
            radius: 16.0,
            priority: 1,
        });
        hints.clear();
        assert!(hints.is_empty());
    }

    #[test]
    fn test_expand_single_chunk() {
        // Center at origin with tiny radius — should yield at least the origin chunk
        let req = PreloadRequest {
            center: Vec3::new(8.0, 8.0, 8.0), // center of chunk (0,0,0)
            radius: 1.0,
            priority: 1,
        };
        let chunks = expand_request_to_chunks(&req);
        assert!(!chunks.is_empty());
        assert!(chunks.contains(&IVec3::ZERO));
    }

    #[test]
    fn test_expand_covers_expected_area() {
        // 32-unit radius ≈ 2 chunks, centered at origin
        let req = PreloadRequest {
            center: Vec3::ZERO,
            radius: 32.0,
            priority: 1,
        };
        let chunks = expand_request_to_chunks(&req);

        // Should include chunks near origin
        assert!(chunks.contains(&IVec3::ZERO));
        assert!(chunks.contains(&IVec3::new(1, 0, 0)));
        assert!(chunks.contains(&IVec3::new(-1, 0, 0)));

        // Should NOT include very distant chunks
        assert!(!chunks.contains(&IVec3::new(10, 0, 0)));
    }

    #[test]
    fn test_expand_zero_radius() {
        let req = PreloadRequest {
            center: Vec3::new(8.0, 8.0, 8.0),
            radius: 0.0,
            priority: 1,
        };
        let chunks = expand_request_to_chunks(&req);
        // Zero radius — might get the containing chunk or nothing, but shouldn't panic
        assert!(chunks.len() <= 1);
    }

    #[test]
    fn test_expand_negative_coordinates() {
        let req = PreloadRequest {
            center: Vec3::new(-50.0, 0.0, -50.0),
            radius: 32.0,
            priority: 1,
        };
        let chunks = expand_request_to_chunks(&req);
        assert!(!chunks.is_empty());

        // All chunks should be in the negative quadrant area
        let center_chunk = world_to_chunk_pos(req.center);
        let has_center = chunks.contains(&center_chunk);
        assert!(has_center, "Should contain the center chunk {:?}", center_chunk);
    }

    #[test]
    fn test_expand_large_radius_reasonable_count() {
        let req = PreloadRequest {
            center: Vec3::ZERO,
            radius: 64.0, // 4 chunks
            priority: 1,
        };
        let chunks = expand_request_to_chunks(&req);
        // Sphere of ~4 chunk radius: volume ~= 4/3 * pi * 4^3 ≈ 268 chunks
        // Actual will be fewer due to discrete grid, but should be substantial
        assert!(chunks.len() > 10, "Expected many chunks, got {}", chunks.len());
        assert!(chunks.len() < 1000, "Too many chunks: {}", chunks.len());
    }
}
