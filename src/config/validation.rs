//! Configuration validation system
//!
//! Validates engine configuration values before they are applied to prevent
//! invalid settings from breaking the engine at runtime. Each config section
//! has its own validation logic that checks value ranges, constraints, and
//! inter-field consistency.
//!
//! # Severity Levels
//!
//! - **Error** — The value is invalid and will be replaced with the default.
//!   Examples: negative render distance, NaN speed values.
//! - **Warning** — The value is technically valid but outside recommended
//!   ranges. The value is clamped to the nearest safe boundary.
//!   Examples: render distance > 32 (extreme memory usage), FOV > 170.
//!
//! # Usage
//!
//! ```rust,ignore
//! let (validated, issues) = validate_config(raw_config);
//! for issue in &issues {
//!     match issue.severity {
//!         Severity::Error => warn!("Config error: {}", issue.message),
//!         Severity::Warning => info!("Config warning: {}", issue.message),
//!     }
//! }
//! // `validated` is safe to apply
//! ```

use super::*;

// ============================================================================
// VALIDATION TYPES
// ============================================================================

/// Severity of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Value was invalid and replaced with the default.
    Error,
    /// Value was outside recommended range and was clamped.
    Warning,
}

/// A single validation issue found during config checking.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Which config section contains the issue (e.g., `"render"`, `"player"`).
    pub section: &'static str,
    /// Which field has the issue (e.g., `"render_distance"`, `"walk_speed"`).
    pub field: &'static str,
    /// Severity level.
    pub severity: Severity,
    /// Human-readable description of what was wrong and what was done.
    pub message: String,
}

/// Result of validating an entire `EngineConfig`.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// The validated (and possibly corrected) config.
    pub config: EngineConfig,
    /// All issues found during validation.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    /// Returns `true` if any errors (not just warnings) were found.
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    /// Returns `true` if no issues at all were found.
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Validate an `EngineConfig`, returning a corrected copy and any issues found.
///
/// This is the main entry point. It validates every section and returns a
/// config that is guaranteed to have sane values.
pub fn validate_config(config: EngineConfig) -> ValidationResult {
    let mut issues = Vec::new();
    let mut config = config;

    validate_render(&mut config.render, &mut issues);
    validate_player(&mut config.player, &mut issues);
    validate_debug(&mut config.debug, &mut issues);
    validate_unload(&mut config.unload, &mut issues);
    validate_world(&mut config.world, &mut issues);
    validate_streaming(&mut config.streaming, &mut issues);
    validate_save(&mut config.save, &mut issues);
    validate_audio(&mut config.audio, &mut issues);
    validate_terrain(&mut config.terrain, &mut issues);
    validate_cycle_duration(&mut config.cycle_duration_seconds, &mut issues);

    ValidationResult { config, issues }
}

// ============================================================================
// SECTION VALIDATORS
// ============================================================================

