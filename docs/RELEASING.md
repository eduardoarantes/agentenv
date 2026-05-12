# Releasing agentenv

End-to-end runbook for shipping the CLI and the VS Code extension. Most
steps are automated by GitHub Actions; the few manual steps are called out
explicitly.

## Channels at a glance

| Component | Channel | Automation |
| --- | --- | --- |
| CLI | [crates.io](https://crates.io/crates/agentenv) (`agentenv`, `agentenv-core`) | ✅ [`publish-crates.yml`](../.github/workflows/publish-crates.yml) |
| CLI | [npm](https://www.npmjs.com/package/@eduardoarantes/agentenv) (`@eduardoarantes/agentenv`) | ✅ [`release.yml`](../.github/workflows/release.yml) (cargo-dist) |
| CLI | [Homebrew tap](https://github.com/eduardoarantes/homebrew-agentenv) (`eduardoarantes/homebrew-agentenv`) | ✅ [`release.yml`](../.github/workflows/release.yml) (cargo-dist) |
| CLI | GitHub Release with prebuilt binaries (macOS arm64/x86, Linux arm64/x86, Windows x86) | ✅ [`release.yml`](../.github/workflows/release.yml) (cargo-dist) |
| VS Code extension | [Open VSX](https://open-vsx.org/extension/eduardoarantes/agentenv) | ✅ [`publish-vscode.yml`](../.github/workflows/publish-vscode.yml) |
| VS Code extension | GitHub Release with `.vsix` | ✅ [`publish-vscode.yml`](../.github/workflows/publish-vscode.yml) |
| VS Code extension | [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=eduardoarantes.agentenv) (`eduardoarantes.agentenv`) | ❌ **manual** — see below |

## Tag patterns

| Tag | Workflows triggered |
| --- | --- |
| `vX.Y.Z` (e.g. `v0.3.1`) | `release.yml` + `publish-crates.yml` |
| `vscode-vX.Y.Z` (e.g. `vscode-v0.3.1`) | `publish-vscode.yml` only |

The two patterns are deliberately disjoint: a `vscode-v*` tag does **not**
trigger the CLI pipelines (fixed by [#8](https://github.com/eduardoarantes/agentenv/pull/8)).
If you ever re-run `dist generate`, the narrowed pattern in `release.yml`
will revert — re-apply it and keep `allow-dirty = ["ci"]` under
`[workspace.metadata.dist]` in `Cargo.toml`.

## Required repository secrets

These are already configured on the repo. New maintainers / forks need
to set them up before the first release.

| Secret | Used by | Where to get it |
| --- | --- | --- |
| `CRATES_IO_TOKEN` | `publish-crates.yml` | https://crates.io/settings/tokens |
| `HOMEBREW_TAP_TOKEN` | `release.yml` (`publish-homebrew-formula`) | A fine-grained PAT with **contents: write** on `eduardoarantes/homebrew-agentenv` |
| `NPM_TOKEN` | `release.yml` (`publish-npm`) | npm automation token (https://www.npmjs.com/settings/eduardoarantes/tokens) |
| `OVSX_PAT` | `publish-vscode.yml` | https://open-vsx.org/user-settings/tokens |
| `VSCE_PAT` (optional) | not used by CI today | Azure DevOps PAT scoped to "Marketplace > Manage". Needed only if you automate the VS Code Marketplace publish locally. |
| `GITHUB_TOKEN` | every workflow | Auto-provided by Actions; no setup needed. |

---

## Releasing the CLI

CLI releases ship to **crates.io, npm, Homebrew, and GitHub Releases**
simultaneously via the `vX.Y.Z` tag.

### 1. Pre-flight

Run these from the repo root:

```bash
cargo test --all
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

All three must pass.

### 2. Pick a version

| Bump kind | When |
| --- | --- |
| **Patch** (`0.3.X`) | Bug fix, doc tweak, opt-in flag with no breaking semantics |
| **Minor** (`0.X.0`) | New user-facing feature, e.g. a new top-level config field |
| **Major** (`X.0.0`) | Breaking change to `.agentrc.yaml` schema, CLI flags, or library API. Avoid pre-1.0; bump minor instead. |

### 3. Bump versions

Edit two files:

- [`Cargo.toml`](../Cargo.toml): `[workspace.package].version`
- [`crates/agentenv-cli/Cargo.toml`](../crates/agentenv-cli/Cargo.toml): the `agentenv-core` dependency's `version` field

Then refresh the lockfile:

```bash
cargo build --release -p agentenv
```

### 4. Commit and tag

```bash
git add Cargo.toml Cargo.lock crates/agentenv-cli/Cargo.toml
git commit -m "chore: release X.Y.Z

<one-paragraph summary referencing the merged PR(s)>"
git push origin main
git tag vX.Y.Z
git push origin vX.Y.Z
```

The release commit goes straight to `main` — this matches the prior
release history. No PR is required for the version bump itself; the
substantive changes are PR-merged separately before bumping.

### 5. Verify (auto-triggered workflows)

The tag push fires two workflows in parallel — both take ~4–5 minutes:

```bash
gh run list --limit 4
# Expect:
#   in_progress  chore: release X.Y.Z  Publish to crates.io  vX.Y.Z
#   in_progress  chore: release X.Y.Z  Release               vX.Y.Z
```

Per-job inspection (handy when something fails):

```bash
gh run view <release-run-id> --json jobs \
  --jq '.jobs[] | "\(.name): \(.conclusion // .status)"'
```

The `Release` workflow has 11 jobs. Expected sequence:

1. `plan` ✅
2. `build-local-artifacts` × 5 (macOS arm64/x86, Linux arm64/x86, Windows x86) ✅
3. `build-global-artifacts` ✅
4. `host` ✅ — **creates the GitHub Release** (the moment users see it)
5. `publish-homebrew-formula` ✅ — pushes Formula to the tap
6. `publish-npm` ✅ — `npm publish`
7. `announce` ✅

The `Publish to crates.io` workflow runs a single `publish` job that
publishes `agentenv-core`, sleeps in a loop until the crates.io index
updates, then publishes `agentenv`. ~3–5 minutes.

Sanity-check at the end:

```bash
gh release list --limit 3                   # vX.Y.Z marked Latest
brew info eduardoarantes/agentenv/agentenv  # version bumped (after `brew update`)
npm view @eduardoarantes/agentenv version   # vX.Y.Z
cargo search agentenv-core --limit 1        # vX.Y.Z
```

### 6. If something fails

| Symptom | Cause | Fix |
| --- | --- | --- |
| `cargo publish` errors with "crate already exists at version X.Y.Z" | Re-running publish on the same tag, or the workspace version didn't get bumped | Bump and retag |
| `plan: failure` on a PR's `Release` workflow with "out of date contents" | Someone re-ran `dist generate` and the narrowed tag pattern reverted, OR `allow-dirty` was dropped from Cargo.toml | Restore the narrow pattern in `release.yml` and keep `allow-dirty = ["ci"]` in `[workspace.metadata.dist]` |
| Homebrew tap push fails | `HOMEBREW_TAP_TOKEN` expired or lost write access | Regenerate the PAT, update the repo secret |
| Tag pushed but no workflows fired | Tag doesn't match the pattern (`vX.Y.Z` only) — `v0.3` or `0.3.1` won't match | Delete the bad tag, re-tag with the canonical `vX.Y.Z` form |

---

## Releasing the VS Code extension

Extension releases ship to **Open VSX** automatically. **VS Code
Marketplace publish is manual** — there is no `vsce publish` step in
[`publish-vscode.yml`](../.github/workflows/publish-vscode.yml).

### 1. Pre-flight

```bash
cd vscode
npm install
npm run compile
npm run lint
cd ..
```

### 2. Bump versions

Edit:

- [`vscode/package.json`](../vscode/package.json): `version`

Refresh the lockfile and the in-bundle version:

```bash
cd vscode && npm install && cd ..
```

That updates `vscode/package-lock.json` to match.

### 3. Add a CHANGELOG entry

Top of [`vscode/CHANGELOG.md`](../vscode/CHANGELOG.md), in reverse-chronological
order:

```markdown
## X.Y.Z

- <bullet describing the user-facing change>
- <bullet referencing the merged PR(s)>
```

Internal-only releases (e.g. workflow-plumbing fixes that re-ship the
same code) should still get a line saying so, so the marketplace listing
doesn't show "no notes for this version".

### 4. Commit and tag

```bash
git add vscode/package.json vscode/package-lock.json vscode/CHANGELOG.md
git commit -m "chore(vscode): release X.Y.Z

<one-line summary or PR reference>"
git push origin main
git tag vscode-vX.Y.Z
git push origin vscode-vX.Y.Z
```

Note the **`vscode-`** prefix on the tag — required by
[`publish-vscode.yml`](../.github/workflows/publish-vscode.yml).

### 5. Verify (auto-triggered workflow)

The tag push fires exactly one workflow:

```bash
gh run list --limit 3
# Expect ONLY:
#   in_progress  chore(vscode): release X.Y.Z  Publish VS Code extension  vscode-vX.Y.Z
```

The workflow does:

1. **Verify tag matches `package.json`** — fails fast if you forgot to
   bump or mistagged.
2. `npm ci` + `npm run compile` + `npx vsce package` — produces
   `agentenv-X.Y.Z.vsix`.
3. **Publishes to Open VSX** via `npx ovsx publish ...`.
4. **Creates the GitHub Release** with the `.vsix` attached.

The whole workflow runs in ~30–60 seconds.

Sanity-check:

```bash
gh release list --limit 3
# Expect: "VS Code extension vX.Y.Z" marked Latest
```

### 6. Manual: publish to the VS Code Marketplace

This is the only **non-automated** step. You have two options.

#### Option A — Web UI (no local secrets)

1. Wait for the GitHub Release `vscode-vX.Y.Z` to appear (Step 5).
2. Download the `agentenv-X.Y.Z.vsix` asset attached to the release.
3. Go to https://marketplace.visualstudio.com/manage/publishers/eduardoarantes
4. Click **Update** on the `agentenv` row and upload the `.vsix`.

Marketplace usually verifies and lists within 1–5 minutes.

#### Option B — Local `vsce publish`

Requires a Personal Access Token from Azure DevOps with **Marketplace >
Manage** scope, stored in the `VSCE_PAT` environment variable.

```bash
cd vscode
# Either use the .vsix the workflow built (download from the GitHub Release)
npx vsce publish --packagePath ./agentenv-X.Y.Z.vsix -p "$VSCE_PAT"

# Or re-package and publish in one shot
npx vsce publish -p "$VSCE_PAT"
```

To automate this in CI, add the corresponding step to
`publish-vscode.yml` and store `VSCE_PAT` as a repo secret. The repo has
intentionally avoided this so a single Azure DevOps PAT doesn't end up
living in CI — but it's a one-step add when you want it.

### 7. Verify Marketplace listing

```bash
# Open in browser
open "https://marketplace.visualstudio.com/items?itemName=eduardoarantes.agentenv"
```

The version should match within a few minutes of the upload completing.

---

## Cross-component coordination

The CLI and extension version independently. Most extension releases work
fine against any 0.x CLI — the extension just shells out to whatever
`agentenv` binary is on PATH (or `agentenv.path`).

When in doubt, **release the CLI first**, then the extension. That way
the extension's behavior matches the CLI users will install fresh after
seeing the marketplace update.

## When NOT to release

Skip a release if:

- The only changes are CI/workflow files that don't affect runtime
  behavior (unless you want to retire phantom failures — that was the
  case for `vscode-v0.3.2`).
- Tests are flaky on `main`. Re-run them; if still red, fix first.
- A previous release within the last few hours is still propagating to
  Homebrew bottle hosts or the crates.io index — wait it out.

## Rollback

There is no first-class rollback. The realistic options are:

- **crates.io**: `cargo yank --version X.Y.Z` (versions are immutable; this
  just hides them from new installs). Then publish `X.Y.Z+1` with the fix.
- **npm**: `npm unpublish @eduardoarantes/agentenv@X.Y.Z` works within
  72 hours of publish; after that, only deprecate and roll forward.
- **Homebrew tap**: revert the Formula commit in
  `eduardoarantes/homebrew-agentenv`.
- **Open VSX**: there's no unpublish endpoint; roll forward with a
  patch release.
- **VS Code Marketplace**: same — roll forward.
- **GitHub Release**: editable, but the binaries are already mirrored
  via Homebrew/npm.

Practically: cut a `+1` patch release with the fix and let it overtake
the bad version everywhere.
