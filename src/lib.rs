//! Procedural Worlds Engine - Library Crate
//!
//! Exposes the engine's modules as a library so that benchmarks and
//! integration tests can import types and functions directly.
//!
//! The binary entry-point lives in `main.rs`.

pub mod actors;
pub mod audio;
pub mod config;
pub mod content;
pub mod editor;
pub mod engine;
pub mod generation;
pub mod physics;
pub mod rendering;
pub mod world;
