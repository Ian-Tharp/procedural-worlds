//! Particle emitter with block light context.
//!
//! Emitters are world-positioned sources of particles. Each emitter
//! captures the ambient block light level at its spawn point so that
//! particles it produces inherit an initial brightness.
//!
//! # Light Sampling
//!
//! At creation time (or when repositioned), the emitter samples the
//! block light at its world position via [`BlockLightQuery`]. This
//! "birth light" is passed to each spawned particle and can be
//! overridden per-tick if the emitter moves through differently-lit areas.

use bevy::prelude::*;

use super::effects::ParticleEffectType;
use super::light_query::{BlockLightQuery, LightSample};

// ============================================================================
// EMITTER CONFIGURATION
// ============================================================================

/// Configuration for a particle emitter.
#[derive(Debug, Clone)]
pub struct EmitterConfig {
    /// Maximum particles alive at once from this emitter.
    pub max_particles: u32,
    /// Particles spawned per second.
    pub spawn_rate: f32,
    /// Initial velocity range (min, max) for spawned particles.
    pub velocity_min: Vec3,
    pub velocity_max: Vec3,
    /// Particle lifetime in seconds.
    pub lifetime: f32,
    /// Particle size (uniform scale).
    pub size: f32,
    /// Base color before light modulation.
    pub base_color: Color,
    /// Whether particles inherit the emitter's light level at spawn.
    pub inherit_emitter_light: bool,
    /// Whether particles should sample light at their own position each frame.
    pub per_particle_light_sampling: bool,
}

impl Default for EmitterConfig {
    fn default() -> Self {
        Self {
            max_particles: 100,
            spawn_rate: 10.0,
            velocity_min: Vec3::new(-0.5, 0.5, -0.5),
            velocity_max: Vec3::new(0.5, 2.0, 0.5),
            lifetime: 3.0,
            size: 0.1,
            base_color: Color::WHITE,
            inherit_emitter_light: true,
            per_particle_light_sampling: false,
        }
    }
}

// ============================================================================
// EMITTER COMPONENT
// ============================================================================

/// Component that turns an entity into a particle emitter.
///
/// The emitter samples block light at its position and passes this
/// context to spawned particles. Light is resampled each frame the
/// emitter is active, allowing particles near light sources to brighten
/// and those in shadows to dim.
#[derive(Component, Debug, Clone)]
pub struct ParticleEmitter {
    /// Effect type controlling particle behavior and appearance.
    pub effect_type: ParticleEffectType,
    /// Emitter configuration.
    pub config: EmitterConfig,
    /// Cached light sample at the emitter's current position.
    pub light_sample: LightSample,
    /// Spawn accumulator — fractional particles carried between frames.
    pub spawn_accumulator: f32,
    /// Whether this emitter is currently active (spawning particles).
    pub active: bool,
    /// Total particles currently alive from this emitter.
    pub alive_count: u32,
}

impl ParticleEmitter {
    /// Create a new particle emitter with the given effect type and config.
    pub fn new(effect_type: ParticleEffectType, config: EmitterConfig) -> Self {
        Self {
            effect_type,
            config,
            light_sample: LightSample::fallback(),
            spawn_accumulator: 0.0,
            active: true,
            alive_count: 0,
        }
    }

    /// Create an emitter with default config for the given effect type.
    pub fn from_effect(effect_type: ParticleEffectType) -> Self {
        Self::new(effect_type, effect_type.default_config())
    }

    /// Sample and cache the block light at the given world position.
    pub fn update_light(&mut self, world_pos: Vec3, light_query: &BlockLightQuery) {
        self.light_sample = light_query.sample_at(world_pos);
    }

    /// Compute how many particles to spawn this frame, returning the count
    /// and updating the accumulator for sub-frame precision.
    pub fn compute_spawn_count(&mut self, dt: f32) -> u32 {
        if !self.active {
            return 0;
        }

        let available = self.config.max_particles.saturating_sub(self.alive_count);
        if available == 0 {
            return 0;
        }

        self.spawn_accumulator += self.config.spawn_rate * dt;
        let to_spawn = self.spawn_accumulator.floor() as u32;
        self.spawn_accumulator -= to_spawn as f32;
        to_spawn.min(available)
    }

