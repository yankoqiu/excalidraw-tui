//! `exv` – zero-disk live Excalidraw preview for the terminal.
//!
//! The crate is split into a library so the conversion + rasterization
//! pipeline can be exercised by integration tests without starting the TUI.

pub mod converter;
pub mod export;
pub mod rasterizer;
pub mod ui;
pub mod watcher;
