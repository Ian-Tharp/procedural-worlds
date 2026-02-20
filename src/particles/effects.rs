//! Predefined Particle Effects
//!
//! High-level effect constructors that integrate with existing game systems:
//! water physics, light sources, block interactions, and weather.

#[cfg(test)]
use super::EmissionMode;
use super::{EmitterBuilder, ParticleEffect, ParticleEmitter};
use crate::weather::WeatherType;

// ============================================================================
// WATER SPLASH EFFECT
// ============================================================================

/// Configuration for water splash/spray particles.
///
/// Spawns short-lived blue-tinted particles when entities move through water.
/// Velocity of the source entity scales the emission rate and splash intensity.
pub struct WaterSplashEffect;

impl WaterSplashEffect {
    /// Create a splash emitter for a given movement speed.
    ///
    /// Higher speeds produce more particles with greater velocity.
    /// Returns a [`ParticleEmitter`] component to attach to the splashing entity.
    pub fn from_speed(speed: f32) -> ParticleEmitter {
        let rate = (speed * 5.0).clamp(5.0, 40.0);
        let particle_speed = (speed * 1.5).clamp(2.0, 12.0);
        let alpha = (speed * 0.15).clamp(0.3, 0.8);

        EmitterBuilder::new(ParticleEffect::Water)
            .continuous(rate)
            .speed(particle_speed)
            .lifetime(0.8 + speed * 0.1)
            .color([0.4, 0.6, 0.95, alpha])
            .size(0.04 + speed * 0.005)
            .max_particles(150)
            .build()
    }

    /// Create a one-shot splash burst (e.g., entity entering water).
    pub fn splash_burst(intensity: f32) -> ParticleEmitter {
        let count = (intensity * 15.0).clamp(5.0, 30.0) as u32;

        EmitterBuilder::new(ParticleEffect::Water)
            .burst(count, None)
            .speed(intensity * 2.0 + 4.0)
            .lifetime(1.2)
            .color([0.5, 0.7, 1.0, 0.7])
            .size(0.06)
            .max_particles(count)
            .emitter_lifetime(Some(2.0))
            .build()
    }
}

// ============================================================================
// LIGHT EMISSION EFFECT
// ============================================================================

/// Configuration for light-source particle effects.
///
/// Produces warm, upward-drifting particles anchored to light sources.
/// Integrates with `BlockDefinition.visuals.light_level` — brighter blocks
/// emit more particles.
pub struct LightEmissionEffect;

impl LightEmissionEffect {
    /// Create a light particle emitter scaled to the block's light level.
    ///
    /// - `light_level`: 0–15 (from `BlockVisuals.light_level`)
    /// - Returns `None` if light_level is 0 (no emission).
    pub fn from_light_level(light_level: u8) -> Option<ParticleEmitter> {
        if light_level == 0 {
            return None;
        }

        let intensity = light_level as f32 / 15.0;
        let rate = 2.0 + intensity * 8.0;

        Some(
            EmitterBuilder::new(ParticleEffect::Light)
                .continuous(rate)
                .speed(0.8 + intensity * 1.5)
                .lifetime(1.5 + intensity * 2.0)
                .color([1.0, 0.7 + intensity * 0.2, 0.2 + intensity * 0.2, 0.6 + intensity * 0.3])
                .size(0.04 + intensity * 0.06)
                .max_particles((10.0 + intensity * 40.0) as u32)
                .build(),
        )
    }

    /// Create a torch/fire particle emitter.
    pub fn torch() -> ParticleEmitter {
        EmitterBuilder::new(ParticleEffect::Light)
            .continuous(6.0)
            .speed(1.2)
            .lifetime(2.0)
            .color([1.0, 0.6, 0.1, 0.8])
            .size(0.05)
            .max_particles(30)
            .build()
    }

    /// Create a magical glow particle emitter.
    pub fn magical(color: [f32; 4]) -> ParticleEmitter {
        EmitterBuilder::new(ParticleEffect::Light)
            .continuous(12.0)
            .speed(2.0)
            .lifetime(3.0)
            .color(color)
            .size(0.07)
            .max_particles(60)
            .build()
    }
}

// ============================================================================
// DUST PARTICLE EFFECT
// ============================================================================

/// Configuration for dust and debris particles from block interactions.
///
/// Spawns outward bursts when blocks are mined, placed, or destroyed.
pub struct DustParticleEffect;

