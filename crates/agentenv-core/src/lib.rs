#![forbid(unsafe_code)]
#![warn(rust_2018_idioms, unreachable_pub)]

//! agentenv-core: Core library for agentenv
//!
//! This library provides the foundational logic for managing project-scoped
//! AI agent and plugin environments.

pub mod config;
pub mod error;
pub mod init;
pub mod loader;
pub mod marketplace;
pub mod plugin;
pub mod resolver;
pub mod symlink;
pub mod sync;
pub mod target;
pub mod targets;

pub use config::{Config, SourceMapping, TargetConfig};
pub use error::{Error, Result};
pub use init::Initializer;
pub use loader::ConfigLoader;
pub use marketplace::Marketplace;
pub use plugin::Plugin;
pub use resolver::{PluginResolver, ResolvedPlugin};
pub use symlink::{InstallAction, SymlinkManager};
pub use sync::{SyncReport, Syncer};
pub use target::Target;
pub use targets::TargetDefaults;
