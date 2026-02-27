//! Particle Effects System
//!
//! A modular, high-performance particle system built on Bevy's ECS architecture.
//! Supports water splash, light emission, dust/debris, and weather particle effects.
//!
//! # Architecture
//!
//! ```text
//! ParticleConfig (Resource) — global settings (pool sizes, limits)
//!     ↓
//! ParticleEmitter (Component) — attached to entities that spawn particles
//!     ↓
//! Particle (Component) — individual particle with velocity, lifetime, age
//!     ↓
//! ParticleRendererSystem — batched billboard rendering
//! ```
//!
//! # Performance
//!
//! The system uses object pooling and batched spawning to maintain 60fps
//! with 5K+ concurrent particles. Particles are despawned when their
//! lifetime expires or they leave the world bounds.

pub mod effects;
pub mod emitter;
pub mod renderer;

use bevy::prelude::*;

pub use effects::{DustParticleEffect, LightEmissionEffect, WaterSplashEffect, WeatherParticleEffect};
pub use emitter::EmitterBuilder;

// ============================================================================
// CORE COMPONENTS
// ============================================================================

/// The type of particle effect an emitter produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum ParticleEffect {
    /// Water spray/splash from movement through water.
    Water,
    /// Glowing particles from light sources (torches, fire, magic).
    Light,
    /// Dust and debris from block interactions (mining, placing, breaking).
    Dust,
    /// Weather particles (rain drops, snowflakes, storm debris).
    Weather,
}

/// How an emitter spawns particles over time.
#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub enum EmissionMode {
    /// Spawns particles at a steady rate (particles per second).
    Continuous {
        /// Particles spawned per second.
        rate: f32,
    },
    /// Spawns a burst of particles at once, then optionally repeats.
    Burst {
        /// Number of particles per burst.
        count: u32,
        /// Seconds between bursts. `None` means fire once.
        interval: Option<f32>,
    },
}

impl Default for EmissionMode {
    fn default() -> Self {
        Self::Continuous { rate: 10.0 }
    }
}

/// Component attached to entities that emit particles.
///
/// Configure via [`EmitterBuilder`] for ergonomic setup.
#[derive(Component, Debug, Clone, Reflect)]
pub struct ParticleEmitter {
    /// The effect type this emitter produces.
    pub effect: ParticleEffect,
    /// How particles are spawned (continuous vs burst).
    pub mode: EmissionMode,
    /// Maximum number of live particles from this emitter.
    pub max_particles: u32,
    /// Current count of live particles owned by this emitter.
    pub active_count: u32,
    /// Base lifetime for spawned particles (seconds).
    pub particle_lifetime: f32,
    /// Base initial speed for spawned particles.
    pub particle_speed: f32,
    /// Color tint for particles `[R, G, B, A]`.
    pub color: [f32; 4],
    /// Size of each particle (world units).
    pub particle_size: f32,
    /// Whether this emitter is currently active.
    pub enabled: bool,
    /// Accumulator for continuous emission timing.
    pub spawn_accumulator: f32,
    /// Timer for burst mode interval.
    pub burst_timer: f32,
    /// Whether the initial burst has fired (for one-shot bursts).
    pub burst_fired: bool,
    /// Optional: total emitter lifetime. `None` = infinite.
    pub emitter_lifetime: Option<f32>,
    /// How long this emitter has been alive (seconds).
    pub age: f32,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            effect: ParticleEffect::Dust,
            mode: EmissionMode::default(),
            max_particles: 100,
            active_count: 0,
            particle_lifetime: 2.0,
            particle_speed: 5.0,
            color: [1.0, 1.0, 1.0, 1.0],
            particle_size: 0.1,
            enabled: true,
            spawn_accumulator: 0.0,
            burst_timer: 0.0,
            burst_fired: false,
            emitter_lifetime: None,
            age: 0.0,
        }
    }
}

/// Component for individual particles.
///
/// Each particle is a lightweight entity with velocity and lifetime.
/// The particle system updates position via velocity each frame and
/// despawns particles when their lifetime expires.
#[derive(Component, Debug, Clone, Reflect)]
pub struct Particle {
    /// Current velocity in world units per second.
    pub velocity: Vec3,
    /// Total lifetime of this particle (seconds).
    pub lifetime: f32,
    /// How long this particle has been alive (seconds).
    pub age: f32,
    /// The effect type (for rendering decisions).
    pub effect: ParticleEffect,
    /// Color tint `[R, G, B, A]`.
    pub color: [f32; 4],
    /// Size in world units.
    pub size: f32,
    /// Entity of the emitter that spawned this particle.
    pub emitter: Entity,
    /// Gravity multiplier (0.0 = no gravity, 1.0 = full gravity).
    pub gravity: f32,
}

