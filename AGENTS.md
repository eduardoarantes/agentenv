---
name: agentenv Agents
description: Specialized agents for agentenv development across Rust CLI and TypeScript extension
version: 1.0.0
---

# agentenv Custom Agents

These agents help coordinate development across the Rust CLI (`agentenv`), core library (`agentenv-core`), and VS Code extension (`vscode`).

## Agents

### rust-core-dev
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

### vscode-extension-dev
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
- `npm test` - Run tests

---

### integration-architect
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

## Configuration-Driven Target System

### Target Definition
Each target is defined as a configuration with `source` and `target` mappings:

```yaml
targets:
  claude-code:
    type: vscode-extension
    tools: [claude-code]
    paths:
      config: ~/.vscode/extensions/github.claude-code
    source_mappings:
      skills:
        - source: ~/.agentenv/marketplace/skills
          target: .claude-code/skills
      commands:
        - source: ~/.agentenv/marketplace/commands
          target: .claude-code/commands
      agents:
        - source: ~/.agentenv/marketplace/agents
          target: .claude-code/agents
```

### Adding New Targets
To add a new target (e.g., `jetbrains-ide`):

1. Add target configuration to `.agentrc.yaml`
2. Optionally add validation rules to config schema
3. No code changes needed for basic support

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
