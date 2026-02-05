//! Integration tests for the chunk loading pipeline.
//!
//! These tests verify the complete flow from chunk generation through
//! persistence and meshing, exercising cross-module interactions that
//! unit tests in individual modules cannot cover.

mod chunk_loading_pipeline;
mod chunk_meshing_pipeline;
mod progress_bar_states;
mod error_handling;
