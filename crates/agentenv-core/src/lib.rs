#![forbid(unsafe_code)]
#![warn(rust_2018_idioms, unreachable_pub)]

//! agentenv-core: Core library for agentenv
//!
//! This library provides the foundational logic for managing project-scoped
//! AI agent and plugin environments.

pub mod claude_config;
pub mod clean;
pub mod config;
pub mod error;
pub mod gitignore;
pub mod init;
pub mod loader;
pub mod marketplace;
pub mod plugin;
pub mod resolver;
pub mod state;
pub mod symlink;
pub mod sync;
pub mod targets;

pub use claude_config::{ClaudeConfigImport, ClaudeConfigLoader};
pub use clean::{CleanOptions, CleanReport, Cleaner};
pub use config::{CleanConfig, Config, SourceMapping, TargetConfig};
pub use error::{Error, Result};
pub use init::Initializer;
pub use loader::ConfigLoader;
pub use marketplace::Marketplace;
pub use plugin::Plugin;
pub use resolver::{PluginResolver, ResolvedPlugin};
pub use state::{State, StateLink};
pub use symlink::{InstallAction, SymlinkManager};
pub use sync::{SyncPlan, SyncReport, Syncer};
pub use targets::TargetDefaults;
