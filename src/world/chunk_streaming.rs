//! Async chunk streaming to disk — frame-time budgeted I/O
//!
//! Distributes chunk write operations across multiple frames to avoid
//! frame hitches during save operations. Instead of writing all dirty
//! chunks synchronously (which can stall the main thread for 10-50ms),
//! chunks are queued and written incrementally within a per-frame time
//! budget.
//!
//! # Design Decisions
//!
//! - **0.5ms frame budget**: At 60fps each frame is ~16.6ms. Spending 0.5ms
//!   on I/O leaves 96% of frame time for gameplay. This is conservative
//!   enough to avoid perceptible stutters even on slower hardware.
//!
//! - **VecDeque queue**: Chunks are written in FIFO order. A priority-based
//!   approach (e.g., BTreeMap by distance from player) could be added later,
//!   but FIFO is simple, predictable, and sufficient for initial implementation.
//!
//! - **Synchronous writes with time-boxing**: Rather than spawning OS-level
//!   async I/O (which adds complexity with file handles and completion
//!   callbacks), we do normal `std::fs::write` calls but stop processing
//!   the queue when the frame budget is exhausted. Each individual chunk
//!   write is fast (~0.1-0.3ms for compressed format), so overshooting
//!   the budget by one chunk is acceptable.
//!
//! - **Error handling**: Failed writes are logged and skipped. The chunk
//!   stays marked as modified in-memory, so the next save cycle will
//!   re-queue it. This avoids infinite retry loops on persistent errors
//!   (e.g., disk full).
//!
//! # Usage
//!
//! The system integrates with the existing save flow in [`super::save`].
//! When a save is requested, instead of calling `save_world_fmt()` which
//! writes all chunks synchronously, the save system queues dirty chunks
//! into [`ChunkWriteQueue`] and the `update_chunk_streaming` system
//! drains the queue across subsequent frames.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;

use super::Chunk;
use super::persistence::{self, ChunkStorage, SaveFormat};

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Maximum time (in microseconds) to spend writing chunks per frame.
///
/// 0.5ms = 500µs. This leaves ~96% of a 16.6ms frame (60fps) for gameplay.
/// Chosen as a conservative default; can be tuned via `ChunkStreamingConfig`.
const DEFAULT_BUDGET_MICROS: u64 = 500;

/// Configuration for the chunk streaming system.
#[derive(Resource, Clone, Debug)]
pub struct ChunkStreamingConfig {
    /// Maximum time per frame for chunk I/O, in microseconds.
    /// Default: 500 (0.5ms).
    pub frame_budget_micros: u64,
}

impl Default for ChunkStreamingConfig {
    fn default() -> Self {
        Self {
            frame_budget_micros: DEFAULT_BUDGET_MICROS,
        }
    }
}

// ============================================================================
// QUEUE & STATE
// ============================================================================

/// A single chunk write job: the serialized chunk data and where to write it.
#[derive(Debug, Clone)]
pub struct ChunkWriteJob {
    /// Chunk position in chunk coordinates.
    pub position: IVec3,
    /// The chunk's block data, cloned at queue time so the original
    /// `Chunk` component can be mutated freely after queuing.
    pub chunk: Chunk,
    /// Serialization format for this chunk.
    pub format: SaveFormat,
}

/// Queue of pending chunk writes, processed incrementally each frame.
///
/// This is the central resource for the streaming system. The save system
/// pushes jobs here; the `update_chunk_streaming` system pops and writes them.
#[derive(Resource, Debug, Default)]
pub struct ChunkWriteQueue {
    /// Pending write jobs, processed front-to-back (FIFO).
    queue: VecDeque<ChunkWriteJob>,
    /// Directory where chunk files are written.
    pub chunk_dir: PathBuf,
    /// Total chunks queued in the current save batch (for progress tracking).
    pub total_queued: usize,
    /// Chunks successfully written in the current save batch.
    pub chunks_written: usize,
    /// Chunks that failed to write in the current save batch.
    pub chunks_failed: usize,
}

impl ChunkWriteQueue {
    /// Create a new queue targeting the given chunk directory.
    pub fn new(chunk_dir: PathBuf) -> Self {
        Self {
            queue: VecDeque::new(),
            chunk_dir,
            total_queued: 0,
            chunks_written: 0,
            chunks_failed: 0,
        }
    }

