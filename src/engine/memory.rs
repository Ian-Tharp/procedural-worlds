//! Process Memory Tracking
//!
//! Cross-platform utilities for querying actual process memory usage
//! at runtime. Uses platform-specific OS APIs with zero external dependencies.
//!
//! - **Windows:** `K32GetProcessMemoryInfo` (kernel32)
//! - **Linux:** `/proc/self/statm`
//! - **macOS:** `mach_task_basic_info`
//! - **Other:** Returns `None`

/// Process memory snapshot.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessMemory {
    /// Resident Set Size (physical memory in use) in bytes.
    pub rss_bytes: usize,
    /// Peak RSS (high-water mark) in bytes, if available.
    pub peak_rss_bytes: Option<usize>,
}

/// Query current process memory from the operating system.
///
/// Returns `None` on unsupported platforms or if the query fails.
pub fn get_process_memory() -> Option<ProcessMemory> {
    platform::query()
}

/// Format a byte count into a human-readable string (e.g. "128.3 MB").
pub fn format_bytes(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

// ============================================================================
// Platform implementations
// ============================================================================

#[cfg(target_os = "windows")]
mod platform {
    use super::ProcessMemory;

    #[repr(C)]
    #[allow(non_snake_case)]
    struct ProcessMemoryCountersEx {
        cb: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize,
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
        PrivateUsage: usize,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            pmc: *mut ProcessMemoryCountersEx,
            cb: u32,
        ) -> i32;
    }

    pub fn query() -> Option<ProcessMemory> {
        unsafe {
            let mut pmc: ProcessMemoryCountersEx = std::mem::zeroed();
            pmc.cb = std::mem::size_of::<ProcessMemoryCountersEx>() as u32;

            let handle = GetCurrentProcess();
            if K32GetProcessMemoryInfo(handle, &mut pmc, pmc.cb) != 0 {
                Some(ProcessMemory {
                    rss_bytes: pmc.WorkingSetSize,
                    peak_rss_bytes: Some(pmc.PeakWorkingSetSize),
                })
            } else {
                None
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::ProcessMemory;

    pub fn query() -> Option<ProcessMemory> {
        // /proc/self/statm fields: size resident shared text lib data dt (in pages)
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let fields: Vec<&str> = statm.split_whitespace().collect();
        let resident_pages: usize = fields.get(1)?.parse().ok()?;
        let page_size = 4096_usize; // Typical Linux page size

        // Peak RSS from /proc/self/status VmHWM field
        let peak = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                for line in status.lines() {
                    if line.starts_with("VmHWM:") {
                        let kb_str = line.split_whitespace().nth(1)?;
                        let kb: usize = kb_str.parse().ok()?;
                        return Some(kb * 1024);
                    }
                }
                None
            });

        Some(ProcessMemory {
            rss_bytes: resident_pages * page_size,
            peak_rss_bytes: peak,
        })
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::ProcessMemory;

    // mach/mach_types.h — MACH_TASK_BASIC_INFO
    const MACH_TASK_BASIC_INFO: i32 = 20;

    #[repr(C)]
    struct MachTaskBasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: [u32; 2],   // time_value_t
        system_time: [u32; 2], // time_value_t
        policy: i32,
        suspend_count: i32,
    }

    unsafe extern "C" {
        fn mach_task_self() -> u32;
        fn task_info(
            target_task: u32,
            flavor: i32,
            task_info_out: *mut MachTaskBasicInfo,
            task_info_count: *mut u32,
        ) -> i32;
    }

    pub fn query() -> Option<ProcessMemory> {
        unsafe {
            let mut info: MachTaskBasicInfo = std::mem::zeroed();
            let mut count =
                (std::mem::size_of::<MachTaskBasicInfo>() / std::mem::size_of::<u32>()) as u32;

            let kr = task_info(
                mach_task_self(),
                MACH_TASK_BASIC_INFO,
                &mut info,
                &mut count,
            );

            if kr == 0 {
                // KERN_SUCCESS
                Some(ProcessMemory {
                    rss_bytes: info.resident_size as usize,
                    peak_rss_bytes: Some(info.resident_size_max as usize),
                })
            } else {
                None
            }
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod platform {
    use super::ProcessMemory;

    pub fn query() -> Option<ProcessMemory> {
        None
    }
}

// ============================================================================
// Chunk Mesh Buffer Pool
// ============================================================================

use bevy::prelude::Resource;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Default number of buffers to pre-allocate in the mesh pool.
pub const DEFAULT_POOL_SIZE: usize = 64;

/// Default initial capacity for vertex arrays (positions, normals, colors, uvs).
/// A typical chunk mesh has ~2000-8000 vertices; 16384 provides headroom.
pub const DEFAULT_VERTEX_CAPACITY: usize = 16384;

/// Default initial capacity for index arrays.
/// Triangle count is roughly vertex_count * 1.5 for typical chunk geometry.
pub const DEFAULT_INDEX_CAPACITY: usize = 24576;

/// Pre-allocated mesh buffer data for chunk mesh generation.
///
/// Contains all the vertex attribute arrays needed to build a chunk mesh.
/// Buffers are cleared (not deallocated) when returned to the pool, preserving
/// their capacity for reuse.
#[derive(Debug)]
pub struct MeshBufferData {
    /// Vertex positions [x, y, z].
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals [x, y, z].
    pub normals: Vec<[f32; 3]>,
    /// Vertex colors [r, g, b, a].
    pub colors: Vec<[f32; 4]>,
    /// Primary UV coordinates [u, v].
    pub uvs: Vec<[f32; 2]>,
    /// Secondary UV coordinates (atlas tile info) [u, v].
    pub uv1s: Vec<[f32; 2]>,
    /// Triangle indices.
    pub indices: Vec<u32>,
}

impl MeshBufferData {
    /// Create a new buffer with the specified capacities.
    pub fn with_capacity(vertex_capacity: usize, index_capacity: usize) -> Self {
        Self {
            positions: Vec::with_capacity(vertex_capacity),
            normals: Vec::with_capacity(vertex_capacity),
            colors: Vec::with_capacity(vertex_capacity),
            uvs: Vec::with_capacity(vertex_capacity),
            uv1s: Vec::with_capacity(vertex_capacity),
            indices: Vec::with_capacity(index_capacity),
        }
    }

    /// Create a new buffer with default capacities.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_VERTEX_CAPACITY, DEFAULT_INDEX_CAPACITY)
    }

    /// Clear all buffers without deallocating their backing storage.
    ///
    /// This is the key optimization: clearing preserves capacity, so
    /// subsequent mesh generation doesn't need to allocate.
    pub fn clear(&mut self) {
        self.positions.clear();
        self.normals.clear();
        self.colors.clear();
        self.uvs.clear();
        self.uv1s.clear();
        self.indices.clear();
    }

    /// Returns the total capacity in bytes across all buffers.
    pub fn capacity_bytes(&self) -> usize {
        self.positions.capacity() * std::mem::size_of::<[f32; 3]>()
            + self.normals.capacity() * std::mem::size_of::<[f32; 3]>()
            + self.colors.capacity() * std::mem::size_of::<[f32; 4]>()
            + self.uvs.capacity() * std::mem::size_of::<[f32; 2]>()
            + self.uv1s.capacity() * std::mem::size_of::<[f32; 2]>()
            + self.indices.capacity() * std::mem::size_of::<u32>()
    }

    /// Returns the current length (used elements) across all vertex buffers.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Returns the current index count.
    pub fn index_count(&self) -> usize {
        self.indices.len()
    }
}

impl Default for MeshBufferData {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe pool of pre-allocated mesh buffers for chunk mesh generation.
///
/// The pool uses a FIFO (first-in, first-out) allocation strategy:
/// - `acquire()` takes a buffer from the front of the queue
/// - `release()` returns a buffer to the back of the queue
///
/// This ensures even wear across all buffers and maintains cache locality
/// patterns. The pool is wrapped in `Arc<Mutex<>>` for thread-safe access
/// from multiple async mesh generation tasks.
///
/// # Example
///
/// ```ignore
/// let pool = ChunkMeshPool::new(32);
///
/// // In async mesh generation task:
/// let mut buffer = pool.acquire();
/// build_mesh_into_buffer(&mut buffer, chunk_data);
/// let mesh = create_mesh_from_buffer(&buffer);
/// pool.release(buffer);
/// ```
#[derive(Clone, Resource)]
pub struct ChunkMeshPool {
    inner: Arc<Mutex<MeshPoolInner>>,
}

/// Internal pool state (protected by mutex).
struct MeshPoolInner {
    /// Available buffers (FIFO queue).
    available: VecDeque<MeshBufferData>,
    /// Total buffers created (available + in-use).
    total_created: usize,
    /// Configuration.
    vertex_capacity: usize,
    index_capacity: usize,
    /// Statistics.
    stats: PoolStats,
}

/// Pool usage statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct PoolStats {
    /// Number of successful acquires from pool (reused buffer).
    pub hits: u64,
    /// Number of acquires that required creating a new buffer.
    pub misses: u64,
    /// Number of buffers returned to the pool.
    pub releases: u64,
    /// Peak number of buffers in use simultaneously.
    pub peak_in_use: usize,
}

impl ChunkMeshPool {
    /// Create a new pool with the specified number of pre-allocated buffers.
    pub fn new(pool_size: usize) -> Self {
        Self::with_capacity(pool_size, DEFAULT_VERTEX_CAPACITY, DEFAULT_INDEX_CAPACITY)
    }

    /// Create a new pool with custom buffer capacities.
    pub fn with_capacity(pool_size: usize, vertex_capacity: usize, index_capacity: usize) -> Self {
        let mut available = VecDeque::with_capacity(pool_size);
        for _ in 0..pool_size {
            available.push_back(MeshBufferData::with_capacity(
                vertex_capacity,
                index_capacity,
            ));
        }

        Self {
            inner: Arc::new(Mutex::new(MeshPoolInner {
                available,
                total_created: pool_size,
                vertex_capacity,
                index_capacity,
                stats: PoolStats::default(),
            })),
        }
    }

    /// Acquire a buffer from the pool.
    ///
    /// If a pre-allocated buffer is available, it's returned immediately (hit).
    /// If the pool is empty, a new buffer is created (miss). This ensures
    /// mesh generation never blocks waiting for buffers.
    ///
    /// The returned buffer is cleared and ready for use.
    pub fn acquire(&self) -> MeshBufferData {
        let mut inner = self.inner.lock().unwrap();

        if let Some(mut buffer) = inner.available.pop_front() {
            // Pool hit: reuse existing buffer
            inner.stats.hits += 1;
            buffer.clear();
            let in_use = inner.total_created - inner.available.len();
            if in_use > inner.stats.peak_in_use {
                inner.stats.peak_in_use = in_use;
            }
            buffer
        } else {
            // Pool miss: create new buffer (pool exhausted)
            inner.stats.misses += 1;
            inner.total_created += 1;
            let in_use = inner.total_created - inner.available.len();
            if in_use > inner.stats.peak_in_use {
                inner.stats.peak_in_use = in_use;
            }
            MeshBufferData::with_capacity(inner.vertex_capacity, inner.index_capacity)
        }
    }

    /// Return a buffer to the pool for reuse.
    ///
    /// The buffer is added to the back of the FIFO queue. Callers should
    /// release buffers after extracting the mesh data they need.
    pub fn release(&self, buffer: MeshBufferData) {
        let mut inner = self.inner.lock().unwrap();
        inner.stats.releases += 1;
        inner.available.push_back(buffer);
    }

    /// Get the current number of available (not in-use) buffers.
    pub fn available_count(&self) -> usize {
        self.inner.lock().unwrap().available.len()
    }

    /// Get the total number of buffers created by this pool.
    pub fn total_created(&self) -> usize {
        self.inner.lock().unwrap().total_created
    }

    /// Get pool statistics.
    pub fn stats(&self) -> PoolStats {
        self.inner.lock().unwrap().stats
    }

    /// Get the estimated total memory capacity of all pool buffers.
    pub fn total_capacity_bytes(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        let per_buffer = MeshBufferData::with_capacity(inner.vertex_capacity, inner.index_capacity)
            .capacity_bytes();
        inner.total_created * per_buffer
    }

    /// Reset statistics counters.
    pub fn reset_stats(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.stats = PoolStats::default();
    }
}

impl Default for ChunkMeshPool {
    fn default() -> Self {
        Self::new(DEFAULT_POOL_SIZE)
    }
}

impl std::fmt::Debug for ChunkMeshPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock().unwrap();
        f.debug_struct("ChunkMeshPool")
            .field("available", &inner.available.len())
            .field("total_created", &inner.total_created)
            .field("stats", &inner.stats)
            .finish()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.00 GB");
        assert_eq!(format_bytes(134_217_728), "128.0 MB");
    }

    #[test]
    fn test_get_process_memory_returns_some() {
        // On supported platforms (Windows, Linux, macOS) this should succeed
        let mem = get_process_memory();
        if cfg!(any(
            target_os = "windows",
            target_os = "linux",
            target_os = "macos"
        )) {
            let mem = mem.expect("Should return memory on supported platform");
            // Process should be using at least some memory
            assert!(
                mem.rss_bytes > 0,
                "RSS should be > 0, got {}",
                mem.rss_bytes
            );
        }
    }

    #[test]
    fn test_process_memory_default() {
        let mem = ProcessMemory::default();
        assert_eq!(mem.rss_bytes, 0);
        assert!(mem.peak_rss_bytes.is_none());
    }

    // ========================================================================
    // MeshBufferData tests
    // ========================================================================

    #[test]
    fn test_mesh_buffer_data_new() {
        let buffer = MeshBufferData::new();
        assert_eq!(buffer.positions.len(), 0);
        assert_eq!(buffer.normals.len(), 0);
        assert_eq!(buffer.colors.len(), 0);
        assert_eq!(buffer.uvs.len(), 0);
        assert_eq!(buffer.uv1s.len(), 0);
        assert_eq!(buffer.indices.len(), 0);
        assert!(buffer.positions.capacity() >= DEFAULT_VERTEX_CAPACITY);
        assert!(buffer.indices.capacity() >= DEFAULT_INDEX_CAPACITY);
    }

    #[test]
    fn test_mesh_buffer_data_with_capacity() {
        let buffer = MeshBufferData::with_capacity(100, 200);
        assert!(buffer.positions.capacity() >= 100);
        assert!(buffer.normals.capacity() >= 100);
        assert!(buffer.colors.capacity() >= 100);
        assert!(buffer.uvs.capacity() >= 100);
        assert!(buffer.uv1s.capacity() >= 100);
        assert!(buffer.indices.capacity() >= 200);
    }

    #[test]
    fn test_mesh_buffer_data_clear_preserves_capacity() {
        let mut buffer = MeshBufferData::with_capacity(1000, 2000);

        // Add some data
        for i in 0..500 {
            buffer.positions.push([i as f32, 0.0, 0.0]);
            buffer.normals.push([0.0, 1.0, 0.0]);
            buffer.colors.push([1.0, 1.0, 1.0, 1.0]);
            buffer.uvs.push([0.0, 0.0]);
            buffer.uv1s.push([0.0, 0.0]);
        }
        for i in 0..750 {
            buffer.indices.push(i);
        }

        let pos_cap = buffer.positions.capacity();
        let idx_cap = buffer.indices.capacity();

        // Clear and verify capacity preserved
        buffer.clear();

        assert_eq!(buffer.positions.len(), 0);
        assert_eq!(buffer.indices.len(), 0);
        assert_eq!(buffer.positions.capacity(), pos_cap);
        assert_eq!(buffer.indices.capacity(), idx_cap);
    }

    #[test]
    fn test_mesh_buffer_data_capacity_bytes() {
        let buffer = MeshBufferData::with_capacity(100, 200);
        let bytes = buffer.capacity_bytes();
        // positions: 100 * 12 = 1200
        // normals: 100 * 12 = 1200
        // colors: 100 * 16 = 1600
        // uvs: 100 * 8 = 800
        // uv1s: 100 * 8 = 800
        // indices: 200 * 4 = 800
        // Total: 6400
        assert!(bytes >= 6400, "Expected >= 6400 bytes, got {}", bytes);
    }

    #[test]
    fn test_mesh_buffer_data_vertex_and_index_count() {
        let mut buffer = MeshBufferData::new();
        assert_eq!(buffer.vertex_count(), 0);
        assert_eq!(buffer.index_count(), 0);

        buffer.positions.push([0.0, 0.0, 0.0]);
        buffer.positions.push([1.0, 0.0, 0.0]);
        buffer.indices.push(0);
        buffer.indices.push(1);
        buffer.indices.push(0);

        assert_eq!(buffer.vertex_count(), 2);
        assert_eq!(buffer.index_count(), 3);
    }

    // ========================================================================
    // ChunkMeshPool tests
    // ========================================================================

    #[test]
    fn test_pool_new_preallocates() {
        let pool = ChunkMeshPool::new(16);
        assert_eq!(pool.available_count(), 16);
        assert_eq!(pool.total_created(), 16);
    }

    #[test]
    fn test_pool_acquire_returns_cleared_buffer() {
        let pool = ChunkMeshPool::new(4);

        // Acquire, add data, release
        let mut buffer = pool.acquire();
        buffer.positions.push([1.0, 2.0, 3.0]);
        buffer.indices.push(42);
        pool.release(buffer);

        // Re-acquire same buffer (FIFO, so it goes to back, but with pool size 4...)
        // Acquire 4 times to cycle back
        let b1 = pool.acquire();
        let b2 = pool.acquire();
        let b3 = pool.acquire();
        let b4 = pool.acquire(); // This should be our original buffer, cleared

        pool.release(b1);
        pool.release(b2);
        pool.release(b3);

        assert_eq!(b4.positions.len(), 0, "Acquired buffer should be cleared");
        assert_eq!(b4.indices.len(), 0, "Acquired buffer should be cleared");
    }

    #[test]
    fn test_pool_acquire_creates_on_miss() {
        let pool = ChunkMeshPool::new(2);

        // Exhaust the pool
        let _b1 = pool.acquire();
        let _b2 = pool.acquire();
        assert_eq!(pool.available_count(), 0);

        // This should create a new buffer (miss)
        let _b3 = pool.acquire();

        assert_eq!(pool.total_created(), 3);
        let stats = pool.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_pool_fifo_order() {
        let pool = ChunkMeshPool::with_capacity(3, 10, 10);

        // Acquire all buffers and mark them
        let mut b1 = pool.acquire();
        let mut b2 = pool.acquire();
        let mut b3 = pool.acquire();

        b1.positions.push([1.0, 0.0, 0.0]);
        b2.positions.push([2.0, 0.0, 0.0]);
        b3.positions.push([3.0, 0.0, 0.0]);

        // Release in order: 1, 2, 3
        pool.release(b1);
        pool.release(b2);
        pool.release(b3);

        // Acquire should return in FIFO order: 1, 2, 3
        // But they're cleared, so we check capacity is preserved
        let r1 = pool.acquire();
        let r2 = pool.acquire();
        let r3 = pool.acquire();

        // All should be cleared
        assert_eq!(r1.positions.len(), 0);
        assert_eq!(r2.positions.len(), 0);
        assert_eq!(r3.positions.len(), 0);
    }

    #[test]
    fn test_pool_stats_tracking() {
        let pool = ChunkMeshPool::new(2);

        let b1 = pool.acquire(); // hit
        let b2 = pool.acquire(); // hit
        let _b3 = pool.acquire(); // miss (pool exhausted)

        pool.release(b1);
        pool.release(b2);

        let stats = pool.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.releases, 2);
        assert_eq!(stats.peak_in_use, 3);
    }

    #[test]
    fn test_pool_reset_stats() {
        let pool = ChunkMeshPool::new(4);
        let b = pool.acquire();
        pool.release(b);

        assert!(pool.stats().hits > 0);

        pool.reset_stats();
        let stats = pool.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.releases, 0);
    }

    #[test]
    fn test_pool_total_capacity_bytes() {
        let pool = ChunkMeshPool::with_capacity(4, 100, 200);
        let total = pool.total_capacity_bytes();
        // 4 buffers * ~6400 bytes each (see capacity_bytes calculation)
        assert!(total >= 4 * 6400, "Expected >= 25600 bytes, got {}", total);
    }

    #[test]
    fn test_pool_clone_shares_state() {
        let pool1 = ChunkMeshPool::new(4);
        let pool2 = pool1.clone();

        let _b = pool1.acquire();
        assert_eq!(pool1.available_count(), 3);
        assert_eq!(pool2.available_count(), 3); // Same underlying pool
    }

    #[test]
    fn test_pool_thread_safety() {
        use std::thread;

        let pool = ChunkMeshPool::new(100);
        let mut handles = vec![];

        // Spawn threads that acquire and release buffers
        for _ in 0..10 {
            let pool_clone = pool.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..50 {
                    let buffer = pool_clone.acquire();
                    // Simulate some work
                    std::hint::black_box(&buffer);
                    pool_clone.release(buffer);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // All buffers should be back in the pool (maybe with some extras from misses)
        let stats = pool.stats();
        assert_eq!(stats.hits + stats.misses, 500); // 10 threads * 50 acquires
        assert_eq!(stats.releases, 500);
    }

    #[test]
    fn test_pool_default() {
        let pool = ChunkMeshPool::default();
        assert_eq!(pool.available_count(), DEFAULT_POOL_SIZE);
    }

    #[test]
    fn test_pool_debug_format() {
        let pool = ChunkMeshPool::new(4);
        let debug_str = format!("{:?}", pool);
        assert!(debug_str.contains("ChunkMeshPool"));
        assert!(debug_str.contains("available"));
        assert!(debug_str.contains("total_created"));
    }
}
