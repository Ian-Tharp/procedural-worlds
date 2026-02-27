//! Particle Emitter — Builder pattern and spawning logic.
//!
//! Provides [`EmitterBuilder`] for ergonomic emitter configuration and
//! systems for managing emitter lifecycles and particle spawning.

use bevy::prelude::*;
use std::f32::consts::TAU;

use super::{EmissionMode, Particle, ParticleConfig, ParticleEffect, ParticleEmitter, ParticleStats};

// ============================================================================
// EMITTER BUILDER
// ============================================================================

/// Builder for constructing [`ParticleEmitter`] components with a fluent API.
///
/// # Example
///
/// ```ignore
/// let emitter = EmitterBuilder::new(ParticleEffect::Water)
///     .continuous(20.0)
///     .lifetime(1.5)
///     .speed(8.0)
///     .color([0.3, 0.6, 1.0, 0.8])
///     .size(0.05)
///     .max_particles(200)
///     .build();
/// ```
pub struct EmitterBuilder {
    emitter: ParticleEmitter,
}

impl EmitterBuilder {
    /// Create a new builder for the given effect type.
    pub fn new(effect: ParticleEffect) -> Self {
        let defaults = match effect {
            ParticleEffect::Water => ParticleEmitter {
                effect,
                mode: EmissionMode::Continuous { rate: 15.0 },
                max_particles: 150,
                particle_lifetime: 1.0,
                particle_speed: 6.0,
                color: [0.4, 0.6, 0.9, 0.7],
                particle_size: 0.06,
                ..Default::default()
            },
            ParticleEffect::Light => ParticleEmitter {
                effect,
                mode: EmissionMode::Continuous { rate: 8.0 },
                max_particles: 50,
                particle_lifetime: 2.5,
                particle_speed: 1.5,
                color: [1.0, 0.8, 0.3, 0.9],
                particle_size: 0.08,
                ..Default::default()
            },
            ParticleEffect::Dust => ParticleEmitter {
                effect,
                mode: EmissionMode::Burst {
                    count: 12,
                    interval: None,
                },
                max_particles: 30,
                particle_lifetime: 1.5,
                particle_speed: 3.0,
                color: [0.6, 0.5, 0.4, 0.6],
                particle_size: 0.04,
                ..Default::default()
            },
            ParticleEffect::Weather => ParticleEmitter {
                effect,
                mode: EmissionMode::Continuous { rate: 25.0 },
                max_particles: 500,
                particle_lifetime: 4.0,
                particle_speed: 12.0,
                color: [0.7, 0.8, 0.9, 0.5],
                particle_size: 0.03,
                ..Default::default()
            },
        };
        Self { emitter: defaults }
    }

    /// Set continuous emission at the given rate (particles/second).
    pub fn continuous(mut self, rate: f32) -> Self {
        self.emitter.mode = EmissionMode::Continuous { rate };
        self
    }

    /// Set burst emission with count and optional repeat interval.
    pub fn burst(mut self, count: u32, interval: Option<f32>) -> Self {
        self.emitter.mode = EmissionMode::Burst { count, interval };
        self
    }

    /// Set the base particle lifetime in seconds.
    pub fn lifetime(mut self, seconds: f32) -> Self {
        self.emitter.particle_lifetime = seconds;
        self
    }

    /// Set the base particle speed.
    pub fn speed(mut self, speed: f32) -> Self {
        self.emitter.particle_speed = speed;
        self
    }

    /// Set the particle color `[R, G, B, A]`.
    pub fn color(mut self, color: [f32; 4]) -> Self {
        self.emitter.color = color;
        self
    }

    /// Set the particle size in world units.
    pub fn size(mut self, size: f32) -> Self {
        self.emitter.particle_size = size;
        self
    }

    /// Set the maximum number of concurrent particles from this emitter.
    pub fn max_particles(mut self, max: u32) -> Self {
        self.emitter.max_particles = max;
        self
    }

