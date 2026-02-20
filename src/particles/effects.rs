//! Particle effect types and their light-aware configurations.
//!
//! Each [`ParticleEffectType`] defines a visual behavior (flame, smoke, etc.)
//! along with default emitter parameters. Effects integrate with the block
//! light system through per-particle light sampling, allowing particles to
//! respond dynamically to nearby light sources.
//!
//! # Light Integration
//!
//! Effects that are themselves light sources (e.g., torch flames) use
//! `self_illuminated: true`, meaning they always render at full brightness
//! regardless of ambient block light. Environmental effects (smoke, rain)
//! sample block light to blend naturally with their surroundings.

use bevy::prelude::*;

use super::emitter::EmitterConfig;
use super::light_query::LightSample;

// ============================================================================
// EFFECT TYPES
// ============================================================================

/// Predefined particle effect types with light-aware behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParticleEffectType {
    /// Torch/fire flame particles — self-illuminated, warm colors.
    TorchFlame,
    /// Smoke particles — affected by ambient light, drift upward.
    Smoke,
    /// Lava bubble particles — self-illuminated, orange/red.
    LavaBubble,
    /// Rain splash particles — affected by ambient light.
    RainSplash,
    /// Snow drift particles — affected by ambient light, slow.
    SnowDrift,
    /// Block break particles — inherit block color, affected by light.
    BlockBreak,
    /// Ambient dust/mote particles — subtle, light-responsive.
    AmbientDust,
}

impl ParticleEffectType {
    /// Whether this effect type is self-illuminated (ignores ambient light).
    ///
    /// Self-illuminated particles always render at full brightness since
    /// they represent light sources themselves (flames, lava glow).
    pub fn is_self_illuminated(&self) -> bool {
        matches!(self, Self::TorchFlame | Self::LavaBubble)
    }

    /// Get the default emitter configuration for this effect type.
    pub fn default_config(&self) -> EmitterConfig {
        match self {
            Self::TorchFlame => EmitterConfig {
                max_particles: 30,
                spawn_rate: 8.0,
                velocity_min: Vec3::new(-0.1, 0.3, -0.1),
                velocity_max: Vec3::new(0.1, 1.0, 0.1),
                lifetime: 0.8,
                size: 0.08,
                base_color: Color::srgba(1.0, 0.7, 0.2, 0.9),
                inherit_emitter_light: false, // Self-illuminated
                per_particle_light_sampling: false,
            },
            Self::Smoke => EmitterConfig {
                max_particles: 50,
                spawn_rate: 5.0,
                velocity_min: Vec3::new(-0.2, 0.5, -0.2),
                velocity_max: Vec3::new(0.2, 1.5, 0.2),
                lifetime: 2.5,
                size: 0.15,
                base_color: Color::srgba(0.4, 0.4, 0.4, 0.6),
                inherit_emitter_light: true,
                per_particle_light_sampling: true,
            },
            Self::LavaBubble => EmitterConfig {
                max_particles: 20,
                spawn_rate: 3.0,
                velocity_min: Vec3::new(-0.05, 0.1, -0.05),
                velocity_max: Vec3::new(0.05, 0.5, 0.05),
                lifetime: 1.2,
                size: 0.12,
                base_color: Color::srgba(1.0, 0.4, 0.05, 0.95),
                inherit_emitter_light: false, // Self-illuminated
                per_particle_light_sampling: false,
            },
            Self::RainSplash => EmitterConfig {
                max_particles: 80,
                spawn_rate: 20.0,
                velocity_min: Vec3::new(-0.3, -4.0, -0.3),
                velocity_max: Vec3::new(0.3, -2.0, 0.3),
                lifetime: 1.5,
                size: 0.04,
                base_color: Color::srgba(0.6, 0.7, 0.9, 0.5),
                inherit_emitter_light: true,
                per_particle_light_sampling: false,
            },
            Self::SnowDrift => EmitterConfig {
                max_particles: 60,
                spawn_rate: 12.0,
                velocity_min: Vec3::new(-0.5, -0.8, -0.5),
                velocity_max: Vec3::new(0.5, -0.3, 0.5),
                lifetime: 4.0,
                size: 0.05,
                base_color: Color::srgba(0.95, 0.95, 1.0, 0.8),
                inherit_emitter_light: true,
                per_particle_light_sampling: true,
            },
            Self::BlockBreak => EmitterConfig {
                max_particles: 15,
                spawn_rate: 100.0, // Burst: high rate, short window
                velocity_min: Vec3::new(-1.5, 0.5, -1.5),
                velocity_max: Vec3::new(1.5, 3.0, 1.5),
                lifetime: 1.0,
                size: 0.06,
                base_color: Color::srgba(0.6, 0.5, 0.4, 1.0),
                inherit_emitter_light: true,
                per_particle_light_sampling: false,
            },
            Self::AmbientDust => EmitterConfig {
                max_particles: 40,
                spawn_rate: 4.0,
                velocity_min: Vec3::new(-0.1, -0.05, -0.1),
                velocity_max: Vec3::new(0.1, 0.05, 0.1),
                lifetime: 5.0,
                size: 0.03,
                base_color: Color::srgba(0.8, 0.75, 0.65, 0.3),
                inherit_emitter_light: true,
                per_particle_light_sampling: true,
            },
        }
    }

