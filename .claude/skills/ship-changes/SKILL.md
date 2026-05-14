---
name: ship-changes
description: Audit uncommitted changes for missing documentation updates, then commit, push, and open a pull request. Use when the user says "ship this", "commit and push", "open a PR", or any variation that means "wrap up the current working state into a published PR."
---

# Ship changes

Wraps a tested working tree into a published PR. Three phases — **audit**,
**commit + push**, **open PR**. Each phase has hard checks; if any fail, stop
and report instead of pushing forward.

## Phase 1 — audit the diff

Inspect what's about to ship and decide whether documentation needs to move
with it.

1. Run the inspection commands **in parallel** (single tool message, multiple
   Bash calls):
   - `git status` — uncommitted files
   - `git diff --stat` — scope at a glance
   - `git diff` (and `git diff --cached` if there's staged content) — actual
     changes
   - `git log -10 --oneline` — recent commit-message style for this repo
   - `git branch --show-current` — branch you're shipping from
2. **Refuse to proceed** if:
   - Working tree is clean (nothing to ship — say so and stop).
   - Files that look sensitive are staged (`.env*`, `*credentials*`, `*.pem`,
     `*.key`, anything matching `**/secrets/**`). Surface the path and ask the
     user before continuing.
3. **Documentation audit** — walk the diff for these signals and report each
   finding:

   | Signal in diff | Doc that probably needs updating |
   | --- | --- |
   | Public API change in `crates/*/src/lib.rs` or any `pub fn`/`pub struct` | doc-comments on the changed item; mentions in `docs/` |
   | New CLI subcommand / flag in `crates/agentenv-cli/src/main.rs` | `README.md` examples; `docs/` runbooks |
   | New config field, target name, or capability in `crates/agentenv-core/src/config.rs` | `schemas/agentrc.schema.json`, `.agentrc.example.yaml`, `CLAUDE.md`, `docs/platform-standards.md` |
   | New / changed writer or reader path | `docs/platform-standards.md` per-tool tables, `docs/HOOKS.md` |
   | New module under `crates/agentenv-core/src/` | `CLAUDE.md` development-areas section |
   | Test fixtures touching `.agentrc.yaml` shape | `.agentrc.example.yaml` |
   | `Cargo.toml` dep added | nothing required, but flag the new dep in the PR body |

   For each finding, **check whether the doc already reflects it** — grep the
   doc for the changed identifier / config key. If it does, note "doc already
   covers it" and move on. If it doesn't, **stop and tell the user before
   committing**. List the missing doc updates as a punch list; let the user
   decide whether to update them now, defer, or proceed without.

4. After reporting findings, **ask the user** for permission to proceed to
   Phase 2. Use `AskUserQuestion` with options:
   - `Proceed` (commit + push + PR as-is)
   - `Update docs first` (pause for the user to drive doc edits, then re-run)
   - `Cancel`

## Phase 2 — commit + push

Only run after the user approves Phase 1.

1. Decide commit scope. Default to **one commit per logical change**; if the
   diff is cohesive, one commit is fine. If it bundles unrelated work (e.g.
   refactor + dep bump + docs), ask whether to split.
2. Draft a commit message that matches `git log` style. Title under 70 chars;
   body explains the **why**. Reference issues/PRs only when the user has
   already mentioned them — don't invent links.
3. Stage with explicit paths (`git add path1 path2 …`) — **never** `git add
   -A` or `git add .` (catches sensitive files and unrelated noise).
4. Commit using a HEREDOC so newlines survive:
   ```sh
   git commit -m "$(cat <<'EOF'
   <title>

   <body — wrapped at ~72 cols>

   Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
   EOF
   )"
   ```
5. If a pre-commit hook fails: fix the root cause, re-stage, and commit
   **again** as a new commit. Do **not** use `--amend`, `--no-verify`, or
   `--no-gpg-sign` unless the user explicitly asked for it.
6. Check the upstream:
   - `git status -sb` to see ahead/behind counts.
   - If the branch has no upstream, push with `-u origin <branch>`.
   - Otherwise plain `git push`.
7. If a `git push` is rejected (non-fast-forward, hook denial), stop and
   report — never force-push without explicit user instruction, and **never**
   force-push to `main`/`master`.

## Phase 3 — open the PR

1. Check whether a PR already exists for this branch: `gh pr view --json
   url,state 2>/dev/null` (the command exits non-zero if no PR — that's fine).
   - If a PR exists and is open, push was enough. Print its URL and stop.
   - If a PR exists and is closed/merged, ask the user before opening a new
     one.
2. Otherwise, draft a PR body from the commits on this branch (not just the
   last commit):
   - `git log <base>..HEAD --oneline` and `git diff <base>...HEAD` where
     `<base>` is the default base (usually `main`).
   - Title: a single concise summary, ≤70 chars.
   - Body template:
     ```
     ## Summary
     - <1–3 bullets covering the user-visible change>

     ## Test plan
     - [ ] <tests / manual checks that verify this>

     🤖 Generated with [Claude Code](https://claude.com/claude-code)
     ```
3. Create the PR with `gh pr create` and a HEREDOC body:
   ```sh
   gh pr create --title "<title>" --body "$(cat <<'EOF'
   ## Summary
   - …

   ## Test plan
   - [ ] …

   🤖 Generated with [Claude Code](https://claude.com/claude-code)
   EOF
   )"
   ```
4. Print the PR URL the command returned. Done.

## Non-negotiable safety rules

- Never run `git push --force`, `git reset --hard`, `git checkout .`, or
  similar destructive commands unless the user explicitly requested them in
  this conversation.
- Never modify `.git/config` or global git config.
- Never bypass hooks (`--no-verify`) or signing (`--no-gpg-sign`, `-c
  commit.gpgsign=false`) unless asked.
- Never commit files matching the sensitive-path patterns in Phase 1 step 2.
- If a hook fails twice in a row on the same change, stop and ask the user —
  don't keep guessing fixes.