impl DustParticleEffect {
    /// Create a dust burst for block breaking.
    ///
    /// - `block_color`: base color of the broken block `[R, G, B, A]`
    /// - `hardness`: block hardness (affects particle count and speed)
    pub fn block_break(block_color: [f32; 4], hardness: f32) -> ParticleEmitter {
        let count = (8.0 + hardness * 3.0).clamp(8.0, 25.0) as u32;
        let speed = 2.0 + hardness * 0.5;

        EmitterBuilder::new(ParticleEffect::Dust)
            .burst(count, None)
            .speed(speed)
            .lifetime(1.0 + hardness * 0.2)
            .color(block_color)
            .size(0.03 + hardness * 0.005)
            .max_particles(count)
            .emitter_lifetime(Some(2.0))
            .build()
    }

    /// Create a small dust puff for block placement.
    pub fn block_place(block_color: [f32; 4]) -> ParticleEmitter {
        EmitterBuilder::new(ParticleEffect::Dust)
            .burst(6, None)
            .speed(1.5)
            .lifetime(0.8)
            .color([
                block_color[0] * 0.8,
                block_color[1] * 0.8,
                block_color[2] * 0.8,
                0.4,
            ])
            .size(0.03)
            .max_particles(6)
            .emitter_lifetime(Some(1.5))
            .build()
    }

    /// Create footstep dust particles.
    pub fn footstep(surface_color: [f32; 4]) -> ParticleEmitter {
        EmitterBuilder::new(ParticleEffect::Dust)
            .burst(3, Some(0.4))
            .speed(1.0)
            .lifetime(0.6)
            .color([
                surface_color[0] * 0.7,
                surface_color[1] * 0.7,
                surface_color[2] * 0.7,
                0.3,
            ])
            .size(0.02)
            .max_particles(10)
            .build()
    }
}

// ============================================================================
// WEATHER PARTICLE EFFECT
// ============================================================================

/// Configuration for weather particles that integrates with the weather system.
///
/// Reads from [`WeatherType`] to produce appropriate precipitation particles.
pub struct WeatherParticleEffect;

impl WeatherParticleEffect {
    /// Create a weather particle emitter for the given weather type and intensity.
    ///
    /// - `weather`: current weather type
    /// - `intensity`: 0.0–1.0 (from `WeatherState.intensity`)
    /// - Returns `None` for `WeatherType::Clear`.
    pub fn from_weather(weather: WeatherType, intensity: f32) -> Option<ParticleEmitter> {
        match weather {
            WeatherType::Clear => None,
            WeatherType::Rain => Some(Self::rain(intensity)),
            WeatherType::Snow => Some(Self::snow(intensity)),
            WeatherType::Storm => Some(Self::storm(intensity)),
        }
    }

    /// Rain particle emitter.
    fn rain(intensity: f32) -> ParticleEmitter {
        let rate = 10.0 + intensity * 30.0;
        EmitterBuilder::new(ParticleEffect::Weather)
            .continuous(rate)
            .speed(15.0 + intensity * 10.0)
            .lifetime(3.0)
            .color([0.6, 0.7, 0.9, 0.3 + intensity * 0.3])
            .size(0.02)
            .max_particles((rate * 3.0) as u32)
            .build()
    }

    /// Snow particle emitter.
    fn snow(intensity: f32) -> ParticleEmitter {
        let rate = 8.0 + intensity * 20.0;
        EmitterBuilder::new(ParticleEffect::Weather)
            .continuous(rate)
            .speed(3.0 + intensity * 2.0)
            .lifetime(5.0)
            .color([0.95, 0.95, 1.0, 0.5 + intensity * 0.3])
            .size(0.04)
            .max_particles((rate * 5.0) as u32)
            .build()
    }

