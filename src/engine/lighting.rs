//! Day/Night Lighting Cycle
//!
//! Simulates a dynamic sun that moves across the sky, with smooth
//! color and intensity transitions through dawn, day, dusk, and night phases.
//!
//! # Time of Day
//!
//! `time_of_day` is a float from 0.0 to 1.0:
//! - 0.0  = midnight
//! - 0.25 = dawn / sunrise
//! - 0.5  = noon
//! - 0.75 = dusk / sunset
//!
//! # Architecture
//!
//! ```text
//! DayNightCycle resource (time_of_day, cycle_duration, paused)
//!     ↓
//! update_day_night_cycle (advances time each frame)
//!     ↓
//! apply_lighting (rotates sun, interpolates color/intensity, adjusts ambient)
//! ```

use bevy::pbr::CascadeShadowConfigBuilder;
use bevy::pbr::DirectionalLightShadowMap;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::TAU;

use crate::config::EngineConfig;
use crate::world::{Chunk, ChunkMesh};

// ============================================================================
// RESOURCE
// ============================================================================

/// Tracks the current time of day and cycle configuration.
///
/// Insert as a resource; the plugin initialises it with defaults and
/// then overwrites `cycle_duration` from [`EngineConfig`] in `PostStartup`.
#[derive(Resource)]
pub struct DayNightCycle {
    /// Current time of day (0.0–1.0, wraps)
    pub time_of_day: f32,
    /// Full cycle duration in seconds
    pub cycle_duration: f32,
    /// Whether the cycle is paused
    pub paused: bool,
}

impl Default for DayNightCycle {
    fn default() -> Self {
        Self {
            time_of_day: 0.35, // Start mid-morning
            cycle_duration: 600.0, // 10 minutes
            paused: false,
        }
    }
}

impl DayNightCycle {
    /// Human-readable phase name for the current time of day.
    pub fn phase_name(&self) -> &'static str {
        match self.time_of_day {
            t if t < 0.20 => "Night",
            t if t < 0.28 => "Dawn",
            t if t < 0.37 => "Morning",
            t if t < 0.63 => "Day",
            t if t < 0.72 => "Afternoon",
            t if t < 0.80 => "Dusk",
            _ => "Night",
        }
    }

    /// Formatted clock-style display (e.g. "06:00" for dawn).
    pub fn clock_display(&self) -> String {
        let total_minutes = (self.time_of_day * 24.0 * 60.0) as u32;
        let hours = (total_minutes / 60) % 24;
        let minutes = total_minutes % 60;
        format!("{:02}:{:02}", hours, minutes)
    }
}

// ============================================================================
// MARKER COMPONENT
// ============================================================================

/// Marker component for the sun directional light entity.
#[derive(Component)]
pub struct Sun;

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds a dynamic day/night lighting cycle.
///
/// Spawns a [`DirectionalLight`] tagged with [`Sun`], inserts an
/// [`AmbientLight`] resource, and runs systems each frame to advance
/// time and update lighting.
pub struct DayNightPlugin;

impl Plugin for DayNightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DayNightCycle>()
            .add_systems(Startup, spawn_sun)
            .add_systems(PostStartup, init_cycle_from_config)
            .add_systems(
                Update,
                (update_day_night_cycle, apply_lighting, update_shadow_casters).chain(),
            );
    }
}

// ============================================================================
// STARTUP SYSTEMS
// ============================================================================

/// Spawn the sun directional light and set initial ambient light.
fn spawn_sun(mut commands: Commands, config: Res<EngineConfig>) {
    // Compute maximum shadow distance
    let shadow_max = if config.render.shadow_max_distance == 0.0 {
        16.0 * (config.render.render_distance as f32 + 2.0)
    } else {
        config.render.shadow_max_distance
    };

    // Directional light (sun) with shadow cascade configuration
    commands.spawn((
        DirectionalLight {
            illuminance: 15_000.0,
            shadows_enabled: true,
            shadow_depth_bias: config.render.shadow_depth_bias,
            shadow_normal_bias: config.render.shadow_normal_bias,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: config.render.shadow_cascade_count as usize,
            maximum_distance: shadow_max,
            ..default()
        }
        .build(),
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.6, 0.4, 0.0)),
        Sun,
    ));

    // Shadow map resolution resource
    commands.insert_resource(DirectionalLightShadowMap {
        size: config.render.shadow_map_resolution as usize,
    });

    // Ambient light — will be overwritten each frame by apply_lighting
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.4, 0.4, 0.5),
        brightness: 800.0,
    });

    // Sky color — will be updated dynamically by apply_lighting
    commands.insert_resource(ClearColor(Color::srgb(0.53, 0.71, 0.92)));

    info!("Day/night cycle: sun and ambient light spawned (shadow map {}px, {} cascades, max dist {:.0})",
        config.render.shadow_map_resolution,
        config.render.shadow_cascade_count,
        shadow_max,
    );
}

