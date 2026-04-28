# agentenv

> Project-scoped AI agent and plugin environment manager

`agentenv` lets you define, version, and reproduce the AI capabilities of a project across tools like Claude Code, Codex, Cursor, and others.

It works similarly to `.nvmrc`, `.jenv`, or `.tool-versions`, but instead of runtime versions, it manages **agents, commands, skills, and plugins**.

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
brew tap your-org/agentenv
brew install agentenv
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

plugins:
  - name: engineering-standards
  - name: python-agents

targets:
  claude-code: {}
  cursor: {}

sync:
  onOpen: true
  refetch: true
  mode: symlink
```


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
```

### Fields

| Field | Required | Description |
|---|---:|---|
| `version` | Yes | Config schema version. |
| `marketplaces` | Yes | Marketplace repos keyed by namespace. |
| `marketplaces.<namespace>.path` | Yes | Local filesystem path to the marketplace repo. |
| `marketplaces.<namespace>.remote` | Yes | Git remote used to clone/fetch the marketplace. |
| `marketplaces.<namespace>.ref` | No | Branch, tag, or commit to use. Defaults to `main`. |
| `plugins` | No | List of plugins to import. A plugin without `namespace` uses `default`. |
| `targets` | Yes | Object of tool adapters keyed by target name. Empty target configs use built-in defaults when available. |
| `sync.onOpen` | No | Whether editor integrations should sync on workspace open. |
| `sync.refetch` | No | Whether to fetch marketplace updates before syncing. |
| `sync.mode` | No | Link strategy. Initially `symlink`. |

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
agentenv sync
```

Expected behavior:

- Fetch failure should not stop sync if a local marketplace already exists
- Broken or missing plugins should produce diagnostics
- Existing managed links should be recreated
- User-created files should not be deleted

---

### `agentenv list`

List plugins available in the configured marketplace.

```bash
agentenv list
```

Optional future filters:

```bash
agentenv list --installed
agentenv list --target claude-code
```

---

### `agentenv doctor`

Diagnose project and system state.

```bash
agentenv doctor
```

Checks:

- config validity
- marketplace availability
- git fetch status
- plugin existence
- plugin manifest validity
- target adapter support
- broken symlinks
- destination folder permissions
- Windows symlink permission issues

---

### `agentenv clean`

Remove links previously managed by `agentenv`.

```bash
agentenv clean
```

This must not delete unmanaged user files.

---

### `agentenv explain`

Explain what would be linked and where.

```bash
agentenv explain
```

Useful for debugging and CI review.

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

The VS Code extension should be thin. The CLI remains the source of truth.

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

Suggested commands:

- `Agentenv: Sync`
- `Agentenv: Doctor`
- `Agentenv: Open Config`
- `Agentenv: List Plugins`

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

- [ ] Lockfile support: `agentenv.lock`
- [ ] Plugin version pinning
- [ ] Remote plugin sources beyond one marketplace
- [ ] Windows symlink fallback strategies
- [ ] More target adapters
- [ ] CI integration: `agentenv check`
- [ ] JSON Schema for `.agentrc.yaml`
- [ ] VS Code extension
- [ ] Dry-run mode
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
