//! Benchmarks for terrain generation and cave carving.
//!
//! Establishes a performance baseline for the core generation pipeline
//! so regressions can be caught early.

use bevy::math::IVec3;
use criterion::{Criterion, criterion_group, criterion_main};

use procedural_worlds::generation::{TerrainConfig, generate_caves, generate_chunk_terrain};
use procedural_worlds::world::Chunk;

/// Benchmark terrain generation for a surface-level chunk (mix of air and solid).
fn bench_generate_terrain_surface(c: &mut Criterion) {
    let config = TerrainConfig::default();

    c.bench_function("generate_chunk_terrain (surface y=2)", |b| {
        b.iter(|| {
            let mut chunk = Chunk::new(IVec3::new(0, 2, 0));
            generate_chunk_terrain(&mut chunk, &config);
            chunk
        });
    });
}

/// Benchmark terrain generation for an underground chunk (almost entirely solid).
fn bench_generate_terrain_underground(c: &mut Criterion) {
    let config = TerrainConfig::default();

    c.bench_function("generate_chunk_terrain (underground y=0)", |b| {
        b.iter(|| {
            let mut chunk = Chunk::new(IVec3::new(0, 0, 0));
            generate_chunk_terrain(&mut chunk, &config);
            chunk
        });
    });
}

/// Benchmark cave carving on a pre-generated underground chunk.
fn bench_generate_caves(c: &mut Criterion) {
    let config = TerrainConfig::default();

    // Pre-generate a chunk to carve caves into.
    let mut template = Chunk::new(IVec3::new(0, 0, 0));
    generate_chunk_terrain(&mut template, &config);

    c.bench_function("generate_caves (underground y=0)", |b| {
        b.iter_batched(
            || template.clone(),
            |mut chunk| {
                generate_caves(&mut chunk, &config);
                chunk
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Benchmark the full generation pipeline (terrain + caves) for a surface chunk.
fn bench_full_pipeline(c: &mut Criterion) {
    let config = TerrainConfig::default();

    c.bench_function("full pipeline terrain+caves (surface y=2)", |b| {
        b.iter(|| {
            let mut chunk = Chunk::new(IVec3::new(0, 2, 0));
            generate_chunk_terrain(&mut chunk, &config);
            generate_caves(&mut chunk, &config);
            chunk
        });
    });
}

criterion_group!(
    benches,
    bench_generate_terrain_surface,
    bench_generate_terrain_underground,
    bench_generate_caves,
    bench_full_pipeline,
);
criterion_main!(benches);
