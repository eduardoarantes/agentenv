# Contributing to agentenv

Thanks for contributing. This project is intended to stay small, deterministic, and modular.

---

## Project philosophy

`agentenv` exists to make project-specific AI agent/plugin environments reproducible across tools.

The project values:

- deterministic filesystem operations
- explicit configuration
- safe symlink management
- clear diagnostics
- target-config-driven support for different AI tools
- minimal hidden state

Avoid adding behavior that is implicit, global, or difficult to explain.

---

## Repository layout

```text
agentenv/
  crates/
    agentenv-core/
    agentenv-cli/
  vscode/
  docs/
  schemas/
```

### `agentenv-core`

Reusable logic, organised by responsibility:

- `config` — `.agentrc.yaml` schema and path resolution
- `loader` — YAML parsing and validation
- `claude_config` — reads `~/.claude/settings.json` and the project's `.claude/settings.json`, translates `extraKnownMarketplaces` / `enabledPlugins` / `hooks` into the agentenv config model. Loaded implicitly when `source: claude-code` is set; missing settings.json files are tolerated.
- `marketplace` — Git-backed plugin source, including the Claude Code `marketplace.json` index format
- `resolver` — locating plugins inside a marketplace and inferring their capabilities
- `skills`, `agents`, `hooks` — source-driven capability pipelines (types, reader, canonical_io, per-target writers). Each module mirrors the others' structure.
- `symlink` — idempotent link creation and removal with cross-platform handling
- `state` — the on-disk record of agentenv-managed links (`.agentenv/state.json`)
- `sync` — the planner + executor that ties the above together
- `clean` — removes only links recorded in state
- `init` — writes a default `.agentrc.yaml`

### `agentenv-cli`

Thin CLI entrypoint over `agentenv-core`. Subcommands: `init`, `sync`, `list`, `doctor`, `explain`, `clean`, `claude-config show`, `canonical show <skills|agents|hooks>`.

### `vscode`

The VS Code extension. It calls the CLI rather than reimplementing sync logic.

---

## Requirements

- Rust stable
- Git
- Node.js and npm only if working on the VS Code extension

---

## Build

```bash
cargo build
```

---

## Run locally

```bash
cargo run -- init
cargo run -- sync
cargo run -- explain   # show what sync would do, without touching the filesystem
cargo run -- doctor
cargo run -- list
cargo run -- clean
```

Use `--project <path>` (global flag) to operate on a project other than the current directory.

---

## Format and lint

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

CI enforces both with `-D warnings`, so warnings break the build.

---

## Pre-commit hook

A pre-commit hook is shipped in `.githooks/pre-commit`. It runs `cargo fmt --check` and `cargo clippy -- -D warnings` whenever a commit touches Rust sources, `Cargo.toml`, `Cargo.lock`, or `rustfmt.toml` — matching the CI fmt + clippy gates.

Enable it once per clone:

```bash
git config core.hooksPath .githooks
```

To bypass it for a single commit (not recommended), use `git commit --no-verify`.

---

## Test

```bash
cargo test --all
```

Integration coverage to maintain:

- config parsing and invalid-config diagnostics
- marketplace path resolution (`~/`, relative, absolute)
- marketplace clone / fetch / offline / refetch-failure paths
- plugin resolution from a `marketplace.json` index
- capability inference from plugin subdirectories
- symlink creation and idempotent re-sync
- symlink cleanup driven by `state.json`
- target-config defaults and per-target overrides

---

## Core concepts

### Config

`.agentrc.yaml` is the project source of truth. See `.agentrc.example.yaml` for a working sample.

Rules:

- Do not mutate config automatically
- Preserve comments when possible
- Prefer explicit fields over inferred global state

---

### Marketplace

A marketplace is an external Git repository containing plugins, indexed by a `.claude-plugin/marketplace.json` file at its root (Claude Code marketplace convention).

Marketplace `path` resolution:

