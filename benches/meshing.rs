//! Benchmarks for chunk mesh building.
//!
//! Tests mesh generation across different chunk compositions:
//! terrain (mixed), fully solid, and fully empty. This establishes a
//! performance baseline and highlights the cost of face-culling logic.

use bevy::math::IVec3;
use criterion::{Criterion, criterion_group, criterion_main};

use procedural_worlds::generation::{TerrainConfig, generate_caves, generate_chunk_terrain};
use procedural_worlds::world::meshing::build_chunk_mesh;
use procedural_worlds::world::{BlockType, Chunk};

/// Benchmark meshing a terrain-generated chunk (realistic mixed content).
fn bench_mesh_terrain_chunk(c: &mut Criterion) {
    let config = TerrainConfig::default();
    let mut chunk = Chunk::new(IVec3::new(0, 2, 0));
    generate_chunk_terrain(&mut chunk, &config);
    generate_caves(&mut chunk, &config);

    c.bench_function("build_chunk_mesh (terrain surface)", |b| {
        b.iter(|| build_chunk_mesh(&chunk));
    });
}

/// Benchmark meshing a fully solid chunk (worst-case: only boundary faces visible).
fn bench_mesh_solid_chunk(c: &mut Criterion) {
    let mut chunk = Chunk::new(IVec3::ZERO);
    chunk.fill(BlockType::Stone);

    c.bench_function("build_chunk_mesh (fully solid)", |b| {
        b.iter(|| build_chunk_mesh(&chunk));
    });
}

/// Benchmark meshing an empty chunk (best-case: nothing to draw).
fn bench_mesh_empty_chunk(c: &mut Criterion) {
    let chunk = Chunk::new(IVec3::ZERO); // All air by default

    c.bench_function("build_chunk_mesh (empty/air)", |b| {
        b.iter(|| build_chunk_mesh(&chunk));
    });
}

criterion_group!(
    benches,
    bench_mesh_terrain_chunk,
    bench_mesh_solid_chunk,
    bench_mesh_empty_chunk,
);
criterion_main!(benches);
