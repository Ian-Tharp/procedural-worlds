//! Post-processing effects
//!
//! Adds ACES tonemapping, bloom, and distance fog to the camera.
//! Runs in `PostStartup` so the camera entity has already been spawned.

use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;

use crate::config::EngineConfig;

/// Plugin that attaches post-processing components to the camera.
pub struct PostProcessingPlugin;

impl Plugin for PostProcessingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup_post_processing);
    }
}

/// Insert tonemapping, bloom, and fog components on every `Camera3d` entity.
fn setup_post_processing(
    config: Res<EngineConfig>,
    mut commands: Commands,
    cameras: Query<Entity, With<Camera3d>>,
) {
    for entity in &cameras {
        // ACES Fitted tonemapping
        commands.entity(entity).insert(Tonemapping::AcesFitted);

        // Bloom
        if config.render.bloom_enabled {
            commands.entity(entity).insert(Bloom {
                intensity: config.render.bloom_intensity,
                ..default()
            });
        }

        // Distance fog
        if config.render.fog_enabled {
            commands.entity(entity).insert(DistanceFog {
                color: Color::srgba(0.5, 0.6, 0.8, 1.0),
                falloff: FogFalloff::Linear {
                    start: config.render.fog_start,
                    end: config.render.fog_end,
                },
                ..default()
            });
        }

        info!(
            "Post-processing: tonemapping=ACES, bloom={} (intensity {:.2}), fog={} ({:.0}..{:.0})",
            config.render.bloom_enabled,
            config.render.bloom_intensity,
            config.render.fog_enabled,
            config.render.fog_start,
            config.render.fog_end,
        );
    }
}