- `~/foo` → expanded against `$HOME`
- absolute paths → used as-is
- relative paths → joined with the project root
- `.` and `..` segments are collapsed lexically (no filesystem `canonicalize`, so paths that don't yet exist still resolve correctly)

Fetch behaviour:

- Missing marketplace + online → cloned with `--single-branch --branch <ref>` and `core.autocrlf=false`
- Missing marketplace + offline → hard error
- Existing marketplace + cache mode → reused as-is
- Existing marketplace + refetch mode → `git fetch` + `git reset --hard FETCH_HEAD`
- Refetch network failure with a local copy present → warning, local copy reused

The marketplace cache directory is treated as agentenv-managed: refetch resets the working tree. Don't put hand-edited content there.

---

### Plugin resolution

A plugin is selected by `name` (and optional `namespace`) from `.agentrc.yaml`. The resolver:

1. Looks up the plugin entry in `<marketplace>/.claude-plugin/marketplace.json`
2. Resolves its `source` directory relative to the marketplace root
3. Infers capabilities by checking which of `agents/`, `commands/`, `skills/`, `hooks/` exist as subdirectories of the plugin source

There is no per-plugin manifest file. Capabilities are folder-driven, which means adding a new capability folder to a plugin is a no-config change.

---

### Sync engine

Sync must be idempotent.

```bash
agentenv sync
agentenv sync
agentenv sync
```

Should produce the same final state every time.

Rules:

- Recreate managed symlinks; reuse existing ones when they already point at the right source
- Do not delete unmanaged files
- Surface conflicts clearly
- Use `agentenv explain` to inspect the planned actions without touching the filesystem

---

### Symlink ownership

Sync owns only the links recorded in `.agentenv/state.json`. `clean` removes those and nothing else.

Acceptable strategies for ownership tracking:

- the existing `state.json` sidecar (current implementation)
- a deterministic managed folder
- a recognizable prefix combined with explicit tracking

Unacceptable:

- deleting arbitrary files from target folders
- overwriting non-symlink user files without explicit user action

---

## Targets

`agentenv` is **source-driven**: `.agentrc.yaml` declares a single
`source: <tool>` (today: `claude-code`) and a set of write `targets:`. For
every capability (hooks, skills, agents) `sync`:

1. reads the source's native layout losslessly into
   `<project>/.agentenv/<capability>.canonical.yaml`, and
2. renders the canonical out to every configured non-source target via a
   per-capability writer.

`TargetConfig` is intentionally empty — opting a target in by listing it
(`cursor: {}`) is enough. Path conventions and write/refuse-on-conflict
logic live inside each capability's writers module
(`crates/agentenv-core/src/<capability>/writers/`), not in config.

### Adding a new writer (write target)

1. Add a `<target>.rs` under `crates/agentenv-core/src/<capability>/writers/`,
   or extend the dispatch in `writers/mod.rs`.
2. Register it in `write_targets()` and the dispatch `match` in that
   module's `write` function.
3. Add the target name to `Config::KNOWN_TARGETS` in
   `crates/agentenv-core/src/config.rs` (and the per-capability v1 write
   list if applicable, e.g. `HOOK_WRITE_TARGETS_V1`).
4. Document the destination path and translation losses in
   `docs/platform-standards.md` and (for hooks) `docs/HOOKS.md`.
5. If the target uses a non-Markdown leaf format (TOML, JSON, …), the
   writer must materialize the canonical to that format and report drops
   in the sync output — silent loss is unacceptable.

### Adding a new source reader

1. Add a `<source>.rs` under `crates/agentenv-core/src/<capability>/readers/`.
2. Register it in the dispatch `match` in that module's `readers/mod.rs`.
3. Add the source name to `Config::SOURCE_TARGETS_V1` so validation
   accepts it.
4. Document the native layout being read in `docs/platform-standards.md`
   (and `docs/HOOKS.md` for hooks).

---

## CLI command guidelines

CLI commands should be:

- predictable
- scriptable
- non-interactive by default
- explicit about filesystem changes

Currently supported subcommands and flags:

```bash
agentenv init [--force]
agentenv sync [--refetch | --no-fetch]
agentenv list
agentenv doctor
agentenv explain
agentenv clean
agentenv [<subcommand>] --project <path>
```

`explain` already covers the dry-run use case; prefer adding a new subcommand over piling more flags onto `sync`.

---

## Error handling

Use the structured `agentenv_core::Error` type.

Guidelines:

- Marketplace fetch failure: warning if local marketplace exists, error otherwise
- Missing selected plugin: error
- Unsupported target: error
- Unsupported plugin capability for target: warning or error depending on severity
- Destination conflict: error unless explicitly overridden

Error messages should include:

- what failed
- why it likely failed
- how to fix it

---

## Logging and diagnostics

`tracing` is the logging facade. The CLI installs a default subscriber filtered to `agentenv=info`; bump it via `RUST_LOG=agentenv=debug` for verbose output.

Recommended levels:

- `info` — normal operations
- `warn` — recoverable issues
- `error` — failed operations
- `debug` — detailed resolution and link planning

---

## Commit convention

Use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` new feature
- `fix:` bug fix
- `docs:` documentation
- `refactor:` internal change
- `test:` tests
- `chore:` maintenance
- `feat!:` / `fix!:` (or a `BREAKING CHANGE:` footer) for breaking changes

Examples:

```bash
git commit -m "feat: add cursor target adapter"
git commit -m "fix: preserve unmanaged files during clean"
git commit -m "docs: document marketplace path resolution"
```

---

## Claude Code skills

This repo ships repo-local [Claude Code](https://claude.com/claude-code) skills under `.claude/skills/` that automate the review-and-ship loop. They're optional — if you don't use Claude Code, the equivalent manual flow is `cargo fmt && cargo clippy && cargo test && git push && gh pr create`.

- **`/review-and-fix`** — runs `/review` on the current branch, writes findings to a tracker file at `.agentenv/review-<timestamp>.md`, asks you which items to fix, and applies each fix in an isolated sub-agent (one commit per item). Use when you want to drive a review through to applied commits without context bleeding between items.
- **`/ship-changes`** — audits the working tree for missing documentation updates, then commits, pushes, and opens a PR. Use when wrapping a tested working state into a published PR.

Tracker files written by `/review-and-fix` land in `.agentenv/`, which is gitignored.

---

## Pull request guidelines

Before opening a PR:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

A good PR:

- has a focused change
- adds tests when behavior changes
- updates documentation when user-facing behavior changes
- explains design decisions

Avoid:

- broad unrelated refactors
- changing config semantics without discussion
- introducing global side effects

PRs to `main` must pass the full CI matrix (Linux/macOS/Windows test suite, rustfmt, clippy, security audit, VS Code extension build) before they can be merged.

---

## Good first issues

- improve `doctor` diagnostics
- expand `explain` output
- add writer-target edge-case tests (refuse-on-conflict, drop reporting)
- improve Windows symlink handling
- improve README examples

---

## Non-goals for early versions

Before the core model is stable, avoid:

- cloud-hosted marketplace service
- plugin execution runtime
- GUI application
- automatic plugin publishing
- opaque global background daemon

---

## Releasing

When changes have landed on `main` and you're ready to publish the CLI or
the VS Code extension to crates.io / npm / Homebrew / Open VSX / VS Code
Marketplace, follow the runbook at [docs/RELEASING.md](docs/RELEASING.md).
It covers the automated tag-push flow for each channel plus the one
manual step (VS Code Marketplace upload).

---

## Security considerations

`agentenv` manipulates filesystem links and fetches Git repositories. Contributions should consider:

- path traversal during marketplace path resolution and plugin install
- symlink escape outside the project root
- unsafe deletion of unmanaged files
- malicious plugin or marketplace content
- shell injection in Git operations

Never execute plugin code during sync.

---

## License

By contributing, you agree that your contributions will be licensed under the project license (MIT).