/// Validate render settings.
fn validate_render(render: &mut RenderConfig, issues: &mut Vec<ValidationIssue>) {
    // render_distance: must be >= 1, warn if > 32
    if render.render_distance < 1 {
        issues.push(ValidationIssue {
            section: "render",
            field: "render_distance",
            severity: Severity::Error,
            message: format!(
                "render_distance {} is invalid (must be >= 1), reset to default (4)",
                render.render_distance
            ),
        });
        render.render_distance = RenderConfig::default().render_distance;
    } else if render.render_distance > 32 {
        issues.push(ValidationIssue {
            section: "render",
            field: "render_distance",
            severity: Severity::Warning,
            message: format!(
                "render_distance {} is very high (>32), may cause extreme memory usage — clamped to 32",
                render.render_distance
            ),
        });
        render.render_distance = 32;
    }

    // max_chunks_per_frame: must be >= 1
    if render.max_chunks_per_frame < 1 {
        issues.push(ValidationIssue {
            section: "render",
            field: "max_chunks_per_frame",
            severity: Severity::Error,
            message: "max_chunks_per_frame must be >= 1, reset to default (4)".into(),
        });
        render.max_chunks_per_frame = RenderConfig::default().max_chunks_per_frame;
    }

    // camera_fov: valid range 30–170 degrees
    if render.camera_fov < 30.0 || render.camera_fov > 170.0 {
        let clamped = render.camera_fov.clamp(30.0, 170.0);
        issues.push(ValidationIssue {
            section: "render",
            field: "camera_fov",
            severity: Severity::Warning,
            message: format!(
                "camera_fov {} outside valid range [30, 170], clamped to {}",
                render.camera_fov, clamped
            ),
        });
        render.camera_fov = clamped;
    }

    // camera_near: must be > 0
    if render.camera_near <= 0.0 {
        issues.push(ValidationIssue {
            section: "render",
            field: "camera_near",
            severity: Severity::Error,
            message: format!(
                "camera_near {} must be > 0, reset to default (0.1)",
                render.camera_near
            ),
        });
        render.camera_near = RenderConfig::default().camera_near;
    }

    // bloom_intensity: clamp to [0, 2]
    if render.bloom_intensity < 0.0 || render.bloom_intensity > 2.0 {
        let clamped = render.bloom_intensity.clamp(0.0, 2.0);
        issues.push(ValidationIssue {
            section: "render",
            field: "bloom_intensity",
            severity: Severity::Warning,
            message: format!(
                "bloom_intensity {} outside range [0, 2], clamped to {}",
                render.bloom_intensity, clamped
            ),
        });
        render.bloom_intensity = clamped;
    }

    // fog_start must be < fog_end when both are non-zero
    if render.fog_enabled && render.fog_start >= render.fog_end && render.fog_end > 0.0 {
        issues.push(ValidationIssue {
            section: "render",
            field: "fog_start",
            severity: Severity::Error,
            message: format!(
                "fog_start ({}) must be < fog_end ({}), reset to defaults",
                render.fog_start, render.fog_end
            ),
        });
        let defaults = RenderConfig::default();
        render.fog_start = defaults.fog_start;
        render.fog_end = defaults.fog_end;
    }

    // shadow_cascade_count: 1–8
    if render.shadow_cascade_count < 1 || render.shadow_cascade_count > 8 {
        let clamped = render.shadow_cascade_count.clamp(1, 8);
        issues.push(ValidationIssue {
            section: "render",
            field: "shadow_cascade_count",
            severity: Severity::Warning,
            message: format!(
                "shadow_cascade_count {} outside range [1, 8], clamped to {}",
                render.shadow_cascade_count, clamped
            ),
        });
        render.shadow_cascade_count = clamped;
    }
}

/// Validate player movement settings.
fn validate_player(player: &mut PlayerConfig, issues: &mut Vec<ValidationIssue>) {
    let defaults = PlayerConfig::default();

    // All speeds must be > 0
    if player.walk_speed <= 0.0 || !player.walk_speed.is_finite() {
        issues.push(ValidationIssue {
            section: "player",
            field: "walk_speed",
            severity: Severity::Error,
            message: format!(
                "walk_speed {} is invalid (must be > 0 and finite), reset to {}",
                player.walk_speed, defaults.walk_speed
            ),
        });
        player.walk_speed = defaults.walk_speed;
    }

    if player.sprint_speed <= 0.0 || !player.sprint_speed.is_finite() {
        issues.push(ValidationIssue {
            section: "player",
            field: "sprint_speed",
            severity: Severity::Error,
            message: format!(
                "sprint_speed {} is invalid, reset to {}",
                player.sprint_speed, defaults.sprint_speed
            ),
        });
        player.sprint_speed = defaults.sprint_speed;
    }

    if player.fly_speed <= 0.0 || !player.fly_speed.is_finite() {
        issues.push(ValidationIssue {
            section: "player",
            field: "fly_speed",
            severity: Severity::Error,
            message: format!(
                "fly_speed {} is invalid, reset to {}",
                player.fly_speed, defaults.fly_speed
            ),
        });
        player.fly_speed = defaults.fly_speed;
    }

    if player.jump_velocity <= 0.0 || !player.jump_velocity.is_finite() {
        issues.push(ValidationIssue {
            section: "player",
            field: "jump_velocity",
            severity: Severity::Error,
            message: format!(
                "jump_velocity {} is invalid, reset to {}",
                player.jump_velocity, defaults.jump_velocity
            ),
        });
        player.jump_velocity = defaults.jump_velocity;
    }

    // mouse_sensitivity: must be > 0, warn if extreme
    if player.mouse_sensitivity <= 0.0 || !player.mouse_sensitivity.is_finite() {
        issues.push(ValidationIssue {
            section: "player",
            field: "mouse_sensitivity",
            severity: Severity::Error,
            message: format!(
                "mouse_sensitivity {} is invalid, reset to {}",
                player.mouse_sensitivity, defaults.mouse_sensitivity
            ),
        });
        player.mouse_sensitivity = defaults.mouse_sensitivity;
    } else if player.mouse_sensitivity > 2.0 {
        issues.push(ValidationIssue {
            section: "player",
            field: "mouse_sensitivity",
            severity: Severity::Warning,
            message: format!(
                "mouse_sensitivity {} is very high (>2.0), may be unusable",
                player.mouse_sensitivity
            ),
        });
        // Don't clamp — just warn. User might want it.
    }
}

