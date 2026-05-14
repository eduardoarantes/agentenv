---
name: agentenv Development Guide
description: Development areas and conventions across the Rust CLI, core library, and VS Code extension
version: 1.0.0
---

# agentenv Development Guide

This file describes the three development areas of agentenv — the Rust CLI (`agentenv`), the core library (`agentenv-core`), and the VS Code extension (`vscode`) — and the conventions, scope, and useful commands for each.

## Development areas

### Rust core and CLI
**Purpose:** Rust core library and CLI development with TDD focus

**Scope:**
- `crates/agentenv-core/` - Core library logic
- `crates/agentenv-cli/` - CLI implementation
- Unit tests and integration tests

**Skills:**
- Test-driven development (write tests first)
- Error handling with `thiserror`
- Async/await with tokio
- Configuration parsing (YAML/JSON)
- File system operations

**Commands:**
- `cargo test` - Run all tests
- `cargo clippy` - Lint checks
- `cargo fmt` - Format code
- `cargo build` - Build project

---

### VS Code extension
**Purpose:** VS Code extension development

**Scope:**
- `vscode/src/` - TypeScript extension code
- Extension manifest and configuration
- Integration with agentenv CLI

**Skills:**
- VS Code API knowledge
- TypeScript strict mode
- ESLint and Prettier formatting
- Extension lifecycle (activation, deactivation)
- Command registration

**Commands:**
- `npm run compile` - Compile TypeScript
- `npm run lint` - ESLint checks
- `npm run format` - Prettier formatting

---

### CLI ↔ extension integration
**Purpose:** Design and implement integration between CLI and extension

**Scope:**
- Cross-crate communication
- Plugin resolution flow
- Symlink management
- Target-tool integration

**Skills:**
- Architectural patterns
- API design
- Error propagation
- Configuration synchronization

**Commands:**
- Document integration points
- Design data flows
- Implement error contracts

---

## Source-driven canonical pipelines

`.agentrc.yaml` declares a single `source: <tool>` — currently `claude-code`.
For every capability (hooks, skills, agents), sync:

1. Reads the source's native layout losslessly into a canonical YAML under
   `<project>/.agentenv/<capability>.canonical.yaml`. Open-map frontmatter
   plus an explicit `Native`-style escape hatch (for hooks) ensure no
   information is dropped between source and canonical.
2. Renders the canonical out to every configured non-source target via
   per-capability writers. Each writer reports `drops` for fields/items it
   cannot represent; nothing is silently lost.

```yaml
source: claude-code

targets:
  cursor: {}
  codex: {}
```

`TargetConfig` is intentionally empty — opting a target in is enough.
Path conventions and translation logic live in
`crates/agentenv-core/src/<capability>/writers/` (one module per
capability), not in config.

### Adding a new writer
1. Add a `<target>.rs` (or extend the dispatch in `writers/mod.rs`) under
   `crates/agentenv-core/src/<capability>/writers/`.
2. Register it in `write_targets()` and the dispatch `match` in that
   module's `write` function.
3. Add the target name to `Config::KNOWN_TARGETS` in
   `crates/agentenv-core/src/config.rs`.
4. Document the path/format in `docs/platform-standards.md`.

### Adding a new source reader
1. Add a `<source>.rs` under `crates/agentenv-core/src/<capability>/readers/`.
2. Register it in the dispatch `match` in that module's `readers/mod.rs`.
3. Add the target name to `Config::SOURCE_TARGETS_V1`.
4. Document the layout in `docs/platform-standards.md`.

---

## Development Workflow

### 1. Define Test Cases
Write tests in `tests/` directory before implementing features.

### 2. Implement Core Logic
Add implementation in `crates/agentenv-core/src/`

### 3. CLI Integration
Wire core logic into CLI commands in `crates/agentenv-cli/src/`

### 4. Extension Integration
Add UI/UX in `vscode/src/`

### 5. Configuration Testing
Test with various target configurations in `.agentrc.yaml`

---

## Testing Strategy

- **Unit tests**: Test individual modules in-place (`#[cfg(test)]`)
- **Integration tests**: Test cross-crate functionality in `tests/`
- **Configuration tests**: Validate config parsing and target resolution
- **E2E tests**: Test full sync flow with temporary environments

---

## Cross-platform conventions (CI runs Linux + macOS + Windows)

CI's `Test Suite (windows-latest, stable)` matrix entry catches Windows-only
regressions late. The pitfalls below have all bitten this repo at least
once — internalize them before writing new filesystem code or tests.