    /// Get the effective brightness for spawned particles.
    ///
    /// If `inherit_emitter_light` is enabled, returns the cached light
    /// brightness. Otherwise returns 1.0 (full brightness).
    pub fn effective_brightness(&self) -> f32 {
        if self.config.inherit_emitter_light {
            // Apply a minimum floor so particles are never completely invisible
            self.light_sample.brightness.max(0.05)
        } else {
            1.0
        }
    }
}

// ============================================================================
// PARTICLE COMPONENT
// ============================================================================

/// Component for an individual particle spawned by an emitter.
#[derive(Component, Debug, Clone)]
pub struct Particle {
    /// Current velocity.
    pub velocity: Vec3,
    /// Remaining lifetime in seconds.
    pub lifetime: f32,
    /// Maximum lifetime (for computing age ratio).
    pub max_lifetime: f32,
    /// Base color (before light modulation).
    pub base_color: Color,
    /// Block light level at this particle's position (0–15).
    pub light_level: u8,
    /// Normalized brightness from block light (0.0–1.0).
    pub light_brightness: f32,
    /// Particle size.
    pub size: f32,
    /// Whether this particle samples light at its own position each frame.
    pub sample_own_light: bool,
}

impl Particle {
    /// Create a new particle with initial light from its emitter.
    pub fn new(
        velocity: Vec3,
        lifetime: f32,
        base_color: Color,
        light_sample: LightSample,
        size: f32,
        sample_own_light: bool,
    ) -> Self {
        Self {
            velocity,
            lifetime,
            max_lifetime: lifetime,
            base_color,
            light_level: light_sample.block_light,
            light_brightness: light_sample.brightness.max(0.05),
            size,
            sample_own_light,
        }
    }

    /// Update the particle's light level from a new sample.
    pub fn update_light(&mut self, sample: LightSample) {
        self.light_level = sample.block_light;
        self.light_brightness = sample.brightness.max(0.05);
    }

    /// Compute the particle's age ratio (0.0 = just born, 1.0 = about to die).
    pub fn age_ratio(&self) -> f32 {
        if self.max_lifetime <= 0.0 {
            return 1.0;
        }
        1.0 - (self.lifetime / self.max_lifetime).clamp(0.0, 1.0)
    }