/// Read cycle duration from engine config (runs after all plugins init).
fn init_cycle_from_config(config: Res<EngineConfig>, mut cycle: ResMut<DayNightCycle>) {
    cycle.cycle_duration = config.cycle_duration_seconds;
    info!(
        "Day/night cycle configured: {:.0}s duration, starting at t={:.2} ({})",
        cycle.cycle_duration,
        cycle.time_of_day,
        cycle.phase_name(),
    );
}

// ============================================================================
// UPDATE SYSTEMS
// ============================================================================

/// Advance `time_of_day` each frame, wrapping at 1.0.
fn update_day_night_cycle(time: Res<Time>, mut cycle: ResMut<DayNightCycle>) {
    if cycle.paused || cycle.cycle_duration <= 0.0 {
        return;
    }

    let delta = time.delta_secs() / cycle.cycle_duration;
    cycle.time_of_day = (cycle.time_of_day + delta) % 1.0;
}

/// Set the sun transform, color, illuminance, ambient light, and sky color based on time of day.
fn apply_lighting(
    cycle: Res<DayNightCycle>,
    mut sun_query: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut ambient: ResMut<AmbientLight>,
    mut clear_color: ResMut<ClearColor>,
) {
    let t = cycle.time_of_day;

    // --- Sun transform, color, illuminance ---
    let elevation = sun_elevation(t);
    for (mut light, mut transform) in &mut sun_query {
        let dir = sun_direction(t);
        let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, dir);
        *transform = Transform::from_rotation(rotation);

        let (color, illuminance) = sun_color_and_intensity(t);
        light.color = color;
        light.illuminance = illuminance;

        // Disable shadows when sun is below the horizon
        light.shadows_enabled = elevation > -0.05;
    }

    // --- Ambient light ---
    let (ambient_color, ambient_brightness) = ambient_settings(t);
    ambient.color = ambient_color;
    ambient.brightness = ambient_brightness;

    // --- Dynamic sky color ---
    clear_color.0 = sky_color(elevation);
}

// ============================================================================
// HELPERS — Sun Direction
// ============================================================================

/// Compute the direction the sunlight shines (FROM sun TO ground).
///
/// The sun orbits in the X-Y plane:
/// - t=0.00 (midnight) → sun below horizon (y = −1)
/// - t=0.25 (dawn)     → sun at east horizon (x = +1)
/// - t=0.50 (noon)     → sun overhead (y = +1)
/// - t=0.75 (dusk)     → sun at west horizon (x = −1)
fn sun_direction(time_of_day: f32) -> Vec3 {
    let angle = time_of_day * TAU;

    // Sun position on unit circle
    let sun_x = angle.sin();
    let sun_y = -angle.cos();

    // Light direction = opposite of sun position, with slight Z tilt
    // for more natural shadow angles
    Vec3::new(-sun_x, -sun_y, -0.2).normalize()
}

/// Sun elevation above the horizon (−1.0 … +1.0).
/// Positive = above horizon, negative = below.
fn sun_elevation(time_of_day: f32) -> f32 {
    -(time_of_day * TAU).cos()
}

// ============================================================================
// HELPERS — Color & Intensity
// ============================================================================