    /// Apply light modulation to a particle color based on this effect type.
    ///
    /// Self-illuminated effects ignore the light sample and return the
    /// base color at full brightness. Other effects modulate the color
    /// by the light brightness factor.
    pub fn apply_light_to_color(&self, base_color: Color, sample: LightSample) -> Color {
        if self.is_self_illuminated() {
            return base_color;
        }

        let brightness = sample.brightness.max(0.05); // Floor to prevent invisible
        let srgba = base_color.to_srgba();
        Color::srgba(
            srgba.red * brightness,
            srgba.green * brightness,
            srgba.blue * brightness,
            srgba.alpha,
        )
    }

    /// Get the additional glow contribution this effect type adds to
    /// nearby blocks (for future bi-directional light integration).
    ///
    /// Returns 0 for non-light-source effects.
    pub fn glow_emission(&self) -> u8 {
        match self {
            Self::TorchFlame => 14,
            Self::LavaBubble => 12,
            _ => 0,
        }
    }

    /// All available effect types (for iteration/testing).
    pub fn all() -> &'static [ParticleEffectType] {
        &[
            Self::TorchFlame,
            Self::Smoke,
            Self::LavaBubble,
            Self::RainSplash,
            Self::SnowDrift,
            Self::BlockBreak,
            Self::AmbientDust,
        ]
    }
}

// ============================================================================
// LIGHT-RESPONSIVE PARTICLE DATA
// ============================================================================

/// Per-particle light data packed for efficient vertex attribute passing.
///
/// This struct holds the light information needed by the particle renderer
/// to modulate vertex colors in the shader.
#[derive(Debug, Clone, Copy)]
pub struct ParticleLightData {
    /// Block light level (0–15), stored as u8 for compact vertex attributes.
    pub block_light: u8,
    /// Whether this particle is self-illuminated (always full brightness).
    pub self_illuminated: bool,
}

impl ParticleLightData {
    /// Create light data for a light-responsive particle.
    pub fn responsive(block_light: u8) -> Self {
        Self {
            block_light: block_light.min(15),
            self_illuminated: false,
        }
    }

    /// Create light data for a self-illuminated particle.
    pub fn self_lit() -> Self {
        Self {
            block_light: 15,
            self_illuminated: true,
        }
    }

    /// Pack into a single f32 for vertex attribute transport.
    ///
    /// Encoding: light_level / 15.0 for responsive, 1.0 + flag for self-lit.
    /// The shader unpacks this to determine brightness.
    pub fn pack_to_f32(&self) -> f32 {
        if self.self_illuminated {
            // Encode as > 1.0 to signal self-illumination to the shader
            1.0 + (self.block_light as f32 / 15.0)
        } else {
            (self.block_light as f32 / 15.0).clamp(0.0, 1.0)
        }
    }

