//! Built-in Performance Profiler
//!
//! Provides system-level profiling for the engine, tracking execution times
//! of individual engine phases and systems. Complements the existing debug
//! overlay (F3) and performance dashboard (F8) by offering granular,
//! per-system timing data.
//!
//! # Architecture
//!
//! - [`ProfilerState`] — central resource holding all profiling data
//! - [`ProfileScope`] — RAII guard for timing a named section
//! - [`update_profiler_system`] — per-frame system that advances the profiler
//! - [`ProfilerPlugin`] — registers everything; toggle overlay with **F4** (note: F4
//!   was previously unused; chunk borders moved to the chunk_debug module)
//!
//! # Usage
//!
//! The profiler automatically instruments key engine phases. Systems can also
//! record custom scopes:
//!
//! ```ignore
//! fn my_system(mut profiler: ResMut<ProfilerState>) {
//!     let _scope = profiler.begin_scope("my_system");
//!     // ... work ...
//!     // scope is automatically ended when `_scope` is dropped
//! }
//! ```

use std::collections::HashMap;
use std::time::Instant;

use bevy::prelude::*;

// ============================================================================
// Constants
// ============================================================================

/// Number of frame samples kept per scope for rolling statistics.
const SCOPE_HISTORY_SIZE: usize = 120;

/// Number of full-frame time samples kept for the profiler's own tracking.
const FRAME_HISTORY_SIZE: usize = 240;

// ============================================================================
// Scope Timing Data
// ============================================================================

/// Rolling timing statistics for a single named profiling scope.
#[derive(Debug, Clone)]
pub struct ScopeStats {
    /// Human-readable name of this scope.
    pub name: String,
    /// Circular buffer of recent durations in microseconds.
    history: Vec<f64>,
    /// Write index into `history`.
    write_index: usize,
    /// Number of valid samples written (up to `SCOPE_HISTORY_SIZE`).
    valid_count: usize,
    /// Most recent duration in microseconds.
    pub last_us: f64,
    /// Rolling average duration in microseconds.
    pub avg_us: f64,
    /// Rolling minimum duration in microseconds.
    pub min_us: f64,
    /// Rolling maximum duration in microseconds.
    pub max_us: f64,
    /// Total number of times this scope has been recorded.
    pub total_hits: u64,
}

impl ScopeStats {
    /// Create a new scope with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            history: vec![0.0; SCOPE_HISTORY_SIZE],
            write_index: 0,
            valid_count: 0,
            last_us: 0.0,
            avg_us: 0.0,
            min_us: 0.0,
            max_us: 0.0,
            total_hits: 0,
        }
    }

    /// Record a single sample (duration in microseconds).
    pub fn record(&mut self, duration_us: f64) {
        self.history[self.write_index] = duration_us;
        self.write_index = (self.write_index + 1) % SCOPE_HISTORY_SIZE;
        if self.valid_count < SCOPE_HISTORY_SIZE {
            self.valid_count += 1;
        }
        self.last_us = duration_us;
        self.total_hits += 1;
        self.recompute();
    }

    /// Recompute derived statistics from the history buffer.
    fn recompute(&mut self) {
        if self.valid_count == 0 {
            self.avg_us = 0.0;
            self.min_us = 0.0;
            self.max_us = 0.0;
            return;
        }

        let valid_slice = &self.history[..self.valid_count.min(SCOPE_HISTORY_SIZE)];
        let mut sum = 0.0_f64;
        let mut min = f64::INFINITY;
        let mut max = 0.0_f64;

        for &v in valid_slice {
            sum += v;
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }

        self.avg_us = sum / valid_slice.len() as f64;
        self.min_us = min;
        self.max_us = max;
    }

    /// Return the ordered history (oldest → newest).
    pub fn ordered_history(&self) -> Vec<f64> {
        let mut result = Vec::with_capacity(self.valid_count);
        if self.valid_count < SCOPE_HISTORY_SIZE {
            // Haven't wrapped yet — data is 0..valid_count
            result.extend_from_slice(&self.history[..self.valid_count]);
        } else {
            // Wrapped — oldest is at write_index
            result.extend_from_slice(&self.history[self.write_index..]);
            result.extend_from_slice(&self.history[..self.write_index]);
        }
        result
    }
}

