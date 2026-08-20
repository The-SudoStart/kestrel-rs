## Closes

<!-- issue number, e.g. Closes #12 -->

## What changed

<!-- Brief description of the change -->

## Why

<!-- What problem this solves / what it enables -->

## Scope check

- [ ] This change is within the current ROADMAP.md phase
- [ ] This change doesn't touch anything on AGENTS.md's "out of scope" list
- [ ] No new dependencies added outside AGENTS.md Section 4 (or, if yes, explained below)

## Architecture/scope decisions made along the way

<!-- Anything not explicitly specified in the issue that you decided on
your own — GUI layout choices, error handling approach, etc. This is so
reviewers can catch scope creep or unintended architecture drift early. -->

## Checklist

- [ ] `cargo fmt` run
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo test` passing
- [ ] `cargo build` succeeds