    /// Unpack from a f32 vertex attribute value.
    pub fn unpack_from_f32(packed: f32) -> Self {
        if packed > 1.0 {
            Self {
                block_light: ((packed - 1.0) * 15.0).round() as u8,
                self_illuminated: true,
            }
        } else {
            Self {
                block_light: (packed * 15.0).round() as u8,
                self_illuminated: false,
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
    fn test_self_illuminated_effects() {
        assert!(ParticleEffectType::TorchFlame.is_self_illuminated());
        assert!(ParticleEffectType::LavaBubble.is_self_illuminated());
        assert!(!ParticleEffectType::Smoke.is_self_illuminated());
        assert!(!ParticleEffectType::RainSplash.is_self_illuminated());
        assert!(!ParticleEffectType::SnowDrift.is_self_illuminated());
        assert!(!ParticleEffectType::BlockBreak.is_self_illuminated());
        assert!(!ParticleEffectType::AmbientDust.is_self_illuminated());
    }

    #[test]
    fn test_effect_glow_emission() {
        assert_eq!(ParticleEffectType::TorchFlame.glow_emission(), 14);
        assert_eq!(ParticleEffectType::LavaBubble.glow_emission(), 12);
        assert_eq!(ParticleEffectType::Smoke.glow_emission(), 0);
        assert_eq!(ParticleEffectType::RainSplash.glow_emission(), 0);
    }

    #[test]
    fn test_apply_light_self_illuminated() {
        let color = Color::srgba(1.0, 0.5, 0.2, 0.9);
        let dark = LightSample::from_level(0);
        let result = ParticleEffectType::TorchFlame.apply_light_to_color(color, dark);
        let result_srgba = result.to_srgba();
        // Self-illuminated should not be darkened
        assert!((result_srgba.red - 1.0).abs() < 0.01);
        assert!((result_srgba.green - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_apply_light_responsive_bright() {
        let color = Color::srgba(1.0, 1.0, 1.0, 1.0);
        let bright = LightSample::from_level(15);
        let result = ParticleEffectType::Smoke.apply_light_to_color(color, bright);
        let result_srgba = result.to_srgba();
        assert!((result_srgba.red - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_apply_light_responsive_dim() {
        let color = Color::srgba(1.0, 1.0, 1.0, 1.0);
        let dim = LightSample::from_level(3);
        let result = ParticleEffectType::Smoke.apply_light_to_color(color, dim);
        let result_srgba = result.to_srgba();
        let expected_brightness = 3.0 / 15.0; // 0.2
        assert!((result_srgba.red - expected_brightness).abs() < 0.01);
    }

    #[test]
    fn test_apply_light_preserves_alpha() {
        let color = Color::srgba(1.0, 1.0, 1.0, 0.5);
        let dim = LightSample::from_level(5);
        let result = ParticleEffectType::Smoke.apply_light_to_color(color, dim);
        let result_srgba = result.to_srgba();
        assert!((result_srgba.alpha - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_default_configs_valid() {
        for effect in ParticleEffectType::all() {
            let config = effect.default_config();
            assert!(config.max_particles > 0, "{effect:?} has 0 max_particles");
            assert!(config.spawn_rate > 0.0, "{effect:?} has 0 spawn_rate");
            assert!(config.lifetime > 0.0, "{effect:?} has 0 lifetime");
            assert!(config.size > 0.0, "{effect:?} has 0 size");
        }
    }

    #[test]
    fn test_self_illuminated_no_per_particle_sampling() {
        // Self-illuminated effects shouldn't waste cycles on per-particle light
        for effect in ParticleEffectType::all() {
            let config = effect.default_config();
            if effect.is_self_illuminated() {
                assert!(
                    !config.inherit_emitter_light,
                    "{effect:?} is self-illuminated but inherits emitter light"
                );
            }
        }
    }

    #[test]
    fn test_particle_light_data_pack_unpack_responsive() {
        let data = ParticleLightData::responsive(10);
        let packed = data.pack_to_f32();
        let unpacked = ParticleLightData::unpack_from_f32(packed);
        assert_eq!(unpacked.block_light, 10);
        assert!(!unpacked.self_illuminated);
    }

    #[test]
    fn test_particle_light_data_pack_unpack_self_lit() {
        let data = ParticleLightData::self_lit();
        let packed = data.pack_to_f32();
        let unpacked = ParticleLightData::unpack_from_f32(packed);
        assert_eq!(unpacked.block_light, 15);
        assert!(unpacked.self_illuminated);
    }

    #[test]
    fn test_particle_light_data_clamps() {
        let data = ParticleLightData::responsive(255);
        assert_eq!(data.block_light, 15);
    }

    #[test]
    fn test_all_effects_enumerated() {
        let all = ParticleEffectType::all();
        assert_eq!(all.len(), 7);
    }
}
