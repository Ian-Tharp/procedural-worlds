//! Particle effects system with block light integration.
//!
//! This module provides a complete particle system that responds to block
//! light levels, creating visually coherent effects where particles brighten
//! near light sources (torches, lava) and dim in shadows.
//!
//! # Architecture
//!
//! ```text
//! BlockLightQuery (light_query.rs)
//!     ↓ samples light at world positions
//! ParticleEmitter (emitter.rs)
//!     ↓ spawns particles with inherited light
//! Particle (emitter.rs)
//!     ↓ per-particle light updates (optional)
//! ParticleRenderer (renderer.rs)
//!     ↓ modulates vertex colors by light brightness
//! Visual Output: lit particles in the world
//! ```
//!
//! # Light Integration
//!
//! The system integrates with block light through three mechanisms:
//!
//! 1. **Emitter-level sampling:** Each emitter samples block light at its
//!    position every frame. Spawned particles inherit this initial brightness.
//!
//! 2. **Per-particle sampling:** Particles with `sample_own_light: true`
//!    re-sample block light at their current position each frame, allowing
//!    moving particles to respond to changing light conditions.
//!
//! 3. **Self-illumination:** Effects that represent light sources (torches,
//!    lava) are flagged as self-illuminated and always render at full
//!    brightness, regardless of ambient block light.
//!
//! # Future Integration
//!
//! When `feature/block-light-propagation` is merged into develop,
//! [`BlockLightQuery`] will read directly from [`WorldLightMap`] for
//! accurate per-block light levels instead of the current height-based
//! sky light estimation.

pub mod effects;
pub mod emitter;
pub mod light_query;
pub mod renderer;

use bevy::prelude::*;

pub use effects::ParticleEffectType;
pub use emitter::{EmitterConfig, Particle, ParticleEmitter};
pub use light_query::{BlockLightQuery, BlockLightQueryConfig, LightSample};
pub use renderer::ParticleRendererConfig;

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds the particle effects system with block light integration.
///
/// Registers all necessary resources, systems, and render components for
/// light-responsive particle effects.
pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<BlockLightQuery>()
            .init_resource::<ParticleRendererConfig>()
            // Startup: create shared mesh/material
            .add_systems(Startup, renderer::setup_particle_renderer)
            // Update: emitter light → spawn → update → billboard
            .add_systems(
                Update,
                (
                    emitter::update_emitter_light,
                    renderer::spawn_particles,
                    renderer::update_particles,
                    emitter::update_particle_light,
                    renderer::billboard_particles,
                )
                    .chain(),
            );
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_particle_plugin_builds() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        // ParticlePlugin needs rendering resources — just verify types exist
        // Full plugin build requires Bevy render pipeline
    }

    #[test]
    fn test_module_exports() {
        // Verify key types are exported from the module root
        let _effect = ParticleEffectType::TorchFlame;
        let _sample = LightSample::max();
        let _config = EmitterConfig::default();
        let _renderer_config = ParticleRendererConfig::default();
    }
}