/// Validate debug settings (all booleans, minimal validation needed).
fn validate_debug(_debug: &mut DebugConfig, _issues: &mut Vec<ValidationIssue>) {
    // All fields are booleans — nothing to validate.
    // This function exists for consistency and future extensibility.
}

/// Validate unload settings.
fn validate_unload(unload: &mut UnloadSettings, issues: &mut Vec<ValidationIssue>) {
    // unload_distance: if Some, must be >= 1
    if let Some(dist) = unload.unload_distance
        && dist < 1
    {
        issues.push(ValidationIssue {
            section: "unload",
            field: "unload_distance",
            severity: Severity::Error,
            message: format!(
                "unload_distance {} must be >= 1, reset to None (auto)",
                dist
            ),
        });
        unload.unload_distance = None;
    }

    // memory_threshold_mb: warn if < 256 MB
    if unload.memory_threshold_mb < 256 {
        issues.push(ValidationIssue {
            section: "unload",
            field: "memory_threshold_mb",
            severity: Severity::Warning,
            message: format!(
                "memory_threshold_mb {} is very low (<256), may cause aggressive unloading",
                unload.memory_threshold_mb
            ),
        });
    }

    // max_saves_per_frame: must be >= 1
    if unload.max_saves_per_frame < 1 {
        issues.push(ValidationIssue {
            section: "unload",
            field: "max_saves_per_frame",
            severity: Severity::Error,
            message: "max_saves_per_frame must be >= 1, reset to default (4)".into(),
        });
        unload.max_saves_per_frame = UnloadSettings::default().max_saves_per_frame;
    }
}

/// Validate world config.
fn validate_world(world: &mut WorldConfig, issues: &mut Vec<ValidationIssue>) {
    // load_distance: if Some, must be >= 1
    if let Some(dist) = world.load_distance
        && dist < 1
    {
        issues.push(ValidationIssue {
            section: "world",
            field: "load_distance",
            severity: Severity::Error,
            message: format!(
                "load_distance {} must be >= 1, reset to None (auto)",
                dist
            ),
        });
        world.load_distance = None;
    }

    // vertical ranges must be >= 0
    if world.vertical_load_up < 0 {
        issues.push(ValidationIssue {
            section: "world",
            field: "vertical_load_up",
            severity: Severity::Error,
            message: format!(
                "vertical_load_up {} must be >= 0, reset to default (4)",
                world.vertical_load_up
            ),
        });
        world.vertical_load_up = WorldConfig::default().vertical_load_up;
    }

    if world.vertical_load_down < 0 {
        issues.push(ValidationIssue {
            section: "world",
            field: "vertical_load_down",
            severity: Severity::Error,
            message: format!(
                "vertical_load_down {} must be >= 0, reset to default (2)",
                world.vertical_load_down
            ),
        });
        world.vertical_load_down = WorldConfig::default().vertical_load_down;
    }
}

