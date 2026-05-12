# Changelog

## 0.3.2

- Release plumbing: `vscode-v*` tags no longer trigger the CLI release pipeline (#8). No user-facing extension changes.

## 0.3.1

- Probe the agentenv CLI on activation when `.agentrc.yaml` is present but `syncOnOpen` is `false` — surfaces a missing CLI immediately instead of waiting for the first command
- Missing-CLI dialog now includes an **Install Guide** action that opens the README install section, plus full install commands (Homebrew / npm / cargo) written to the output channel

## 0.3.0

- First release published via the `publish-vscode` GitHub Actions workflow

## 0.2.0

- Added extension icon

## 0.1.0

Initial release.

- `agentenv: Sync Plugins`, `Doctor`, `List Plugins`, `Open Config`, and `Clean Managed Links` commands
- Auto-sync on workspace open (configurable via `agentenv.syncOnOpen`)
- Debounced auto-sync on `.agentrc.yaml` changes (configurable via `agentenv.syncOnConfigChange` and `agentenv.configChangeDebounceMs`)
- Configurable CLI binary path via `agentenv.path`
- Optional `--refetch` flag via `agentenv.refetchOnSync`
