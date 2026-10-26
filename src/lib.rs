//! smoltop — a terminal UI over the smolvm CLI.
//!
//! smolvm is never invoked through a shell. [`runner::SystemRunner`] builds a
//! `std::process::Command` with one argument per parameter, and
//! [`commands::screen_invocation`] rejects flags this tool must not pass.

#![forbid(unsafe_code)]

pub mod app;
pub mod commands;
pub mod config;
pub mod dispatch;
pub mod names;
pub mod packs;
pub mod parse;
pub mod runner;
pub mod theme;
pub mod ui;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