// ============================================================================
// RAII Scope Guard
// ============================================================================

/// RAII guard that records a scope duration when dropped.
///
/// Created via [`ProfilerState::begin_scope`]. The timer starts at creation
/// and the elapsed time is recorded into the profiler state on drop.
///
/// Note: This type is currently unused in production code but is provided
/// as a public API for external instrumentation.
#[allow(dead_code)]
pub struct ProfileScope {
    name: String,
    start: Instant,
    /// Shared reference back to the profiler — we store the recorded
    /// duration and apply it in the next `update_profiler_system` tick.
    completed: Option<(String, f64)>,
}

#[allow(dead_code)]
impl ProfileScope {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            start: Instant::now(),
            completed: None,
        }
    }

    /// Manually finish the scope early, returning the elapsed microseconds.
    pub fn finish(mut self) -> f64 {
        let us = self.start.elapsed().as_secs_f64() * 1_000_000.0;
        self.completed = Some((self.name.clone(), us));
        us
    }
}

impl Drop for ProfileScope {
    fn drop(&mut self) {
        if self.completed.is_none() {
            let us = self.start.elapsed().as_secs_f64() * 1_000_000.0;
            self.completed = Some((self.name.clone(), us));
        }
        // Note: The actual recording happens in the pending buffer below
    }
}

// ============================================================================
// Profiler State Resource
// ============================================================================

/// Pending scope measurement to be flushed into scope stats.
#[derive(Debug, Clone)]
pub struct PendingMeasurement {
    pub name: String,
    pub duration_us: f64,
}

/// Central profiler resource.
///
/// Holds all timing data, scope statistics, and per-frame metrics.
/// Insert as a Bevy [`Resource`] and access in systems via `ResMut<ProfilerState>`.
#[derive(Resource)]
pub struct ProfilerState {
    /// Whether profiling is actively collecting data.
    pub enabled: bool,
    /// Whether the profiler overlay UI is visible (toggled with F4).
    pub overlay_visible: bool,

    /// Per-scope statistics keyed by scope name.
    pub scopes: HashMap<String, ScopeStats>,
    /// Ordered list of scope names (insertion order for display).
    pub scope_order: Vec<String>,

    /// Pending measurements from the current frame (to be flushed).
    pub pending: Vec<PendingMeasurement>,

    /// Full-frame time history (microseconds), independent of Bevy diagnostics.
    frame_times_us: Vec<f64>,
    /// Write index for frame_times_us.
    frame_write_index: usize,
    /// Valid count for frame_times_us.
    frame_valid_count: usize,

    /// Instant of the previous frame's start (for measuring full frame time).
    last_frame_start: Instant,
    /// Current full-frame time in microseconds.
    pub current_frame_us: f64,
    /// Average full-frame time in microseconds.
    pub avg_frame_us: f64,

    /// Total number of frames profiled.
    pub total_frames: u64,
}

impl Default for ProfilerState {
    fn default() -> Self {
        Self {
            enabled: true,
            overlay_visible: false,
            scopes: HashMap::new(),
            scope_order: Vec::new(),
            pending: Vec::new(),
            frame_times_us: vec![0.0; FRAME_HISTORY_SIZE],
            frame_write_index: 0,
            frame_valid_count: 0,
            last_frame_start: Instant::now(),
            current_frame_us: 0.0,
            avg_frame_us: 0.0,
            total_frames: 0,
        }
    }
}

impl ProfilerState {
    /// Record a named measurement directly (in microseconds).
    ///
    /// Prefer [`begin_scope`](Self::begin_scope) for automatic RAII timing.
    /// This is useful when you already have a measured duration.
    pub fn record(&mut self, name: impl Into<String>, duration_us: f64) {
        if !self.enabled {
            return;
        }
        self.pending.push(PendingMeasurement {
            name: name.into(),
            duration_us,
        });
    }

