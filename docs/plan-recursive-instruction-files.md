# Plan: Recursive Instruction File Discovery

## Context

`instruction_files` currently propagates a single project-root file (e.g. `CLAUDE.md`) to a fixed list of named destinations (e.g. `AGENTS.md`, `.junie/AGENTS.md`). But every major AI tool supports hierarchical instruction files in subdirectories:

- **Codex**: reads `AGENTS.md` at repo root and every directory along the path to `$CWD`
- **Cursor**: reads `AGENTS.md` at root and nested subdirs (more specific wins)
- **Copilot**: accepts `AGENTS.md` anywhere in the repo
- **Claude Code**: discovers skills from nested `.claude/skills/` in monorepos

So in a monorepo with `packages/frontend/CLAUDE.md` and `packages/backend/CLAUDE.md`, those files are invisible to agentenv. Users must manually maintain tool equivalents in every subdirectory.

The fix is to make recursive discovery the **default** behavior, driven by the existing `instruction_files` mappings, with two new config knobs to opt out and control depth.

---

## Design

### Behavior change: recursive by default

Recursive discovery is **on by default** — no new opt-in required. The existing `instruction_files` mappings already define which source filenames to look for and what destinations to produce. The recursive walker reuses those mappings, just applying them to every subdirectory instead of only the project root.

Example: if the user has:

```yaml
instruction_files:
  CLAUDE.md:
    - AGENTS.md
    - GEMINI.md
```

Then for every `<subdir>/CLAUDE.md` found in the project tree, agentenv also creates symlinks at `<subdir>/AGENTS.md` and `<subdir>/GEMINI.md` — same rule, same directory.

### Destination filename inferred from source

No new config key for recursive destinations. The destination filenames come from the same `instruction_files` value list. Users configure once at the top level; the recursive walk applies the mapping everywhere it finds the source file.

### Two new config fields

```yaml
# Disable recursive discovery entirely (default: true)
recursive_instruction_files: false

# How many directory levels deep to search (default: 8)
recursive_instruction_files_depth: 4
```

Both fields live at the top level of `.agentrc.yaml`, alongside `instruction_files`.

### Schema changes (`schemas/agentrc.schema.json`)

Add two fields:

```json
"recursive_instruction_files": {
  "type": "boolean",
  "default": true,
  "description": "When true (default), agentenv walks subdirectories and applies instruction_files mappings to every discovered source file, not just the one at the project root."
},
"recursive_instruction_files_depth": {
  "type": "integer",
  "default": 8,
  "minimum": 1,
  "description": "Maximum directory depth for recursive instruction file discovery. Depth 1 means only immediate children of the project root."
}
```

### Config struct (`crates/agentenv-core/src/config.rs:60`)

Add two fields next to `instruction_files`:

```rust
#[serde(default = "default_recursive_instruction_files")]
pub recursive_instruction_files: bool,

#[serde(default = "default_recursive_instruction_files_depth")]
pub recursive_instruction_files_depth: u32,
```

With defaults:

```rust
fn default_recursive_instruction_files() -> bool { true }
fn default_recursive_instruction_files_depth() -> u32 { 8 }
```

Update all `Config` construction sites (~14 occurrences) to include both fields.

No change to `Config::has_work` — recursive discovery runs whenever `instruction_files` is non-empty and `recursive_instruction_files` is true.

### Walker (`crates/agentenv-core/src/sync.rs`)

Add a new function:

```rust
fn discover_recursive_instruction_files(
    project_root: &Path,
    source_filename: &str,
    max_depth: u32,
) -> Result<Vec<PathBuf>>
```

- Use the `ignore` crate (`WalkBuilder`) — add `ignore = "0.4"` to `Cargo.toml` if not present
- Respects `.gitignore` automatically
- Skips hidden dirs and a hard-coded skip list: `node_modules`, `target`, `dist`, `build`
- Applies `max_depth` from config
- **Skips the project root itself** (root-level files are already handled by the existing `plan_instruction_propagations`)
- Returns sorted `Vec<PathBuf>` for deterministic output, capped at 1 000 files (emit a warning if exceeded)

### Propagation logic (`crates/agentenv-core/src/sync.rs:313`)

Extend `plan_instruction_propagations` and `execute_instruction_propagations` rather than adding separate functions — keeps the call sites unchanged.

At the top of each function, after the root-level loop, add a second loop:

```rust
if config.recursive_instruction_files {
    for source_name in &sources {
        let destinations = &config.instruction_files[source_name];
        let discovered = discover_recursive_instruction_files(
            project_root,
            source_name,
            config.recursive_instruction_files_depth,
        )?;
        for discovered_source in discovered {
            let dir = discovered_source.parent().unwrap();
            for dest in destinations {
                let dest_path = dir.join(dest);
                // same classify_instruction_destination call as root loop
            }
        }
    }
}
```

---

## Performance Considerations

| Concern | Mitigation |
|---|---|
| Large repos with many files | `ignore` crate prunes via `.gitignore` before any stat call |
| `node_modules` / `target` not gitignored | Hard-coded skip list; prune whole subtrees immediately |
| Repeated syncs re-walk the tree | Guard: skip if `instruction_files` is empty or `recursive_instruction_files` is false |
| Very deep directory trees | `recursive_instruction_files_depth` cap (default 8) |
| Many matching files | Cap at 1 000 found files; warn if exceeded |
| Parallel walk | `ignore::WalkBuilder::build_parallel()` gives rayon-style parallelism for free |

The dominant cost is directory enumeration. With `.gitignore` pruning and the skip list, a typical monorepo walk (1 000–5 000 source dirs) completes in <100 ms on an SSD.

---

## Files to Modify

| File | Change |
|---|---|
| `crates/agentenv-core/src/config.rs:60` | Add `recursive_instruction_files: bool` and `recursive_instruction_files_depth: u32` + update all `Config` construction sites |
| `crates/agentenv-core/src/sync.rs:313` | Add `discover_recursive_instruction_files`; extend `plan_instruction_propagations` and `execute_instruction_propagations` with the recursive loop |
| `crates/agentenv-core/Cargo.toml` | Add `ignore = "0.4"` if not present |
| `schemas/agentrc.schema.json:122` | Add `recursive_instruction_files` boolean and `recursive_instruction_files_depth` integer schemas |
| `.agentrc.example.yaml` | Add commented examples for both new flags |
| `docs/platform-standards.md` | Document the new behavior |

---

## Verification

1. Create a test project with `packages/frontend/CLAUDE.md` and `packages/backend/CLAUDE.md`
2. Configure `instruction_files: { "CLAUDE.md": ["AGENTS.md"] }` (no extra flags needed — recursive is on by default)
3. Run `agentenv sync` — verify symlinks at `packages/frontend/AGENTS.md` and `packages/backend/AGENTS.md`
4. Set `recursive_instruction_files: false` → re-sync → symlinks removed from subdirs, root still works
5. Set `recursive_instruction_files_depth: 1` → verify only immediate children are discovered
6. Add unit tests for `discover_recursive_instruction_files`:
   - Skips project root (root handled separately)
   - Skips hidden dirs and `node_modules`
   - Respects `max_depth`
   - Deterministic ordering
   - Warns and caps at 1 000 results
7. `cargo test && cargo clippy` — clean
