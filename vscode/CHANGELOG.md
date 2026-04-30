# Changelog

## 0.1.0

Initial release.

- `agentenv: Sync Plugins`, `Doctor`, `List Plugins`, `Open Config`, and `Clean Managed Links` commands
- Auto-sync on workspace open (configurable via `agentenv.syncOnOpen`)
- Debounced auto-sync on `.agentrc.yaml` changes (configurable via `agentenv.syncOnConfigChange` and `agentenv.configChangeDebounceMs`)
- Configurable CLI binary path via `agentenv.path`
- Optional `--refetch` flag via `agentenv.refetchOnSync`
