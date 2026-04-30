# agentenv

> Project-scoped AI agent and plugin environment manager.

The **agentenv** VS Code extension wraps the [`agentenv`](https://github.com/eduardoarantes/agentenv) CLI so your project's AI agents, commands, skills, and plugins are reproducible across machines and tools (Claude Code, Codex, Cursor, and more).

It works similarly to `.nvmrc` or `.tool-versions`, but instead of runtime versions, it manages **agents, commands, skills, and plugins** declared in a project-local `.agentrc.yaml`.

## Features

- Runs `agentenv sync` automatically when a workspace with `.agentrc.yaml` is opened
- Debounced auto-sync when `.agentrc.yaml` is edited
- Commands palette entries for sync, doctor, list, clean, and open config
- Output channel surfaces CLI logs without blocking the editor

## Requirements

You must have the `agentenv` CLI installed and on your `PATH`. Install it via:

```bash
cargo install agentenv
```

Or download a release binary from the [GitHub releases page](https://github.com/eduardoarantes/agentenv/releases).

## Commands

| Command | Description |
|---|---|
| `agentenv: Sync Plugins` | Fetch the marketplace, resolve plugins, recreate managed links |
| `agentenv: Doctor` | Diagnose project and system state |
| `agentenv: List Plugins` | List configured marketplaces, plugins, and targets |
| `agentenv: Open Config` | Open `.agentrc.yaml` in the editor |
| `agentenv: Clean Managed Links` | Remove every link recorded in `.agentenv/state.json` |

## Settings

| Setting | Default | Description |
|---|---|---|
| `agentenv.path` | `agentenv` | Path to the agentenv binary. Resolved against `$PATH` if not absolute. |
| `agentenv.syncOnOpen` | `true` | Run `agentenv sync` automatically when a workspace with `.agentrc.yaml` is opened. |
| `agentenv.syncOnConfigChange` | `true` | Run `agentenv sync` automatically when `.agentrc.yaml` is modified or created. |
| `agentenv.configChangeDebounceMs` | `1500` | Debounce window (ms) before auto-syncing after a `.agentrc.yaml` change. |
| `agentenv.refetchOnSync` | `false` | Pass `--refetch` when invoking sync, forcing a marketplace refresh. |

## Quick start

1. Install the `agentenv` CLI (see Requirements).
2. In your project root, run `agentenv init` to scaffold `.agentrc.yaml`.
3. Open the project in VS Code — the extension runs `agentenv sync` automatically.

See the [main README](https://github.com/eduardoarantes/agentenv#readme) for the full configuration reference.

## License

[MIT](https://github.com/eduardoarantes/agentenv/blob/main/LICENSE)
