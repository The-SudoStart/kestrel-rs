# CONTRIBUTING.md

This applies equally to human contributors and AI coding agents.

## Before you start

1. Read `AGENTS.md` (project context/scope) and `ROADMAP.md` (what phase
   we're in, what's allowed right now).
2. Find or open an issue. Every PR must reference one — don't start
   unscoped work.
3. Confirm the issue isn't in a future ROADMAP phase or on the "out of
   scope" list in AGENTS.md Section 3. If it is, comment on the issue
   instead of starting work.

## Branching

- Branch off `main`.
- Name: `type/short-description` — e.g. `feat/tab-management`,
  `fix/crash-on-close`, `docs/update-roadmap`.

## Commits

Conventional Commits format:

```
feat: add keyboard shortcut for new tab
fix: prevent crash when closing last tab
docs: update roadmap phase 1 checklist
refactor: extract webview lifecycle into shell module
```

## Before opening a PR

- [ ] `cargo fmt` — run it, don't just check it
- [ ] `cargo clippy -- -D warnings` — zero warnings
- [ ] `cargo test` — all passing
- [ ] `cargo build` succeeds clean
- [ ] No new dependencies outside those listed in AGENTS.md Section 4
      without flagging it in the PR description
- [ ] Change is within current ROADMAP phase scope

## Opening the PR

Use `.github/PULL_REQUEST_TEMPLATE.md`. At minimum, the PR description
must state:
- Which issue it closes
- What was changed and why
- Any scope/architecture decisions made along the way that weren't
  explicitly specified in the issue (so a human reviewer can catch scope
  creep early)

## Review

- One approval required before merge.
- If a PR touches `ARCHITECTURE.md`-locked decisions (GUI framework,
  fork-vs-dependency, module boundaries), it needs explicit human
  sign-off, not just an agent review.

## For AI agents specifically

- Don't self-merge.
- If you hit ambiguity about *scope* (not implementation detail), stop
  and comment on the issue rather than guessing and building the wrong
  thing — that costs more tokens than asking upfront.
- Small, reviewable PRs. Prefer several focused PRs over one large one.
