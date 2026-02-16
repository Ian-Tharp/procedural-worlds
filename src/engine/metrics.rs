//! Performance Metrics Collection
//!
//! Centralized resource for aggregating performance data from various engine
//! subsystems. Provides configurable thresholds for warning levels and
//! integrates with Bevy's diagnostic system.
//!
//! # Integration Points
//!
//! - **Bevy Diagnostics**: Reads FPS and frame time from `FrameTimeDiagnosticsPlugin`
//! - **Memory Module**: Uses `crate::engine::memory::get_process_memory()`
//! - **World Module**: Reads chunk count from `ChunkManager` and `ChunkLoadMetrics`
//! - **Profiler**: Optionally reads from `ProfilerState` for system-level data
//!
//! # Metrics Dashboard Integration
//!
//! This module provides the core metrics infrastructure used by:
//! - F2: Minimal performance overlay (PerformanceOverlay)
//! - F4: System profiler (ProfilerState)
//! - F8: Full performance dashboard (PerformanceDashboard)

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::engine::memory;

// ============================================================================
// Warning Level
// ============================================================================

/// Visual warning level for a metric based on threshold comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WarningLevel {
    /// Metric is within acceptable range (green).
    #[default]
    Normal,
    /// Metric is approaching problematic levels (yellow).
    Warning,
    /// Metric is in critical range (red).
    Critical,
}

impl WarningLevel {
    /// Convert to an egui-compatible RGB color.
    pub fn to_rgb(&self) -> (u8, u8, u8) {
        match self {
            WarningLevel::Normal => (100, 255, 100),   // Green
            WarningLevel::Warning => (255, 255, 100),  // Yellow
            WarningLevel::Critical => (255, 100, 100), // Red
        }
    }

    /// Convert to hex color string for logging/export.
    pub fn to_hex(&self) -> &'static str {
        match self {
            WarningLevel::Normal => "#64FF64",
            WarningLevel::Warning => "#FFFF64",
            WarningLevel::Critical => "#FF6464",
        }
    }
}

// ============================================================================
// Performance Thresholds
// ============================================================================

/// Configurable thresholds for performance warning levels.
///
/// Each metric has two thresholds: one for entering warning state and one
/// for entering critical state. Values crossing these thresholds trigger
/// color-coded indicators in the overlay.
#[derive(Debug, Clone, Resource)]
pub struct PerformanceThresholds {
    // ── FPS thresholds (higher is better) ──
    /// FPS below this triggers critical (red).
    pub fps_critical: f64,
    /// FPS below this triggers warning (yellow).
    pub fps_warning: f64,

    // ── Frame time thresholds (lower is better) ──
    /// Frame time above this (ms) triggers critical.
    pub frame_time_critical_ms: f64,
    /// Frame time above this (ms) triggers warning.
    pub frame_time_warning_ms: f64,

    // ── Memory thresholds (lower is better) ──
    /// Memory usage above this (MB) triggers critical.
    pub memory_critical_mb: f64,
    /// Memory usage above this (MB) triggers warning.
    pub memory_warning_mb: f64,

    // ── Chunk loading thresholds ──
    /// Avg chunk load time above this (ms) triggers critical.
    pub chunk_load_critical_ms: f32,
    /// Avg chunk load time above this (ms) triggers warning.
    pub chunk_load_warning_ms: f32,
}

impl Default for PerformanceThresholds {
    fn default() -> Self {
        Self {
            // FPS: 60+ = normal, 30-59 = warning, <30 = critical
            fps_critical: 30.0,
            fps_warning: 60.0,

            // Frame time: <16.67ms = normal (60fps), 16.67-33.33 = warning, >33.33 = critical
            frame_time_critical_ms: 33.33,
            frame_time_warning_ms: 16.67,

            // Memory: <2GB = normal, 2-4GB = warning, >4GB = critical
            memory_critical_mb: 4096.0,
            memory_warning_mb: 2048.0,

            // Chunk loading: <20ms = normal, 20-100ms = warning, >100ms = critical
            chunk_load_critical_ms: 100.0,
            chunk_load_warning_ms: 20.0,
        }
    }
}

