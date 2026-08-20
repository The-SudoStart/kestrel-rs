# ROADMAP.md

Scope is intentionally narrow. Do not add to a phase's checklist without
updating this file first and explaining why in the commit/PR.

## Phase 0 — Spike (before any feature work)

Goal: prove Servo's embedding API works the way we think it does.

- [ ] Get a minimal Iced window rendering a Servo WebView showing a static
      page
- [ ] Confirm navigation (load URL, back/forward) works through the
      embedding API
- [ ] Document any API surface surprises in `/docs/servo-embedding-notes.md`

If this phase reveals Servo's embedding API is unworkable with Iced,
STOP and flag it — do not push forward into Phase 1 on a shaky
foundation.

## Phase 1 — MVP (target: 3-6 months, 1-2 people)

Platform target: pick ONE (Linux or macOS — whichever the team primarily
develops on) and ship well on it before touching a second platform.

- [ ] Single window, multi-tab browsing
- [ ] Basic navigation: back / forward / reload / URL bar
- [ ] Keyboard-first navigation (the differentiator — invest real time
      here)
- [ ] Command palette (VSCode-style, minimal command set: new tab, close
      tab, go to URL, etc.)
- [ ] Basic settings (TOML file, no settings UI required for v1 — editing
      the file by hand is fine)
- [ ] History (in-memory or simple local storage, no sync)

Explicitly NOT in Phase 1 (see AGENTS.md Section 3): plugins, multi-OS,
privacy hardening, themes marketplace, bookmarks sync, DevTools.

## Phase 2 — Post-MVP (only after Phase 1 ships and gets real usage)

- [ ] Theme engine (CSS-based, local themes only — no marketplace yet)
- [ ] Bookmarks
- [ ] Second platform target
- [ ] Basic settings UI (replacing hand-edited TOML)

## Phase 3 — Later (do not plan in detail yet)

- [ ] Plugin/WASM extension system
- [ ] Privacy controls (fingerprint spoofing, per-domain isolation)
- [ ] DevTools integration

## Non-negotiable rule

An issue or ticket from a later phase should not be started while an
earlier phase has incomplete items, unless explicitly reprioritized by a
human in this file.
