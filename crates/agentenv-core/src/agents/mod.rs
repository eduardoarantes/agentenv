//! Source-driven agents pipeline.
//!
//! `.agentrc.yaml` declares `source: <target>`; sync reads that target's
//! native agent layout losslessly into [`types::Canonical`], serializes
//! the canonical to `.agentenv/agents.canonical.yaml`, and renders it out
//! to every other supporting target.
//!
//! Mirrors the architecture of [`crate::hooks`] and [`crate::skills`].

pub mod canonical_io;
pub mod pipeline;
pub mod readers;
pub mod types;
pub mod writers;

pub use pipeline::{run, PipelineReport};
pub use types::{Canonical, CanonicalAgent, WriteReport};
