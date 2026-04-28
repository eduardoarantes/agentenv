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
- adapter-based support for different AI tools
- minimal hidden state

Avoid adding behavior that is implicit, global, or difficult to explain.

---

## Expected architecture

Recommended layout:

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

Contains reusable logic:

- config parsing
- marketplace fetching
- plugin resolution
- target adapter interfaces
- symlink reconciliation
- diagnostics

### `agentenv-cli`

Contains the CLI entrypoint and commands:

- `init`
- `sync`
- `list`
- `doctor`
- `clean`
- `explain`

### `vscode`

Contains the VS Code extension.

The extension should call the CLI rather than reimplementing sync logic.

---

## Requirements

- Rust stable
- Git
- Node.js and npm/pnpm only if working on the VS Code extension

---

## Build

```bash
cargo build
```

---

## Run locally

```bash
cargo run -- sync
cargo run -- doctor
cargo run -- explain
```

---

## Format and lint

```bash
cargo fmt
cargo clippy --all-targets --all-features
```

---

## Test

```bash
cargo test
```

Integration tests should cover:

- config parsing
- invalid config diagnostics
- marketplace resolution
- missing plugin behavior
- symlink creation
- symlink cleanup
- target adapter mappings
- fetch failure fallback behavior

---

## Core concepts

### Config

`.agentrc.yaml` is the project source of truth.

Rules:

- Do not mutate config automatically
- Preserve comments when possible
- Prefer explicit fields over inferred global state

---

### Marketplace

A marketplace is an external Git repository containing plugins.

Rules:

- Fetch failures must be recoverable if a local copy exists
- The user should receive a warning if fetch fails
- Sync should continue with the existing local marketplace when possible
- Missing marketplace with failed clone is a hard error

---

### Plugin resolution

A plugin is selected by name from `.agentrc.yaml`.

The resolver should:

- locate the plugin in the marketplace
- read its manifest
- validate expected folders
- report unsupported or missing capabilities

---

### Sync engine

Sync must be idempotent.

Running this multiple times should produce the same final state:

```bash
agentenv sync
agentenv sync
agentenv sync
```

Rules:

- Recreate managed symlinks
- Do not delete unmanaged files
- Surface conflicts clearly
- Prefer dry-run support for debugging

---

### Symlink ownership

The sync engine must know which files it owns.

Acceptable strategies:

- managed manifest file
- deterministic managed folder
- metadata sidecar
- recognizable prefix combined with explicit tracking

Unacceptable:

- deleting arbitrary files from target folders
- overwriting non-symlink user files without explicit user action

---

## Target adapters

Each supported tool must have an isolated adapter.

A target adapter defines:

- target name
- destination paths
- supported plugin capabilities
- mapping rules
- validation rules

Example conceptual trait:

```rust
trait TargetAdapter {
    fn name(&self) -> &str;
    fn resolve_paths(&self, project_root: &Path) -> Result<TargetPaths>;
    fn supports(&self, plugin: &PluginManifest) -> bool;
    fn plan_links(&self, plugin: &ResolvedPlugin) -> Result<Vec<LinkPlan>>;
}
```

---

## Adding a new target

1. Create a new adapter module.
2. Implement the target adapter interface.
3. Add tests for path resolution and link planning.
4. Register the adapter.
5. Document the target in `README.md`.

Adapter PRs should include examples of the target tool’s expected folder structure.

---

## CLI command guidelines

CLI commands should be:

- predictable
- scriptable
- non-interactive by default
- explicit about filesystem changes

Preferred flags:

```bash
agentenv sync --dry-run
agentenv sync --no-fetch
agentenv doctor --json
agentenv explain --target claude-code
```

---

## Error handling

Use structured errors.

Guidelines:

- Marketplace fetch failure: warning if local marketplace exists
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

Recommended levels:

- `info`: normal operations
- `warn`: recoverable issues
- `error`: failed operations
- `debug`: detailed resolution and link planning

Machine-readable output should be supported for CI in the future.

---

## Commit convention

Use conventional commits:

- `feat:` new feature
- `fix:` bug fix
- `docs:` documentation
- `refactor:` internal change
- `test:` tests
- `chore:` maintenance

Examples:

```bash
git commit -m "feat: add cursor target adapter"
git commit -m "fix: preserve unmanaged files during clean"
git commit -m "docs: document marketplace config"
```

---

## Pull request guidelines

Before opening a PR:

```bash
cargo fmt
cargo clippy --all-targets --all-features
cargo test
```

A good PR includes:

- a focused change
- tests when behavior changes
- documentation updates when user-facing behavior changes
- clear explanation of design decisions

Avoid:

- broad unrelated refactors
- changing config semantics without discussion
- introducing global side effects

---

## Good first issues

Good initial contributions:

- improve diagnostics
- add dry-run output
- add JSON Schema for `.agentrc.yaml`
- add target adapter tests
- improve Windows symlink handling
- improve README examples

---

## Non-goals for early versions

Avoid adding these before the core model is stable:

- cloud-hosted marketplace service
- plugin execution runtime
- GUI application
- automatic plugin publishing
- opaque global background daemon

---

## Security considerations

This project manipulates filesystem links and may fetch Git repositories.

Contributions should consider:

- path traversal
- symlink escape attacks
- unsafe deletion
- malicious plugin names
- untrusted marketplace contents
- shell injection in Git operations

Never execute plugin code during sync.

---

## License

By contributing, you agree that your contributions will be licensed under the project license.
