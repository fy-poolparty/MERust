//! Mirror's Edge (2008) player movement, ported from the game's own logic: the UnrealScript in
//! TdGame.u (TdPawn, TdPlayerController, TdMove_*) and the C++ natives in MirrorsEdge.exe
//! (TdPawn physics, TdMove natives, UE3 APawn physics as compiled into the game).
//!
//! Everything is in Unreal space: units are uu (cm), Z is up, X forward, Y right.

pub mod anim;
pub mod body;
pub mod bots;
pub mod combat;
pub mod collision;
pub mod config;
pub mod floor;
pub mod controller;
pub mod ladder;
pub mod math;
pub mod moves;
pub mod natives;
pub mod pawn;
pub mod physics;
pub mod physics_wall;
pub mod sim;
pub mod sound;
pub mod testmap;
pub mod volumes;
pub mod weapons;

pub use collision::{World, WorldBuilder};
pub use math::{Rotator, Vec3};
pub use pawn::{Move, Physics};
pub use sim::{InputFrame, Sim};
