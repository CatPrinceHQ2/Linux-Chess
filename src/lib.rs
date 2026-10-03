//! Prince's Linux-Chess core library: chess rules, the engine layer (UCI and friends),
//! persistent configuration and platform helpers. The GTK front-end lives in the binary crate
//! behind the `gui` feature; everything here is testable without a display.
pub mod chess;
pub mod config;
pub mod engine;
pub mod platform;

#[cfg(feature = "gui")]
pub mod ui;
