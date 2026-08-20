# Servo embedding notes

Fill this in during Phase 0. Track here:

- Which crate(s) you actually depend on (exact names/versions from crates.io)
- The WebView API surface as it exists today (verify against current
  servo.org / GitHub docs — do not trust assumptions, the API is young
  and evolving)
- Any gaps between what the embedding API offers and what we need for
  Phase 1 (ROADMAP.md)
- Any workarounds required, and whether they imply we need to
  fork-for-patch (see ARCHITECTURE.md "when we would fork")
note: Cargo.lock is gitignored here since this is a binary that's still pre-alpha;
switch to committing Cargo.lock once you're past Phase 0 and want reproducible builds.
