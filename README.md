# agentenv

[![CI](https://img.shields.io/github/actions/workflow/status/eduardoarantes/agentenv/ci.yml?branch=main&label=CI)](https://github.com/eduardoarantes/agentenv/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/agentenv.svg?label=crates.io)](https://crates.io/crates/agentenv)
[![npm](https://img.shields.io/npm/v/@eduardoarantes/agentenv.svg?label=npm)](https://www.npmjs.com/package/@eduardoarantes/agentenv)
[![Homebrew](https://img.shields.io/badge/homebrew-tap-orange?logo=homebrew)](https://github.com/eduardoarantes/homebrew-agentenv)
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
npm install -g @eduardoarantes/agentenv
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

# Source-of-truth tool. agentenv reads its native layout losslessly and
# renders the canonical out to every other configured target. v1 supports
# `claude-code` as the source.
source: claude-code

marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/eduardoarantes/claude-code-plugin-marketplace.git
    ref: main

# Add plugin entries here, e.g.:
#   - name: git-simple
plugins: []

# Tools to materialize the canonical for. `claude-code` is the source and
# is never written. Opt in by uncommenting.
targets:
  cursor: {}
  # codex: {}
  # copilot: {}
  # gemini-cli: {}
  # junie: {}

sync:
  onOpen: true
  refetch: true
```

The plugin list is empty so the first `agentenv sync` succeeds — open
`.agentrc.yaml` and add plugins as you need them. See the
[Configuration](#configuration) section below for the full shape.

`init` also adds `.agentenv/` to the project's `.gitignore` (creating the file
if needed). That directory holds `state.json`, a per-machine manifest of the
links agentenv installed — it stores absolute paths and is regenerated on
every `agentenv sync`, so it should not be committed.


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

# Source of truth for hooks, skills, and agents. agentenv reads the named
# tool's native layout losslessly into .agentenv/<capability>.canonical.yaml
# and renders that canonical out to every other configured target. Setting
# `source: claude-code` also implicitly imports marketplaces / plugins from
# `~/.claude/settings.json` and `<project>/.claude/settings.json` (missing
# files are tolerated). See "Importing from Claude Code" below.
source: claude-code

marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/eduardoarantes/claude-code-plugin-marketplace.git
    ref: main

plugins:
  - name: python-agents
  - name: pr-review
  - name: engineering-standards

# Set-membership map of write targets. Per-target object is currently `{}`;
# the source target is never written, even if listed.
targets:
  cursor: {}
  codex: {}

sync:
  onOpen: true
  refetch: true

clean:
  pruneEmptyDirs: true
```

### Fields

| Field | Required | Description |
|---|---:|---|
| `version` | Yes | Config schema version. |
| `source` | Yes² | Source-of-truth tool for the canonical pipelines. v1 supports `claude-code`. Setting `source: claude-code` also implicitly imports `extraKnownMarketplaces` and `enabledPlugins` from `~/.claude/settings.json` and `<project>/.claude/settings.json` (project wins over global; explicit `.agentrc.yaml` entries win over both). The source target is read-only and is dropped from `targets:` during sync if listed. See [Importing from Claude Code](#importing-from-claude-code) below. |
| `marketplaces` | Yes¹ | Marketplace repos keyed by namespace. |
| `marketplaces.<namespace>.path` | Yes | Local cache directory where agentenv clones the marketplace repo (from `remote` at `ref`). It's not a path inside the project — it's an agentenv-managed checkout used as a read-only source. Supports `~` (home), absolute paths, and relative paths (resolved against the project root). Refetches reset the working tree to `origin/<ref>`, so don't hand-edit anything inside it. A common choice is `~/.agentenv/marketplace` to share the cache across projects. |
| `marketplaces.<namespace>.remote` | Yes | Git remote used to clone/fetch the marketplace. |
| `marketplaces.<namespace>.ref` | No | Branch, tag, or commit to use. Defaults to `main`. |
| `plugins` | No | List of plugins to import. A plugin without `namespace` uses `default`. |
| `targets` | No | Set-membership map of tools to materialize the canonical for. Each per-target object is currently `{}` — listing a target opts it in; path conventions live inside each capability's writers module. Recognised names: `claude-code`, `cursor`, `codex`, `copilot`, `gemini-cli`, `junie`, `antigravity`. Required when at least one capability needs propagation; `source:` is required whenever `targets:` is non-empty. |
| `sync.onOpen` | No | Whether editor integrations should sync on workspace open. |
| `sync.refetch` | No | Whether to fetch marketplace updates before syncing. |
| `clean.pruneEmptyDirs` | No | After `agentenv clean` removes managed links, prune any now-empty directories inside the project root. Stops at the project root and never touches dirs that still hold user files. Defaults to `true`. |
| `gitignore_managed_links` | No | When `true`, agentenv maintains a sentinel-delimited block in `<project>/.gitignore` listing every link/copy it currently owns. User-authored `.gitignore` lines outside the block are preserved verbatim. `agentenv clean` strips the block entirely. See [Auto-gitignoring managed links](#auto-gitignoring-managed-links) below. |
| `instruction_files` | No | Map of root-level source files (`CLAUDE.md`, `AGENTS.md`, `CURSOR.md`, …) to lists of project-relative destination paths. agentenv symlinks each source into each destination. **Never overrides** existing user files; agentenv-managed symlinks are updated when the source changes. See [Propagating instruction files](#propagating-instruction-files) below. |

¹ `marketplaces` may be omitted when `source: claude-code` is set and
Claude's `settings.json` provides at least one `extraKnownMarketplaces`
entry.

² `source` is required whenever `targets:` is non-empty (every capability
pipeline needs somewhere to read from). v1 only implements `claude-code`
as a source — setting any other value fails validation.

### Importing from Claude Code

Setting `source: claude-code` does double duty:

1. It declares Claude as the source of truth for the **canonical
   pipelines** — agentenv reads `.claude/settings.json` (hooks),
   `.claude/skills/` and `.claude/agents/` losslessly into
   `.agentenv/<capability>.canonical.yaml` and renders them out to every
   other configured write target.
2. It implicitly imports Claude's `extraKnownMarketplaces` and
   `enabledPlugins` from both `~/.claude/settings.json` and
   `<project>/.claude/settings.json` (project beats global; explicit
   `.agentrc.yaml` entries win over both). Missing settings.json files
   are tolerated.

```yaml
version: 1
source: claude-code

# Propagate Claude's plugins, skills, agents, and hooks to your other
# tools. The claude-code target is the source — listing it has no effect.
targets:
  cursor: {}
  codex: {}
```

Inspect what got imported with [`agentenv claude-config show`](#agentenv-claude-config-show)
and the generated canonical with [`agentenv canonical show`](#agentenv-canonical-show).

**Local assets are first-class.** With `source: claude-code`, your
project's `<project>/.claude/{agents,skills}/` directories ARE the source
— agentenv reads them directly into the canonical and writes each one out
to every configured target. A skill at `.claude/skills/refactor/SKILL.md`
materializes at `.cursor/skills/refactor/SKILL.md`,
`.codex/skills/refactor/SKILL.md` (via `.agents/`), and so on. Subagents
defined in `.claude/agents/<name>.md` go through the same flow, with the
Codex writer transforming Markdown frontmatter to TOML and reporting any
fields it had to drop.

Hooks come from the merged `~/.claude/settings.json` + project
`settings.json` `hooks` block. The writers are documented in
[docs/HOOKS.md](docs/HOOKS.md).

### Propagating instruction files

Different AI tools expect their cross-tool instruction sheet in different
places — Claude Code reads `CLAUDE.md`, Codex/Cursor/Copilot read
`AGENTS.md`, Junie reads `.junie/AGENTS.md`, Antigravity reads `agents.md`,
and so on. Rather than duplicate the same content N times, point at a single
source file and let agentenv mirror it everywhere.

#### Automatic defaults (`source: claude-code`)

When `source: claude-code` is set and you haven't written your own
`instruction_files:` block, agentenv applies a sensible default: it uses
`CLAUDE.md` (or `AGENTS.md` if no `CLAUDE.md` exists) as the source and
propagates it to each configured target's expected instruction-sheet path:

| Target | Default destination(s) |
| --- | --- |
| `codex`, `cursor`, `copilot` | `AGENTS.md` |
| `gemini-cli` | `GEMINI.md`, `AGENTS.md` |
| `junie` | `.junie/AGENTS.md` |
| `antigravity` | `agents.md` |
| (any other) | none |

So a minimal config like

```yaml
version: 1
source: claude-code
targets:
  cursor: {}
  junie: {}
```

automatically links `CLAUDE.md → AGENTS.md` and `CLAUDE.md → .junie/AGENTS.md`
on the next `agentenv sync`. Run `agentenv explain` to see exactly which
links would be created. To opt out of the defaults, write your own
`instruction_files:` block (described below) — any explicit entry replaces
the defaults entirely.

#### Manual propagation

For full control, write `instruction_files:` yourself:

```yaml
instruction_files:
  CLAUDE.md:                # source file at project root
    - AGENTS.md             # destinations (project-relative)
    - .junie/AGENTS.md
    - agents.md
  CURSOR.md:
    - .cursor/rules/main.md
```

After `agentenv sync`, each destination is a symlink pointing at the source
file. The destinations are tracked in `.agentenv/state.json` so
`agentenv clean` reverts them.

**Safety: agentenv never overrides existing files.** If a destination
already contains user content (a regular file, a directory, or a symlink
agentenv doesn't own), it's left untouched and a warning is logged.
Agentenv-managed symlinks pointing at the wrong source are updated when the
config changes — those are owned by agentenv, so updating them isn't an
override of user content.

If a configured source file doesn't exist at the project root, its
destinations are skipped with a warning. Removing an entry from
`instruction_files` and re-syncing cleans up the previously-managed
destinations.

### Auto-gitignoring managed links

Agentenv-managed symlinks are *derived state* — they're regenerated every
time `agentenv sync` runs, so committing them adds noise (every plugin
update produces a diff) and risks broken links across machines if absolute
paths leak in. Set `gitignore_managed_links: true` to have agentenv keep
your `.gitignore` honest:

```yaml
version: 1
source: claude-code
gitignore_managed_links: true
targets:
  cursor: {}
```

After `agentenv sync`, `<project>/.gitignore` looks like:

```
# (your existing user-authored lines stay above)
node_modules/
.env

# >>> agentenv managed (do not edit; regenerated by `agentenv sync`) <<<
/.cursor/agents/task-executor-tdd.md
/.cursor/agents/test-standard-reviewer.md
/.junie/AGENTS.md
# <<< agentenv managed >>>
```

Rules:

- **User-authored lines outside the block are preserved verbatim** — agentenv
  only rewrites the contents between the sentinel comments.
- **One entry per managed link**, sorted alphabetically for stable diffs.
- **Idempotent**: re-running sync with no changes produces an identical
  `.gitignore`.
- **`agentenv clean` strips the block entirely**, regardless of whether the
  flag is still set in `.agentrc.yaml`. Cleanup means cleanup.
- If `.gitignore` doesn't exist when sync runs, agentenv creates one
  containing just the managed block.

When the flag is `false` (the default), agentenv never touches `.gitignore`.

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

### `agentenv claude-config show`

Print the marketplaces, plugins, and hooks `agentenv` would import from your
Claude `settings.json` files. Useful for debugging the implicit Claude
settings import that runs whenever `source: claude-code` is set.

```bash
agentenv claude-config show          # human-readable
agentenv claude-config show --json   # machine-readable
```

This command reads `~/.claude/settings.json` and
`<project>/.claude/settings.json` directly — it does not require
`source: claude-code` to be set in `.agentrc.yaml`, so you can preview the
import before flipping the switch.

---

### `agentenv canonical show`

Print the canonical YAML agentenv generated for a capability under
`<project>/.agentenv/<capability>.canonical.yaml`. Useful for inspecting
what the source reader actually captured before the writers materialize
it to each target.

```bash
agentenv canonical show skills
agentenv canonical show agents
agentenv canonical show hooks
```

Errors out with a helpful message if no canonical exists yet — run
`agentenv sync` first.

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

agentenv is **source-driven**: one tool is declared the source of truth
(`source: claude-code` in v1) and each other listed target receives the
capability via a per-capability writer module under
`crates/agentenv-core/src/<capability>/writers/`. Each writer owns:

- the destination path on disk
- format translation (e.g. Codex's agent files are TOML, not Markdown)
- refuse-on-conflict detection so agentenv never clobbers user-authored
  content
- a `drops` report so any field/event a target cannot represent is
  surfaced in the sync output rather than silently lost

Internal layout (one module per capability):

```text
crates/agentenv-core/src/
  hooks/
    readers/{claude_code.rs, mod.rs}
    writers/{cursor.rs, codex.rs, mod.rs}
    canonical_io.rs
    pipeline.rs
  skills/
    readers/{claude_code.rs, mod.rs}
    writers/mod.rs       # symlink writer for all targets
    ...
  agents/
    readers/{claude_code.rs, mod.rs}
    writers/{codex.rs, mod.rs}   # codex.rs materializes to TOML
    ...
```

v1 source: `claude-code`. v1 write targets:

| Capability | cursor | codex | copilot | gemini-cli | junie | antigravity |
| --- | --- | --- | --- | --- | --- | --- |
| skills | ✓ | ✓ (`.agents/`) | ✓ | ✓ | ✓ | ✓ (`.agent/`) |
| agents | ✓ | ✓ (TOML) | ✓ (`.agent.md`) | ✓ | ✓ | skip |
| hooks  | ✓ | ✓ (`~/.codex/config.toml`) | — | — | — | — |

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
- [x] Import config + propagate capabilities from Claude Code (`source: claude-code`)
- [x] Source-driven canonical pipelines for hooks, skills, and agents
- [x] Propagate instruction files (`CLAUDE.md` → `AGENTS.md`, `.junie/AGENTS.md`, …) via `instruction_files`
- [x] Auto-gitignore managed links (`gitignore_managed_links: true`)

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