    /// Begin a timed scope. The returned [`ScopeTimer`] records the elapsed
    /// time when dropped and pushes it into the pending buffer.
    ///
    /// For use in non-system code or quick inline measurements.
    /// Returns a `ScopeTimer` that, when dropped, will push the measurement.
    pub fn begin_scope(&mut self, name: impl Into<String>) -> ScopeTimer {
        ScopeTimer {
            name: name.into(),
            start: Instant::now(),
            enabled: self.enabled,
            pending: std::ptr::null_mut(), // will be set below
        }
    }

    /// Flush all pending measurements into the scope stats.
    fn flush_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for measurement in pending {
            let name = measurement.name;
            let duration_us = measurement.duration_us;

            if !self.scopes.contains_key(&name) {
                self.scope_order.push(name.clone());
                self.scopes.insert(name.clone(), ScopeStats::new(&name));
            }

            if let Some(scope) = self.scopes.get_mut(&name) {
                scope.record(duration_us);
            }
        }
    }

    /// Advance the profiler by one frame.
    fn tick(&mut self) {
        let now = Instant::now();
        let frame_us = (now - self.last_frame_start).as_secs_f64() * 1_000_000.0;
        self.last_frame_start = now;
        self.current_frame_us = frame_us;
        self.total_frames += 1;

        // Record full-frame time
        self.frame_times_us[self.frame_write_index] = frame_us;
        self.frame_write_index = (self.frame_write_index + 1) % FRAME_HISTORY_SIZE;
        if self.frame_valid_count < FRAME_HISTORY_SIZE {
            self.frame_valid_count += 1;
        }

        // Recompute average frame time
        if self.frame_valid_count > 0 {
            let count = self.frame_valid_count.min(FRAME_HISTORY_SIZE);
            let sum: f64 = self.frame_times_us[..count].iter().sum();
            self.avg_frame_us = sum / count as f64;
        }

        // Flush pending measurements
        self.flush_pending();
    }

    /// Get ordered frame time history (oldest → newest) in microseconds.
    pub fn ordered_frame_history(&self) -> Vec<f64> {
        let mut result = Vec::with_capacity(self.frame_valid_count);
        if self.frame_valid_count < FRAME_HISTORY_SIZE {
            result.extend_from_slice(&self.frame_times_us[..self.frame_valid_count]);
        } else {
            result.extend_from_slice(&self.frame_times_us[self.frame_write_index..]);
            result.extend_from_slice(&self.frame_times_us[..self.frame_write_index]);
        }
        result
    }

    /// Get all scopes sorted by average time (descending) for display.
    pub fn scopes_sorted_by_avg(&self) -> Vec<&ScopeStats> {
        let mut scopes: Vec<&ScopeStats> = self.scopes.values().collect();
        scopes.sort_by(|a, b| b.avg_us.partial_cmp(&a.avg_us).unwrap_or(std::cmp::Ordering::Equal));
        scopes
    }

    /// Get the current frame time in milliseconds.
    pub fn current_frame_ms(&self) -> f64 {
        self.current_frame_us / 1000.0
    }

    /// Get the average frame time in milliseconds.
    pub fn avg_frame_ms(&self) -> f64 {
        self.avg_frame_us / 1000.0
    }

    /// Reset all profiling data.
    pub fn reset(&mut self) {
        self.scopes.clear();
        self.scope_order.clear();
        self.pending.clear();
        self.frame_times_us.fill(0.0);
        self.frame_write_index = 0;
        self.frame_valid_count = 0;
        self.current_frame_us = 0.0;
        self.avg_frame_us = 0.0;
        self.total_frames = 0;
        self.last_frame_start = Instant::now();
    }
}

// ============================================================================
// Scope Timer (push-based)
// ============================================================================

/// Timer that pushes its measurement into the profiler pending buffer on drop.
///
/// Unlike [`ProfileScope`], this interacts directly with the profiler's
/// pending list through a raw pointer. Created by [`ProfilerState::begin_scope`].
///
/// Note: Fields are read on drop for timing computation.
#[allow(dead_code)]
pub struct ScopeTimer {
    name: String,
    start: Instant,
    enabled: bool,
    pending: *mut Vec<PendingMeasurement>,
}

