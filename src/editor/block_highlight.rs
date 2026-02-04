//! Block Highlight Wireframe
//!
//! Renders a wireframe cube around the block the player is currently looking at.
//! Uses `PrimitiveTopology::LineList` for clean edges without triangulation.
//!
//! Requires:
//! - [`CurrentTarget`] resource (from [`crate::engine::raycast::RaycastPlugin`])

use bevy::prelude::*;
use bevy::render::mesh::PrimitiveTopology;
use bevy::render::render_asset::RenderAssetUsages;

use crate::engine::raycast::CurrentTarget;

/// Plugin that spawns and updates a wireframe block highlight.
pub struct BlockHighlightPlugin;

impl Plugin for BlockHighlightPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_block_highlight)
            .add_systems(Update, update_block_highlight);
    }
}

/// Marker component for the block highlight entity.
#[derive(Component)]
struct BlockHighlight;

/// Create a wireframe cube mesh using `LineList` topology.
///
/// The cube is centered at the origin with half-extent `s` (slightly larger
/// than 0.5 to avoid z-fighting with block faces). 12 edges × 2 vertices = 24
/// vertices total.
fn create_wireframe_cube() -> Mesh {
    let s = 0.501; // slightly larger than 0.5 to avoid z-fighting

    // 12 edges, each defined by 2 vertices (LineList)
    let positions: Vec<[f32; 3]> = vec![
        // Bottom face edges
        [-s, -s, -s], [ s, -s, -s],
        [ s, -s, -s], [ s, -s,  s],
        [ s, -s,  s], [-s, -s,  s],
        [-s, -s,  s], [-s, -s, -s],
        // Top face edges
        [-s,  s, -s], [ s,  s, -s],
        [ s,  s, -s], [ s,  s,  s],
        [ s,  s,  s], [-s,  s,  s],
        [-s,  s,  s], [-s,  s, -s],
        // Vertical edges
        [-s, -s, -s], [-s,  s, -s],
        [ s, -s, -s], [ s,  s, -s],
        [ s, -s,  s], [ s,  s,  s],
        [-s, -s,  s], [-s,  s,  s],
    ];

    let mut mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh
}

/// Spawn the highlight entity (hidden by default).
fn spawn_block_highlight(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(create_wireframe_cube());
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 0.0, 0.8),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    commands.spawn((
        BlockHighlight,
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// Move the highlight to the targeted block, or hide it if nothing is targeted.
fn update_block_highlight(
    current_target: Option<Res<CurrentTarget>>,
    mut query: Query<(&mut Transform, &mut Visibility), With<BlockHighlight>>,
) {
    let Ok((mut transform, mut visibility)) = query.get_single_mut() else {
        return;
    };

    let has_hit = current_target
        .as_ref()
        .and_then(|t| t.0.as_ref())
        .filter(|r| r.hit);

    match has_hit {
        Some(result) => {
            // Center the wireframe on the targeted block (block coords are corner-based)
            transform.translation = Vec3::new(
                result.block_pos.x as f32 + 0.5,
                result.block_pos.y as f32 + 0.5,
                result.block_pos.z as f32 + 0.5,
            );
            *visibility = Visibility::Visible;
        }
        None => {
            *visibility = Visibility::Hidden;
        }
    }
}
