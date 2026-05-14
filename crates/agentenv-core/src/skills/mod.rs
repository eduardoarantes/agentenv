//! Source-driven skills pipeline.
//!
//! `.agentrc.yaml` declares `source: <target>`; sync reads that target's
//! native skill layout losslessly into [`types::Canonical`], serializes the
//! canonical to `.agentenv/skills.canonical.yaml`, and renders it out to
//! every other supporting target.
//!
//! Mirrors the architecture of [`crate::hooks`].

pub mod canonical_io;
pub mod pipeline;
pub mod readers;
pub mod types;
pub mod writers;

pub use pipeline::{run, PipelineReport};
pub use types::{Canonical, CanonicalSkill, SidecarFile, SidecarKind, WriteReport};