    /// Compute the final display color with light modulation applied.
    ///
    /// The base color is multiplied by the light brightness factor,
    /// creating darker particles in shadows and brighter ones near
    /// light sources.
    pub fn lit_color(&self) -> Color {
        let srgba = self.base_color.to_srgba();
        Color::srgba(
            srgba.red * self.light_brightness,
            srgba.green * self.light_brightness,
            srgba.blue * self.light_brightness,
            srgba.alpha,
        )
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// System that updates emitter light samples based on their world position.
pub fn update_emitter_light(
    light_query: Res<BlockLightQuery>,
    mut emitters: Query<(&GlobalTransform, &mut ParticleEmitter)>,
) {
    for (transform, mut emitter) in &mut emitters {
        emitter.update_light(transform.translation(), &light_query);
    }
}

/// System that updates per-particle light levels for particles that
/// have `sample_own_light` enabled.
pub fn update_particle_light(
    light_query: Res<BlockLightQuery>,
    mut particles: Query<(&GlobalTransform, &mut Particle)>,
) {
    for (transform, mut particle) in &mut particles {
        if particle.sample_own_light {
            let sample = light_query.sample_at(transform.translation());
            particle.update_light(sample);
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
    fn test_emitter_creation() {
        let emitter = ParticleEmitter::from_effect(ParticleEffectType::TorchFlame);
        assert!(emitter.active);
        assert_eq!(emitter.alive_count, 0);
        assert_eq!(emitter.spawn_accumulator, 0.0);
    }

    #[test]
    fn test_emitter_spawn_count() {
        let mut emitter = ParticleEmitter::from_effect(ParticleEffectType::TorchFlame);
        emitter.config.spawn_rate = 10.0;
        emitter.config.max_particles = 100;

        // At 10 particles/sec, 0.1s should give 1 particle
        let count = emitter.compute_spawn_count(0.1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_emitter_spawn_count_respects_max() {
        let mut emitter = ParticleEmitter::from_effect(ParticleEffectType::TorchFlame);
        emitter.config.spawn_rate = 100.0;
        emitter.config.max_particles = 5;
        emitter.alive_count = 5;

        let count = emitter.compute_spawn_count(1.0);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_emitter_inactive_no_spawn() {
        let mut emitter = ParticleEmitter::from_effect(ParticleEffectType::TorchFlame);
        emitter.active = false;

        let count = emitter.compute_spawn_count(1.0);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_emitter_effective_brightness() {
        // Use Smoke (light-responsive) to test brightness modulation
        let mut emitter = ParticleEmitter::from_effect(ParticleEffectType::Smoke);
        assert!(emitter.config.inherit_emitter_light); // Smoke inherits light

        emitter.light_sample = LightSample::from_level(15);
        assert!((emitter.effective_brightness() - 1.0).abs() < f32::EPSILON);

        emitter.light_sample = LightSample::from_level(0);
        // Should clamp to minimum 0.05
        assert!((emitter.effective_brightness() - 0.05).abs() < 0.001);
    }

    #[test]
    fn test_emitter_brightness_disabled() {
        // TorchFlame is self-illuminated (inherit_emitter_light = false)
        let mut emitter = ParticleEmitter::from_effect(ParticleEffectType::TorchFlame);
        assert!(!emitter.config.inherit_emitter_light);
        emitter.light_sample = LightSample::from_level(0);
        assert!((emitter.effective_brightness() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_particle_creation() {
        let particle = Particle::new(
            Vec3::Y,
            3.0,
            Color::WHITE,
            LightSample::from_level(10),
            0.1,
            false,
        );
        assert_eq!(particle.light_level, 10);
        assert!((particle.light_brightness - 10.0 / 15.0).abs() < 0.001);
        assert!((particle.age_ratio() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_particle_age_ratio() {
        let mut particle = Particle::new(
            Vec3::Y,
            4.0,
            Color::WHITE,
            LightSample::max(),
            0.1,
            false,
        );
        assert!((particle.age_ratio() - 0.0).abs() < f32::EPSILON);

        particle.lifetime = 2.0; // Half spent
        assert!((particle.age_ratio() - 0.5).abs() < f32::EPSILON);

        particle.lifetime = 0.0; // Fully spent
        assert!((particle.age_ratio() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_particle_lit_color_bright() {
        let particle = Particle::new(
            Vec3::Y,
            3.0,
            Color::srgba(1.0, 0.5, 0.25, 1.0),
            LightSample::from_level(15),
            0.1,
            false,
        );
        let lit = particle.lit_color().to_srgba();
        assert!((lit.red - 1.0).abs() < 0.01);
        assert!((lit.green - 0.5).abs() < 0.01);
        assert!((lit.blue - 0.25).abs() < 0.01);
        assert!((lit.alpha - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_particle_lit_color_dim() {
        let particle = Particle::new(
            Vec3::Y,
            3.0,
            Color::srgba(1.0, 1.0, 1.0, 1.0),
            LightSample::from_level(0),
            0.1,
            false,
        );
        let lit = particle.lit_color().to_srgba();
        // Brightness should be clamped to 0.05 minimum
        assert!((lit.red - 0.05).abs() < 0.01);
        assert!((lit.green - 0.05).abs() < 0.01);
        assert!((lit.blue - 0.05).abs() < 0.01);
    }

    #[test]
    fn test_particle_update_light() {
        let mut particle = Particle::new(
            Vec3::Y,
            3.0,
            Color::WHITE,
            LightSample::from_level(5),
            0.1,
            true,
        );
        assert_eq!(particle.light_level, 5);

        particle.update_light(LightSample::from_level(12));
        assert_eq!(particle.light_level, 12);
        assert!((particle.light_brightness - 12.0 / 15.0).abs() < 0.001);
    }
}
