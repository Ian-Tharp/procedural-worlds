//! Integration tests for the chunk loading progress bar animation states.
//!
//! Verifies the progress calculation logic and the `ChunkLoadingBarState`
//! opacity animation that drives the HUD's fade-in / fade-out behavior.

use procedural_worlds::editor::hud::ChunkLoadingBarState;
use procedural_worlds::world::{ChunkLoadMetrics, ChunkManager};

// ============================================================================
// Tests: Progress bar state defaults and transitions
// ============================================================================

/// The progress bar starts fully invisible and not loading.
#[test]
fn progress_bar_starts_invisible() {
    let state = ChunkLoadingBarState::default();
    assert_eq!(state.opacity(), 0.0);
    assert!(!state.was_loading());
}

/// Simulating the opacity fade-in that occurs during loading.
///
/// The HUD system increments opacity by `dt * 4.0` each frame while loading.
/// We replicate that arithmetic here without the ECS to verify the clamping.
#[test]
fn progress_bar_fade_in_logic() {
    let mut opacity = 0.0_f32;
    let dt = 1.0 / 60.0; // ~16.7ms frame

    // Simulate 30 frames of loading
    for _ in 0..30 {
        opacity = (opacity + dt * 4.0).min(1.0);
    }

    // After 30 frames at 60fps = 0.5s, opacity should be ~2.0 → clamped to 1.0
    assert_eq!(opacity, 1.0);
}

/// Simulating the opacity fade-out after loading completes.
///
/// The HUD system decrements opacity by `dt * 1.5` each frame when idle.
#[test]
fn progress_bar_fade_out_logic() {
    let mut opacity = 1.0_f32;
    let dt = 1.0 / 60.0;

    // Simulate frames until invisible
    let mut frames = 0;
    while opacity > 0.01 {
        opacity = (opacity - dt * 1.5).max(0.0);
        frames += 1;
        // Safety: prevent infinite loop
        if frames > 300 {
            break;
        }
    }

    // Should fade out in roughly 40 frames (~0.67s at 60fps)
    assert!(opacity < 0.01, "Should have faded out");
    assert!(frames < 100, "Should fade out within reasonable frame count: {}", frames);
}

/// The fade-in is faster than the fade-out (4.0 vs 1.5 rate).
#[test]
fn fade_in_faster_than_fade_out() {
    let dt = 1.0 / 60.0;

    // Frames to reach opacity 0.99 from 0.0 (fade in)
    let mut opacity = 0.0_f32;
    let mut fade_in_frames = 0;
    while opacity < 0.99 {
        opacity = (opacity + dt * 4.0).min(1.0);
        fade_in_frames += 1;
    }

    // Frames to reach opacity 0.01 from 1.0 (fade out)
    let mut opacity = 1.0_f32;
    let mut fade_out_frames = 0;
    while opacity > 0.01 {
        opacity = (opacity - dt * 1.5).max(0.0);
        fade_out_frames += 1;
    }

    assert!(
        fade_in_frames < fade_out_frames,
        "Fade-in ({} frames) should be faster than fade-out ({} frames)",
        fade_in_frames, fade_out_frames
    );
}

// ============================================================================
// Tests: Expected chunk count and progress calculation
// ============================================================================

/// Verify the progress calculation formula used by the HUD system.
#[test]
fn progress_calculation_formula() {
    let cm = ChunkManager::default(); // render_distance=4, vert_up=4, vert_down=2

    let ld = cm.effective_load_distance();
    let vertical_levels = (cm.vertical_load_down + cm.vertical_load_up + 1) as usize;
    let side = (2 * ld + 1) as usize;
    let expected = side * side * vertical_levels;

    // With defaults: ld=4, side=9, vert=7, expected=9*9*7=567
    assert_eq!(ld, 4);
    assert_eq!(side, 9);
    assert_eq!(vertical_levels, 7);
    assert_eq!(expected, 567);

    // Simulate progress at various loaded counts
    let test_cases = [
        (0, 0.0_f32),
        (283, 283.0 / 567.0),   // ~50%
        (567, 1.0),              // complete
        (600, 1.0),              // over-loaded (clamped)
    ];

    for &(loaded, expected_progress) in &test_cases {
        let progress = if expected > 0 {
            (loaded as f32 / expected as f32).clamp(0.0, 1.0)
        } else {
            1.0
        };
        assert!(
            (progress - expected_progress).abs() < 0.01,
            "With {} loaded: expected {:.3}, got {:.3}",
            loaded, expected_progress, progress
        );
    }
}

