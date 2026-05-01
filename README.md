# agentenv

[![CI](https://img.shields.io/github/actions/workflow/status/eduardoarantes/agentenv/ci.yml?branch=main&label=CI)](https://github.com/eduardoarantes/agentenv/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/agentenv.svg)](https://crates.io/crates/agentenv)
[![VS Code Marketplace](https://vsmarketplacebadges.dev/version-short/eduardoarantes.agentenv.svg?label=VS%20Code&color=blue)](https://marketplace.visualstudio.com/items?itemName=eduardoarantes.agentenv)
[![Open VSX](https://img.shields.io/open-vsx/v/eduardoarantes/agentenv?label=Open%20VSX&color=blue)](https://open-vsx.org/extension/eduardoarantes/agentenv)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

> Project-scoped AI agent and plugin environment manager

`agentenv` lets you define, version, and reproduce the AI capabilities of a project across tools like Claude Code, Codex, Cursor, and others.

It works similarly to `.nvmrc`, `.jenv`, or `.tool-versions`, but instead of runtime versions, it manages **agents, commands, skills, and plugins**.

---

## Table of contents

- [Problem](#problem)
- [Solution](#solution)
- [How it works](#how-it-works)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Configuration](#configuration)
- [Commands](#commands)
- [Marketplace structure](#marketplace-structure)
- [Target adapters](#target-adapters)
- [VS Code integration](#vs-code-integration)
- [Design principles](#design-principles)
- [Roadmap](#roadmap)
- [Non-goals](#non-goals)
- [License](#license)

---

## Problem

AI coding tools are fragmented:

- Each tool has its own plugin, agent, command, or skill structure
- No standard way to share project-specific AI setups
- Teams cannot reliably reproduce AI capabilities across machines
- Global installs break project isolation
- Marketplace-style agent repositories are useful, but importing everything globally is too broad

---

## Solution

`agentenv` introduces:

- A **project-local config**: `.agentrc.yaml`
- A **plugin marketplace source**
- A **selective plugin import system**
- A **cross-tool sync engine**
- **Idempotent symlink-based linking**
- Non-blocking startup behavior for editors such as VS Code

---

## How it works

```text
Project opens
    ↓
agentenv sync
    ↓
Fetch/update marketplace
    ↓
Resolve selected plugins
    ↓
Recreate managed symlinks into target tools
    ↓
Warn on recoverable failures
```

The marketplace is treated as a source of truth. The project config decides which plugins are imported and which tools receive them.

---

## Installation

### macOS / Linux with Homebrew

```bash
brew install eduardoarantes/agentenv/agentenv
```

### npm (cross-platform)

```bash
npm install -g agentenv
```

### Rust / Cargo

```bash
cargo install agentenv
```

### Manual install

Download the latest binary from GitHub Releases and place it on your `PATH`.

Example:

```bash
chmod +x agentenv
mv agentenv /usr/local/bin/agentenv
```

### VS Code extension

The extension wraps the CLI so syncs run automatically when you open or edit a project's `.agentrc.yaml`. The CLI must be installed and on `PATH` first (see the options above).

From the Marketplace:

```bash
code --install-extension eduardoarantes.agentenv
```

Or search **agentenv** in the Extensions panel ([Marketplace listing](https://marketplace.visualstudio.com/items?itemName=eduardoarantes.agentenv)).

From a `.vsix` (offline or pre-release builds):

```bash
code --install-extension agentenv-<version>.vsix
```

---

## Quick start

### 1. Initialize a project

```bash
agentenv init
```

This creates:

```yaml
# .agentrc.yaml
version: 1

marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/eduardoarantes/claude-code-plugin-marketplace.git
    ref: main

# Add plugin entries here, e.g.:
#   - name: engineering-standards
plugins: []

targets:
  claude-code: {}

sync:
  onOpen: true
  refetch: true
  mode: symlink
```

The plugin list is empty so the first `agentenv sync` succeeds — open
`.agentrc.yaml` and add plugins as you need them. See the
[Configuration](#configuration) section below for the full shape.


### 2. Sync the environment

```bash
agentenv sync
```

### 3. Validate setup

```bash
agentenv doctor
```

---

## Configuration

### `.agentrc.yaml`

```yaml
version: 1

marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/eduardoarantes/claude-code-plugin-marketplace.git
    ref: main

plugins:
  - name: python-agents
  - name: pr-review
  - name: engineering-standards

targets:
  claude-code: {}
  cursor: {}

sync:
  onOpen: true
  refetch: true
  mode: symlink

clean:
  pruneEmptyDirs: true
```

### Fields

| Field | Required | Description |
|---|---:|---|
| `version` | Yes | Config schema version. |
| `marketplaces` | Yes | Marketplace repos keyed by namespace. |
| `marketplaces.<namespace>.path` | Yes | Local cache directory where agentenv clones the marketplace repo (from `remote` at `ref`). It's not a path inside the project — it's an agentenv-managed checkout used as a read-only source. Supports `~` (home), absolute paths, and relative paths (resolved against the project root). Refetches reset the working tree to `origin/<ref>`, so don't hand-edit anything inside it. A common choice is `~/.agentenv/marketplace` to share the cache across projects. |
| `marketplaces.<namespace>.remote` | Yes | Git remote used to clone/fetch the marketplace. |
| `marketplaces.<namespace>.ref` | No | Branch, tag, or commit to use. Defaults to `main`. |
| `plugins` | No | List of plugins to import. A plugin without `namespace` uses `default`. |
| `targets` | Yes | Object of tool adapters keyed by target name. Empty target configs use built-in defaults when available. |
| `sync.onOpen` | No | Whether editor integrations should sync on workspace open. |
| `sync.refetch` | No | Whether to fetch marketplace updates before syncing. |
| `sync.mode` | No | Link strategy. Initially `symlink`. |
| `clean.pruneEmptyDirs` | No | After `agentenv clean` removes managed links, prune any now-empty directories inside the project root. Stops at the project root and never touches dirs that still hold user files. Defaults to `true`. |

---

## Commands

### `agentenv init`

Create a starter `.agentrc.yaml`.

```bash
agentenv init
```

---

### `agentenv sync`

Fetch the marketplace, resolve plugins, and reconcile managed links.

```bash
agentenv sync             # honor sync.refetch from config
agentenv sync --refetch   # force fetch even if config disables it
agentenv sync --no-fetch  # skip the network; require an existing local copy
```

Behavior:

- Marketplaces missing locally are cloned from `remote` at `ref`.
- With `sync.refetch: true` (or `--refetch`), existing marketplaces are
  fetched and reset to `origin/<ref>`. The cache directory is
  agentenv-managed; don't hand-edit it.
- Fetch failures are non-fatal when a local copy exists — they surface as
  warnings and sync continues with the cached content.
- `--no-fetch` errors out only when a marketplace is missing locally.
- Broken or missing plugins produce diagnostics.
- Existing managed links are recreated.
- User-created files are not deleted.

---

### `agentenv list`

List configured marketplaces, plugins, and targets from `.agentrc.yaml`.

```bash
agentenv list
```

---

### `agentenv doctor`

Diagnose project and system state. Exits non-zero when issues are found, so
it's safe to use as a CI gate.

```bash
agentenv doctor
```

Checks:

- `.agentrc.yaml` exists and parses
- each configured marketplace exists locally
- every selected plugin resolves against its marketplace
- managed links recorded in `.agentenv/state.json` still exist on disk

---

### `agentenv explain`

Show what `sync` would do without touching the filesystem. Useful for
debugging and code review.

```bash
agentenv explain
```

Marketplaces are not fetched — the cache must already be populated (run
`agentenv sync` once first).

---

### `agentenv clean`

Remove every link recorded in `.agentenv/state.json`, then delete the state
file.

```bash
agentenv clean
```

Defensive: only removes symlinks that still point at the source agentenv
recorded. If you replaced a managed link with your own file, that file is
left untouched and reported as `skipped`.

After links are removed, `clean` prunes any now-empty directories inside the
project root (e.g. a leftover `.claude/skills/`). It stops at the project
root and never touches dirs that still hold user files. Disable via
`clean.pruneEmptyDirs: false` in `.agentrc.yaml`.

---

## Marketplace structure

`agentenv` expects a marketplace compatible with the Claude Code plugin marketplace style.

Example:

```text
plugins/
  my-plugin/
    .claude-plugin/
      plugin.json
    agents/
    commands/
    skills/
    hooks/
```

The marketplace is not owned by `agentenv`. It is consumed as an external source.

---

## Target adapters

Each AI tool is supported through a target adapter.

Adapters define:

- where agents go
- where commands go
- where skills go
- whether hooks are supported
- whether symlinks or copies are required
- compatibility rules

Example internal layout:

```text
src/
  targets/
    claude_code.rs
    codex.rs
    cursor.rs
    copilot.rs
```

---

## VS Code integration

The VS Code extension is thin — the CLI remains the source of truth. Install
it from the [Marketplace](https://marketplace.visualstudio.com/items?itemName=eduardoarantes.agentenv)
(see [Installation](#installation)).

On workspace open:

```text
VS Code activates
    ↓
Runs `agentenv sync`
    ↓
Shows warning only on failure
    ↓
Writes details to Agentenv output channel
```

### Commands

- `agentenv: Sync Plugins`
- `agentenv: Doctor`
- `agentenv: Open Config`
- `agentenv: List Plugins`
- `agentenv: Clean Managed Links`

### Settings

| Setting | Default | Description |
|---|---|---|
| `agentenv.path` | `agentenv` | Path to the agentenv binary. Resolved against `$PATH` if not absolute. |
| `agentenv.syncOnOpen` | `true` | Run `agentenv sync` automatically when a workspace with `.agentrc.yaml` is opened. |
| `agentenv.syncOnConfigChange` | `true` | Run `agentenv sync` automatically when `.agentrc.yaml` is modified or created. |
| `agentenv.configChangeDebounceMs` | `1500` | Debounce window (ms) before auto-syncing after a `.agentrc.yaml` change. Higher values batch rapid saves; lower values feel snappier. |
| `agentenv.refetchOnSync` | `false` | Pass `--refetch` when invoking sync, forcing a marketplace refresh. |

---

## Design principles

- **Project-first**: no global pollution
- **Deterministic**: same config produces same linked environment
- **Non-blocking**: recoverable failures warn instead of stopping work
- **Composable**: target adapters isolate tool-specific behavior
- **Transparent**: filesystem changes are explainable
- **Safe by default**: never delete unmanaged files

---

## Roadmap

Shipped:

- [x] Remote plugin sources beyond one marketplace (multiple namespaces under `marketplaces`)
- [x] Windows symlink fallback strategies
- [x] More target adapters (Claude Code, Codex, Cursor, Copilot)
- [x] JSON Schema for `.agentrc.yaml` (`schemas/agentrc.schema.json`)
- [x] VS Code extension
- [x] Dry-run mode (`agentenv explain`)

Planned:

- [ ] Lockfile support: `agentenv.lock`
- [ ] Plugin version pinning
- [ ] CI integration: `agentenv check`
- [ ] Plugin compatibility metadata

---

## Non-goals

For the initial version, `agentenv` is not:

- a plugin marketplace host
- a plugin runtime
- a cloud sync service
- a GUI app
- a replacement for each target tool’s native configuration system

---

## License

MIT