    /// Set the emitter lifetime. `None` means infinite.
    pub fn emitter_lifetime(mut self, seconds: Option<f32>) -> Self {
        self.emitter.emitter_lifetime = seconds;
        self
    }

    /// Set whether the emitter starts enabled.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.emitter.enabled = enabled;
        self
    }

    /// Build the configured [`ParticleEmitter`] component.
    pub fn build(self) -> ParticleEmitter {
        self.emitter
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Manages emitter lifecycles — ages emitters and despawns expired ones.
pub fn emitter_lifecycle_system(
    mut commands: Commands,
    time: Res<Time>,
    mut emitters: Query<(Entity, &mut ParticleEmitter)>,
) {
    let dt = time.delta_secs();

    for (entity, mut emitter) in &mut emitters {
        emitter.age += dt;

        // Check if emitter has expired
        if let Some(lifetime) = emitter.emitter_lifetime
            && emitter.age >= lifetime
        {
            // Disable spawning; entity will be cleaned up when all particles die
            emitter.enabled = false;
            if emitter.active_count == 0 {
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Spawns new particles from active emitters.
///
/// Respects the global particle budget ([`ParticleConfig::max_total_particles`])
/// and per-frame spawn limit ([`ParticleConfig::max_spawns_per_frame`]).
pub fn particle_spawn_system(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<ParticleConfig>,
    stats: Res<ParticleStats>,
    mut emitters: Query<(Entity, &mut ParticleEmitter, &GlobalTransform)>,
) {
    if !config.enabled {
        return;
    }

    let dt = time.delta_secs();
    let mut spawned_this_frame: u32 = 0;

    for (emitter_entity, mut emitter, global_transform) in &mut emitters {
        if !emitter.enabled {
            continue;
        }

        // Check per-emitter limit
        if emitter.active_count >= emitter.max_particles {
            continue;
        }

        let spawn_pos = global_transform.translation();
        let to_spawn = calculate_spawn_count(&mut emitter, dt);

        for _ in 0..to_spawn {
            // Check global limits
            if stats.active_count + spawned_this_frame >= config.max_total_particles {
                break;
            }
            if spawned_this_frame >= config.max_spawns_per_frame {
                break;
            }

            let (velocity, gravity) = generate_particle_velocity(&emitter, spawn_pos);

            // Add slight randomness to lifetime
            let lifetime_variance = 0.8 + pseudo_random(spawn_pos, emitter.age + spawned_this_frame as f32) * 0.4;
            let lifetime = emitter.particle_lifetime * lifetime_variance;

            commands.spawn((
                Transform::from_translation(spawn_pos),
                Particle {
                    velocity,
                    lifetime,
                    age: 0.0,
                    effect: emitter.effect,
                    color: emitter.color,
                    size: emitter.particle_size,
                    emitter: emitter_entity,
                    gravity,
                },
            ));

            emitter.active_count += 1;
            spawned_this_frame += 1;
        }
    }
}

/// Calculate how many particles to spawn this frame based on emission mode.
fn calculate_spawn_count(emitter: &mut ParticleEmitter, dt: f32) -> u32 {
    match emitter.mode {
        EmissionMode::Continuous { rate } => {
            emitter.spawn_accumulator += rate * dt;
            let count = emitter.spawn_accumulator.floor() as u32;
            emitter.spawn_accumulator -= count as f32;
            count
        }
        EmissionMode::Burst { count, interval } => {
            if !emitter.burst_fired {
                emitter.burst_fired = true;
                emitter.burst_timer = 0.0;
                return count;
            }
            if let Some(interval_secs) = interval {
                emitter.burst_timer += dt;
                if emitter.burst_timer >= interval_secs {
                    emitter.burst_timer -= interval_secs;
                    return count;
                }
            }
            0
        }
    }
}

/// Generate a velocity vector appropriate for the particle effect type.
fn generate_particle_velocity(emitter: &ParticleEmitter, pos: Vec3) -> (Vec3, f32) {
    let speed = emitter.particle_speed;

    match emitter.effect {
        ParticleEffect::Water => {
            // Upward splash with horizontal spread
            let angle = pseudo_random(pos, emitter.age) * TAU;
            let spread = 0.3 + pseudo_random(pos, emitter.age + 1.0) * 0.7;
            let velocity = Vec3::new(
                angle.cos() * speed * spread * 0.5,
                speed * (0.5 + pseudo_random(pos, emitter.age + 2.0) * 0.5),
                angle.sin() * speed * spread * 0.5,
            );
            (velocity, 1.0) // Full gravity for water
        }
        ParticleEffect::Light => {
            // Gentle upward drift with slight wander
            let angle = pseudo_random(pos, emitter.age) * TAU;
            let velocity = Vec3::new(
                angle.cos() * speed * 0.3,
                speed * 0.5,
                angle.sin() * speed * 0.3,
            );
            (velocity, -0.1) // Slight negative gravity (floats up)
        }
        ParticleEffect::Dust => {
            // Outward burst in all directions
            let theta = pseudo_random(pos, emitter.age) * TAU;
            let phi = pseudo_random(pos, emitter.age + 1.0) * std::f32::consts::PI;
            let velocity = Vec3::new(
                theta.cos() * phi.sin() * speed,
                phi.cos().abs() * speed * 0.5,
                theta.sin() * phi.sin() * speed,
            );
            (velocity, 0.5) // Half gravity
        }
        ParticleEffect::Weather => {
            // Downward with slight horizontal drift
            let drift_x = (pseudo_random(pos, emitter.age) - 0.5) * speed * 0.2;
            let drift_z = (pseudo_random(pos, emitter.age + 1.0) - 0.5) * speed * 0.2;
            let velocity = Vec3::new(drift_x, -speed, drift_z);
            (velocity, 0.0) // No additional gravity (constant speed fall)
        }
    }
}

/// Simple deterministic pseudo-random function based on position and time.
///
/// Returns a value in `[0.0, 1.0)`. Not cryptographically secure,
/// but fast and adequate for particle variation.
fn pseudo_random(pos: Vec3, t: f32) -> f32 {
    let v = (pos.x * 12.9898 + pos.y * 78.233 + pos.z * 45.164 + t * 43_758.547).sin();
    v.fract().abs()
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emitter_builder_water() {
        let emitter = EmitterBuilder::new(ParticleEffect::Water).build();
        assert_eq!(emitter.effect, ParticleEffect::Water);
        assert!(emitter.enabled);
        match emitter.mode {
            EmissionMode::Continuous { rate } => assert!(rate > 0.0),
            _ => panic!("Water should default to continuous"),
        }
    }

    #[test]
    fn test_emitter_builder_dust_burst() {
        let emitter = EmitterBuilder::new(ParticleEffect::Dust).build();
        assert_eq!(emitter.effect, ParticleEffect::Dust);
        match emitter.mode {
            EmissionMode::Burst { count, interval } => {
                assert!(count > 0);
                assert!(interval.is_none(), "Dust default should be one-shot burst");
            }
            _ => panic!("Dust should default to burst"),
        }
    }

    #[test]
    fn test_emitter_builder_overrides() {
        let emitter = EmitterBuilder::new(ParticleEffect::Light)
            .continuous(50.0)
            .lifetime(5.0)
            .speed(10.0)
            .color([1.0, 0.0, 0.0, 1.0])
            .size(0.2)
            .max_particles(300)
            .emitter_lifetime(Some(10.0))
            .build();

        assert_eq!(emitter.particle_lifetime, 5.0);
        assert_eq!(emitter.particle_speed, 10.0);
        assert_eq!(emitter.color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(emitter.particle_size, 0.2);
        assert_eq!(emitter.max_particles, 300);
        assert_eq!(emitter.emitter_lifetime, Some(10.0));
        match emitter.mode {
            EmissionMode::Continuous { rate } => assert_eq!(rate, 50.0),
            _ => panic!("Should be continuous after override"),
        }
    }

    #[test]
    fn test_calculate_spawn_count_continuous() {
        let mut emitter = ParticleEmitter {
            mode: EmissionMode::Continuous { rate: 10.0 },
            ..Default::default()
        };
        // At 10/sec, 0.1s should yield 1 particle
        let count = calculate_spawn_count(&mut emitter, 0.1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_calculate_spawn_count_continuous_accumulation() {
        let mut emitter = ParticleEmitter {
            mode: EmissionMode::Continuous { rate: 3.0 },
            ..Default::default()
        };
        // 3/sec * 0.1s = 0.3, floor = 0
        let count1 = calculate_spawn_count(&mut emitter, 0.1);
        assert_eq!(count1, 0);
        // accumulator = 0.3, + 0.3 = 0.6, floor = 0
        let count2 = calculate_spawn_count(&mut emitter, 0.1);
        assert_eq!(count2, 0);
        // accumulator = 0.6, + 0.3 = 0.9, floor = 0
        let count3 = calculate_spawn_count(&mut emitter, 0.1);
        assert_eq!(count3, 0);
        // accumulator = 0.9, + 0.3 = 1.2, floor = 1
        let count4 = calculate_spawn_count(&mut emitter, 0.1);
        assert_eq!(count4, 1);
    }

    #[test]
    fn test_calculate_spawn_count_burst_one_shot() {
        let mut emitter = ParticleEmitter {
            mode: EmissionMode::Burst {
                count: 10,
                interval: None,
            },
            ..Default::default()
        };
        let count1 = calculate_spawn_count(&mut emitter, 0.016);
        assert_eq!(count1, 10);
        // Subsequent calls should return 0 (one-shot)
        let count2 = calculate_spawn_count(&mut emitter, 0.016);
        assert_eq!(count2, 0);
    }

    #[test]
    fn test_calculate_spawn_count_burst_repeating() {
        let mut emitter = ParticleEmitter {
            mode: EmissionMode::Burst {
                count: 5,
                interval: Some(1.0),
            },
            ..Default::default()
        };
        // First burst fires immediately
        let count1 = calculate_spawn_count(&mut emitter, 0.016);
        assert_eq!(count1, 5);
        // 0.5s later — no burst yet
        let count2 = calculate_spawn_count(&mut emitter, 0.5);
        assert_eq!(count2, 0);
        // 0.6s later (total 1.1s) — second burst
        let count3 = calculate_spawn_count(&mut emitter, 0.6);
        assert_eq!(count3, 5);
    }

    #[test]
    fn test_pseudo_random_deterministic() {
        let pos = Vec3::new(1.0, 2.0, 3.0);
        let a = pseudo_random(pos, 1.0);
        let b = pseudo_random(pos, 1.0);
        assert_eq!(a, b, "Same inputs should give same output");
    }

    #[test]
    fn test_pseudo_random_range() {
        for i in 0..100 {
            let v = pseudo_random(Vec3::new(i as f32, 0.0, 0.0), i as f32 * 0.1);
            assert!((0.0..1.0).contains(&v), "pseudo_random out of range: {v}");
        }
    }

    #[test]
    fn test_generate_velocity_water_has_gravity() {
        let emitter = EmitterBuilder::new(ParticleEffect::Water).build();
        let (_, gravity) = generate_particle_velocity(&emitter, Vec3::ZERO);
        assert_eq!(gravity, 1.0);
    }

    #[test]
    fn test_generate_velocity_light_floats() {
        let emitter = EmitterBuilder::new(ParticleEffect::Light).build();
        let (velocity, gravity) = generate_particle_velocity(&emitter, Vec3::ZERO);
        assert!(gravity < 0.0, "Light particles should float (negative gravity)");
        assert!(velocity.y > 0.0, "Light particles should move upward");
    }

    #[test]
    fn test_generate_velocity_weather_falls() {
        let emitter = EmitterBuilder::new(ParticleEffect::Weather).build();
        let (velocity, _) = generate_particle_velocity(&emitter, Vec3::ZERO);
        assert!(velocity.y < 0.0, "Weather particles should fall");
    }
}
