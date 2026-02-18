//! Persistence module — full world state serialization with entity and session support
//!
//! Extends the chunk-level persistence in [`world::persistence`] and [`world::save`]
//! with:
//! - **Entity serialization** (creatures, dropped items — position, type, health)
//! - **Time state** (in-game time, day count)
//! - **Player inventory state** (per-player inventory snapshots)
//! - **Multiplayer session metadata** (session ID, connected players, host info)
//!
//! The existing chunk save/load system handles block data efficiently via
//! compressed binary format. This module adds the higher-level world state
//! that wraps around it.

pub mod save_system;
pub mod scenario;
pub mod world_state;

#[cfg(test)]
mod tests;