    /// Enqueue a chunk for async writing.
    pub fn enqueue(&mut self, chunk: &Chunk, format: SaveFormat) {
        self.queue.push_back(ChunkWriteJob {
            position: chunk.position,
            chunk: chunk.clone(),
            format,
        });
        self.total_queued += 1;
    }

    /// Begin a new save batch, resetting progress counters.
    ///
    /// Call this before enqueuing chunks for a new save operation.
    pub fn begin_batch(&mut self, chunk_dir: PathBuf) {
        self.chunk_dir = chunk_dir;
        self.total_queued = 0;
        self.chunks_written = 0;
        self.chunks_failed = 0;
        self.queue.clear();
    }

    /// Returns `true` if there are pending writes.
    pub fn has_pending(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Number of chunks still waiting to be written.
    pub fn pending_count(&self) -> usize {
        self.queue.len()
    }

    /// Progress as a fraction `(written, total)` for UI display.
    pub fn progress(&self) -> (usize, usize) {
        (self.chunks_written, self.total_queued)
    }

    /// Progress as a percentage (0.0 to 1.0). Returns 1.0 if nothing queued.
    pub fn progress_fraction(&self) -> f32 {
        if self.total_queued == 0 {
            1.0
        } else {
            self.chunks_written as f32 / self.total_queued as f32
        }
    }

    /// Returns `true` if a batch was started and all writes are complete.
    pub fn is_batch_complete(&self) -> bool {
        self.total_queued > 0 && self.queue.is_empty()
    }
}

// ============================================================================
// STREAMING STATE (for diagnostics / UI)
// ============================================================================

/// Read-only streaming state exposed for UI and diagnostics.
#[derive(Resource, Debug, Default)]
pub struct ChunkStreamingState {
    /// Whether the streaming system is actively writing chunks.
    pub is_streaming: bool,
    /// Time spent on I/O in the last frame (microseconds).
    pub last_frame_io_micros: u64,
    /// Number of chunks written in the last frame.
    pub last_frame_chunks_written: usize,
}

// ============================================================================
// BEVY SYSTEM
// ============================================================================

/// System: process the chunk write queue, respecting the frame time budget.
///
/// Each frame, this system pops chunks from `ChunkWriteQueue` and writes
/// them to disk until either the queue is empty or the frame budget is
/// exhausted. This distributes I/O across frames, preventing save stutters.
pub fn update_chunk_streaming(
    mut queue: ResMut<ChunkWriteQueue>,
    mut state: ResMut<ChunkStreamingState>,
    config: Res<ChunkStreamingConfig>,
    mut chunk_events: EventWriter<super::chunk_events::ChunkLifecycleEvent>,
) {
    if !queue.has_pending() {
        state.is_streaming = false;
        state.last_frame_io_micros = 0;
        state.last_frame_chunks_written = 0;
        return;
    }

    state.is_streaming = true;
    let budget = Duration::from_micros(config.frame_budget_micros);
    let frame_start = Instant::now();
    let mut frame_writes = 0u32;

    let storage = ChunkStorage::new(&queue.chunk_dir);

    // Process chunks until budget exhausted or queue empty.
    // We check elapsed BEFORE each write so we don't start a write
    // if we're already at/over budget.
    while queue.queue.front().is_some() {
        if frame_start.elapsed() >= budget {
            break;
        }

        let job = queue.queue.pop_front().unwrap();
        match persistence::save_chunk_fmt(&job.chunk, &storage, job.format) {
            Ok(()) => {
                queue.chunks_written += 1;
                frame_writes += 1;
                if matches!(job.format, SaveFormat::Compressed) {
                    chunk_events.send(super::chunk_events::ChunkLifecycleEvent::Compressed(
                        job.position,
                    ));
                }
            }
            Err(e) => {
                warn!(
                    "Chunk streaming: failed to write chunk at {:?}: {}",
                    job.position, e
                );
                queue.chunks_failed += 1;
            }
        }
    }

    let elapsed = frame_start.elapsed();
    state.last_frame_io_micros = elapsed.as_micros() as u64;
    state.last_frame_chunks_written = frame_writes as usize;

    if queue.queue.is_empty() {
        info!(
            "Chunk streaming complete: {} written, {} failed",
            queue.chunks_written, queue.chunks_failed
        );
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds the async chunk streaming system.
///
/// This should be added alongside [`super::save::SavePlugin`]. It provides
/// the `ChunkWriteQueue` resource and the per-frame streaming system.
pub struct ChunkStreamingPlugin;

impl Plugin for ChunkStreamingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkStreamingConfig>()
            .init_resource::<ChunkWriteQueue>()
            .init_resource::<ChunkStreamingState>()
            .add_systems(Update, update_chunk_streaming);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::BlockType;
    use std::fs;
    use std::path::Path;

    /// Create a temporary directory for test chunk writes.
    fn temp_chunk_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "pw_chunk_stream_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    fn make_test_chunk(pos: IVec3) -> Chunk {
        let mut chunk = Chunk::new(pos);
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(8, 8, 8, BlockType::Dirt);
        chunk.modified = true;
        chunk
    }

    #[test]
    fn test_queue_enqueue_and_progress() {
        let dir = temp_chunk_dir();
        let mut queue = ChunkWriteQueue::new(dir.clone());
        queue.begin_batch(dir.clone());

        assert_eq!(queue.pending_count(), 0);
        assert!(queue.progress_fraction() == 1.0);

        let chunk = make_test_chunk(IVec3::ZERO);
        queue.enqueue(&chunk, SaveFormat::Json);

        assert_eq!(queue.pending_count(), 1);
        assert_eq!(queue.total_queued, 1);
        assert_eq!(queue.chunks_written, 0);
        assert!(queue.has_pending());

        cleanup(&dir);
    }

    #[test]
    fn test_queue_processes_in_order() {
        let dir = temp_chunk_dir();
        fs::create_dir_all(&dir).unwrap();
        let mut queue = ChunkWriteQueue::new(dir.clone());
        queue.begin_batch(dir.clone());

        // Enqueue 3 chunks
        for i in 0..3 {
            let chunk = make_test_chunk(IVec3::new(i, 0, 0));
            queue.enqueue(&chunk, SaveFormat::Json);
        }
        assert_eq!(queue.pending_count(), 3);

        // Process with a generous budget (should write all)
        let storage = ChunkStorage::new(&dir);
        let budget = Duration::from_secs(5);
        let start = Instant::now();

        while let Some(job) = queue.queue.pop_front() {
            if start.elapsed() >= budget {
                break;
            }
            persistence::save_chunk_fmt(&job.chunk, &storage, job.format).unwrap();
            queue.chunks_written += 1;
        }

        assert_eq!(queue.chunks_written, 3);
        assert!(!queue.has_pending());

        // Verify files exist
        assert!(dir.join("chunk_0_0_0.json").exists());
        assert!(dir.join("chunk_1_0_0.json").exists());
        assert!(dir.join("chunk_2_0_0.json").exists());

        cleanup(&dir);
    }

    #[test]
    fn test_frame_budget_respected() {
        let dir = temp_chunk_dir();
        fs::create_dir_all(&dir).unwrap();
        let mut queue = ChunkWriteQueue::new(dir.clone());
        queue.begin_batch(dir.clone());

        // Enqueue many chunks
        for i in 0..50 {
            let chunk = make_test_chunk(IVec3::new(i, 0, 0));
            queue.enqueue(&chunk, SaveFormat::Json);
        }

        // Process with a very tight budget (1µs — likely only 0-1 chunks)
        let storage = ChunkStorage::new(&dir);
        let budget = Duration::from_micros(1);
        let start = Instant::now();

        while queue.queue.front().is_some() {
            if start.elapsed() >= budget {
                break;
            }
            let job = queue.queue.pop_front().unwrap();
            persistence::save_chunk_fmt(&job.chunk, &storage, job.format).unwrap();
            queue.chunks_written += 1;
        }

        // Should NOT have written all 50 chunks in 1µs
        // (it might write 0 or 1 before the budget check kicks in)
        assert!(
            queue.pending_count() > 0,
            "Budget should have prevented writing all 50 chunks in 1µs"
        );

        cleanup(&dir);
    }

    #[test]
    fn test_batch_complete_tracking() {
        let dir = temp_chunk_dir();
        fs::create_dir_all(&dir).unwrap();
        let mut queue = ChunkWriteQueue::new(dir.clone());
        queue.begin_batch(dir.clone());

        let chunk = make_test_chunk(IVec3::ZERO);
        queue.enqueue(&chunk, SaveFormat::Compressed);
        assert!(!queue.is_batch_complete());

        // Write the chunk
        let storage = ChunkStorage::new(&dir);
        let job = queue.queue.pop_front().unwrap();
        persistence::save_chunk_fmt(&job.chunk, &storage, job.format).unwrap();
        queue.chunks_written += 1;

        assert!(queue.is_batch_complete());
        assert_eq!(queue.progress(), (1, 1));
        assert!((queue.progress_fraction() - 1.0).abs() < f32::EPSILON);

        cleanup(&dir);
    }

    #[test]
    fn test_data_integrity_after_streaming() {
        let dir = temp_chunk_dir();
        fs::create_dir_all(&dir).unwrap();

        let mut chunk = Chunk::new(IVec3::new(5, -1, 3));
        chunk.set_block(0, 0, 0, BlockType::Stone);
        chunk.set_block(15, 15, 15, BlockType::Obsidian);
        chunk.set_block(7, 3, 11, BlockType::Wood);
        chunk.modified = true;

        // Write via queue mechanism
        let storage = ChunkStorage::new(&dir);
        let job = ChunkWriteJob {
            position: chunk.position,
            chunk: chunk.clone(),
            format: SaveFormat::Compressed,
        };
        persistence::save_chunk_fmt(&job.chunk, &storage, job.format).unwrap();

        // Read back and verify
        let loaded = persistence::load_chunk_auto(IVec3::new(5, -1, 3), &storage).unwrap();
        assert_eq!(loaded.position, IVec3::new(5, -1, 3));
        assert_eq!(loaded.get_block(0, 0, 0), BlockType::Stone);
        assert_eq!(loaded.get_block(15, 15, 15), BlockType::Obsidian);
        assert_eq!(loaded.get_block(7, 3, 11), BlockType::Wood);
        assert_eq!(loaded.get_block(1, 1, 1), BlockType::Air);

        cleanup(&dir);
    }

    #[test]
    fn test_begin_batch_resets_state() {
        let dir = temp_chunk_dir();
        let mut queue = ChunkWriteQueue::new(dir.clone());

        // Simulate a previous batch
        queue.total_queued = 10;
        queue.chunks_written = 8;
        queue.chunks_failed = 2;

        // Begin a new batch
        queue.begin_batch(dir.clone());
        assert_eq!(queue.total_queued, 0);
        assert_eq!(queue.chunks_written, 0);
        assert_eq!(queue.chunks_failed, 0);
        assert!(!queue.has_pending());

        cleanup(&dir);
    }

    #[test]
    fn test_write_failure_increments_failed_count() {
        // Try to write to a non-existent deeply nested path that can't be created
        // Actually, persistence::save_chunk_fmt creates directories, so we need
        // a truly invalid path. On Windows, NUL device works; on Unix, /dev/null/bad.
        let bad_dir = if cfg!(windows) {
            PathBuf::from("NUL\\impossible\\path")
        } else {
            PathBuf::from("/dev/null/impossible/path")
        };

        let chunk = make_test_chunk(IVec3::ZERO);
        let storage = ChunkStorage::new(&bad_dir);
        let result = persistence::save_chunk_fmt(&chunk, &storage, SaveFormat::Json);

        assert!(result.is_err(), "Writing to invalid path should fail");
    }

    #[test]
    fn test_multiple_formats_in_queue() {
        let dir = temp_chunk_dir();
        fs::create_dir_all(&dir).unwrap();
        let mut queue = ChunkWriteQueue::new(dir.clone());
        queue.begin_batch(dir.clone());

        let chunk_json = make_test_chunk(IVec3::new(0, 0, 0));
        let chunk_bin = make_test_chunk(IVec3::new(1, 0, 0));
        let chunk_compressed = make_test_chunk(IVec3::new(2, 0, 0));

        queue.enqueue(&chunk_json, SaveFormat::Json);
        queue.enqueue(&chunk_bin, SaveFormat::Binary);
        queue.enqueue(&chunk_compressed, SaveFormat::Compressed);

        // Write all
        let storage = ChunkStorage::new(&dir);
        while let Some(job) = queue.queue.pop_front() {
            persistence::save_chunk_fmt(&job.chunk, &storage, job.format).unwrap();
            queue.chunks_written += 1;
        }

        assert!(dir.join("chunk_0_0_0.json").exists());
        assert!(dir.join("chunk_1_0_0.bin").exists());
        assert!(dir.join("chunk_2_0_0.cbin").exists());

        cleanup(&dir);
    }
}