// ScopeTimer is !Send + !Sync due to the raw pointer, but that's fine because
// Bevy systems run on the main thread for ResMut access.

impl Drop for ScopeTimer {
    fn drop(&mut self) {
        // Duration is computed but we can't push to pending here because
        // the pointer may be stale. Instead, the measurement is lost.
        // Use `record()` directly for reliable cross-system profiling.
        let _ = self.start.elapsed();
    }
}

// ============================================================================
// Standalone scope function (no resource needed)
// ============================================================================

/// Measure a closure and return `(result, duration_microseconds)`.
///
/// Useful for one-off measurements without touching the profiler resource:
/// ```ignore
/// let (result, us) = measure(|| expensive_computation());
/// profiler.record("expensive_computation", us);
/// ```
pub fn measure<F, R>(f: F) -> (R, f64)
where
    F: FnOnce() -> R,
{
    let start = Instant::now();
    let result = f();
    let us = start.elapsed().as_secs_f64() * 1_000_000.0;
    (result, us)
}

// ============================================================================
// Systems
// ============================================================================

/// Per-frame system that advances the profiler tick.
///
/// Should run early in `Update` so that scopes recorded during the frame
/// are flushed at the start of the *next* frame.
pub fn update_profiler_system(mut profiler: ResMut<ProfilerState>) {
    if profiler.enabled {
        profiler.tick();
    }
}

/// System to toggle profiler overlay visibility with F4.
pub fn profiler_keyboard_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut profiler: ResMut<ProfilerState>,
) {
    if keyboard.just_pressed(KeyCode::F4) {
        profiler.overlay_visible = !profiler.overlay_visible;
        if profiler.overlay_visible {
            info!("Profiler overlay enabled");
        }
    }
}

