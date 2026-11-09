---
name: dependabot-merge
description: Use this skill when the user asks to test, review, or merge Dependabot pull requests. It verifies GitHub CI passed, tests each PR merged into the latest main locally (fmt, pedantic clippy, nextest, docs, release build), and squash-merges the ones that pass.
---

# Dependabot Merge

Process every open Dependabot PR one at a time, oldest first. Merge a PR only if
**all** of the following pass:

1. Every GitHub CI check on the PR.
2. The full local suite, run on the PR merged into the latest `origin/main`.

Never merge a PR when a check fails, is still pending, or is missing. Never weaken a
check (e.g. `#[allow]`, editing CI, skipping a step) to get a PR through. Never
push code to Dependabot branches.

Each bash tool call starts a fresh shell, so substitute concrete values for
`<N>` (PR number), `<SHA>` (PR head commit), and `<REPO_ROOT>` (main checkout
path) in every command.

## 1. Preflight

```bash
gh auth status && cargo nextest --version && just --version && git fetch origin --prune
```

Leave the user's checkout and current branch alone. Do all testing in temporary
worktrees.

## 2. Baseline: check `main` first

The local suite must pass on `origin/main` before any PR can pass:

```bash
git worktree add --detach /tmp/blobrs-dependabot-main origin/main
```

Run the [local suite](#local-suite) there, then
`git worktree remove --force /tmp/blobrs-dependabot-main`.

If the baseline fails, **stop** and tell the user which step failed on `main`.
Every PR would fail the same way, so testing them is wasted time.

## 3. List Dependabot PRs

```bash
gh pr list --author app/dependabot --state open \
  --json number,title,headRefName,headRefOid \
  --jq 'sort_by(.number) | .[] | "\(.number)\t\(.headRefOid)\t\(.title)"'
```

## 4. For each PR

### 4a. GitHub CI

```bash
gh pr checks <N> --json name,bucket,state
```

- `bucket` is one of `pass`, `fail`, `pending`, `skipping`, `cancel`.
- If any check is `pending`, wait with `gh pr checks <N> --watch --interval 30`,
  then re-check.
- Accept only `pass` and `skipping`. An empty list means CI never ran: skip the PR.
- If any check is `fail` or `cancel`, see whether the PR is behind main:

  ```bash
  git fetch origin main "pull/<N>/head" && git merge-base --is-ancestor origin/main <SHA> && echo up-to-date || echo behind
  ```

  If it is behind, CI may have run against an old workflow or old code. Run
  `gh pr comment <N> --body "@dependabot rebase"` so it gets fresh CI, and skip
  the PR for this run. If it is up to date, skip it as a real CI failure.

### 4b. Mergeability

```bash
gh pr view <N> --json mergeable,headRefOid
```

- `CONFLICTING`: comment `@dependabot rebase` and skip.
- `UNKNOWN`: GitHub is still computing. Wait a few seconds and re-query.
- If `headRefOid` differs from `<SHA>`, the PR changed. Restart this PR from 4a.

### 4c. Local test: the PR merged into the latest main

```bash
git fetch origin main "pull/<N>/head" \
  && git worktree add --detach /tmp/blobrs-dependabot-<N> origin/main \
  && git -C /tmp/blobrs-dependabot-<N> merge --no-edit <SHA>
```

If the merge conflicts, remove the worktree, comment `@dependabot rebase`, and
skip. Otherwise run the [local suite](#local-suite) in `/tmp/blobrs-dependabot-<N>`.

### 4d. Merge

Merge only if 4a–4c all passed:

```bash
gh pr merge <N> --repo natemcintosh/blobrs --squash --delete-branch --match-head-commit <SHA>
```

- `--match-head-commit` refuses to merge if the PR changed after testing.
- `--repo` makes `gh` delete only the remote branch. It won't touch the user's
  local checkout or switch branches.

Then run `git fetch origin main` so the next PR is tested against the new main.

### 4e. Clean up

Run `git worktree remove --force /tmp/blobrs-dependabot-<N>` whether the PR
passed or failed.

## Local suite

Run this as one command (allow 10+ minutes). It stops at the first failure.
Sharing `CARGO_TARGET_DIR` across worktrees keeps builds incremental:

```bash
cd <WORKTREE> && export CARGO_TARGET_DIR=<REPO_ROOT>/target/dependabot \
  && cargo fmt -- --check \
  && just pedantic \
  && cargo nextest run --locked --all-features --all-targets \
  && cargo doc --no-deps --all-features \
  && cargo build --release --locked
```

`just pedantic` is strict: any pedantic clippy warning fails.

GitHub Actions bumps (`dependabot/github_actions/...`) still get the full local
suite, because it confirms that PR plus the latest main still builds and tests
clean. The workflow change itself is covered by the PR's own CI.

Major version bumps are merged automatically if everything passes.

## Final report

End with a table of every PR processed:

| PR | Title | GH CI | Local | Result |
| --- | --- | --- | --- | --- |

`Result` is `merged`, `rebase requested`, or `skipped`. For each non-merged PR,
give the failing check or local step and a short error excerpt. Finally, run
`git worktree prune` and confirm that no `/tmp/blobrs-dependabot-*` worktrees
remain.