/// Validate streaming settings.
fn validate_streaming(streaming: &mut StreamingSettings, issues: &mut Vec<ValidationIssue>) {
    if streaming.lookahead_chunks < 0 {
        issues.push(ValidationIssue {
            section: "streaming",
            field: "lookahead_chunks",
            severity: Severity::Error,
            message: format!(
                "lookahead_chunks {} must be >= 0, reset to default (3)",
                streaming.lookahead_chunks
            ),
        });
        streaming.lookahead_chunks = StreamingSettings::default().lookahead_chunks;
    }

    if streaming.velocity_smoothing < 0.0 || streaming.velocity_smoothing > 1.0 {
        let clamped = streaming.velocity_smoothing.clamp(0.0, 1.0);
        issues.push(ValidationIssue {
            section: "streaming",
            field: "velocity_smoothing",
            severity: Severity::Warning,
            message: format!(
                "velocity_smoothing {} outside [0, 1], clamped to {}",
                streaming.velocity_smoothing, clamped
            ),
        });
        streaming.velocity_smoothing = clamped;
    }

    if streaming.min_speed_threshold < 0.0 {
        issues.push(ValidationIssue {
            section: "streaming",
            field: "min_speed_threshold",
            severity: Severity::Error,
            message: format!(
                "min_speed_threshold {} must be >= 0, reset to default (0.5)",
                streaming.min_speed_threshold
            ),
        });
        streaming.min_speed_threshold = StreamingSettings::default().min_speed_threshold;
    }
}

/// Validate save settings.
fn validate_save(save: &mut SaveConfig, issues: &mut Vec<ValidationIssue>) {
    // save_dir: must not be empty
    if save.save_dir.trim().is_empty() {
        issues.push(ValidationIssue {
            section: "save",
            field: "save_dir",
            severity: Severity::Error,
            message: "save_dir cannot be empty, reset to default (saves/default)".into(),
        });
        save.save_dir = SaveConfig::default().save_dir;
    }

    // auto_save_interval: must be >= 0
    if save.auto_save_interval < 0.0 {
        issues.push(ValidationIssue {
            section: "save",
            field: "auto_save_interval",
            severity: Severity::Error,
            message: format!(
                "auto_save_interval {} must be >= 0, reset to default (300)",
                save.auto_save_interval
            ),
        });
        save.auto_save_interval = SaveConfig::default().auto_save_interval;
    }

    // chunk_format: must be "json" or "binary"
    if save.chunk_format != "json" && save.chunk_format != "binary" {
        issues.push(ValidationIssue {
            section: "save",
            field: "chunk_format",
            severity: Severity::Error,
            message: format!(
                "chunk_format '{}' is invalid (must be 'json' or 'binary'), reset to 'binary'",
                save.chunk_format
            ),
        });
        save.chunk_format = SaveConfig::default().chunk_format;
    }
}

/// Validate audio settings.
fn validate_audio(audio: &mut audio::AudioSettings, issues: &mut Vec<ValidationIssue>) {
    // Volume fields: clamp to [0, 1]
    let volume_fields: &mut [(&'static str, &mut f32)] = &mut [
        ("master_volume", &mut audio.master_volume),
        ("ambience_intensity", &mut audio.ambience_intensity),
        ("music_volume", &mut audio.music_volume),
        ("sfx_volume", &mut audio.sfx_volume),
    ];

    for (name, value) in volume_fields.iter_mut() {
        if !value.is_finite() || **value < 0.0 || **value > 1.0 {
            let clamped = if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.8 // fallback for NaN/Inf
            };
            issues.push(ValidationIssue {
                section: "audio",
                field: name,
                severity: Severity::Warning,
                message: format!(
                    "{} ({}) outside [0, 1], clamped to {}",
                    name, **value, clamped
                ),
            });
            **value = clamped;
        }
    }

    // distance_falloff: valid range [0.1, 5.0]
    if !audio.distance_falloff.is_finite() || audio.distance_falloff < 0.1 || audio.distance_falloff > 5.0 {
        let clamped = if audio.distance_falloff.is_finite() {
            audio.distance_falloff.clamp(0.1, 5.0)
        } else {
            1.0
        };
        issues.push(ValidationIssue {
            section: "audio",
            field: "distance_falloff",
            severity: Severity::Warning,
            message: format!(
                "distance_falloff {} outside [0.1, 5.0], clamped to {}",
                audio.distance_falloff, clamped
            ),
        });
        audio.distance_falloff = clamped;
    }
}

