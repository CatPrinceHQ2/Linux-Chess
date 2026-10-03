//! Engine layer. The application talks to the [`ChessEngine`] trait; [`uci::UciEngine`] is the
//! only implementation today, but nothing outside `engine::uci` depends on UCI details.
pub mod analysis;
pub mod error;
pub mod log;
pub mod options;
pub mod parser;
pub mod process;
pub mod uci;

use crate::chess::Position;
pub use error::EngineError;
pub use options::{Capabilities, EngineOption, OptionKind};
pub use parser::{Bound, Info, Score};
use std::path::PathBuf;

/// How to start an engine. Arguments are passed directly to the process, never to a shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineSpec {
    pub path: PathBuf,
    pub args: Vec<String>,
}

impl EngineSpec {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        EngineSpec { path: path.into(), args: Vec::new() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineIdentity {
    pub name: String,
    pub author: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchLimit {
    Time { ms: u64 },
    Depth(u32),
    Nodes(u64),
    Infinite,
}

#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub position: Position,
    pub limit: SearchLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineState {
    Idle,
    /// `isready` sent; waiting for `readyok` before starting the search.
    Preparing,
    Searching,
    /// `stop` sent; waiting for `bestmove`.
    Stopping,
    Exited,
}

#[derive(Debug)]
pub enum EngineEvent {
    Info(Info),
    /// `best` is `None` when the engine reports no legal move (`bestmove (none)`).
    BestMove { best: Option<String>, ponder: Option<String> },
    /// A search was cancelled before the engine started it.
    SearchCancelled,
    /// A human-readable engine message (for example `info string ...`).
    Message(String),
    /// The engine died or misbehaved; the engine is no longer usable until restarted.
    Failed(EngineError),
}

/// Protocol-independent engine interface.
pub trait ChessEngine: Send {
    fn identity(&self) -> &EngineIdentity;
    fn options(&self) -> &[EngineOption];
    fn state(&self) -> EngineState;
    /// `value` is `None` for button options. Applied immediately when idle, otherwise queued
    /// until the current search has finished.
    fn set_option(&mut self, name: &str, value: Option<&str>) -> Result<(), EngineError>;
    fn new_game(&mut self) -> Result<(), EngineError>;
    fn start_search(&mut self, request: SearchRequest) -> Result<(), EngineError>;
    fn stop_search(&mut self) -> Result<(), EngineError>;
    /// Non-blocking: drains whatever the engine has produced so far.
    fn poll(&mut self) -> Vec<EngineEvent>;
    fn restart(&mut self) -> Result<(), EngineError>;
    fn shutdown(&mut self);
}
