# AGENTS.md — Context for AI coding agents

Read this file fully before touching any code, opening a PR, or picking up an
issue. It exists so you don't have to re-derive project scope, architecture,
or conventions from scratch every session — that costs tokens and produces
inconsistent decisions across agents/sessions.

If something in this file conflicts with what you'd normally assume from a
generic Rust/browser project, THIS FILE WINS.

---

## 1. What this project is

A lightweight, keyboard-first, customizable desktop browser shell built in
Rust, using **Servo** (https://github.com/servo/servo, servo.org) as the
embedded rendering engine via its crate/WebView API — not a forked or
custom rendering engine.

We are NOT building a Chrome/Firefox competitor. We are building a niche
browser for developers/power users who want a fast, hackable,
keyboard-driven browsing experience, and who accept that some sites may not
render perfectly yet (Servo's web-platform-test pass rate is currently
lower than Chromium's — this is expected and acceptable for our audience).

Team size: 1-2 people (+ AI coding agents). Every scope decision below
exists to keep this achievable at that team size. Do not silently expand
scope, even if it seems like "obviously the right architecture" — flag it
in the PR description instead and let a human decide.

## 2. Repo relationship to Servo

- **This repo does NOT contain a fork of Servo.** Servo is a Cargo
  dependency (crates.io / git dependency), not vendored code.
- Never copy Servo source files into this repo "to modify them locally."
  If Servo's behavior needs to change, that's a separate, explicit
  decision (fork-for-patch + upstream PR) — flag it, don't just do it.
- Check `servo.org` and the Servo GitHub repo for the current state of the
  embedding/WebView API before assuming any API surface — Servo is early
  (0.1.x as of mid-2026) and its embedding API is still evolving. Don't
  rely on training-data assumptions about its API shape; verify against
  the current crate docs/README.

## 3. Architecture (current, locked for MVP)

```
┌─────────────────────────────┐
│   GUI shell (this repo)      │  ← Rust, GUI framework: Iced
│   - window/tab management    │
│   - keyboard nav + cmd palette│
│   - settings/theme           │
└───────────────┬──────────────┘
                │  Servo WebView API
┌───────────────▼──────────────┐
│   Servo (external crate dep) │  ← rendering, JS, networking, DOM
└───────────────────────────────┘
```

**Explicitly out of scope for MVP** (do not implement, do not scaffold,
do not add TODOs suggesting future work here unless asked):
- Plugin/WASM extension system
- Multi-OS builds (target ONE platform first — see ROADMAP.md for which)
- Fingerprint spoofing / advanced privacy controls
- Per-domain sandboxing/resource limits
- Any custom rendering/layout/JS engine code
- Extension marketplace, theme marketplace

If an issue or ticket asks for something in this list, stop and flag it
for human review rather than implementing it.

## 4. Tech stack (locked — do not swap without a human decision)

| Concern         | Choice                     |
|------------------|----------------------------|
| Language         | Rust (stable toolchain)    |
| Rendering engine | Servo (crate dependency)   |
| GUI framework    | Iced                       |
| Async runtime    | tokio (only if/when Servo's API requires it directly — don't add for its own sake) |
| Config format    | TOML                       |

Do not introduce new major dependencies (new GUI framework, new async
runtime, new build tool) without flagging it in the PR description first.

## 5. Repo structure

```
/src
  /shell        - window, tab management, app lifecycle
  /ui           - Iced views/widgets, theme
  /keymap       - keyboard-first navigation, command palette
  /config       - settings, TOML load/save
  main.rs
/docs           - architecture notes, decisions
AGENTS.md        - this file
ARCHITECTURE.md  - deeper technical rationale
ROADMAP.md       - phased scope, what's in/out per milestone
CONTRIBUTING.md  - PR/issue workflow
```

## 6. Conventions

- Commit style: Conventional Commits (`feat:`, `fix:`, `chore:`, `docs:`,
  `refactor:`, `test:`).
- Branch naming: `type/short-description` e.g. `feat/tab-management`,
  `fix/window-resize-crash`.
- Every PR must reference an issue number.
- Run `cargo fmt` and `cargo clippy -- -D warnings` before opening a PR.
  A PR with clippy warnings will not be reviewed.
- Write tests for anything in `/src/shell` and `/src/config` (core logic).
  UI code in `/src/ui` doesn't need exhaustive tests, but should not break
  `cargo build`.
- No `unsafe` blocks without a comment explaining why it's necessary and
  why it's sound. Prefer finding a safe alternative first.

## 7. What to do before starting any issue/ticket

1. Read the issue fully. Check it against Section 3's "out of scope" list.
2. Check ROADMAP.md — confirm the issue belongs to the current milestone.
   If it's from a future milestone, don't start it; comment on the issue
   instead.
3. Check CONTRIBUTING.md for the PR checklist.
4. If the issue is ambiguous about *how* (not *what*) to implement
   something, make the simplest choice consistent with Section 3 and 4,
   note the choice in the PR description, and proceed — don't block on
   asking a human for implementation details that don't change scope.
5. If the issue seems to require expanding scope beyond Section 3/4,
   stop and comment on the issue rather than implementing it.

## 8. Definition of done for a PR

- Builds clean (`cargo build`), no clippy warnings.
- Tests pass (`cargo test`).
- PR description follows `.github/PULL_REQUEST_TEMPLATE.md`.
- No new dependencies outside Section 4 without a flagged note.
- No scope creep outside Section 3.
