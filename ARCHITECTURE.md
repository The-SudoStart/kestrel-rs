# ARCHITECTURE.md

## Core decision: embed, don't fork

We depend on Servo as a library (crate dependency), rather than forking
its repository.

**Why:**
- Servo is now published on crates.io, making it a normal embeddable
  dependency rather than something you must vendor/fork to use.
- A fork means permanently maintaining a diff against upstream and
  manually merging their fixes. A dependency means `cargo update` pulls
  their improvements for free.
- Our differentiation is the shell (keyboard nav, command palette,
  theming, workflow) — not the rendering engine. We have no reason to
  modify Servo's internals for MVP.

**When we would fork (not now, flag before doing):**
- If we hit a hard blocker in Servo's embedding API that requires a
  source-level patch. In that case: fork only the minimum, patch, point
  `Cargo.toml` at the fork via git dependency, and open an upstream PR to
  Servo so the patch doesn't become permanent maintenance debt.

## GUI framework: Iced

Chosen over Egui/Druid/GTK bindings because:
- Pure Rust, no native toolkit dependency to manage across platforms.
- Elm-style architecture (Model/Update/View) is predictable and easy for
  both humans and agents to reason about without needing broader context —
  state changes are explicit, not scattered across callback closures.
- Actively maintained, reasonable community size.

Revisit only if Servo's WebView API turns out to integrate poorly with
Iced's rendering model — check this early (see ROADMAP.md Phase 0) before
committing further engineering time.

## Module boundaries

- `shell`: owns window/tab lifecycle, holds Servo WebView instances. No UI
  rendering code lives here.
- `ui`: pure presentation — Iced views, widgets, theme application. Should
  not directly manage Servo WebView instances; talks to `shell` via
  messages.
- `keymap`: keyboard input handling and the command palette. Dispatches
  actions to `shell`; doesn't own state itself.
- `config`: load/save TOML settings. No UI or Servo-specific code.

Keep these boundaries. Cross-module logic (e.g., "keyboard shortcut
directly manipulates a WebView") is a smell — route it through `shell`.

## Non-goals (see AGENTS.md Section 3 for the authoritative list)

This section intentionally does not re-litigate what's out of scope —
AGENTS.md is the source of truth. If you're unsure whether something is
in scope, check there first.
