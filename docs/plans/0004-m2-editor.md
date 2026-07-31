# M2.2 Editor Implementation Plan

- Status: In progress
- Tracking issue: [#5 — M2.2: Native editor](https://github.com/js503/strukt/issues/5)

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the M2.2 editor slice: preview and pinned tabs, Unicode editing,
safe saves, undo/redo, find/replace, external-change handling, syntax highlighting,
encrypted recovery, restoration, and cross-platform validation.

**Architecture:** A new UI-independent `strukt-editor` crate owns rope-backed
documents, revisioned edit transactions, history, find/replace, tab state, and
grammar descriptors. `strukt-fs` adds capability-confined document read/save
contracts, `strukt-persistence` owns encrypted recovery envelopes and editor
snapshots, and `strukt-app` adapts Iced's native text editor without leaking Iced
types into the domain crates.

**Tech Stack:** Rust 1.97.1, Rust 2024, Iced 0.14, Ropey 1.6.1, regex 1,
XChaCha20-Poly1305 0.11, keyring 4.1.5, serde, atomic application-data persistence,
cap-std, Tokio, Cargo tests, GitHub Actions.

---

## Scope and sequencing

This plan implements only [`../specs/0004-m2-editor.md`](../specs/0004-m2-editor.md).
It does not add LSP behavior, terminals, remote files, editor splits, collaboration,
or a custom GPU text renderer. Each task ends in a buildable, tested commit.

The implementation sequence is:

1. editor identifiers, buffer, and transaction core;
2. history and find/replace;
3. document and tab state;
4. capability-confined reads and saves;
5. encrypted recovery and editor persistence;
6. grammar registry and syntax adapter;
7. native editor surface and commands;
8. external changes and restoration;
9. deterministic smoke and cross-platform evidence;
10. complete review and delivery artifacts.

## File map

### New editor domain crate

- `crates/strukt-editor/src/position.rs`: character-based positions, ranges, and
  conversion validation.
- `crates/strukt-editor/src/buffer.rs`: Ropey-backed canonical text and line-ending
  metadata.
- `crates/strukt-editor/src/transaction.rs`: revision-bound non-overlapping edits
  and inverse transactions.
- `crates/strukt-editor/src/history.rs`: bounded undo/redo and typing coalescing.
- `crates/strukt-editor/src/find.rs`: literal and regex query compilation, matches,
  navigation, and replace transactions.
- `crates/strukt-editor/src/document.rs`: document identity, disk baseline, dirty,
  conflict, missing, read-only, and recovery state.
- `crates/strukt-editor/src/tabs.rs`: one preview slot, pinned tabs, focus, close
  decisions, and serializable view state.
- `crates/strukt-editor/src/grammar.rs`: bundled data-backed syntax descriptors.
- `crates/strukt-editor/src/lib.rs`: public domain API only.

### Existing crates

- `crates/strukt-fs/src/document.rs`: confined reads, binary/size classification,
  disk revisions, staged saves, and save conflicts.
- `crates/strukt-persistence/src/editor_store.rs`: editor layout snapshots and
  encrypted recovery envelopes.
- `crates/strukt-app/src/editor.rs`: Iced/domain adapter, surface instances, focus,
  and async command routing.
- `crates/strukt-app/src/app.rs`: application messages and workspace/editor
  coordination.
- `crates/strukt-app/src/view.rs`: tabs, editor, find bar, status, conflicts, and
  close dialogs.
- `crates/strukt-theme/src/lib.rs`: semantic editor and syntax tokens.
- `.github/workflows/ci.yml`: deterministic editor smoke on all hosted platforms.

## Task 1: Add the editor buffer and transaction core

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/strukt-editor/Cargo.toml`
- Create: `crates/strukt-editor/src/lib.rs`
- Create: `crates/strukt-editor/src/position.rs`
- Create: `crates/strukt-editor/src/buffer.rs`
- Create: `crates/strukt-editor/src/transaction.rs`
- Create: `crates/strukt-editor/tests/transactions.rs`

- [ ] **Step 1: Register the crate and dependencies**

Add `crates/strukt-editor` to the workspace and add:

```toml
regex = "1"
ropey = "1.6.1"
strukt-editor = { path = "crates/strukt-editor" }
```

The crate depends on `ropey`, `serde`, `thiserror`, and the workspace lints. It must
not depend on Iced, filesystem APIs, or persistence.

- [ ] **Step 2: Write failing position and transaction tests**

Cover:

```rust
#[test]
fn transaction_rejects_overlap_and_stale_revision() {
    let mut buffer = TextBuffer::new("alpha beta");
    let overlapping = EditTransaction::new(
        Revision::INITIAL,
        vec![
            Replacement::new(CharRange::new(0, 5), "one"),
            Replacement::new(CharRange::new(4, 8), "two"),
        ],
    );
    assert_eq!(overlapping.unwrap_err(), TransactionError::OverlappingRanges);

    buffer.apply(EditTransaction::insert(Revision::INITIAL, 0, "x")).unwrap();
    assert_eq!(
        buffer.apply(EditTransaction::insert(Revision::INITIAL, 0, "y")),
        Err(TransactionError::StaleRevision {
            expected: Revision::new(1),
            actual: Revision::INITIAL,
        })
    );
}
```

Also test Unicode scalar boundaries, CRLF detection, multiple non-overlapping
replacements, and inverse-transaction round trips.

- [ ] **Step 3: Run the focused tests and observe failure**

```bash
cargo test -p strukt-editor --test transactions
```

Expected: compilation fails because the editor types do not exist.

- [ ] **Step 4: Implement positions, rope buffer, and transactions**

Define:

```rust
pub struct Revision(u64);

pub struct CharRange {
    pub start: usize,
    pub end: usize,
}

pub struct Replacement {
    pub range: CharRange,
    pub text: String,
}

pub struct EditTransaction {
    pub expected_revision: Revision,
    pub replacements: Vec<Replacement>,
}

pub struct AppliedTransaction {
    pub revision: Revision,
    pub inverse: EditTransaction,
    pub inserted_bytes: usize,
    pub removed_bytes: usize,
}
```

`TextBuffer` wraps `ropey::Rope`, stores the original line-ending mode, validates
all ranges before mutation, applies replacements from the end toward the start, and
returns a complete inverse transaction. No partial transaction may be observable.

- [ ] **Step 5: Verify and commit**

```bash
cargo fmt --all --check
cargo clippy -p strukt-editor --all-targets -- -D warnings
cargo test -p strukt-editor
git add Cargo.toml Cargo.lock crates/strukt-editor
git commit -m "feat: add editor transaction core"
```

## Task 2: Add bounded history and find/replace

**Files:**

- Create: `crates/strukt-editor/src/history.rs`
- Create: `crates/strukt-editor/src/find.rs`
- Modify: `crates/strukt-editor/src/lib.rs`
- Create: `crates/strukt-editor/tests/history.rs`
- Create: `crates/strukt-editor/tests/find.rs`

- [ ] **Step 1: Write failing history tests**

Test that adjacent insertions coalesce, cursor discontinuity breaks coalescing,
undo returns an inverse transaction, a new edit clears redo, and both the 10,000
entry and 64 MiB byte budgets evict oldest complete entries.

```rust
#[test]
fn new_edit_after_undo_clears_redo() {
    let mut document = test_document("abc");
    document.insert(3, "d").unwrap();
    document.undo().unwrap();
    document.insert(3, "x").unwrap();
    assert_eq!(document.redo(), Err(HistoryError::NothingToRedo));
}
```

- [ ] **Step 2: Write failing find/replace tests**

Cover literal, case-insensitive, whole-word, regex, invalid regex, previous/next
wraparound, multi-byte text, zero-width matches, replace current, and replace all.
Replace-all must produce one undoable transaction.

- [ ] **Step 3: Implement history and find**

Define `HistoryBudget`, `History`, `FindQuery`, `FindOptions`, `FindMatch`, and
`FindResult`. Regex compilation errors are typed and never panic. Zero-width regex
matches advance by one Unicode scalar to guarantee progress.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p strukt-editor --test history --test find
cargo clippy -p strukt-editor --all-targets -- -D warnings
git add crates/strukt-editor
git commit -m "feat: add editor history and find"
```

## Task 3: Add document and tab state

**Files:**

- Create: `crates/strukt-editor/src/document.rs`
- Create: `crates/strukt-editor/src/tabs.rs`
- Modify: `crates/strukt-editor/src/lib.rs`
- Create: `crates/strukt-editor/tests/documents.rs`
- Create: `crates/strukt-editor/tests/tabs.rs`

- [ ] **Step 1: Write failing document-state tests**

Cover unique `DocumentId`, normalized relative paths, dirty baseline transitions,
read-only refusal, clean reload, dirty conflict, missing-file recovery, stale event
rejection, and save-success state.

```rust
#[test]
fn dirty_external_change_preserves_local_content() {
    let mut document = document("src/main.rs", "one");
    document.insert(3, " local").unwrap();
    document
        .observe_disk_change(DiskRevision::test(2), "disk")
        .unwrap();
    assert_eq!(document.text(), "one local");
    assert!(matches!(document.status(), DocumentStatus::Conflict { .. }));
}
```

- [ ] **Step 2: Write failing preview-tab tests**

Cover replacement of a clean preview, reuse of an already-open path, promotion by
edit/double-click/pin, non-replacement of dirty/conflicted/missing/recovered tabs,
active-tab fallback, and `Save`/`Discard`/`Cancel` close decisions.

- [ ] **Step 3: Implement documents and tabs**

`EditorWorkspace` owns an ordered map of documents, tab order, one optional preview
ID, and the active ID. It exposes commands and immutable `EditorViewState`; callers
cannot mutate a `Document` directly without a revisioned event.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p strukt-editor
cargo clippy -p strukt-editor --all-targets -- -D warnings
git add crates/strukt-editor
git commit -m "feat: model editor documents and tabs"
```

## Task 4: Add confined document reads and staged saves

**Files:**

- Create: `crates/strukt-fs/src/document.rs`
- Modify: `crates/strukt-fs/src/lib.rs`
- Create: `crates/strukt-fs/tests/document_io.rs`

- [ ] **Step 1: Write failing read-classification tests**

Test normal UTF-8, CRLF, initial-8-KiB NUL detection, invalid UTF-8, 4 MiB editable
limit, first-1-MiB large preview, full-size reporting, explicit override, traversal,
symlink escape, root replacement, and FIFO rejection.

- [ ] **Step 2: Write failing save tests**

Test expected revision success, external change conflict, staged-write failure,
permission preservation, traversal rejection, symlink-parent replacement, root
replacement, and save publication that leaves either the old complete file or the
new complete file after injected failure.

```rust
#[test]
fn changed_disk_revision_is_never_knowingly_overwritten() {
    let fixture = WorkspaceFixture::new("before");
    let opened = read_document(fixture.root(), "file.txt", ReadOptions::default())
        .unwrap();
    fixture.write_ambient("file.txt", "external");
    let error = save_document(
        fixture.root(),
        SaveRequest::new("file.txt", "local", opened.disk_revision),
    )
    .unwrap_err();
    assert!(matches!(error, DocumentIoError::SaveConflict { .. }));
    assert_eq!(fixture.read("file.txt"), "external");
}
```

- [ ] **Step 3: Implement confined read and save contracts**

Define `ReadOptions`, `DocumentRead`, `DocumentKind`, `DiskRevision`, `SaveRequest`,
`SaveMode::{IfUnchanged, Force}`, `SaveOutcome`, and `DocumentIoError`. `Force` is
accepted only from the explicit conflict-resolution command and never from autosave.
Reuse the retained `WorkspaceRoot` capability
and the staged no-escape patterns from file operations. Never fall back to an
ambient absolute path.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p strukt-fs --test document_io
cargo clippy -p strukt-fs --all-targets -- -D warnings
git add crates/strukt-fs
git commit -m "feat: add confined document IO"
```

## Task 5: Add encrypted recovery and editor persistence

**Files:**

- Modify: `Cargo.toml`
- Modify: `crates/strukt-persistence/Cargo.toml`
- Create: `crates/strukt-persistence/src/editor_store.rs`
- Modify: `crates/strukt-persistence/src/lib.rs`
- Create: `crates/strukt-persistence/tests/editor_store.rs`
- Modify: `crates/strukt-app/Cargo.toml`
- Create: `crates/strukt-app/src/recovery_key.rs`

- [ ] **Step 1: Add authenticated-encryption and keyring dependencies**

Use:

```toml
chacha20poly1305 = { version = "0.11", features = ["getrandom"] }
keyring = "4.1.5"
zeroize = "1"
```

`keyring` belongs only to `strukt-app`; persistence depends on a project-owned key
provider trait and XChaCha20-Poly1305.

- [ ] **Step 2: Write failing envelope and store tests**

Cover encryption/decryption, unique nonces, authenticated metadata, wrong key,
tampering, unsupported schema, corrupt-current fallback, atomic replacement,
key-unavailable behavior, deleting recovery after save/discard, and no `.strukt`
path in the workspace.

- [ ] **Step 3: Implement the recovery store**

Define:

```rust
pub trait RecoveryKeyProvider: Send + Sync {
    fn load_or_create(&self) -> Result<RecoveryKey, RecoveryKeyError>;
    fn delete(&self) -> Result<(), RecoveryKeyError>;
}

#[derive(Serialize, Deserialize)]
pub struct RecoveryEnvelope {
    pub schema_version: u32,
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}
```

Authenticate schema version, workspace ID, document path, and baseline as AAD.
Zeroize key material on drop. Store current and last-valid envelopes in the
application-data editor directory.

- [ ] **Step 4: Implement the native key provider**

Use keyring service `dev.strukt.editor-recovery` and account `default`. Retrieve or
generate exactly 32 secret bytes through `Entry::get_secret`/`set_secret`. Map
`NoDefaultStore`, locked, denied, and unavailable cases to a visible disabled state;
never create a plaintext fallback.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-persistence --test editor_store
cargo clippy --workspace --all-targets -- -D warnings
git add Cargo.toml Cargo.lock crates/strukt-persistence crates/strukt-app
git commit -m "feat: persist encrypted editor recovery"
```

## Task 6: Add the grammar registry and theme tokens

**Files:**

- Create: `crates/strukt-editor/src/grammar.rs`
- Modify: `crates/strukt-editor/src/lib.rs`
- Create: `crates/strukt-editor/tests/grammar.rs`
- Modify: `crates/strukt-theme/src/lib.rs`
- Modify: `crates/strukt-theme/tests/builtin_themes.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Write failing registry and theme tests**

Test exact file names, case-insensitive extensions where appropriate, overrides,
unknown fallback, all bundled languages, distinct light/dark syntax colors, and
semantic selection/gutter/conflict colors.

- [ ] **Step 2: Implement descriptors and tokens**

Define a static `GrammarDescriptor` registry for Rust, JavaScript, TypeScript,
Python, JSON, TOML, Markdown, shell, YAML, HTML, CSS, and plain text. The descriptor
contains stable ID, display name, extensions, exact file names, and the Iced
highlighter token name as data; it contains no parser executable.

Add semantic theme fields for editor background/foreground, gutter, active line,
selection, matching bracket, dirty, conflict, missing, and syntax categories.

- [ ] **Step 3: Enable Iced highlighting and verify**

Add the Iced `highlighter` feature in the workspace dependency. Run:

```bash
cargo test -p strukt-editor --test grammar
cargo test -p strukt-theme
cargo clippy --workspace --all-targets -- -D warnings
git add Cargo.toml Cargo.lock crates/strukt-editor crates/strukt-theme
git commit -m "feat: register editor syntax themes"
```

## Task 7: Integrate the native editor surface

**Files:**

- Create: `crates/strukt-app/src/editor.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/view.rs`
- Modify: `crates/strukt-app/Cargo.toml`
- Modify: `crates/strukt-core/src/lib.rs`

- [ ] **Step 1: Write failing reducer tests**

Add tests for explorer and Quick Open opening one document, preview replacement,
double-click/edit pinning, already-open focus, surface edit to domain transaction,
undo/redo, find/replace, save task routing, dirty close dialog, large-file override,
binary metadata view, shortcuts, and workspace replacement cancellation.

- [ ] **Step 2: Register editor capabilities and messages**

Add typed app messages for open completion, surface action, pin, select, close,
close-decision, save completion, undo/redo, find updates, replace, language override,
large-file override, explicit force-save confirmation, and focus. Register
`editor.documents` and `editor.syntax`
capabilities in `strukt-core`.

- [ ] **Step 3: Implement the Iced/domain adapter**

`EditorSurfaces` maps `DocumentId` to `iced::widget::text_editor::Content`. Translate
Iced cursor and selection state to domain character ranges before each edit. Apply
the domain transaction and native action in one reducer turn; if domain validation
fails, rebuild the surface from the unchanged domain snapshot and show the error.

Ordinary insert/delete/paste must not call `Content::text()` for the full document.
Undo, redo, replace-all, reload, and recovery restore may rebuild the surface.

- [ ] **Step 4: Render tabs, editor, find bar, and dialogs**

Replace the representative file canvas with the active document view. Render dirty,
conflict, missing, preview, and recovery indicators; close controls; line/status
information; find/replace modes; binary/large-file metadata; save-conflict actions;
and the consolidated dirty-close dialog. Keep Files one keyboard command away.

- [ ] **Step 5: Verify and commit**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p strukt-app
cargo run -p strukt-app
git add crates/strukt-app crates/strukt-core
git commit -m "feat: edit local workspace documents"
```

## Task 8: Wire external changes, recovery, and restoration

**Files:**

- Modify: `crates/strukt-app/src/editor.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/workspace.rs`
- Modify: `crates/strukt-persistence/src/workspace_store.rs`
- Modify: `crates/strukt-workspace/src/state.rs`
- Create: `crates/strukt-app/tests/editor_integration.rs`

- [ ] **Step 1: Write failing integration tests**

Cover clean watcher reload, dirty watcher conflict, compare data, reload undo boundary,
keep-editing behavior, missing file, save/watcher loop suppression, coalesced two-second
recovery, save/discard cleanup, key unavailable, tab/view restoration, recovered
unsaved content, missing placeholder, and stale async completion rejection.

- [ ] **Step 2: Extend workspace persistence schemas**

Add a versioned editor contribution containing tab order, preview, active document,
paths, cursor/selection/scroll, find settings, language override, and read-only
choice. Preserve unknown contribution payloads and migrate M2.1 snapshots with an
empty editor contribution.

- [ ] **Step 3: Coordinate watcher and editor events**

Batch watcher paths, identify open documents, read disk revisions off the UI thread,
and dispatch revision-bound events. Suppress only the matching save outcome; a
different post-save revision must still surface.

- [ ] **Step 4: Coordinate recovery and restore**

Coalesce recovery writes per document after two idle seconds. On startup, show the
native shell immediately, restore clean documents asynchronously, decrypt recovery
off the UI thread, and reject results for a different workspace generation.

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p strukt-app --test editor_integration
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
git add crates/strukt-app crates/strukt-persistence crates/strukt-workspace
git commit -m "feat: restore and reconcile editor state"
```

## Task 9: Add deterministic cross-platform editor smoke

**Files:**

- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `.github/workflows/ci.yml`
- Create: `docs/evidence/m2-editor-validation.md`

- [ ] **Step 1: Write failing launch-mode tests**

Accept only `--editor-smoke <fixture-root>`. Reject missing paths, extra arguments,
near-match flags, binary fixtures, and fixtures without `strukt-editor-smoke.txt`.

- [ ] **Step 2: Implement the smoke workflow**

The smoke opens the sentinel through the workspace capability, creates a preview,
edits and pins it, undoes/redoes, saves, verifies disk content, writes and reloads a
workspace/editor snapshot, verifies no `.strukt` path, prints exactly:

```text
strukt editor smoke: open, edit, save, and restore passed
```

and exits zero. Use an isolated application-data directory and in-memory recovery
key provider so CI never touches a real credential store.

- [ ] **Step 3: Add hosted smoke steps**

Create the same temporary UTF-8 fixture on macOS, Ubuntu, and Windows. Require the
exact marker and a zero exit code. Keep the existing native startup and workspace
files smokes.

- [ ] **Step 4: Draft evidence and commit**

Record the local commands, smoke contract, manual checklist, Windows-hosted key
provider contract, known Iced limitations, and remaining M2 slices.

```bash
git add .github/workflows/ci.yml crates/strukt-app docs/evidence/m2-editor-validation.md
git commit -m "test: add cross-platform editor smoke"
```

## Task 10: Complete validation, review, and delivery artifacts

**Files:**

- Modify: `README.md`
- Modify: `docs/evidence/m2-editor-validation.md`
- Modify: `docs/plans/0004-m2-editor.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/tracker.md`

- [ ] **Step 1: Run the complete local gate**

```bash
forj check .
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline
cargo build -p strukt-app --locked --offline
cargo run -p strukt-app --locked --offline -- --editor-smoke <fixture>
cargo check -p strukt-app --target x86_64-unknown-linux-gnu --locked --offline
```

Run Windows-target strict Clippy for every UI-independent modified crate. The native
Windows hosted job is authoritative where macOS lacks Microsoft build tools.

- [ ] **Step 2: Complete the manual macOS walkthrough**

Validate preview/pin behavior, tabs, Unicode typing, IME composition, selection,
clipboard, undo/redo, find/replace, save, dirty close, external clean reload, dirty
conflict actions, recovery across restart, binary metadata, large-file override,
syntax themes, keyboard traversal, focus, accessibility labels, and no workspace
metadata.

- [ ] **Step 3: Run agentic review**

Review the full diff against the spec for transaction correctness, UTF-8 indexing,
history bounds, path/symlink/root replacement, atomic saves, external races,
recovery cryptography and key handling, stale async results, large-file allocation,
IME/accessibility regressions, persistence migration, cross-platform behavior, and
scope drift. Resolve all critical and important findings.

- [ ] **Step 4: Record hosted results and update tracking**

After macOS, Ubuntu, and Windows jobs pass at the final implementation SHA, complete
the evidence, mark M2.2 complete while keeping M2 in progress, link the issue and PR,
and document remaining terminal/language/integration gates.

- [ ] **Step 5: Commit completion evidence**

```bash
git add README.md docs/evidence/m2-editor-validation.md \
  docs/plans/0004-m2-editor.md docs/roadmap.md docs/tracker.md
git commit -m "docs: record M2 editor validation"
```

## Final verification

Before claiming M2.2 complete, rerun the complete local gate on a clean tree, push
the evidence-only commit, and require the exact final PR head to pass hosted macOS,
Ubuntu, and Windows CI. The pull request must link the spec, this plan, the tracking
issue, validation evidence, and substantive agentic review summary.