/// Global configuration resource for the particle system.
///
/// Controls system-wide limits and performance tuning.
#[derive(Resource, Debug, Clone, Reflect)]
pub struct ParticleConfig {
    /// Maximum total particles across all emitters.
    pub max_total_particles: u32,
    /// Maximum particles spawned per frame (throttle).
    pub max_spawns_per_frame: u32,
    /// Global gravity applied to particles (units/sec²).
    pub gravity: f32,
    /// Below this Y coordinate, particles are despawned.
    pub despawn_y: f32,
    /// Whether the particle system is globally enabled.
    pub enabled: bool,
}

impl Default for ParticleConfig {
    fn default() -> Self {
        Self {
            max_total_particles: 10_000,
            max_spawns_per_frame: 50,
            gravity: 9.81,
            despawn_y: -10.0,
            enabled: true,
        }
    }
}

/// Resource tracking the total number of active particles.
#[derive(Resource, Default, Debug)]
pub struct ParticleStats {
    /// Current number of live particle entities.
    pub active_count: u32,
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds the particle effects system.
///
/// Registers resources, components, and systems for particle emission,
/// simulation, and rendering.
pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleConfig>()
            .init_resource::<ParticleStats>()
            .register_type::<ParticleEmitter>()
            .register_type::<Particle>()
            .register_type::<ParticleConfig>()
            .add_systems(
                Update,
                (
                    emitter::emitter_lifecycle_system,
                    emitter::particle_spawn_system,
                    particle_update_system,
                    particle_despawn_system,
                    update_particle_stats_system,
                )
                    .chain(),
            );
    }
}

// ============================================================================
// CORE SYSTEMS
// ============================================================================

/// Updates particle positions based on velocity and applies gravity.
fn particle_update_system(
    time: Res<Time>,
    config: Res<ParticleConfig>,
    mut particles: Query<(&mut Transform, &mut Particle)>,
) {
    if !config.enabled {
        return;
    }

    let dt = time.delta_secs();
    let gravity_vec = Vec3::new(0.0, -config.gravity, 0.0);

    for (mut transform, mut particle) in &mut particles {
        particle.age += dt;
        let grav = particle.gravity;
        particle.velocity += gravity_vec * grav * dt;
        transform.translation += particle.velocity * dt;
    }
}

/// Despawns particles that have exceeded their lifetime or fallen below bounds.
fn particle_despawn_system(
    mut commands: Commands,
    config: Res<ParticleConfig>,
    particles: Query<(Entity, &Transform, &Particle)>,
    mut emitters: Query<&mut ParticleEmitter>,
) {
    for (entity, transform, particle) in &particles {
        let should_despawn = particle.age >= particle.lifetime
            || transform.translation.y < config.despawn_y;

        if should_despawn {
            // Decrement the emitter's active count
            if let Ok(mut emitter) = emitters.get_mut(particle.emitter) {
                emitter.active_count = emitter.active_count.saturating_sub(1);
            }
            commands.entity(entity).despawn();
        }
    }
}

/// Updates the global particle stats resource.
fn update_particle_stats_system(
    mut stats: ResMut<ParticleStats>,
    particles: Query<&Particle>,
) {
    stats.active_count = particles.iter().count() as u32;
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_particle_config_default() {
        let config = ParticleConfig::default();
        assert_eq!(config.max_total_particles, 10_000);
        assert_eq!(config.max_spawns_per_frame, 50);
        assert!(config.enabled);
    }

    #[test]
    fn test_particle_emitter_default() {
        let emitter = ParticleEmitter::default();
        assert_eq!(emitter.effect, ParticleEffect::Dust);
        assert!(emitter.enabled);
        assert_eq!(emitter.active_count, 0);
        assert_eq!(emitter.max_particles, 100);
    }

    #[test]
    fn test_emission_mode_default() {
        let mode = EmissionMode::default();
        match mode {
            EmissionMode::Continuous { rate } => assert_eq!(rate, 10.0),
            _ => panic!("Default should be Continuous"),
        }
    }

    #[test]
    fn test_particle_effect_variants() {
        let effects = [
            ParticleEffect::Water,
            ParticleEffect::Light,
            ParticleEffect::Dust,
            ParticleEffect::Weather,
        ];
        assert_eq!(effects.len(), 4);
        assert_ne!(effects[0], effects[1]);
    }

    #[test]
    fn test_particle_plugin_builds() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins(ParticlePlugin);
        // Verify resources were inserted
        assert!(app.world().get_resource::<ParticleConfig>().is_some());
        assert!(app.world().get_resource::<ParticleStats>().is_some());
    }
}
