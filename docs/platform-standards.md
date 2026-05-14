# Platform Standards for AI Coding Tools

This document captures the canonical install paths, file formats, and discovery
rules for each AI coding tool that `agentenv` may target. It is the source of
truth for the per-target writer modules under
`crates/agentenv-core/src/<capability>/writers/`.

Every entry is sourced from the tool's **official** documentation, retrieved
2026-04-28. URLs are included so claims can be re-verified when the docs
change.

> **The big finding.** Most major tools converge on the open
> [Agent Skills](https://agentskills.io) standard for *skills*, but each tool
> still has its own conventions for *subagents*, *slash commands*, and *hooks*.
> Plan for shared skill paths but per-target paths for everything else.

## Quick orientation

| Capability      | Cross-tool standard?  | Default unit of install              |
| --------------- | --------------------- | ------------------------------------ |
| Skills          | Yes — `agentskills.io` (`SKILL.md`) | `<scope>/skills/<skill-name>/SKILL.md` |
| Subagents       | No                    | `<scope>/agents/<name>.md` for most; Codex uses TOML |
| Slash commands  | No                    | Mostly Markdown; Gemini CLI uses TOML; Antigravity uses workflow files |
| Hooks           | No                    | JSON config; key shape varies        |
| MCP servers     | Yes — MCP protocol    | JSON config (project / user)         |

The `.agents/` (plural) project directory is recognized natively by Codex,
Cursor, Gemini CLI, and Copilot for skills. Antigravity's `.agent/` (singular)
is its own thing — don't conflate them.

---

## 1. Skills

### 1.1 The Agent Skills standard (`agentskills.io`)

**Source:** https://agentskills.io/specification

```
skill-name/
├── SKILL.md          # Required: metadata + instructions
├── scripts/          # Optional: executable code
├── references/       # Optional: documentation
├── assets/           # Optional: templates, resources
└── ...               # Any additional files
```

`SKILL.md` frontmatter:

| Field           | Required | Constraints |
| --------------- | -------- | ----------- |
| `name`          | Yes      | 1–64 chars; lowercase `a-z`, digits, `-`. No leading/trailing/consecutive hyphens. **Must match the parent directory name.** |
| `description`   | Yes      | 1–1024 chars. |
| `license`       | No       | License name or bundled license filename. |
| `compatibility` | No       | ≤500 chars. Environment requirements. |
| `metadata`      | No       | Free-form key/value map. |
| `allowed-tools` | No       | Space-separated list of pre-approved tools. (Experimental.) |

Progressive disclosure: name + description (~100 tokens) at startup → full
`SKILL.md` body on activation → `scripts/`/`references/`/`assets/` on demand.
Recommended `SKILL.md` ≤500 lines.

### 1.2 Per-platform skill paths

| Tool             | Primary project       | Primary user                       | Aliases                                                                                      | Source |
| ---------------- | --------------------- | ---------------------------------- | -------------------------------------------------------------------------------------------- | ------ |
| Claude Code      | `.claude/skills/`     | `~/.claude/skills/`                | also discovered from nested `.claude/skills/` in subdirs (monorepo)                          | [docs](https://code.claude.com/docs/en/skills) |
| OpenAI Codex     | `.agents/skills/`     | `~/.agents/skills/`                | walks every dir from `$CWD` up to repo root; admin: `/etc/codex/skills`                      | [docs](https://developers.openai.com/codex/skills) |
| Cursor           | `.cursor/skills/`, `.agents/skills/` | `~/.cursor/skills/`, `~/.agents/skills/` | also: `.claude/skills/`, `.codex/skills/`, `~/.claude/skills/`, `~/.codex/skills/`     | [docs](https://cursor.com/docs/context/skills) |
| GitHub Copilot   | `.github/skills/`, `.claude/skills/`, `.agents/skills/` | `~/.copilot/skills/`, `~/.agents/skills/` | — | [docs](https://docs.github.com/en/copilot/concepts/agents/about-agent-skills) |
| VS Code Copilot  | `.github/skills/`, `.claude/skills/`, `.agents/skills/` | `~/.copilot/skills/`, `~/.claude/skills/`, `~/.agents/skills/` | — | [docs](https://code.visualstudio.com/docs/copilot/customization/agent-skills) |
| Gemini CLI       | `.gemini/skills/`, `.agents/skills/` | `~/.gemini/skills/`, `~/.agents/skills/` | precedence: `.agents/` > `.gemini/`, workspace > user > extension | [docs](https://geminicli.com/docs/cli/skills/) |
| JetBrains Junie  | `.junie/skills/`      | `~/.junie/skills/`                 | — | [docs](https://junie.jetbrains.com/docs/agent-skills.html) |
| Google Antigravity | `.agent/skills/` (singular) | `~/.gemini/antigravity/skills/` | workflows live in `.agents/workflows/` (plural — different concept) | codelabs ¹ |

¹ Antigravity's official docs site is JS-rendered; paths above came from the
[Authoring Google Antigravity Skills](https://codelabs.developers.google.com/getting-started-with-antigravity-skills)
codelab. Verify in-browser before shipping.

---

## 2. Subagents

A "subagent" is a specialized agent with its own system prompt, tool access,
and (often) model. Most tools store them as Markdown with YAML frontmatter, but
Codex switched to TOML.

| Tool             | Project                 | User                            | Format                              | Notes |
| ---------------- | ----------------------- | ------------------------------- | ----------------------------------- | ----- |
| Claude Code      | `.claude/agents/<name>.md` | `~/.claude/agents/<name>.md`  | Markdown + YAML frontmatter         | Plugin: `<plugin>/agents/`. Plugin agents can't use `hooks`, `mcpServers`, `permissionMode` for security. ([docs](https://code.claude.com/docs/en/sub-agents)) |
| OpenAI Codex     | `.codex/agents/<name>.toml` | `~/.codex/agents/<name>.toml` | **TOML** (was Markdown frontmatter, switched) | Inherits parent session for omitted fields like `model`, `mcp_servers`. ([docs](https://developers.openai.com/codex/subagents)) |
| Cursor           | `.cursor/agents/<name>.md` | `~/.cursor/agents/<name>.md`  | Markdown + YAML frontmatter         | Filename = identity. Frontmatter: `name`, `description`, `model`, `readonly`, `is_background`. Built-ins: `Explore`, `Bash`, `Browser`. ([docs](https://cursor.com/docs/subagents)) |
| Gemini CLI       | `.gemini/agents/<name>.md` | `~/.gemini/agents/<name>.md`  | Markdown + YAML frontmatter         | Extensions can bundle `agents/` dir too. Body becomes the system prompt. ([docs](https://geminicli.com/docs/core/subagents/)) |
| GitHub Copilot   | `.github/agents/<name>.agent.md` | (no user-level on disk)   | Markdown + YAML frontmatter         | Note the `.agent.md` suffix. Org-level: `{org}/.github/agents/`. ([docs](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/create-custom-agents)) |
| JetBrains Junie  | `.junie/agents/<name>.md`, `.agents/<name>.md` | `~/.junie/agents/<name>.md`, `~/.agents/<name>.md` | Markdown + YAML frontmatter | Cannot be invoked manually — only auto-delegated. ([docs](https://junie.jetbrains.com/docs/junie-cli-subagents.html)) |
| Antigravity      | `agents.md` (single file at root) | n/a                       | Markdown                            | A single Markdown file defining multiple personas, not per-agent files. ([codelab](https://codelabs.developers.google.com/autonomous-ai-developer-pipelines-antigravity)) |

**Cross-cutting frontmatter pattern** (Markdown-based tools):

```yaml
---
name: code-reviewer
description: Reviews PRs for security issues
tools: [Read, Grep]
model: claude-haiku-4-5
---
System prompt body in Markdown...
```

Each tool extends this with its own fields (`disallowedTools`, `permissionMode`,
`maxTurns`, `mcpServers`, `is_background`, etc.). For portable subagents, stick
to `name` + `description` + body.

---

## 3. Slash commands / custom prompts

Less standardized than skills. Several tools are deprecating "commands" in
favor of agent skills (Claude Code most aggressively).

| Tool             | Project                              | User                                | Format          | Notes |
| ---------------- | ------------------------------------ | ----------------------------------- | --------------- | ----- |
| Claude Code      | `.claude/commands/<name>.md`         | `~/.claude/commands/<name>.md`      | Markdown        | **Legacy** — superseded by skills. `.claude/commands/deploy.md` and `.claude/skills/deploy/SKILL.md` both create `/deploy`. Existing files keep working. ([docs](https://code.claude.com/docs/en/skills)) |
| OpenAI Codex     | (none)                               | `~/.codex/prompts/<name>.md`        | Markdown + YAML frontmatter | Only top-level `.md` files; subdirectories ignored. Filename → `/<name>` command. **Deprecated** in favor of skills. ([docs](https://developers.openai.com/codex/custom-prompts)) |
| Cursor           | (no documented project commands)     | `~/.cursor/commands/<name>.md`      | Markdown        | Lightly documented; community-maintained gist references this path. |
| Gemini CLI       | `.gemini/commands/<name>.toml`       | `~/.gemini/commands/<name>.toml`    | **TOML**        | Subdirs become namespaced: `<project>/.gemini/commands/git/commit.toml` → `/git:commit`. Required field `prompt`; optional `description`. Reload via `/commands reload`. ([docs](https://geminicli.com/docs/cli/custom-commands/)) |
| GitHub Copilot   | `.github/prompts/<name>.prompt.md`   | (no user-level on disk)             | Markdown + YAML frontmatter | Frontmatter `mode` (`ask` \| `edit` \| `agent`), `tools`, `description`, `model`. ([docs](https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot)) |
| JetBrains Junie  | (no separate concept)                | n/a                                 | n/a             | Customization happens through subagents and `AGENTS.md`. |
| Antigravity      | `.agents/workflows/<name>` (text file) | n/a                               | Plain text      | "By saving a text file inside `.agents/workflows/`, you are registering a brand new command directly into Antigravity's chat interface." Note this is the **plural** `.agents/`, not the `.agent/` used for skills. ([codelab](https://codelabs.developers.google.com/autonomous-ai-developer-pipelines-antigravity)) |

> **Migration trend:** Both Claude Code and Codex now treat custom commands as
> a legacy form of skills. New work should use the skills standard; commands
> should be supported only for back-compat.

---

## 4. Hooks

Hooks are event handlers that fire on lifecycle events (session start, tool
use, etc.). All tools that support hooks use JSON config; the event names
differ.

### 4.1 Claude Code hooks

**Source:** https://code.claude.com/docs/en/hooks

| Location                        | Scope               | Shareable |
| ------------------------------- | ------------------- | --------- |
| `~/.claude/settings.json`       | All projects        | No |
| `.claude/settings.json`         | Single project      | Yes (commit) |
| `.claude/settings.local.json`   | Single project      | No (gitignored) |
| Managed policy settings         | Organization-wide   | Admin |
| Plugin `hooks/hooks.json`       | When plugin enabled | Yes |
| Skill / agent frontmatter `hooks` field | Component lifetime | Yes |

JSON shape:

```json
{
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/block-rm.sh" }] }
    ]
  }
}
```

Plugins use a top-level `description` field too. **28 events**: `SessionStart`,
`UserPromptSubmit`, `UserPromptExpansion`, `PreToolUse`, `PermissionRequest`,
`PermissionDenied`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch`,
`Notification`, `SubagentStart`, `SubagentStop`, `TaskCreated`,
`TaskCompleted`, `Stop`, `StopFailure`, `TeammateIdle`, `InstructionsLoaded`,
`ConfigChange`, `CwdChanged`, `FileChanged`, `WorktreeCreate`,
`WorktreeRemove`, `PreCompact`, `PostCompact`, `Elicitation`,
`ElicitationResult`, `SessionEnd`.

### 4.2 Cursor hooks

**Source:** https://cursor.com/docs/agent/hooks

| Path                            | Scope              |
| ------------------------------- | ------------------ |
| `.cursor/hooks.json`            | Project            |
| `~/.cursor/hooks.json`          | User               |
| `/Library/Application Support/Cursor/hooks.json` (macOS), `/etc/cursor/hooks.json` (Linux), `C:\ProgramData\Cursor\hooks.json` (Win) | Enterprise system-wide |
| Cloud (Enterprise dashboard)    | Team               |

Hooks run as spawned processes communicating over stdio with JSON. The docs
explicitly say "Cursor supports loading hooks from third-party tools like
Claude Code" — so Cursor will read Claude-shaped hooks too.

### 4.3 GitHub Copilot hooks

**Source:** https://docs.github.com/en/copilot/reference/hooks-configuration

| Path                                | Scope              |
| ----------------------------------- | ------------------ |
| `.github/hooks/hooks.json`          | Repository         |
| Copilot CLI `cwd`/hooks.json        | Per-invocation     |

JSON: top-level `version: 1` + `hooks` map. Events: `sessionStart`,
`sessionEnd`, `userPromptSubmitted`, `preToolUse`, `postToolUse`,
`errorOccurred`. `preToolUse` can return approve/deny.

### 4.4 OpenAI Codex hooks

Codex's "notify" hook is configured in `~/.codex/config.toml` rather than as
file-system hook scripts. It fires when a turn finishes; payload includes a
`client` field for the originating UI. ([docs](https://developers.openai.com/codex/config-reference))

### 4.5 Other tools

Gemini CLI, Junie, and Antigravity don't currently have public file-system
hook conventions. If hooks ship there, expect a similar JSON config pattern.

---

## 5. MCP servers

The Model Context Protocol is the cross-tool plugin equivalent for tools.
Configurations are JSON or TOML, never Markdown.

| Tool         | Project                         | User                          | Format |
| ------------ | ------------------------------- | ----------------------------- | ------ |
| Cursor       | `.cursor/mcp.json`              | `~/.cursor/mcp.json`          | JSON   |
| Claude Code  | plugin's `.mcp.json` (when bundled in a plugin) | `~/.claude/settings.json` (`mcpServers` field) | JSON   |
| OpenAI Codex | (n/a)                           | `~/.codex/config.toml` (`[mcp_servers]`) | TOML   |
| Gemini CLI   | configured via extensions       | settings UI / config          | varies |

The Cursor and Claude Code JSON shapes are largely interoperable (same MCP
protocol). The Codex TOML shape is bespoke but exposes the same protocol.

---

## 6. Adjacent conventions

### 6.1 `AGENTS.md`

A repo-root Markdown file that serves as a tool-agnostic instruction sheet.

| Tool              | Path                                                         | Notes |
| ----------------- | ------------------------------------------------------------ | ----- |
| OpenAI Codex      | repo root + every dir along path to `$CWD`; user `~/.codex/AGENTS.md` (and `AGENTS.override.md`) | Concatenated root-down; deeper files override. ([docs](https://developers.openai.com/codex/guides/agents-md)) |
| GitHub Copilot    | "one or more `AGENTS.md` files, stored anywhere within the repository" — nearest wins | Also accepts `CLAUDE.md` or `GEMINI.md` at repo root. ([docs](https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot)) |
| Cursor            | repo root + nested subdirs; "more specific instructions taking precedence" | ([docs](https://cursor.com/docs/context/rules)) |
| Junie             | `.junie/AGENTS.md` at project root | ([docs](https://junie.jetbrains.com/docs/guidelines-and-memory.html)) |
| Antigravity       | `agents.md` at project root | Defines specialized AI personas. |

### 6.2 Cursor rules and plans

Separate from skills:

* `.cursor/rules/*.md` or `.cursor/rules/*.mdc` — `.mdc` form supports YAML
  frontmatter (`description`, `globs`, `alwaysApply`).
* `.cursor/plans/` — agent plans saved as files on disk by default; editable
  with normal tools. ([changelog](https://cursor.com/changelog))
* User rules live in Cursor Settings UI (no on-disk path).

### 6.3 GitHub Copilot legacy customization

Still supported alongside agent skills:

* `.github/copilot-instructions.md` — single repo-wide instructions file.
* `.github/instructions/<name>.instructions.md` — scoped via YAML `applyTo:` glob.
* `.github/prompts/<name>.prompt.md` — reusable prompt files (covered in
  §3 above).

### 6.4 Junie guidelines + allowlist

* `.junie/AGENTS.md` — project context (tech stack, conventions, rules).
* `~/.junie/allowlist.json` — allowed shell commands and command patterns.

---

## 7. The Claude Code plugin model

Claude Code is the only tool that bundles `skills/`, `commands/`, `agents/`,
and hooks together as a versioned **plugin**.

**Manifest:** `<plugin>/.claude-plugin/plugin.json`

```json
{
  "name": "my-plugin",
  "description": "...",
  "version": "1.0.0",
  "author": { "name": "..." }
}
```

**Layout:**

| Directory          | Purpose                                                                |
| ------------------ | ---------------------------------------------------------------------- |
| `.claude-plugin/`  | Contains `plugin.json` only. Nothing else.                             |
| `skills/`          | Skills as `<name>/SKILL.md` directories.                               |
| `commands/`        | Legacy: skills as flat Markdown files. Use `skills/` for new plugins.  |
| `agents/`          | Subagent definitions.                                                  |
| `hooks/`           | `hooks.json` (with optional top-level `description`).                  |
| `.mcp.json`        | MCP server configs.                                                    |
| `.lsp.json`        | LSP server configs.                                                    |
| `monitors/`        | `monitors.json` for background monitors.                               |
| `bin/`             | Executables added to `PATH` while the plugin is enabled.               |
| `settings.json`    | Default settings applied when the plugin is enabled.                   |

**Skill namespacing inside a plugin:** `/<plugin-name>:<skill-name>` (e.g.
`/my-plugin:hello`). Plugin skills cannot collide with user/project/enterprise
skills.

**Source:** https://code.claude.com/docs/en/plugins

---

## 8. Where agentenv writes — by capability

agentenv is **source-driven**: declare one tool's native layout as the
source of truth (today: Claude Code), and `agentenv sync` translates each
capability into every configured target. Path conventions live inside the
per-capability writer modules (`crates/agentenv-core/src/<capability>/writers/`);
the per-tool tables above (§1–4) are the underlying reference.

### Hooks
Source-driven. See [HOOKS.md](HOOKS.md) for the canonical model. v1
sources: `claude-code`. v1 writers: `cursor`, `codex`.

### Skills
Source-driven. Skills follow the cross-tool agentskills.io shape — every
writer symlinks the source `<name>/SKILL.md` directory into the target's
native location (no content transformation). v1 sources: `claude-code`.
v1 writers: `cursor`, `codex` (via `.agents/`), `copilot`, `gemini-cli`,
`junie`, `antigravity` (singular `.agent/`).

### Agents
Source-driven. Markdown-with-frontmatter on most targets; **Codex
requires TOML** so its writer materializes (instead of symlinking) and
drops Claude-only frontmatter keys (`permissionMode`, `hooks`,
`mcpServers`) with warnings. **Copilot** writes `<name>.agent.md`
(suffix differs from `.md`). **Antigravity** uses a single root
`agents.md` file, so the writer skips every per-agent definition with a
warning. v1 sources: `claude-code`. v1 writers: `cursor`, `codex`,
`copilot`, `gemini-cli`, `junie`, `antigravity` (skip).

### Slash commands
Out of scope for the source-driven pipeline. Both Claude Code and Codex
are deprecating commands in favor of skills, and the per-target
translation cost is the worst of any capability (TOML for Gemini, plain
text for Antigravity, `.prompt.md` for Copilot). Plugin authors who need
commands can still ship them as raw files — agentenv will not propagate.

### MCP
Same story as hooks: protocol is shared but config shape varies. Cursor
and Claude Code MCP JSON is interoperable. Codex needs TOML translation.
Deferred to a follow-up source-driven implementation.

### Anti-recommendations

* **Drop `jetbrains` from defaults.** JetBrains AI Assistant has no
  file-system skill convention. Use `junie` for the agentic JetBrains
  product.
* **Don't auto-translate Markdown subagents to Codex TOML without
  refuse-on-conflict.** The Codex writer manages the destination file
  exclusively (sentinel header). Manual edits to `.codex/agents/*.toml`
  are not preserved on resync.

---

## 9. Open questions

1. **Antigravity verification.** Skills paths come from third-party codelabs
   because the official docs site renders client-side and is opaque to plain
   HTTP fetches. Verify in-browser before shipping.
2. **Codex subagent invocation.** GitHub issue
   [openai/codex#15250](https://github.com/openai/codex/issues/15250) reports
   that custom agents in `.codex/agents/` aren't always reachable from
   tool-backed sessions. Watch for resolution before promising a Codex
   subagent target.
3. **`.agents/` (plural) cross-tool alias.** Codex, Cursor, Copilot, Gemini
   CLI all accept it for skills. We default to tool-native paths because they
   are most discoverable when users edit config by hand. A future "portable
   mode" target could emit only `.agents/skills/`.
4. **Hook portability.** Resolved in [HOOKS.md](HOOKS.md) — `agentenv` owns a
   canonical PascalCase event vocabulary plus per-target translators. The spec
   is in place; implementation is pending on branch `feat/hooks-integration`.
5. **`AGENTS.md` syncing.** Closest thing to a universal cross-tool surface.
   Out of scope for the skill installer model; may warrant a separate
   `agentenv apply-agents-md` flow later.