impl PerformanceThresholds {
    /// Evaluate FPS against thresholds.
    pub fn fps_level(&self, fps: f64) -> WarningLevel {
        if fps < self.fps_critical {
            WarningLevel::Critical
        } else if fps < self.fps_warning {
            WarningLevel::Warning
        } else {
            WarningLevel::Normal
        }
    }

    /// Evaluate frame time (ms) against thresholds.
    pub fn frame_time_level(&self, frame_time_ms: f64) -> WarningLevel {
        if frame_time_ms > self.frame_time_critical_ms {
            WarningLevel::Critical
        } else if frame_time_ms > self.frame_time_warning_ms {
            WarningLevel::Warning
        } else {
            WarningLevel::Normal
        }
    }

    /// Evaluate memory usage (MB) against thresholds.
    pub fn memory_level(&self, memory_mb: f64) -> WarningLevel {
        if memory_mb > self.memory_critical_mb {
            WarningLevel::Critical
        } else if memory_mb > self.memory_warning_mb {
            WarningLevel::Warning
        } else {
            WarningLevel::Normal
        }
    }

    /// Evaluate chunk load time (ms) against thresholds.
    pub fn chunk_load_level(&self, load_time_ms: f32) -> WarningLevel {
        if load_time_ms > self.chunk_load_critical_ms {
            WarningLevel::Critical
        } else if load_time_ms > self.chunk_load_warning_ms {
            WarningLevel::Warning
        } else {
            WarningLevel::Normal
        }
    }
}

// ============================================================================
// Performance Metrics Resource
// ============================================================================

/// Central resource holding aggregated performance metrics.
///
/// Updated every frame by [`update_performance_metrics`]. Provides a unified
/// view of engine performance for the overlay and other systems.
#[derive(Resource)]
pub struct PerformanceMetrics {
    // ── Frame timing ──
    /// Current frames per second (smoothed).
    pub fps: f64,
    /// Current frame time in milliseconds (smoothed).
    pub frame_time_ms: f64,

    // ── Memory ──
    /// Process memory usage in megabytes.
    pub memory_mb: f64,
    /// Peak memory usage in megabytes (if available).
    pub peak_memory_mb: Option<f64>,

    // ── Chunks ──
    /// Number of currently loaded chunks.
    pub active_chunks: usize,
    /// Average chunk load time in milliseconds.
    pub avg_chunk_load_ms: f32,
    /// Chunks loaded per second.
    pub chunks_per_second: f32,

    // ── Internal state ──
    /// Timer for throttling memory queries (OS calls are expensive).
    memory_query_timer: f32,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            fps: 0.0,
            frame_time_ms: 0.0,
            memory_mb: 0.0,
            peak_memory_mb: None,
            active_chunks: 0,
            avg_chunk_load_ms: 0.0,
            chunks_per_second: 0.0,
            memory_query_timer: 0.0,
        }
    }
}

impl PerformanceMetrics {
    /// Evaluate all metrics against thresholds and return warning levels.
    pub fn warning_levels(&self, thresholds: &PerformanceThresholds) -> MetricWarnings {
        MetricWarnings {
            fps: thresholds.fps_level(self.fps),
            frame_time: thresholds.frame_time_level(self.frame_time_ms),
            memory: thresholds.memory_level(self.memory_mb),
            chunk_load: thresholds.chunk_load_level(self.avg_chunk_load_ms),
        }
    }

    /// Get the overall (worst) warning level across all metrics.
    pub fn overall_level(&self, thresholds: &PerformanceThresholds) -> WarningLevel {
        let warnings = self.warning_levels(thresholds);
        [
            warnings.fps,
            warnings.frame_time,
            warnings.memory,
            warnings.chunk_load,
        ]
        .into_iter()
        .max_by_key(|level| match level {
            WarningLevel::Normal => 0,
            WarningLevel::Warning => 1,
            WarningLevel::Critical => 2,
        })
        .unwrap_or(WarningLevel::Normal)
    }
}