/// Validate terrain settings (light checks — these are startup-only but we
/// still validate to catch obviously broken values).
fn validate_terrain(terrain: &mut TerrainSettings, issues: &mut Vec<ValidationIssue>) {
    let defaults = TerrainSettings::default();

    if terrain.octaves == 0 || terrain.octaves > 16 {
        issues.push(ValidationIssue {
            section: "terrain",
            field: "octaves",
            severity: Severity::Error,
            message: format!(
                "octaves {} is invalid (must be 1–16), reset to {}",
                terrain.octaves, defaults.octaves
            ),
        });
        terrain.octaves = defaults.octaves;
    }

    if !terrain.frequency.is_finite() || terrain.frequency <= 0.0 {
        issues.push(ValidationIssue {
            section: "terrain",
            field: "frequency",
            severity: Severity::Error,
            message: format!(
                "frequency {} is invalid (must be > 0), reset to {}",
                terrain.frequency, defaults.frequency
            ),
        });
        terrain.frequency = defaults.frequency;
    }

    if !terrain.biome_blend_distance.is_finite() || terrain.biome_blend_distance < 0.0 {
        issues.push(ValidationIssue {
            section: "terrain",
            field: "biome_blend_distance",
            severity: Severity::Error,
            message: format!(
                "biome_blend_distance {} is invalid, reset to {}",
                terrain.biome_blend_distance, defaults.biome_blend_distance
            ),
        });
        terrain.biome_blend_distance = defaults.biome_blend_distance;
    }
}

