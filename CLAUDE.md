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

## Related Files

- `.agentrc.example.yaml` - Example configuration with target definitions
- `schemas/agentrc.schema.json` - JSON schema for configuration validation
- `Cargo.toml` - Rust project configuration
- `vscode/package.json` - Extension configuration
