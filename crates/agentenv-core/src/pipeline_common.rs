//! Helpers shared between the skills and agents pipelines.
//!
//! Both pipelines need the same filter: "which of the user's configured
//! targets does this capability know how to write to, minus the source
//! (which is read-only)?" The hooks pipeline answers a similar question
//! via `Config::hook_write_targets`, but its known-writer set is a
//! `const` in `config.rs` (v1 only supports cursor + codex for hooks),
//! while skills and agents derive theirs from per-capability writers
//! modules — so this helper takes the known writers as an argument.

use crate::config::Config;
use std::collections::HashSet;

/// Configured targets that intersect with `known_writers`, excluding the
/// `source` target (always read-only). Sorted for deterministic write
/// order across runs.
pub fn configured_write_targets(
    config: &Config,
    source: &str,
    known_writers: &[&str],
) -> Vec<String> {
    let known: HashSet<&str> = known_writers.iter().copied().collect();
    let mut out: Vec<String> = config
        .targets
        .keys()
        .filter(|name| known.contains(name.as_str()))
        .filter(|name| name.as_str() != source)
        .cloned()
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, MarketplaceConfig, TargetConfig};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn base_config() -> Config {
        let mut marketplaces = HashMap::new();
        marketplaces.insert(
            "default".to_string(),
            MarketplaceConfig {
                path: PathBuf::from("~/.agentenv/marketplace"),
                remote: "https://example.com/m.git".to_string(),
                r#ref: "main".to_string(),
            },
        );
        Config {
            version: 1,
            marketplaces,
            plugins: vec![],
            targets: HashMap::new(),
            sync: Default::default(),
            clean: Default::default(),
            gitignore_managed_links: false,
            instruction_files: HashMap::new(),
            source: None,
        }
    }

    #[test]
    fn keeps_only_targets_that_are_known_writers_and_drops_source() {
        let mut config = base_config();
        config
            .targets
            .insert("cursor".to_string(), TargetConfig::default());
        config
            .targets
            .insert("codex".to_string(), TargetConfig::default());
        // Hypothetically: the user opts the source in as a target too.
        // `Config::validate` rejects this in practice; the helper itself
        // is defensive about it.
        config
            .targets
            .insert("claude-code".to_string(), TargetConfig::default());
        // A target the capability has no writer for.
        config
            .targets
            .insert("unknown".to_string(), TargetConfig::default());
        let known = ["cursor", "codex", "copilot"];
        let out = configured_write_targets(&config, "claude-code", &known);
        assert_eq!(out, vec!["codex".to_string(), "cursor".to_string()]);
    }

    #[test]
    fn output_is_sorted_for_deterministic_writes() {
        let mut config = base_config();
        for name in ["junie", "cursor", "codex"] {
            config
                .targets
                .insert(name.to_string(), TargetConfig::default());
        }
        let known = ["junie", "cursor", "codex"];
        let out = configured_write_targets(&config, "claude-code", &known);
        assert_eq!(
            out,
            vec![
                "codex".to_string(),
                "cursor".to_string(),
                "junie".to_string()
            ]
        );
    }
}