/// Validate day/night cycle duration.
fn validate_cycle_duration(duration: &mut f32, issues: &mut Vec<ValidationIssue>) {
    if !duration.is_finite() || *duration <= 0.0 {
        issues.push(ValidationIssue {
            section: "root",
            field: "cycle_duration_seconds",
            severity: Severity::Error,
            message: format!(
                "cycle_duration_seconds {} is invalid (must be > 0), reset to 600",
                duration
            ),
        });
        *duration = 600.0;
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_default_config_produces_no_issues() {
        let result = validate_config(EngineConfig::default());
        assert!(result.is_clean(), "Default config should pass validation, got: {:?}", result.issues);
    }

    #[test]
    fn test_negative_render_distance_is_error() {
        let mut config = EngineConfig::default();
        config.render.render_distance = -5;
        let result = validate_config(config);
        assert!(result.has_errors());
        assert_eq!(result.config.render.render_distance, 4); // reset to default
        assert!(result.issues.iter().any(|i| i.field == "render_distance"));
    }

    #[test]
    fn test_extreme_render_distance_is_warning() {
        let mut config = EngineConfig::default();
        config.render.render_distance = 64;
        let result = validate_config(config);
        assert!(!result.has_errors());
        assert!(!result.is_clean());
        assert_eq!(result.config.render.render_distance, 32); // clamped
    }

    #[test]
    fn test_invalid_fov_is_clamped() {
        let mut config = EngineConfig::default();
        config.render.camera_fov = 200.0;
        let result = validate_config(config);
        assert_eq!(result.config.render.camera_fov, 170.0);

        let mut config = EngineConfig::default();
        config.render.camera_fov = 10.0;
        let result = validate_config(config);
        assert_eq!(result.config.render.camera_fov, 30.0);
    }

    #[test]
    fn test_zero_walk_speed_is_error() {
        let mut config = EngineConfig::default();
        config.player.walk_speed = 0.0;
        let result = validate_config(config);
        assert!(result.has_errors());
        assert_eq!(result.config.player.walk_speed, PlayerConfig::default().walk_speed);
    }

    #[test]
    fn test_nan_speed_is_error() {
        let mut config = EngineConfig::default();
        config.player.fly_speed = f32::NAN;
        let result = validate_config(config);
        assert!(result.has_errors());
        assert_eq!(result.config.player.fly_speed, PlayerConfig::default().fly_speed);
    }

    #[test]
    fn test_negative_sensitivity_is_error() {
        let mut config = EngineConfig::default();
        config.player.mouse_sensitivity = -0.5;
        let result = validate_config(config);
        assert!(result.has_errors());
        assert_eq!(
            result.config.player.mouse_sensitivity,
            PlayerConfig::default().mouse_sensitivity
        );
    }

    #[test]
    fn test_invalid_save_dir_is_error() {
        let mut config = EngineConfig::default();
        config.save.save_dir = "   ".into();
        let result = validate_config(config);
        assert!(result.has_errors());
        assert_eq!(result.config.save.save_dir, "saves/default");
    }

    #[test]
    fn test_invalid_chunk_format_is_error() {
        let mut config = EngineConfig::default();
        config.save.chunk_format = "xml".into();
        let result = validate_config(config);
        assert!(result.has_errors());
        assert_eq!(result.config.save.chunk_format, "binary");
    }

    #[test]
    fn test_audio_volume_clamping() {
        let mut config = EngineConfig::default();
        config.audio.master_volume = 1.5;
        config.audio.sfx_volume = -0.3;
        let result = validate_config(config);
        assert_eq!(result.config.audio.master_volume, 1.0);
        assert_eq!(result.config.audio.sfx_volume, 0.0);
    }

    #[test]
    fn test_fog_start_must_be_less_than_end() {
        let mut config = EngineConfig::default();
        config.render.fog_enabled = true;
        config.render.fog_start = 300.0;
        config.render.fog_end = 100.0;
        let result = validate_config(config);
        assert!(result.has_errors());
        // Reset to defaults
        let defaults = RenderConfig::default();
        assert_eq!(result.config.render.fog_start, defaults.fog_start);
        assert_eq!(result.config.render.fog_end, defaults.fog_end);
    }

    #[test]
    fn test_streaming_velocity_smoothing_clamped() {
        let mut config = EngineConfig::default();
        config.streaming.velocity_smoothing = 1.5;
        let result = validate_config(config);
        assert_eq!(result.config.streaming.velocity_smoothing, 1.0);
    }

    #[test]
    fn test_terrain_zero_octaves_is_error() {
        let mut config = EngineConfig::default();
        config.terrain.octaves = 0;
        let result = validate_config(config);
        assert!(result.has_errors());
        assert_eq!(result.config.terrain.octaves, TerrainSettings::default().octaves);
    }

    #[test]
    fn test_negative_cycle_duration_is_error() {
        let mut config = EngineConfig::default();
        config.cycle_duration_seconds = -10.0;
        let result = validate_config(config);
        assert!(result.has_errors());
        assert_eq!(result.config.cycle_duration_seconds, 600.0);
    }

    #[test]
    fn test_multiple_issues_collected() {
        let mut config = EngineConfig::default();
        config.render.render_distance = -1;
        config.player.walk_speed = 0.0;
        config.save.chunk_format = "yaml".into();
        config.audio.master_volume = 5.0;
        let result = validate_config(config);
        assert!(result.issues.len() >= 4, "Expected at least 4 issues, got {}", result.issues.len());
    }

    #[test]
    fn test_world_negative_vertical_load_is_error() {
        let mut config = EngineConfig::default();
        config.world.vertical_load_up = -2;
        let result = validate_config(config);
        assert!(result.has_errors());
        assert_eq!(result.config.world.vertical_load_up, WorldConfig::default().vertical_load_up);
    }

    #[test]
    fn test_zero_camera_near_is_error() {
        let mut config = EngineConfig::default();
        config.render.camera_near = 0.0;
        let result = validate_config(config);
        assert!(result.has_errors());
        assert_eq!(result.config.render.camera_near, RenderConfig::default().camera_near);
    }
}