/// Compute the directional light color and illuminance for a given time.
fn sun_color_and_intensity(time_of_day: f32) -> (Color, f32) {
    let elevation = sun_elevation(time_of_day);

    if elevation < -0.1 {
        // Sun well below horizon — no direct sunlight
        return (Color::BLACK, 0.0);
    }

    if elevation < 0.0 {
        // Twilight zone: sun just below horizon, faint glow
        let fade = (elevation + 0.1) / 0.1; // 0 → 1
        let color = Color::srgb(0.9 * fade, 0.4 * fade, 0.15 * fade);
        return (color, 800.0 * fade);
    }

    // Sun above horizon
    let intensity_factor = elevation.min(1.0);
    let illuminance = 1_500.0 + 13_500.0 * intensity_factor;

    // Warm hue at low elevation, neutral white when high
    let warmth = 1.0 - (elevation.min(0.6) / 0.6); // 1.0 at horizon → 0.0 high up
    let r = 1.0;
    let g = 0.65 + 0.35 * (1.0 - warmth);
    let b = 0.35 + 0.65 * (1.0 - warmth);

    (Color::srgb(r, g, b), illuminance)
}

/// Compute ambient light color and brightness for a given time.
fn ambient_settings(time_of_day: f32) -> (Color, f32) {
    let elevation = sun_elevation(time_of_day);

    if elevation < -0.1 {
        // Deep night — brighter blue so shadow faces aren't pitch black
        (Color::srgb(0.12, 0.12, 0.22), 150.0)
    } else if elevation < 0.0 {
        // Twilight transition
        let t = (elevation + 0.1) / 0.1;
        let color = Color::srgb(
            lerp(0.12, 0.25, t),
            lerp(0.12, 0.18, t),
            lerp(0.22, 0.22, t),
        );
        (color, lerp(150.0, 400.0, t))
    } else {
        // Daytime — brighter, neutral ambient for visible shadow detail
        let day = elevation.min(1.0);
        let color = Color::srgb(
            lerp(0.3, 0.5, day),
            lerp(0.3, 0.5, day),
            lerp(0.35, 0.6, day),
        );
        (color, lerp(400.0, 1200.0, day))
    }
}

/// Compute the sky (clear) color based on sun elevation.
///
/// - Deep night: dark blue
/// - Dawn/Dusk (twilight): warm orange-pink gradient
/// - Day: light blue
fn sky_color(elevation: f32) -> Color {
    if elevation < -0.1 {
        // Night
        Color::srgb(0.02, 0.02, 0.08)
    } else if elevation < 0.0 {
        // Twilight — warm orange-pink
        let t = (elevation + 0.1) / 0.1; // 0 at deep night edge → 1 at horizon
        Color::srgb(
            lerp(0.02, 0.8, t),
            lerp(0.02, 0.45, t),
            lerp(0.08, 0.35, t),
        )
    } else if elevation < 0.3 {
        // Dawn/dusk above horizon — transition from warm to blue
        let t = elevation / 0.3; // 0 at horizon → 1 at 0.3 elevation
        Color::srgb(
            lerp(0.8, 0.53, t),
            lerp(0.45, 0.71, t),
            lerp(0.35, 0.92, t),
        )
    } else {
        // Full day — light sky blue
        Color::srgb(0.53, 0.71, 0.92)
    }
}

/// Linear interpolation between `a` and `b` by `t` (unclamped).
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// ============================================================================
// SHADOW CULLING
// ============================================================================

