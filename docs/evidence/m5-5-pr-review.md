# M5.5 checkpoint review — 2026-09-07

Disposition: draft PR only; not ready to merge or release.

Review scope: uncommitted Session Deck, quiet UI, persistence changes, activity
icons and Stillpoint logo following a3bd8f9. Independent reviewer plus primary
agent inspection. The PR also includes 32 earlier M5.5 commits relative to
f92042a; this review is not an exhaustive re-review of all earlier commits.

## Fresh verification

- `cargo fmt --all --check`: passed.
- `cargo test --workspace`: 567 passed, one intentionally ignored disposable
  real-OpenSSH integration test requiring an external configured target.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `bash scripts/check-ui-semantics.sh`: passed.
- `bash scripts/m5-5-visual-foundation-smoke.sh`: passed.
- `forj check .`: passed manifest check.
- `git diff --check`: passed.

Existing native captures are evidence from the earlier implementation sessions,
not fresh interactive verification on this date. Passing source guards does not
prove rendered behavior. Dependency `block 0.1.6` emits a future-compatibility warning.

## Unresolved findings / merge blockers

1. Quick Open dispatch in `view/mod.rs` follows the new session/editor primary
   branches, so Cmd+P can focus an input that is never rendered in Session Deck.
2. `DocumentOpened` opens the supporting drawer even when the same editor is
   already the secondary split surface, duplicating the editor. Problems state
   also needs reconciliation when explicitly opening the editor drawer.
3. Supporting editor placement handlers return without scheduling persistence;
   placement updates depend on later unrelated saves.
4. Remote `connection_latency_ms` measures connection setup, not network RTT;
   the status label must not present it as live latency.
5. Persisted promoted layouts restore without `promotion` return metadata;
   returning/closing the restored supporting editor can become a no-op.

The reviewer also identified hidden binary/invalid-UTF8 notices in Session Deck,
missing drawer placement controls after closing the final tab, and absent full
occlusion handling for the brand. Overall risk: moderate-high. Independent review
was source-only; verification commands above were run by the primary agent.

Additional acceptance gaps remain in the visual evidence: responsive tab overflow,
provider error recovery, direct persistent terminal input, native font resolution,
screen-reader support, and light/remote/motion walkthroughs.

The user requested review, commit, and PR. This commit is a reviewable checkpoint,
not acceptance of these risks for merge. No merge or release is authorized here.