/// Warning levels for each tracked metric.
#[derive(Debug, Clone, Copy, Default)]
pub struct MetricWarnings {
    pub fps: WarningLevel,
    pub frame_time: WarningLevel,
    pub memory: WarningLevel,
    pub chunk_load: WarningLevel,
}

// ============================================================================
// Minimal Overlay State
// ============================================================================

/// Configuration and state for the minimal performance overlay (F2).
///
/// This is a lightweight always-on-top display showing just the essentials:
/// FPS, memory, and chunk count with color-coded warnings.
#[derive(Resource)]
pub struct PerformanceOverlay {
    /// Whether the overlay is currently visible.
    pub visible: bool,
    /// X position in pixels from left edge.
    pub x: f32,
    /// Y position in pixels from top edge.
    pub y: f32,
    /// Show FPS counter.
    pub show_fps: bool,
    /// Show memory usage.
    pub show_memory: bool,
    /// Show chunk statistics.
    pub show_chunks: bool,
    /// Compact mode (single-line summary).
    pub compact: bool,
}

impl Default for PerformanceOverlay {
    fn default() -> Self {
        Self {
            visible: false,
            x: 10.0,
            y: 40.0,
            show_fps: true,
            show_memory: true,
            show_chunks: true,
            compact: false,
        }
    }
}

// ============================================================================
// Systems
// ============================================================================

/// How often to query OS for memory usage (seconds).
const MEMORY_QUERY_INTERVAL: f32 = 0.5;

/// System that updates the central performance metrics resource.
///
/// Runs every frame, collecting data from:
/// - Bevy's `DiagnosticsStore` for FPS/frame time
/// - `memory::get_process_memory()` for RAM usage (throttled)
/// - `ChunkManager` for active chunk count
/// - `ChunkLoadMetrics` for chunk loading performance
pub fn update_performance_metrics(
    diagnostics: Res<DiagnosticsStore>,
    time: Res<Time>,
    mut metrics: ResMut<PerformanceMetrics>,
    chunk_manager: Option<Res<crate::world::ChunkManager>>,
    chunk_load_metrics: Option<Res<crate::world::ChunkLoadMetrics>>,
) {
    // ── FPS and frame time from Bevy diagnostics ──
    if let Some(fps_diag) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS)
        && let Some(fps) = fps_diag.smoothed()
    {
        metrics.fps = fps;
        if fps > 0.0 {
            metrics.frame_time_ms = 1000.0 / fps;
        }
    }

    // ── Active chunks from ChunkManager ──
    if let Some(ref cm) = chunk_manager {
        metrics.active_chunks = cm.chunks.len();
    }

    // ── Chunk loading metrics ──
    if let Some(ref load_metrics) = chunk_load_metrics {
        metrics.avg_chunk_load_ms = load_metrics.avg_load_time_ms;
        metrics.chunks_per_second = load_metrics.chunks_per_second;
    }

    // ── Memory usage (throttled to reduce OS call overhead) ──
    let dt = time.delta_secs();
    metrics.memory_query_timer += dt;
    if metrics.memory_query_timer >= MEMORY_QUERY_INTERVAL {
        metrics.memory_query_timer = 0.0;
        if let Some(mem) = memory::get_process_memory() {
            metrics.memory_mb = mem.rss_bytes as f64 / (1024.0 * 1024.0);
            metrics.peak_memory_mb = mem.peak_rss_bytes.map(|p| p as f64 / (1024.0 * 1024.0));
        }
    }
}