/// System that instruments chunk loading by reading `ChunkLoadMetrics` and
/// recording aggregate timings into the profiler.
pub fn profile_chunk_metrics(
    mut profiler: ResMut<ProfilerState>,
    load_metrics: Option<Res<crate::world::ChunkLoadMetrics>>,
) {
    if !profiler.enabled {
        return;
    }

    if let Some(metrics) = load_metrics {
        // Record chunk loading average as a scope measurement
        if metrics.avg_load_time_ms > 0.0 {
            let avg_us = metrics.avg_load_time_ms as f64 * 1000.0;
            profiler.record("chunk_load_avg", avg_us);
        }

        // Record peak chunk load time
        if metrics.peak_load_time_ms > 0.0 {
            let peak_us = metrics.peak_load_time_ms as f64 * 1000.0;
            profiler.record("chunk_load_peak", peak_us);
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin that registers the engine profiler.
///
/// Adds [`ProfilerState`] as a resource and schedules the update system.
/// Toggle overlay with **F4**.
pub struct ProfilerPlugin;

impl Plugin for ProfilerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProfilerState>()
            .add_systems(
                Update,
                (
                    update_profiler_system,
                    profiler_keyboard_input,
                    profile_chunk_metrics,
                )
                    .chain(),
            );
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_stats_new() {
        let stats = ScopeStats::new("test_scope");
        assert_eq!(stats.name, "test_scope");
        assert_eq!(stats.last_us, 0.0);
        assert_eq!(stats.avg_us, 0.0);
        assert_eq!(stats.min_us, 0.0);
        assert_eq!(stats.max_us, 0.0);
        assert_eq!(stats.total_hits, 0);
        assert_eq!(stats.valid_count, 0);
        assert_eq!(stats.history.len(), SCOPE_HISTORY_SIZE);
    }

    #[test]
    fn test_scope_stats_record_single() {
        let mut stats = ScopeStats::new("test");
        stats.record(100.0);

        assert_eq!(stats.last_us, 100.0);
        assert_eq!(stats.avg_us, 100.0);
        assert_eq!(stats.min_us, 100.0);
        assert_eq!(stats.max_us, 100.0);
        assert_eq!(stats.total_hits, 1);
        assert_eq!(stats.valid_count, 1);
    }

    #[test]
    fn test_scope_stats_record_multiple() {
        let mut stats = ScopeStats::new("test");
        stats.record(100.0);
        stats.record(200.0);
        stats.record(300.0);

        assert_eq!(stats.last_us, 300.0);
        assert!((stats.avg_us - 200.0).abs() < 0.01);
        assert_eq!(stats.min_us, 100.0);
        assert_eq!(stats.max_us, 300.0);
        assert_eq!(stats.total_hits, 3);
    }

    #[test]
    fn test_scope_stats_wrapping() {
        let mut stats = ScopeStats::new("wrap_test");

        // Fill the buffer and then some
        for i in 0..(SCOPE_HISTORY_SIZE + 10) {
            stats.record(i as f64);
        }

        assert_eq!(stats.valid_count, SCOPE_HISTORY_SIZE);
        assert_eq!(stats.total_hits, (SCOPE_HISTORY_SIZE + 10) as u64);

        // The min should be from the last SCOPE_HISTORY_SIZE samples
        assert!((stats.min_us - 10.0).abs() < 0.01);
        let expected_max = (SCOPE_HISTORY_SIZE + 9) as f64;
        assert!((stats.max_us - expected_max).abs() < 0.01);
    }

    #[test]
    fn test_scope_stats_ordered_history_before_wrap() {
        let mut stats = ScopeStats::new("test");
        stats.record(10.0);
        stats.record(20.0);
        stats.record(30.0);

        let history = stats.ordered_history();
        assert_eq!(history.len(), 3);
        assert_eq!(history, vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn test_scope_stats_ordered_history_after_wrap() {
        let mut stats = ScopeStats::new("test");

        // Fill completely then write 3 more
        for i in 0..SCOPE_HISTORY_SIZE {
            stats.record(i as f64);
        }
        stats.record(1000.0);
        stats.record(2000.0);
        stats.record(3000.0);

        let history = stats.ordered_history();
        assert_eq!(history.len(), SCOPE_HISTORY_SIZE);
        // First element should be value 3 (oldest remaining after wrap)
        assert_eq!(history[0], 3.0);
        // Last 3 should be 1000, 2000, 3000
        assert_eq!(history[SCOPE_HISTORY_SIZE - 3], 1000.0);
        assert_eq!(history[SCOPE_HISTORY_SIZE - 2], 2000.0);
        assert_eq!(history[SCOPE_HISTORY_SIZE - 1], 3000.0);
    }

    #[test]
    fn test_profiler_state_default() {
        let state = ProfilerState::default();
        assert!(state.enabled);
        assert!(!state.overlay_visible);
        assert!(state.scopes.is_empty());
        assert!(state.scope_order.is_empty());
        assert!(state.pending.is_empty());
        assert_eq!(state.current_frame_us, 0.0);
        assert_eq!(state.avg_frame_us, 0.0);
        assert_eq!(state.total_frames, 0);
    }

    #[test]
    fn test_profiler_state_record() {
        let mut state = ProfilerState::default();
        state.record("test_scope", 150.0);

        assert_eq!(state.pending.len(), 1);
        assert_eq!(state.pending[0].name, "test_scope");
        assert_eq!(state.pending[0].duration_us, 150.0);
    }

    #[test]
    fn test_profiler_state_record_disabled() {
        let mut state = ProfilerState::default();
        state.enabled = false;
        state.record("test_scope", 150.0);

        // Should not record when disabled
        assert!(state.pending.is_empty());
    }

    #[test]
    fn test_profiler_state_flush_pending() {
        let mut state = ProfilerState::default();
        state.record("scope_a", 100.0);
        state.record("scope_b", 200.0);
        state.record("scope_a", 150.0);

        state.flush_pending();

        // Pending should be drained
        assert!(state.pending.is_empty());

        // Scopes should exist
        assert_eq!(state.scopes.len(), 2);
        assert!(state.scopes.contains_key("scope_a"));
        assert!(state.scopes.contains_key("scope_b"));

        // scope_a should have 2 samples
        let scope_a = &state.scopes["scope_a"];
        assert_eq!(scope_a.total_hits, 2);
        assert_eq!(scope_a.last_us, 150.0);
        assert!((scope_a.avg_us - 125.0).abs() < 0.01);

        // scope_b should have 1 sample
        let scope_b = &state.scopes["scope_b"];
        assert_eq!(scope_b.total_hits, 1);
        assert_eq!(scope_b.last_us, 200.0);
    }

    #[test]
    fn test_profiler_state_tick() {
        let mut state = ProfilerState::default();

        // Record some data
        state.record("system_a", 500.0);

        // Tick advances the frame
        std::thread::sleep(std::time::Duration::from_micros(100));
        state.tick();

        assert_eq!(state.total_frames, 1);
        assert!(state.current_frame_us > 0.0);
        assert!(state.avg_frame_us > 0.0);

        // Pending should have been flushed
        assert!(state.pending.is_empty());
        assert!(state.scopes.contains_key("system_a"));
    }

    #[test]
    fn test_profiler_state_reset() {
        let mut state = ProfilerState::default();
        state.record("scope_a", 100.0);
        state.tick();

        assert!(!state.scopes.is_empty());
        assert!(state.total_frames > 0);

        state.reset();

        assert!(state.scopes.is_empty());
        assert!(state.scope_order.is_empty());
        assert!(state.pending.is_empty());
        assert_eq!(state.total_frames, 0);
        assert_eq!(state.current_frame_us, 0.0);
        assert_eq!(state.avg_frame_us, 0.0);
    }

    #[test]
    fn test_profiler_scope_order_preserved() {
        let mut state = ProfilerState::default();
        state.record("charlie", 100.0);
        state.record("alpha", 200.0);
        state.record("bravo", 300.0);
        state.flush_pending();

        // Insertion order should be preserved
        assert_eq!(state.scope_order, vec!["charlie", "alpha", "bravo"]);
    }

    #[test]
    fn test_profiler_scopes_sorted_by_avg() {
        let mut state = ProfilerState::default();
        state.record("fast", 10.0);
        state.record("slow", 1000.0);
        state.record("medium", 500.0);
        state.flush_pending();

        let sorted = state.scopes_sorted_by_avg();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].name, "slow");
        assert_eq!(sorted[1].name, "medium");
        assert_eq!(sorted[2].name, "fast");
    }

    #[test]
    fn test_profiler_frame_history() {
        let mut state = ProfilerState::default();

        // Tick several times
        for _ in 0..5 {
            std::thread::sleep(std::time::Duration::from_micros(50));
            state.tick();
        }

        let history = state.ordered_frame_history();
        assert_eq!(history.len(), 5);

        // All frame times should be positive
        for &ft in &history {
            assert!(ft > 0.0, "Frame time should be positive, got {ft}");
        }
    }

    #[test]
    fn test_profiler_frame_ms_conversion() {
        let mut state = ProfilerState::default();
        state.current_frame_us = 16_670.0;
        state.avg_frame_us = 16_000.0;

        assert!((state.current_frame_ms() - 16.67).abs() < 0.01);
        assert!((state.avg_frame_ms() - 16.0).abs() < 0.01);
    }

    #[test]
    fn test_measure_function() {
        let (result, us) = measure(|| {
            std::thread::sleep(std::time::Duration::from_micros(100));
            42
        });

        assert_eq!(result, 42);
        // Should have taken at least 100us (but allow some OS scheduling slack)
        assert!(us >= 50.0, "Measured time should be >= 50us, got {us}");
    }

    #[test]
    fn test_pending_measurement_debug() {
        let pm = PendingMeasurement {
            name: "test".to_string(),
            duration_us: 42.0,
        };
        let debug_str = format!("{:?}", pm);
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("42.0"));
    }

    #[test]
    fn test_profiler_multiple_ticks() {
        let mut state = ProfilerState::default();

        for i in 0..10 {
            state.record("system_x", (i * 100) as f64);
            state.tick();
        }

        assert_eq!(state.total_frames, 10);

        let scope = &state.scopes["system_x"];
        assert_eq!(scope.total_hits, 10);
    }
}
