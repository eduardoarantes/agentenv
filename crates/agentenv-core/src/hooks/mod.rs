//! Source-driven hooks pipeline.
//!
//! `.agentrc.yaml` declares `source: <target>`; sync reads that target's
//! native hooks file losslessly into [`types::Canonical`], serializes the
//! canonical to `.agentenv/hooks.canonical.yaml`, and renders it out to
//! every other supporting target.
//!
//! See `docs/HOOKS.md` for the user-facing spec.

pub mod canonical_io;
pub mod pipeline;
pub mod readers;
pub mod types;
pub mod writers;

pub use pipeline::{run, PipelineReport};
pub use types::{Action, Canonical, CommonEvent, Event, Hook, Matcher, NativeEvent, WriteReport};

/// Process-global mutex for tests that mutate `$HOME` to redirect
/// `dirs::home_dir()` (codex writer + codex hooks reader both touch
/// `~/.codex/config.toml`). Cargo's unit-test binary runs them in
/// parallel, so they MUST serialize against the same lock — using
/// per-module locks lets them race and clobber each other's `$HOME`.
///
/// Unix-only — on Windows the resolver reads `%USERPROFILE%` and the
/// HOME-mutating tests are gated `#[cfg(unix)]` already.
#[cfg(all(test, unix))]
pub(crate) static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