/// System to toggle minimal overlay visibility with F2.
pub fn overlay_keyboard_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut overlay: ResMut<PerformanceOverlay>,
) {
    if keyboard.just_pressed(KeyCode::F2) {
        overlay.visible = !overlay.visible;
        if overlay.visible {
            info!("Performance overlay enabled (F2)");
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin that registers the performance metrics collection systems.
///
/// # Resources Registered
///
/// - [`PerformanceMetrics`] — Central aggregated metrics
/// - [`PerformanceThresholds`] — Warning threshold configuration
/// - [`PerformanceOverlay`] — Minimal overlay state (F2)
///
/// # Systems Added
///
/// - [`update_performance_metrics`] — Collects metrics each frame
/// - [`overlay_keyboard_input`] — F2 toggle handling
pub struct MetricsPlugin;

impl Plugin for MetricsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PerformanceMetrics>()
            .init_resource::<PerformanceThresholds>()
            .init_resource::<PerformanceOverlay>()
            .add_systems(Update, (update_performance_metrics, overlay_keyboard_input));
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_warning_level_default() {
        assert_eq!(WarningLevel::default(), WarningLevel::Normal);
    }

    #[test]
    fn test_warning_level_colors() {
        assert_eq!(WarningLevel::Normal.to_rgb(), (100, 255, 100));
        assert_eq!(WarningLevel::Warning.to_rgb(), (255, 255, 100));
        assert_eq!(WarningLevel::Critical.to_rgb(), (255, 100, 100));
    }

    #[test]
    fn test_warning_level_hex() {
        assert_eq!(WarningLevel::Normal.to_hex(), "#64FF64");
        assert_eq!(WarningLevel::Warning.to_hex(), "#FFFF64");
        assert_eq!(WarningLevel::Critical.to_hex(), "#FF6464");
    }

    #[test]
    fn test_thresholds_default() {
        let t = PerformanceThresholds::default();
        assert_eq!(t.fps_critical, 30.0);
        assert_eq!(t.fps_warning, 60.0);
        assert!((t.frame_time_critical_ms - 33.33).abs() < 0.01);
        assert!((t.frame_time_warning_ms - 16.67).abs() < 0.01);
    }

    #[test]
    fn test_fps_level() {
        let t = PerformanceThresholds::default();

        assert_eq!(t.fps_level(120.0), WarningLevel::Normal);
        assert_eq!(t.fps_level(60.0), WarningLevel::Normal);
        assert_eq!(t.fps_level(59.0), WarningLevel::Warning);
        assert_eq!(t.fps_level(45.0), WarningLevel::Warning);
        assert_eq!(t.fps_level(30.0), WarningLevel::Warning);
        assert_eq!(t.fps_level(29.0), WarningLevel::Critical);
        assert_eq!(t.fps_level(15.0), WarningLevel::Critical);
    }

    #[test]
    fn test_frame_time_level() {
        let t = PerformanceThresholds::default();

        assert_eq!(t.frame_time_level(8.0), WarningLevel::Normal);
        assert_eq!(t.frame_time_level(16.67), WarningLevel::Normal);
        assert_eq!(t.frame_time_level(16.68), WarningLevel::Warning);
        assert_eq!(t.frame_time_level(25.0), WarningLevel::Warning);
        assert_eq!(t.frame_time_level(33.33), WarningLevel::Warning);
        assert_eq!(t.frame_time_level(33.34), WarningLevel::Critical);
        assert_eq!(t.frame_time_level(50.0), WarningLevel::Critical);
    }

    #[test]
    fn test_memory_level() {
        let t = PerformanceThresholds::default();

        assert_eq!(t.memory_level(512.0), WarningLevel::Normal);
        assert_eq!(t.memory_level(2048.0), WarningLevel::Normal);
        assert_eq!(t.memory_level(2049.0), WarningLevel::Warning);
        assert_eq!(t.memory_level(3000.0), WarningLevel::Warning);
        assert_eq!(t.memory_level(4096.0), WarningLevel::Warning);
        assert_eq!(t.memory_level(4097.0), WarningLevel::Critical);
        assert_eq!(t.memory_level(8000.0), WarningLevel::Critical);
    }

    #[test]
    fn test_chunk_load_level() {
        let t = PerformanceThresholds::default();

        assert_eq!(t.chunk_load_level(5.0), WarningLevel::Normal);
        assert_eq!(t.chunk_load_level(20.0), WarningLevel::Normal);
        assert_eq!(t.chunk_load_level(21.0), WarningLevel::Warning);
        assert_eq!(t.chunk_load_level(50.0), WarningLevel::Warning);
        assert_eq!(t.chunk_load_level(100.0), WarningLevel::Warning);
        assert_eq!(t.chunk_load_level(101.0), WarningLevel::Critical);
    }

    #[test]
    fn test_metrics_default() {
        let m = PerformanceMetrics::default();
        assert_eq!(m.fps, 0.0);
        assert_eq!(m.frame_time_ms, 0.0);
        assert_eq!(m.memory_mb, 0.0);
        assert!(m.peak_memory_mb.is_none());
        assert_eq!(m.active_chunks, 0);
        assert_eq!(m.avg_chunk_load_ms, 0.0);
        assert_eq!(m.chunks_per_second, 0.0);
    }

    #[test]
    fn test_metrics_warning_levels() {
        let m = PerformanceMetrics {
            fps: 45.0,           // Warning
            frame_time_ms: 22.0, // Warning
            memory_mb: 1000.0,   // Normal
            active_chunks: 100,
            avg_chunk_load_ms: 150.0, // Critical
            ..Default::default()
        };
        let t = PerformanceThresholds::default();

        let warnings = m.warning_levels(&t);
        assert_eq!(warnings.fps, WarningLevel::Warning);
        assert_eq!(warnings.frame_time, WarningLevel::Warning);
        assert_eq!(warnings.memory, WarningLevel::Normal);
        assert_eq!(warnings.chunk_load, WarningLevel::Critical);
    }

    #[test]
    fn test_metrics_overall_level() {
        let t = PerformanceThresholds::default();

        // All normal
        let m1 = PerformanceMetrics {
            fps: 60.0,
            frame_time_ms: 16.0,
            memory_mb: 500.0,
            avg_chunk_load_ms: 10.0,
            ..Default::default()
        };
        assert_eq!(m1.overall_level(&t), WarningLevel::Normal);

        // One warning
        let m2 = PerformanceMetrics {
            fps: 45.0, // Warning
            frame_time_ms: 16.0,
            memory_mb: 500.0,
            avg_chunk_load_ms: 10.0,
            ..Default::default()
        };
        assert_eq!(m2.overall_level(&t), WarningLevel::Warning);

        // One critical
        let m3 = PerformanceMetrics {
            fps: 60.0,
            frame_time_ms: 16.0,
            memory_mb: 500.0,
            avg_chunk_load_ms: 150.0, // Critical
            ..Default::default()
        };
        assert_eq!(m3.overall_level(&t), WarningLevel::Critical);
    }

    #[test]
    fn test_metric_warnings_default() {
        let warnings = MetricWarnings::default();
        assert_eq!(warnings.fps, WarningLevel::Normal);
        assert_eq!(warnings.frame_time, WarningLevel::Normal);
        assert_eq!(warnings.memory, WarningLevel::Normal);
        assert_eq!(warnings.chunk_load, WarningLevel::Normal);
    }

    #[test]
    fn test_overlay_default() {
        let overlay = PerformanceOverlay::default();
        assert!(!overlay.visible);
        assert_eq!(overlay.x, 10.0);
        assert_eq!(overlay.y, 40.0);
        assert!(overlay.show_fps);
        assert!(overlay.show_memory);
        assert!(overlay.show_chunks);
        assert!(!overlay.compact);
    }

    #[test]
    fn test_overlay_toggle() {
        let mut overlay = PerformanceOverlay::default();
        assert!(!overlay.visible);

        overlay.visible = !overlay.visible;
        assert!(overlay.visible);

        overlay.visible = !overlay.visible;
        assert!(!overlay.visible);
    }
}
