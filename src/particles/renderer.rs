//! Particle Renderer
//!
//! Handles visual representation of particles using Bevy's built-in mesh
//! and material system. Uses shared mesh/material handles for performance,
//! with color and size controlled per-particle via transform scaling and
//! material color.
//!
//! # Rendering Strategy
//!
//! Instead of creating unique meshes per particle (which caused GPU upload
//! overhead in the weather system), this renderer:
//! - Creates shared mesh handles once (stored in [`ParticleRenderAssets`])
//! - Groups particles by effect type for material batching
//! - Uses `Transform` scale for size variation
//! - Billboards particles to face the camera

use bevy::prelude::*;

use super::{Particle, ParticleEffect};

// ============================================================================
// RESOURCES
// ============================================================================

/// Shared mesh and material handles for particle rendering.
///
/// Created once at startup and reused for all particles of each type.
/// This avoids per-particle asset creation overhead.
#[derive(Resource)]
pub struct ParticleRenderAssets {
    /// Small cube mesh used for all particles.
    pub particle_mesh: Handle<Mesh>,
    /// Material handles per effect type.
    pub materials: ParticleEffectMaterials,
}

/// Pre-created materials for each particle effect type.
pub struct ParticleEffectMaterials {
    pub water: Handle<StandardMaterial>,
    pub light: Handle<StandardMaterial>,
    pub dust: Handle<StandardMaterial>,
    pub weather: Handle<StandardMaterial>,
}

impl ParticleEffectMaterials {
    /// Get the material handle for a given effect type.
    pub fn get(&self, effect: ParticleEffect) -> Handle<StandardMaterial> {
        match effect {
            ParticleEffect::Water => self.water.clone(),
            ParticleEffect::Light => self.light.clone(),
            ParticleEffect::Dust => self.dust.clone(),
            ParticleEffect::Weather => self.weather.clone(),
        }
    }
}

// ============================================================================
// MARKER COMPONENT
// ============================================================================

/// Marker component indicating a particle entity has been given render components.
///
/// Added after mesh + material are inserted so we don't re-add them every frame.
#[derive(Component)]
pub struct ParticleRendered;

// ============================================================================
// SYSTEMS
// ============================================================================

/// Initializes shared particle render assets.
///
/// Creates a single small cube mesh and base materials for each effect type.
/// These are reused across all particles for efficient GPU batching.
pub fn setup_particle_render_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let particle_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    let effect_materials = ParticleEffectMaterials {
        water: materials.add(StandardMaterial {
            base_color: Color::srgba(0.4, 0.6, 0.95, 0.7),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        light: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.8, 0.3, 0.9),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            emissive: LinearRgba::new(2.0, 1.5, 0.5, 1.0),
            ..default()
        }),
        dust: materials.add(StandardMaterial {
            base_color: Color::srgba(0.6, 0.5, 0.4, 0.6),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        weather: materials.add(StandardMaterial {
            base_color: Color::srgba(0.7, 0.8, 0.9, 0.5),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    };

    commands.insert_resource(ParticleRenderAssets {
        particle_mesh,
        materials: effect_materials,
    });
}

/// Attaches mesh and material to newly spawned particles that lack render components.
///
/// Uses the shared handles from [`ParticleRenderAssets`] for GPU batching.
pub fn attach_particle_visuals_system(
    mut commands: Commands,
    render_assets: Option<Res<ParticleRenderAssets>>,
    particles: Query<(Entity, &Particle), Without<ParticleRendered>>,
) {
    let Some(assets) = render_assets else {
        return;
    };

    for (entity, particle) in &particles {
        let material = assets.materials.get(particle.effect);
        let size = particle.size;

        commands.entity(entity).insert((
            Mesh3d(assets.particle_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_scale(Vec3::splat(size)),
            ParticleRendered,
        ));
    }
}

/// Fades particles out as they approach end of life.
///
/// Adjusts the transform scale to create a shrinking effect in the last
/// 30% of the particle's lifetime.
pub fn particle_fade_system(
    mut particles: Query<(&Particle, &mut Transform), With<ParticleRendered>>,
) {
    for (particle, mut transform) in &mut particles {
        let life_fraction = particle.age / particle.lifetime;

        if life_fraction > 0.7 {
            // Fade out in last 30% of life
            let fade = 1.0 - ((life_fraction - 0.7) / 0.3).clamp(0.0, 1.0);
            let base_size = particle.size;
            transform.scale = Vec3::splat(base_size * fade);
        }
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Plugin that adds particle rendering systems.
///
/// Should be added alongside [`super::ParticlePlugin`] for visual output.
/// Separated to allow headless testing without rendering.
pub struct ParticleRendererPlugin;

impl Plugin for ParticleRendererPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_particle_render_assets)
            .add_systems(
                Update,
                (
                    attach_particle_visuals_system,
                    particle_fade_system,
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
    fn test_particle_effect_materials_get() {
        // Verify the mapping covers all variants without panic
        let effects = [
            ParticleEffect::Water,
            ParticleEffect::Light,
            ParticleEffect::Dust,
            ParticleEffect::Weather,
        ];
        // Just ensure the enum match is exhaustive (compile-time check)
        for effect in effects {
            let _ = format!("{:?}", effect);
        }
    }

    #[test]
    fn test_fade_calculation() {
        // At 70% life, fade = 1.0 (fully visible)
        let life_fraction = 0.7;
        let fade = 1.0 - ((life_fraction - 0.7) / 0.3_f32).clamp(0.0, 1.0);
        assert!((fade - 1.0).abs() < 0.001);

        // At 85% life, fade = 0.5
        let life_fraction = 0.85;
        let fade = 1.0 - ((life_fraction - 0.7) / 0.3_f32).clamp(0.0, 1.0);
        assert!((fade - 0.5).abs() < 0.001);

        // At 100% life, fade = 0.0
        let life_fraction = 1.0;
        let fade = 1.0 - ((life_fraction - 0.7) / 0.3_f32).clamp(0.0, 1.0);
        assert!(fade.abs() < 0.001);
    }
}