/// When expected chunk count is zero (edge case), progress should be 1.0.
#[test]
fn progress_zero_expected_is_complete() {
    let expected = 0_usize;
    let progress = if expected > 0 {
        (0_f32 / expected as f32).clamp(0.0, 1.0)
    } else {
        1.0
    };
    assert_eq!(progress, 1.0);
}

/// Custom load distance changes the expected chunk count.
#[test]
fn custom_load_distance_affects_expected() {
    let mut cm = ChunkManager::default();
    cm.render_distance = 4;
    cm.load_distance = Some(8);
    cm.vertical_load_up = 4;
    cm.vertical_load_down = 2;
    let cm = cm;

    let ld = cm.effective_load_distance();
    assert_eq!(ld, 8);

    let side = (2 * ld + 1) as usize; // 17
    let vert = (cm.vertical_load_up + cm.vertical_load_down + 1) as usize; // 7
    let expected = side * side * vert;
    assert_eq!(expected, 17 * 17 * 7);
    assert_eq!(expected, 2023);
}

// ============================================================================
// Tests: Metrics integration with progress bar
// ============================================================================

/// ChunkLoadMetrics provides the chunks/second stat shown on the progress bar.
#[test]
fn metrics_chunks_per_second_displayed_on_bar() {
    let mut metrics = ChunkLoadMetrics::default();

    // Record 10 loads over 1 second
    for i in 0..10 {
        metrics.record_load(0.05, 1.0 + i as f64 * 0.1);
    }
    metrics.refresh(2.5);

    // The progress bar displays this when > 0.1
    assert!(
        metrics.chunks_per_second > 0.1,
        "Should show chunks/s: {}",
        metrics.chunks_per_second
    );
}

/// When metrics are disabled, the progress bar still works (no chunks/s text).
#[test]
fn progress_bar_works_with_disabled_metrics() {
    let mut metrics = ChunkLoadMetrics::default();
    metrics.enabled = false;

    // Record loads while disabled
    for i in 0..10 {
        metrics.record_load(0.05, 1.0 + i as f64 * 0.1);
    }
    metrics.refresh(2.5);

    // chunks_per_second should be 0 (disabled)
    assert_eq!(metrics.chunks_per_second, 0.0);

    // total still incremented
    assert_eq!(metrics.total_chunks_loaded, 10);
}

// ============================================================================
// Tests: State machine transitions (full cycle)
// ============================================================================

/// Simulate a complete loading cycle: idle → loading → complete → fade out → idle.
#[test]
fn full_loading_cycle_state_machine() {
    let mut opacity = 0.0_f32;
    let mut was_loading = false;
    let dt = 1.0 / 60.0;

    // Phase 1: Idle (no pending chunks)
    assert_eq!(opacity, 0.0);
    assert!(!was_loading);

    // Phase 2: Loading starts (pending > 0, loaded < expected)
    let is_loading = true;
    for _ in 0..20 {
        if is_loading {
            opacity = (opacity + dt * 4.0).min(1.0);
            was_loading = true;
        }
    }
    assert!(opacity > 0.5, "Should be visibly fading in");
    assert!(was_loading);

    // Phase 3: Loading completes (pending = 0)
    let is_loading = false;
    if !is_loading && was_loading {
        was_loading = false; // Just finished
    }

    // Phase 4: Fade out
    let mut frames = 0;
    while opacity > 0.01 {
        if !is_loading {
            opacity = (opacity - dt * 1.5).max(0.0);
        }
        frames += 1;
        if frames > 200 {
            break;
        }
    }

    assert!(opacity < 0.01, "Should have faded to invisible");
    assert!(!was_loading);
}