### 1. `std::os::unix::fs::symlink` does not compile on Windows
Reach for it in test setup ("seed a foreign / stale symlink so the writer's
detection logic has something to detect") and the entire `agentenv-core`
test binary fails to compile, taking the Windows job with it.

- **Production code:** use `SymlinkManager::create_symlink`
  (`crates/agentenv-core/src/symlink.rs`). It already dispatches to
  `std::os::unix::fs::symlink` on Unix and `std::os::windows::fs::symlink_dir`
  on Windows under the appropriate `cfg`.
- **Tests:** gate the whole `#[test]` function with `#[cfg(unix)]` — the
  established pattern in `crates/agentenv-core/src/hooks/writers/codex.rs`
  (nearly every test there carries the attribute). Production code reachable
  on Windows must not be the thing being skipped; only test scaffolding that
  needs to *manually* place a Unix symlink before invoking the writer
  belongs behind `#[cfg(unix)]`.

### 2. `symlink_dir` is the only Windows symlink primitive we use
`SymlinkManager::create_symlink` calls `std::os::windows::fs::symlink_dir`
unconditionally on Windows. That's correct for skills (which ARE
directories), but it's a known sharp edge when symlinking individual files
on Windows (e.g. an agent `.md` file). If you need file symlinks on
Windows, extend `SymlinkManager` to choose `symlink_file` vs `symlink_dir`
based on the source's `file_type()` rather than guessing inline. Don't
sprinkle `std::os::windows::fs::symlink_file` across callers.

Windows also requires either Administrator privileges or **Developer Mode**
to create symlinks. GitHub's `windows-latest` runner has Developer Mode on,
but local Windows machines may not — surfacing a clearer error in that
case is welcome.

### 3. Never serialize OS-native path separators into portable artifacts
Canonical YAML under `<project>/.agentenv/<capability>.canonical.yaml` is
meant to round-trip across machines: a project synced on Windows must
produce a canonical that's bit-for-bit identical to the same project
synced on macOS or Linux (modulo the `source_dir` / `source_file` fields,
which are `#[serde(skip)]`). Any path field that IS serialized must use
forward slashes regardless of the host OS.

`Path::strip_prefix` returns OS-native components (`scripts\run.sh` on
Windows), so the obvious code is wrong:

```rust
// WRONG on Windows — emits backslashes into the canonical
let relative = path.strip_prefix(root)?.to_path_buf();
```

Normalize at construction time by joining the components with `/`:

```rust
// Right — portable canonical regardless of OS.
let relative: String = path
    .strip_prefix(root)?
    .components()
    .filter_map(|c| c.as_os_str().to_str())
    .collect::<Vec<_>>()
    .join("/");
let relative_buf = std::path::PathBuf::from(relative);
```

Tests that compare these paths must use forward-slash literals
(`"scripts/run.sh"`, never `"scripts\\run.sh"`) and run on every OS —
don't paper over the bug with `#[cfg(unix)]`.

Path fields meant only for the local sync run (e.g. `CanonicalSkill.source_dir`
or `CanonicalAgent.source_file`) should be marked `#[serde(skip)]` so they
stay environment-local and never reach the cross-platform canonical.

### 4. Don't rely on git's default line-ending munging
The marketplace clone passes `-c core.autocrlf=false` and persists that
setting so refetches keep LF intact (see
[`crates/agentenv-core/src/marketplace.rs`](crates/agentenv-core/src/marketplace.rs:214)
and the runbook in CONTRIBUTING.md). When you add new git operations,
preserve that flag explicitly — a future commit that drops it would silently
corrupt skill scripts and hook command strings on Windows.

### 5. Treat Windows CI as a hard gate
- Run `cargo test --all` locally before pushing. If you have a Windows
  VM/box, run it there too — but at minimum, scan every new test for the
  three checklist items above (Unix symlinks, Windows `symlink_dir` for
  file targets, path-separator string comparisons).
- A Windows-only failure means the `Test Suite (windows-latest, stable)`
  job in the PR is red and the PR is **not** mergeable until fixed —
  even if Linux and macOS are green. Don't merge around it.

When in doubt, ask: *does this code path appear in a Windows CI failure?*
If it does or could, write the cross-platform version first.

---

## Related Files

- `.agentrc.example.yaml` - Example configuration with target definitions
- `schemas/agentrc.schema.json` - JSON schema for configuration validation
- `Cargo.toml` - Rust project configuration
- `vscode/package.json` - Extension configuration