    /// Storm particle emitter (heavy rain + debris).
    fn storm(intensity: f32) -> ParticleEmitter {
        let rate = 20.0 + intensity * 40.0;
        EmitterBuilder::new(ParticleEffect::Weather)
            .continuous(rate)
            .speed(20.0 + intensity * 15.0)
            .lifetime(2.5)
            .color([0.5, 0.55, 0.7, 0.4 + intensity * 0.4])
            .size(0.025)
            .max_particles((rate * 2.5) as u32)
            .build()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_water_splash_from_speed() {
        let emitter = WaterSplashEffect::from_speed(5.0);
        assert_eq!(emitter.effect, ParticleEffect::Water);
        assert!(emitter.enabled);
        match emitter.mode {
            EmissionMode::Continuous { rate } => assert!(rate > 0.0),
            _ => panic!("Water splash should be continuous"),
        }
    }

    #[test]
    fn test_water_splash_burst() {
        let emitter = WaterSplashEffect::splash_burst(2.0);
        assert_eq!(emitter.effect, ParticleEffect::Water);
        match emitter.mode {
            EmissionMode::Burst { count, interval } => {
                assert!(count > 0);
                assert!(interval.is_none());
            }
            _ => panic!("Splash burst should be burst mode"),
        }
        assert!(emitter.emitter_lifetime.is_some());
    }

    #[test]
    fn test_light_emission_zero_returns_none() {
        assert!(LightEmissionEffect::from_light_level(0).is_none());
    }

    #[test]
    fn test_light_emission_nonzero_returns_some() {
        let emitter = LightEmissionEffect::from_light_level(10).unwrap();
        assert_eq!(emitter.effect, ParticleEffect::Light);
    }

    #[test]
    fn test_light_emission_max_level() {
        let emitter = LightEmissionEffect::from_light_level(15).unwrap();
        assert_eq!(emitter.effect, ParticleEffect::Light);
        match emitter.mode {
            EmissionMode::Continuous { rate } => assert!(rate >= 10.0),
            _ => panic!("Light should be continuous"),
        }
    }

    #[test]
    fn test_torch_effect() {
        let emitter = LightEmissionEffect::torch();
        assert_eq!(emitter.effect, ParticleEffect::Light);
        assert!(emitter.color[0] > emitter.color[2], "Torch should be warm-colored");
    }

    #[test]
    fn test_magical_effect() {
        let color = [0.5, 0.0, 1.0, 0.9];
        let emitter = LightEmissionEffect::magical(color);
        assert_eq!(emitter.color, color);
    }

    #[test]
    fn test_dust_block_break() {
        let emitter = DustParticleEffect::block_break([0.5, 0.5, 0.5, 1.0], 3.0);
        assert_eq!(emitter.effect, ParticleEffect::Dust);
        match emitter.mode {
            EmissionMode::Burst { count, .. } => assert!(count >= 8),
            _ => panic!("Block break should be burst"),
        }
    }

    #[test]
    fn test_dust_block_place() {
        let emitter = DustParticleEffect::block_place([0.6, 0.4, 0.3, 1.0]);
        assert_eq!(emitter.effect, ParticleEffect::Dust);
        assert!(emitter.emitter_lifetime.is_some());
    }

    #[test]
    fn test_dust_footstep() {
        let emitter = DustParticleEffect::footstep([0.5, 0.4, 0.3, 1.0]);
        assert_eq!(emitter.effect, ParticleEffect::Dust);
        match emitter.mode {
            EmissionMode::Burst { interval, .. } => {
                assert!(interval.is_some(), "Footsteps should repeat");
            }
            _ => panic!("Footsteps should be burst"),
        }
    }

    #[test]
    fn test_weather_clear_returns_none() {
        assert!(WeatherParticleEffect::from_weather(WeatherType::Clear, 1.0).is_none());
    }

    #[test]
    fn test_weather_rain() {
        let emitter = WeatherParticleEffect::from_weather(WeatherType::Rain, 0.5).unwrap();
        assert_eq!(emitter.effect, ParticleEffect::Weather);
    }

    #[test]
    fn test_weather_snow() {
        let emitter = WeatherParticleEffect::from_weather(WeatherType::Snow, 0.7).unwrap();
        assert_eq!(emitter.effect, ParticleEffect::Weather);
    }

    #[test]
    fn test_weather_storm() {
        let emitter = WeatherParticleEffect::from_weather(WeatherType::Storm, 1.0).unwrap();
        assert_eq!(emitter.effect, ParticleEffect::Weather);
        match emitter.mode {
            EmissionMode::Continuous { rate } => {
                assert!(rate > 40.0, "Storm should have high emission rate");
            }
            _ => panic!("Weather should be continuous"),
        }
    }

    #[test]
    fn test_weather_intensity_scales_rate() {
        let low = WeatherParticleEffect::from_weather(WeatherType::Rain, 0.1).unwrap();
        let high = WeatherParticleEffect::from_weather(WeatherType::Rain, 1.0).unwrap();
        let low_rate = match low.mode {
            EmissionMode::Continuous { rate } => rate,
            _ => panic!(),
        };
        let high_rate = match high.mode {
            EmissionMode::Continuous { rate } => rate,
            _ => panic!(),
        };
        assert!(high_rate > low_rate, "Higher intensity should mean more particles");
    }
}
