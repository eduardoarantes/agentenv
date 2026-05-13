# Hooks — Source-Driven Canonical Pipeline

Hooks are event handlers that fire on AI tool lifecycle events (session start,
prompt submitted, tool use, turn end, ...). Every target tool that supports
hooks exposes its own event names, file format, and on-disk location, so a
hook authored for one tool does not run on another.

`agentenv`'s goal is **author once, materialize everywhere**: declare a
**source** target whose existing hooks file is the source of truth, and
`agentenv sync` translates those hooks into a canonical, agentenv-internal
model, then renders them out to every other supporting target's native file.

This document is the **specification**. It defines:

1. The source-driven pipeline (`source` → canonical → targets).
2. The canonical hook domain model (events, matchers, action shape).
3. Per-target native conventions, with links to official docs.
4. The canonical → native event mapping and translation losses.
5. Conflict handling and the read-only-source invariant.

Filesystem path conventions (where each tool reads its hooks file) remain
specified in [platform-standards.md §4](platform-standards.md#4-hooks). This
document references those paths but does not redefine them.

---

## Pipeline

```
.agentrc.yaml `source: <tool>`        (opt-in; no source ⇒ pipeline is a no-op)
         │
         ▼
<source tool's native hooks file>     (e.g. .claude/settings.json — READ-ONLY)
         │  lossless translation (never drop information)
         ▼
.agentenv/hooks.canonical.yaml        (generated, inspectable, gitignored)
         │  per-target render (drops with warning if unsupported)
         ▼
.cursor/hooks.json , ~/.codex/config.toml [notify]
```

### Invariants

- **Source is always read-only.** Whichever target you set as `source`, sync
  never writes back to its native hooks file. Edit the source tool's config
  the way you always have.
- **Source → canonical is lossless.** Anything the source tool can express
  round-trips through canonical. Common-vocabulary events become typed
  variants; anything source-specific is preserved verbatim via the `Native`
  escape hatch (see below).
- **Canonical → target may drop events with a warning.** Each target only
  receives events it can natively dispatch. Drops are surfaced in the sync
  report — they are never silent.
- **Writers refuse to clobber.** If a target's destination file already
  contains user-authored hooks, sync fails with a hard error. agentenv-
  managed files are recognized by an explicit marker.

### Opt-in semantics

The pipeline runs only when `source:` is set in `.agentrc.yaml`. Configuring
`cursor` or `codex` as a target without setting `source` is valid (e.g. you
want skills/agents propagation only) — the hooks pipeline simply stays
inactive.

---

## Quickstart

In `.agentrc.yaml`:

```yaml
version: 1
use_claude_config: true
source: claude-code   # declare Claude as the hooks source of truth

marketplaces:
  default:
    path: ~/.agentenv/marketplace
    remote: https://github.com/example/marketplace.git

targets:
  cursor: {}
  codex: {}
```

Hooks live in `~/.claude/settings.json` and/or `<project>/.claude/settings.json`
as usual:

```jsonc
{
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/audit.sh" }] }
    ],
    "Stop": [
      { "matcher": ".*",
        "hooks": [{ "type": "command", "command": "scripts/notify-done.sh" }] }
    ]
  }
}
```

After `agentenv sync` you get:

- `.agentenv/hooks.canonical.yaml` — the canonical artifact.
- `.cursor/hooks.json` — translated Cursor-native hooks.
- `~/.codex/config.toml` — managed `notify = [...]` block that fans out
  `Stop` hooks via an agentenv-emitted dispatcher script in
  `.agentenv/hooks/codex-notify-dispatch.sh`.

`.claude/settings.json` is untouched (source is read-only).

---

## Canonical model spec

The canonical artifact is written by sync to `.agentenv/hooks.canonical.yaml`
inside the project. Schema:

```yaml
# .agentenv/hooks.canonical.yaml  (generated)
source: claude-code            # echoes .agentrc.yaml `source`
hooks:
  - event: PreToolUse          # canonical PascalCase OR a Native object
    matcher: { tool: Bash }    # optional
    action:
      type: command
      command: "$CLAUDE_PROJECT_DIR/.claude/hooks/audit.sh"
      timeout_ms: 5000         # optional
      cwd: null                # optional
  - event:
      source: claude-code
      native_event: TeammateIdle
      payload:                 # verbatim source-tool entry
        matcher: "*"
        hooks:
          - { type: command, command: "..." }
    action:
      type: command
      command: ""              # ignored — Native carries its own payload
```

### Canonical events

Common-vocabulary events serialize as a bare PascalCase string. The catalog
deliberately mirrors Claude's event names so Claude/Cursor passthrough is
zero-translation.

| Canonical | Semantic |
| --- | --- |
| `SessionStart` | A new agent session begins. |
| `SessionEnd` | The agent session terminates. |
| `UserPromptSubmit` | The user submits a prompt to the agent. |
| `PreToolUse` | Before the agent invokes a tool. May be allowed to veto. |
| `PostToolUse` | After a tool invocation completes. |
| `Stop` | The assistant's turn ends. |
| `SubagentStop` | A subagent's turn ends. |
| `Error` | An error occurred during the interaction. |
| `PreCompact` | Before context compaction. |
| `Notification` | The agent requires user attention. |

### `Native` — the lossless-read escape hatch

Anything the source tool emits that does not match a common-vocabulary event
becomes a `Native` entry that preserves the original event name and the
verbatim source-tool payload:

```yaml
- event:
    source: claude-code
    native_event: TeammateIdle
    payload: { ... }           # the raw matcher entry, untouched
```

Writers may pass `Native` events through to the same target family (e.g. a
Cursor writer accepts `Native { source: claude-code, ... }` because Cursor
docs explicitly say Cursor reads Claude-shaped hooks). Other writers drop
them with a warning.

### Matcher

A single optional field:

```yaml
matcher:
  tool: "Bash"
```

The value matches the tool that fired the event, using the verbatim
tool-name vocabulary Claude and Cursor share. Omitting `matcher` means
"match all invocations of the event".

### Action

```yaml
action:
  type: command
  command: "scripts/audit-bash.sh"
  timeout_ms: 5000
  cwd: "./scripts"
```

- `type: command` is the only documented variant across all targets.
- `command` is a shell command string, executed by the target's native hook
  runner. Environment variables exposed to the command (e.g.
  `$CLAUDE_PROJECT_DIR`) are target-specific.
- `timeout_ms` is optional; defaults to the target's default.
- `cwd` is optional.

---

## Per-target reference

### Claude Code (v1: source only)

- **Project file:** `.claude/settings.json`
- **User file:** `~/.claude/settings.json`
- **Project-local untracked:** `.claude/settings.local.json`
- **Plugin file:** `<plugin>/hooks/hooks.json` (with optional top-level
  `description`)
- **Format:** JSON, top-level `"hooks"` field
- **Doc:** <https://code.claude.com/docs/en/hooks>
- **Role in agentenv:** the only `source` implemented in v1. agentenv reads
  the merged project + user `hooks` block via the existing
  `use_claude_config: true` flow (see [claude_config.rs](../crates/agentenv-core/src/claude_config.rs))
  and never writes back.

Native events Claude supports (28 total): `SessionStart`, `UserPromptSubmit`,
`UserPromptExpansion`, `PreToolUse`, `PermissionRequest`, `PermissionDenied`,
`PostToolUse`, `PostToolUseFailure`, `PostToolBatch`, `Notification`,
`SubagentStart`, `SubagentStop`, `TaskCreated`, `TaskCompleted`, `Stop`,
`StopFailure`, `TeammateIdle`, `InstructionsLoaded`, `ConfigChange`,
`CwdChanged`, `FileChanged`, `WorktreeCreate`, `WorktreeRemove`,
`PreCompact`, `PostCompact`, `Elicitation`, `ElicitationResult`,
`SessionEnd`.

Events outside the canonical common-vocabulary table above are preserved as
`Native` entries.

### Cursor (v1: write target)

- **Project file:** `.cursor/hooks.json`
- **User file:** `~/.cursor/hooks.json`
- **System file:** `/Library/Application Support/Cursor/hooks.json` (macOS),
  `/etc/cursor/hooks.json` (Linux), `C:\ProgramData\Cursor\hooks.json` (Win)
- **Format:** JSON; hooks run as spawned processes over stdio with JSON
- **Doc:** <https://cursor.com/docs/agent/hooks>
- **agentenv behavior:** the writer treats `.cursor/hooks.json` as a fully
  managed file. It writes a top-level `"_agentenv": "managed"` marker so a
  later sync can recognise its own output. If the file already exists
  *without* that marker and has any hook content, sync errors out and
  refuses to overwrite.

Cursor's docs state explicitly that "Cursor supports loading hooks from
third-party tools like Claude Code", so `Native` entries from a `claude-code`
source pass through unchanged.

### OpenAI Codex (v1: write target)

- **User file:** `~/.codex/config.toml` (`[notify]` entry)
- **Format:** TOML, single-command notification
- **Doc:** <https://developers.openai.com/codex/config-reference>
- **agentenv behavior:** the writer manages a sentinel-delimited block in
  `~/.codex/config.toml`, leaving every other line of the file (auth, MCP
  servers, model preferences) untouched:

  ```toml
  # >>> agentenv managed (do not edit; regenerated by `agentenv sync`) <<<
  notify = ["bash", "/abs/path/.agentenv/hooks/codex-notify-dispatch.sh"]
  # <<< agentenv managed >>>
  ```

  Only canonical `Stop` events map to Codex (everything else is dropped with
  a warning). Multiple `Stop` hooks fan out via an agentenv-emitted bash
  dispatcher at `<project>/.agentenv/hooks/codex-notify-dispatch.sh`.

  If `~/.codex/config.toml` already has a top-level `notify = ...`
  assignment outside the managed block, sync errors out.

### GitHub Copilot (out of scope for v1)

- **Repository file:** `.github/hooks/hooks.json`
- **Format:** JSON with top-level `"version": 1` and `"hooks"` map
- **Doc:** <https://docs.github.com/en/copilot/reference/hooks-configuration>

No agentenv writer is implemented in v1. Tracked for a follow-up PR.

### Gemini CLI, JetBrains Junie, Google Antigravity

No public file-system hook convention as of writing. Setting `source:` to
any of these is rejected by config validation.

---

## Canonical → native event mapping

| Canonical | claude-code (source) | cursor (write) | codex (write) | copilot (deferred) |
| --- | --- | --- | --- | --- |
| `SessionStart` | `SessionStart` | `SessionStart` | — | `sessionStart` |
| `SessionEnd` | `SessionEnd` | `SessionEnd` | — | `sessionEnd` |
| `UserPromptSubmit` | `UserPromptSubmit` | `UserPromptSubmit` | — | `userPromptSubmitted` |
| `PreToolUse` | `PreToolUse` | `PreToolUse` | — | `preToolUse` |
| `PostToolUse` | `PostToolUse` | `PostToolUse` | — | `postToolUse` |
| `Stop` | `Stop` | `Stop` | `notify` | — |
| `SubagentStop` | `SubagentStop` | — | — | — |
| `Error` | — | — | — | `errorOccurred` |
| `PreCompact` | `PreCompact` | — | — | — |
| `Notification` | `Notification` | — | — | — |
| **Destination file** | `.claude/settings.json` | `.cursor/hooks.json` | `~/.codex/config.toml` | `.github/hooks/hooks.json` |
| **Merge strategy** | _read-only_ | fully managed file (marker) | sentinel block in TOML | _not yet implemented_ |

A `—` means the target has no native event matching the canonical one and
the hook is dropped on that target with a warning at sync time.

---

## Translation losses

- **`PreCompact`, `Notification`, `SubagentStop`** — Claude-only in v1.
  Materialize for Cursor only if the Native escape hatch carries them and
  Cursor accepts Claude-shaped passthrough (it does for these). Skipped
  for Codex with a warning.
- **All non-`Stop` events on Codex** — Codex only has the `notify` turn-end
  hook. Only `Stop` materializes to Codex; everything else is skipped with
  a warning.
- **Multiple `Stop` hooks on Codex** — Codex's `notify` is a single command.
  Multiple canonical `Stop` hooks are fanned out by an agentenv-emitted
  dispatcher script. Each downstream command receives Codex's JSON payload
  as `$EVENT_JSON`.
- **Claude-native specialist events** — events not in the canonical
  common-vocabulary table (e.g. `PermissionRequest`, `UserPromptExpansion`,
  `WorktreeCreate`) are preserved as `Native` entries. They round-trip into
  Cursor (Claude-shaped passthrough) but are dropped for Codex with a
  warning.

Sync emits one warning per `(target, dropped event)` pair so authors can
see exactly which hooks were not materialized and why.

---

## Refuse-on-conflict

`agentenv` will not overwrite a hooks file that contains user-authored
content. The detection mechanism is per-target:

- **Cursor (`.cursor/hooks.json`)** — agentenv-authored files have a
  top-level `"_agentenv": "managed"` field. If the file exists without that
  field and contains a non-empty `hooks` object, sync fails with:

  ```
  .cursor/hooks.json already contains user-authored hooks. agentenv refuses
  to overwrite. Either remove these hooks or set source to a different
  target.
  ```

- **Codex (`~/.codex/config.toml`)** — agentenv content lives inside a
  sentinel block. A top-level `notify = ...` assignment outside that block
  triggers a hard error:

  ```
  ~/.codex/config.toml already declares a top-level `notify` setting
  outside the agentenv-managed block. agentenv refuses to overwrite.
  ```

In both cases the file is left untouched. To proceed, remove the conflicting
content yourself or set `source:` to a different target.

---

## v1 status

| Direction | Target | Status |
| --- | --- | --- |
| Read (source) | `claude-code` | ✅ implemented |
| Read (source) | `cursor`, `codex`, `copilot` | rejected at config validation with a clear "not yet implemented" error |
| Write | `cursor` | ✅ implemented |
| Write | `codex` | ✅ implemented |
| Write | `copilot` | deferred |
| Write | `gemini-cli`, `junie`, `antigravity` | no public hook convention; skipped |

Known v1 limitations:

- Removing `source:` from `.agentrc.yaml` does not retroactively clean up
  previously-materialized cursor/codex hook files. Run `agentenv clean` or
  delete the destination files manually if you want a fresh start.
- Codex's `~/.codex/config.toml` is user-global; if two projects both
  declare `codex` as a write target and set up notify, the second one to
  sync wins (and the first project's dispatcher path is overwritten).

Both limitations are tracked for follow-up PRs.