/// Chunks beyond shadow distance don't cast shadows (GPU optimization).
///
/// Inserts [`NotShadowCaster`] on distant chunk mesh entities and removes it
/// from nearby ones so the shadow map only covers the area close to the camera.
fn update_shadow_casters(
    config: Res<EngineConfig>,
    player_pos: Query<&GlobalTransform, With<Camera3d>>,
    chunks: Query<(Entity, &Chunk), With<ChunkMesh>>,
    mut commands: Commands,
) {
    let shadow_dist = if config.render.shadow_max_distance == 0.0 {
        3 // default 3 chunks
    } else {
        (config.render.shadow_max_distance / 16.0) as i32
    };

    if let Ok(cam) = player_pos.get_single() {
        let cam_pos = cam.translation();
        let cam_chunk_x = (cam_pos.x / 16.0).floor() as i32;
        let cam_chunk_z = (cam_pos.z / 16.0).floor() as i32;

        for (entity, chunk) in &chunks {
            let dx = (chunk.position.x - cam_chunk_x).abs();
            let dz = (chunk.position.z - cam_chunk_z).abs();
            let dist = dx.max(dz);

            // Use try_insert to safely handle entities that may be despawned
            // by other systems (e.g., world regeneration). remove() is already
            // safe if the component doesn't exist.
            if dist > shadow_dist {
                commands.entity(entity).try_insert(NotShadowCaster);
            } else {
                // This may fail silently if entity is despawned, which is fine
                if let Some(mut entity_commands) = commands.get_entity(entity) {
                    entity_commands.remove::<NotShadowCaster>();
                }
            }
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_day_night_cycle_defaults() {
        let cycle = DayNightCycle::default();
        assert_eq!(cycle.time_of_day, 0.35);
        assert_eq!(cycle.cycle_duration, 600.0);
        assert!(!cycle.paused);
    }

    #[test]
    fn test_phase_names_cover_full_day() {
        let phases: Vec<&str> = (0..100)
            .map(|i| {
                let cycle = DayNightCycle {
                    time_of_day: i as f32 / 100.0,
                    ..Default::default()
                };
                cycle.phase_name()
            })
            .collect();

        // Verify we get Night, Dawn, Morning, Day, Afternoon, Dusk, Night
        assert!(phases.contains(&"Night"));
        assert!(phases.contains(&"Dawn"));
        assert!(phases.contains(&"Morning"));
        assert!(phases.contains(&"Day"));
        assert!(phases.contains(&"Afternoon"));
        assert!(phases.contains(&"Dusk"));
    }

    #[test]
    fn test_clock_display() {
        let mut cycle = DayNightCycle::default();

        cycle.time_of_day = 0.0;
        assert_eq!(cycle.clock_display(), "00:00");

        cycle.time_of_day = 0.5;
        assert_eq!(cycle.clock_display(), "12:00");

        cycle.time_of_day = 0.25;
        assert_eq!(cycle.clock_display(), "06:00");

        cycle.time_of_day = 0.75;
        assert_eq!(cycle.clock_display(), "18:00");
    }

    #[test]
    fn test_sun_direction_is_normalised() {
        for i in 0..100 {
            let t = i as f32 / 100.0;
            let dir = sun_direction(t);
            let len = dir.length();
            assert!(
                (len - 1.0).abs() < 0.001,
                "sun_direction({}) has length {} (expected 1.0)",
                t,
                len,
            );
        }
    }

    #[test]
    fn test_sun_overhead_at_noon() {
        let dir = sun_direction(0.5);
        // At noon the light should point mostly downward (negative Y)
        assert!(dir.y < -0.9, "Noon light should point down, got y={}", dir.y);
    }

    #[test]
    fn test_sun_elevation_range() {
        for i in 0..100 {
            let t = i as f32 / 100.0;
            let elev = sun_elevation(t);
            assert!(
                (-1.0..=1.0).contains(&elev),
                "Elevation {} out of range at t={}",
                elev,
                t,
            );
        }
        // Noon should be highest
        let noon = sun_elevation(0.5);
        assert!(noon > 0.9, "Noon elevation should be ~1.0, got {}", noon);
        // Midnight should be lowest
        let midnight = sun_elevation(0.0);
        assert!(midnight < -0.9, "Midnight elevation should be ~-1.0, got {}", midnight);
    }

    #[test]
    fn test_no_light_at_midnight() {
        let (_color, illuminance) = sun_color_and_intensity(0.0);
        assert_eq!(illuminance, 0.0, "No direct sunlight at midnight");
    }

    #[test]
    fn test_full_light_at_noon() {
        let (_color, illuminance) = sun_color_and_intensity(0.5);
        assert!(
            illuminance > 14_000.0,
            "Noon should have near-max illuminance, got {}",
            illuminance,
        );
    }

    #[test]
    fn test_ambient_dim_at_night() {
        let (_color, brightness) = ambient_settings(0.0);
        assert!(brightness <= 150.0, "Night ambient should be dim, got {}", brightness);
    }

    #[test]
    fn test_ambient_bright_at_noon() {
        let (_color, brightness) = ambient_settings(0.5);
        assert!(brightness > 800.0, "Noon ambient should be bright, got {}", brightness);
    }

    #[test]
    fn test_lerp() {
        assert_eq!(lerp(0.0, 10.0, 0.0), 0.0);
        assert_eq!(lerp(0.0, 10.0, 1.0), 10.0);
        assert_eq!(lerp(0.0, 10.0, 0.5), 5.0);
    }
}
