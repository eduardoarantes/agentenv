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
