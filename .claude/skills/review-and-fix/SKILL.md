---
name: review-and-fix
description: Run /review on the current branch, save findings as a tracker file, ask the user which items to fix, then fix and commit each selected item in an isolated sub-agent (one commit per item). Use when the user says "review and fix", "review then apply", "fix review issues", or any variation meaning "run review then walk me through fixing the findings one at a time."
---

# Review and fix

Drives a code review through to applied commits. The flow has four phases —
**review**, **track**, **select**, **fix-loop**. Each fix runs in a fresh
sub-agent so context never bleeds between items.

## Phase 1 — run the review

1. Capture a timestamp once and reuse it for the tracker filename. Use
   UTC, `YYYYMMDDTHHMMSSZ` format (filesystem-safe, sortable). Example:
   `20260514T103000Z`.
2. Collect minimal branch context **in parallel** (single tool message):
   - `git branch --show-current`
   - `git rev-parse --abbrev-ref --symbolic-full-name @{u} 2>/dev/null || true`
     (upstream — may be empty)
   - `git log -1 --format=%H` (HEAD sha at start of run)
3. Invoke the built-in review skill via the `Skill` tool:
   `Skill(skill="review")`. The review output appears in this
   conversation — treat it as the source of truth for findings.
4. **Refuse to proceed** if the review skill reports no findings or only
   informational/positive notes (nothing actionable). Say so and stop —
   don't fabricate items to fix.

## Phase 2 — build the tracker

Write `.agentenv/review-<timestamp>.md` using the structure below. The
file is the **single source of truth** for run state: the orchestrator
re-reads it before each fix and updates it after each fix.

```markdown
# Code Review Fix Run

- **Branch:** <branch>
- **Upstream:** <upstream or "(none)">
- **Started:** <ISO 8601 UTC>
- **HEAD at start:** <sha>
- **Review skill:** /review

## Summary
- Total: <n>
- Selected: <n>
- Fixed: <n>
- Failed: <n>
- Skipped: <n>
- Pending: <n>

## Items

### 1. <short title — 6–10 words>
- **Status:** pending
- **Severity:** high | medium | low | nit
- **File:** <path>:<line> (or "multiple")
- **Issue:** <1–3 sentences from the review>
- **Suggested fix:** <1–3 sentences — what the review proposed, or your read>
- **Commit:** —
- **Notes:** —

### 2. <…>
…
```

Rules for the tracker:

- One item per concrete, atomic fix. If the review bundles unrelated
  problems into one bullet, split them into separate tracker items.
- Skip items that are purely "consider …" / "you might want to …" with
  no clear action. Note them in a final `## Deferred` section instead.
- Use forward slashes in paths (cross-platform — same rule as the rest
  of this repo's canonical artifacts).
- `.agentenv/` is already gitignored, so the tracker stays local. Don't
  add a separate `.gitignore` entry.

After writing the tracker, **print its path to the user** so they can
open it alongside the conversation.

## Phase 3 — let the user pick items

1. Present the items as a numbered list in chat (title + severity + file
   only — full detail lives in the tracker).
2. Ask the user which to fix.
   - **If 4 or fewer items:** use `AskUserQuestion` with `multiSelect:
     true`, one option per item, plus a final question with options
     `Fix all`, `Pick subset`, `Cancel`. Use a single question with the
     items as options.
   - **If more than 4 items:** ask in plain text. Prompt:
     `"Reply with the item numbers to fix (e.g. '1, 3, 5'), 'all', or
     'cancel'."` Parse the reply.
3. Update the tracker:
   - Selected items: keep `Status: pending`.
   - Unselected items: set `Status: skipped`.
   - Recompute the `## Summary` counts.
4. If the user picks zero items or cancels, stop and report.

## Phase 4 — fix loop

For each selected item, in tracker order:

1. **Re-read the tracker file** to pick up the next pending item. (Don't
   rely on an in-memory list — the tracker is the source of truth.)
2. Mark the item `Status: in-progress` and save.
3. Spawn a sub-agent with `Agent(subagent_type="claude", …)`. The
   sub-agent gets a **self-contained** prompt — it has none of this
   conversation's context. The prompt must include:
   - The item's title, severity, file, issue, and suggested fix
     (copy verbatim from the tracker).
   - The absolute path to the tracker file.
   - Explicit instructions:
     - Read the cited file(s); apply the fix.
     - Run the relevant checks for the file type before committing
       (Rust: `cargo build` + `cargo test` + `cargo clippy --
       -D warnings` scoped to the touched crate when possible;
       TypeScript under `vscode/`: `npm run compile` + `npm run lint`;
       docs/config: no checks required).
     - Stage only the files you changed (`git add <paths>` — never
       `git add -A`/`-.`).
     - Commit with a HEREDOC message that starts with a conventional-style
       title under 70 chars and references the tracker item number in
       the body (e.g. `Addresses review item #3.`).
     - Do **not** use `--amend`, `--no-verify`, or `--no-gpg-sign`.
     - Do **not** push — pushing is the user's call after the loop.
     - On success: return the new commit SHA and a one-line summary.
     - On failure (checks fail, hook blocks commit, fix isn't possible):
       return `FAILED: <one-line reason>`. Do not retry; do not leave
       partial changes staged — `git restore --staged <paths>` and
       `git checkout -- <paths>` to clean up before reporting.
   - A hard cap: **one commit, one item**. The sub-agent must not touch
     unrelated files or chain multiple commits.
4. Parse the sub-agent's result:
   - If it returned a SHA: set `Status: fixed`, fill `Commit:` with the
     short SHA, save the tracker.
   - If it returned `FAILED: …`: set `Status: failed`, fill `Notes:`
     with the reason, save the tracker. **Continue to the next item** —
     do not stop the run.
   - If the result is ambiguous (no SHA, no FAILED marker): verify with
     `git log -1 --format=%H` — if HEAD advanced since the previous
     item's recorded SHA (or since the run's start sha), treat as
     fixed and capture the new SHA; otherwise treat as failed.
5. Recompute `## Summary` counts each time the tracker is saved.

## Phase 5 — wrap up

After the loop ends:

1. Print a concise report:
   - Count of fixed / failed / skipped.
   - For each failed item, one line: `#<n> <title> — <reason>`.
   - Tracker path so the user can audit.
2. Ask the user (`AskUserQuestion`, single-select) what to do next:
   - `Push branch` — run `git push` (with `-u origin <branch>` if no
     upstream). Stop after push; do **not** open a PR (that's
     `ship-changes`' job).
   - `Retry failed items` — re-run Phase 4 for items whose status is
     `failed`, after asking the user whether to feed each one's failure
     reason as extra context into the sub-agent prompt.
   - `Stop` — leave the branch as-is.

## Non-negotiable safety rules

- **Never** delete the tracker file mid-run. If the run ends or errors,
  the tracker stays so the user can resume manually.
- **Never** force-push, reset --hard, or rewrite history. Each fix is a
  forward-only commit.
- **Never** stage with `git add -A` or `git add .` (in the orchestrator
  *or* the sub-agent prompt).
- **Never** silently squash multiple review items into one commit. If
  two items touch the same line and conflict, mark the second `failed`
  with reason `conflicts with item #<n>` and let the user resolve.
- **Never** invoke `/review` more than once per run — if the user wants
  a fresh review, they re-invoke `/review-and-fix`.
- If the sub-agent's commit hook fails twice across the run for unrelated
  items (suggesting a repo-wide problem, not a per-item bug), stop and
  ask the user before continuing.
