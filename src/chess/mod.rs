//! Chess rules layer: positions, FEN, legal moves, SAN. Independent of UCI and of the GUI.
mod position;
mod san;
mod types;

pub use position::{FenError, MoveError, Position, PositionIssue};
pub use types::*;
