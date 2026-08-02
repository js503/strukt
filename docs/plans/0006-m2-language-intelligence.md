# M2.4 Language Intelligence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bounded language-agnostic LSP client with diagnostics, completion, hover, definition, secure discovery/approval, stopped restoration, and final M2 integration validation.

**Architecture:** A new UI-independent `strukt-language` crate owns descriptors, discovery, JSON-RPC/LSP framing, process lifecycle, position conversion, and normalized results. `strukt-app` coordinates revision-scoped work and renders language state without allowing servers to mutate editor documents directly; `strukt-persistence` stores only selection, exact approval, and presentation state.

**Tech Stack:** Rust 1.97.1, Serde/Serde JSON, `url` 2, `blake3`, bounded standard channels and threads, Tokio task wiring, Iced 0.14, repository-owned fake LSP server, GitHub Actions macOS/Ubuntu/Windows matrix.

---

## File Structure

- `crates/strukt-language/src/descriptor.rs`: validated descriptor, discovery, and approval domain.
- `crates/strukt-language/src/framing.rs`: bounded LSP `Content-Length` decoder and encoder.
- `crates/strukt-language/src/position.rs`: Unicode scalar to negotiated LSP position conversion.
- `crates/strukt-language/src/feature.rs`: normalized diagnostics, completion, hover, and definition values.
- `crates/strukt-language/src/protocol.rs`: JSON-RPC IDs, messages, method builders, and response routing.
- `crates/strukt-language/src/transport.rs`: bounded stdio process adapter and lifecycle contract.
- `crates/strukt-language/src/client.rs`: generation-scoped LSP state machine, synchronization, cancellation, and restart policy.
- `crates/strukt-language/src/bin/language-fixture.rs`: deterministic fake language server for native tests and CI smoke.
- `crates/strukt-language/tests/*.rs`: domain, protocol, lifecycle, and native contract tests.
- `crates/strukt-persistence/src/language_store.rs`: privacy-safe language selection/approval snapshots.
- `crates/strukt-app/src/language.rs`: application coordinator and immutable UI snapshot projection.
- `crates/strukt-app/src/app.rs`: message reduction, async scheduling, persistence, smoke, and integration.
- `crates/strukt-app/src/view.rs`: status controls, Problems pane, and language overlays.
- `crates/strukt-app/src/editor.rs`: revision-safe application of completion/navigation results.
- `crates/strukt-theme/src/tokens.rs`: semantic diagnostic and language-state tokens.
- `.github/workflows/ci.yml`: all-platform language and final M2 smoke gates.

## Task 1: Scaffold the Language Domain and Descriptor Registry

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/strukt-language/Cargo.toml`
- Create: `crates/strukt-language/src/lib.rs`
- Create: `crates/strukt-language/src/descriptor.rs`
- Create: `crates/strukt-language/tests/descriptors.rs`

- [x] **Step 1: Write failing descriptor and approval tests**

Define tests that require deterministic language matching, bare `PATH` names or
absolute executables, no shell strings, stable exact approval fingerprints, and
workspace-command approval invalidation:

```rust
#[test]
fn registry_matches_language_without_language_specific_control_flow() {
    let registry = DescriptorRegistry::new(vec![descriptor(
        "rust-analyzer",
        ["rust"],
        ["rust-analyzer"],
    )])
    .unwrap();

    assert_eq!(registry.for_language("rust").unwrap().id(), "rust-analyzer");
    assert!(registry.for_language("python").is_none());
}

#[test]
fn workspace_approval_is_exact_and_invalidates_on_argument_change() {
    let command = ResolvedCommand::new(
        PathBuf::from("/workspace/tools/server"),
        vec!["--stdio".into()],
    )
    .unwrap();
    let approval = CommandApproval::grant(workspace_id(), &command);

    assert!(approval.authorizes(workspace_id(), &command));
    assert!(!approval.authorizes(
        workspace_id(),
        &ResolvedCommand::new(
            PathBuf::from("/workspace/tools/server"),
            vec!["--stdio".into(), "--unsafe".into()],
        )
        .unwrap()
    ));
}
```

- [x] **Step 2: Run the descriptor tests and confirm the crate is missing**

Run: `cargo test -p strukt-language --test descriptors --locked --offline`

Expected: fail because `strukt-language` and its public types do not exist.

- [x] **Step 3: Add the crate and validated descriptor types**

Add `strukt-language` to the workspace and expose this boundary:

```rust
pub struct LanguageServerDescriptor {
    id: DescriptorId,
    display_name: String,
    language_ids: BTreeSet<String>,
    candidates: Vec<ExecutableCandidate>,
    arguments: Vec<OsString>,
    workspace_markers: Vec<PathBuf>,
    initialization_options: serde_json::Value,
    documentation_url: Option<Url>,
    installation_guidance: Option<String>,
    default_enabled: bool,
    source: DescriptorSource,
}

pub enum ExecutableCandidate {
    PathName(OsString),
    Absolute(PathBuf),
}

pub struct CommandApproval {
    workspace: WorkspaceId,
    command_fingerprint: [u8; 32],
}
```

Validate IDs, language sets, executable shapes, NUL-free arguments, 256 KiB
serialized configuration, unique IDs, and source-specific trust. Hash canonical
executable bytes plus length-framed argument bytes with `blake3`.

- [x] **Step 4: Add built-in and JSON registry coverage**

Test schema version 1, unknown-field preservation, duplicate rejection, deterministic
selection, built-in public-alpha descriptors, user configuration, and confined
`.strukt-language.json` parsing without symlink following.

- [x] **Step 5: Run strict domain verification**

Run: `cargo test -p strukt-language --test descriptors --locked --offline`

Run: `cargo clippy -p strukt-language --all-targets --locked --offline -- -D warnings`

Expected: pass.

- [x] **Step 6: Commit the descriptor foundation**

```bash
git add Cargo.toml Cargo.lock crates/strukt-language
git commit -m "feat: add language server descriptors"
```

## Task 2: Implement Bounded Framing, Protocol Types, and Position Conversion

**Files:**

- Create: `crates/strukt-language/src/framing.rs`
- Create: `crates/strukt-language/src/protocol.rs`
- Create: `crates/strukt-language/src/position.rs`
- Create: `crates/strukt-language/src/feature.rs`
- Modify: `crates/strukt-language/src/lib.rs`
- Create: `crates/strukt-language/tests/framing.rs`
- Create: `crates/strukt-language/tests/positions.rs`
- Create: `crates/strukt-language/tests/features.rs`

- [x] **Step 1: Write failing fragmented-frame and bound tests**

```rust
#[test]
fn decoder_handles_fragmented_and_combined_frames() {
    let mut decoder = FrameDecoder::new(FrameLimits::default());
    assert!(decoder.push(b"Content-Len").unwrap().is_empty());
    assert!(decoder
        .push(b"gth: 2\r\n\r\n{}Content-Length: 4\r\n\r\nnull")
        .unwrap()
        .iter()
        .map(Frame::body)
        .eq([b"{}".as_slice(), b"null".as_slice()]));
}

#[test]
fn decoder_rejects_oversized_headers_and_bodies_without_retaining_them() {
    let limits = FrameLimits::new(32, 64).unwrap();
    assert_eq!(
        FrameDecoder::new(limits).push(&vec![b'x'; 33]),
        Err(FrameError::HeaderTooLarge)
    );
    assert_eq!(
        FrameDecoder::new(limits).push(b"Content-Length: 65\r\n\r\n"),
        Err(FrameError::BodyTooLarge { declared: 65 })
    );
}
```

- [x] **Step 2: Run framing tests and confirm missing types**

Run: `cargo test -p strukt-language --test framing --locked --offline`

Expected: compile failure for `FrameDecoder`.

- [x] **Step 3: Implement incremental `Content-Length` framing**

Use a small state machine with a 16 KiB header budget and 16 MiB body budget.
Encode bodies with exact UTF-8 byte lengths:

```rust
pub fn encode_frame(body: &[u8], limits: FrameLimits) -> Result<Vec<u8>, FrameError> {
    limits.validate_body(body.len())?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut encoded = Vec::with_capacity(header.len() + body.len());
    encoded.extend_from_slice(header.as_bytes());
    encoded.extend_from_slice(body);
    Ok(encoded)
}
```

- [x] **Step 4: Write failing JSON-RPC routing and normalized feature tests**

Require monotonic numeric IDs, duplicate-response rejection, notification routing,
bounded error text, push diagnostics, completion limits, safe hover Markdown, and
file-definition normalization.

- [x] **Step 5: Implement the protocol and normalized feature boundary**

Expose:

```rust
pub enum IncomingMessage {
    Response(ResponseMessage),
    Notification(NotificationMessage),
    Request(ServerRequest),
}

pub struct Diagnostic {
    pub uri: DocumentUri,
    pub range: LanguageRange,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
}

pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub insertion: CompletionInsertion,
    pub documentation: Option<MarkupContent>,
}
```

Deserialize only the supported subset while tolerating unknown additive fields.
Cap completion output at 200 items and hover content at 256 KiB.

- [x] **Step 6: Write failing UTF-8/UTF-16 round-trip tests**

```rust
#[test]
fn utf16_positions_round_trip_astral_and_combining_text() {
    let text = "a😀e\u{301}\r\n界";
    let scalar = ScalarPosition::new(0, 2).unwrap();
    let lsp = to_lsp_position(text, scalar, PositionEncoding::Utf16).unwrap();
    assert_eq!(lsp, LspPosition::new(0, 3));
    assert_eq!(from_lsp_position(text, lsp, PositionEncoding::Utf16).unwrap(), scalar);
}
```

- [x] **Step 7: Implement strict position conversion**

Walk line slices without normalizing line endings. Reject columns inside surrogate
pairs, beyond line bounds, or after invalid line numbers. Support UTF-8 and UTF-16;
default to UTF-16 when initialization does not negotiate an encoding.

- [x] **Step 8: Run protocol verification and commit**

Run: `cargo test -p strukt-language --locked --offline`

Run: `cargo clippy -p strukt-language --all-targets --locked --offline -- -D warnings`

Expected: pass.

```bash
git add crates/strukt-language
git commit -m "feat: add bounded LSP protocol core"
```

## Task 3: Add Executable Discovery and Exact Workspace Trust

**Files:**

- Modify: `crates/strukt-language/src/descriptor.rs`
- Create: `crates/strukt-language/src/discovery.rs`
- Modify: `crates/strukt-language/src/lib.rs`
- Create: `crates/strukt-language/tests/discovery.rs`

- [ ] **Step 1: Write failing deterministic discovery tests**

Use temporary executable fixtures to require PATH-order resolution, absolute-path
canonicalization, platform executable suffix behavior, marker ranking, no execution,
confined workspace descriptor reads, and approval-required outcomes.

```rust
#[test]
fn discovery_resolves_but_never_executes_path_candidates() {
    let fixture = executable_fixture("fake-lsp", "must-not-run");
    let outcome = discover(&descriptor_for("fake-lsp"), fixture.path_env(), root()).unwrap();

    assert_eq!(outcome.command().unwrap().executable(), fixture.executable());
    assert!(!fixture.execution_marker().exists());
}
```

- [ ] **Step 2: Run the focused test and confirm failure**

Run: `cargo test -p strukt-language --test discovery --locked --offline`

Expected: compile failure for the discovery interface.

- [ ] **Step 3: Implement discovery without shell execution**

Split the inherited PATH using `std::env::split_paths`, test regular executable
files in deterministic order, canonicalize the selected path, and return:

```rust
pub enum DiscoveryOutcome {
    Available(DiscoveredServer),
    ApprovalRequired(DiscoveredServer),
    Unavailable { guidance: Option<String> },
    Disabled,
}
```

Use platform metadata checks and never run `which`, a shell, an installer, or the
candidate during discovery.

- [ ] **Step 4: Add trust and configuration failure coverage**

Test symlinked workspace config rejection, changed executable identity, changed
arguments, changed workspace ID, denied approval, disabled descriptors, invalid
JSON, and 256 KiB bounds.

- [ ] **Step 5: Verify and commit discovery**

Run: `cargo test -p strukt-language --test discovery --locked --offline`

Run: `cargo clippy -p strukt-language --all-targets --locked --offline -- -D warnings`

Expected: pass.

```bash
git add crates/strukt-language
git commit -m "feat: discover trusted language servers"
```

## Task 4: Implement Process Transport and Generation-scoped LSP Lifecycle

**Files:**

- Create: `crates/strukt-language/src/transport.rs`
- Create: `crates/strukt-language/src/client.rs`
- Modify: `crates/strukt-language/src/lib.rs`
- Create: `crates/strukt-language/tests/client.rs`

- [ ] **Step 1: Write failing lifecycle and stale-generation tests**

Use a fake transport to require initialize-first ordering, capability capture,
full synchronization, cancellation, bounded requests, restart delays, crash-loop
failure, graceful shutdown, and stale completion rejection:

```rust
#[test]
fn stale_response_cannot_cross_document_revision_or_server_generation() {
    let mut client = ready_client();
    let request = client.request_hover(document(), revision(4), position()).unwrap();
    client.did_change(document(), revision(5), "changed").unwrap();
    client.restart_generation();

    assert_eq!(
        client.accept_response(request.id(), hover_response()),
        ResponseDisposition::Stale
    );
    assert!(client.snapshot().hover().is_none());
}
```

- [ ] **Step 2: Run client tests and confirm missing state machine**

Run: `cargo test -p strukt-language --test client --locked --offline`

Expected: compile failure for `LanguageClient`.

- [ ] **Step 3: Implement transport contracts and bounded stdio adapter**

Expose UI-independent contracts:

```rust
pub trait LanguageTransport: Send + Sync {
    fn spawn(&self, request: SpawnRequest) -> Result<Box<dyn LanguageProcess>, TransportError>;
}

pub trait LanguageProcess: Send {
    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError>;
    fn try_read(&mut self) -> Result<Option<Vec<u8>>, TransportError>;
    fn try_read_stderr(&mut self) -> Result<Option<Vec<u8>>, TransportError>;
    fn try_wait(&mut self) -> Result<Option<ProcessExit>, TransportError>;
    fn terminate(&mut self, grace: Duration) -> Result<(), TransportError>;
}
```

Use piped stdin/stdout/stderr, a dedicated reader per stream, 64 KiB chunks,
4 MiB stdout accounting, 1 MiB stderr ring accounting, and no shell.

- [ ] **Step 4: Implement lifecycle reduction**

Model `Discovering` through `Stopped`, generation-scoped requests, 250 ms full-text
change coalescing, 256 request/message limits, 10-second initialize timeout,
5-second request timeout, 30-second idle shutdown, and restart delays of 250 ms,
1 second, and 4 seconds within a ten-minute window.

- [ ] **Step 5: Add failure and isolation tests**

Cover malformed frames, oversized declarations, stderr flood, write failure,
initialize error/timeout, request timeout, unexpected server requests, shutdown
timeout, process crash, manual restart, and independent clients.

- [ ] **Step 6: Run strict lifecycle verification and commit**

Run: `cargo test -p strukt-language --locked --offline`

Run: `cargo clippy -p strukt-language --all-targets --locked --offline -- -D warnings`

Expected: pass.

```bash
git add crates/strukt-language
git commit -m "feat: run bounded language server clients"
```

## Task 5: Build the Deterministic Fake Language Server and Native Contract

**Files:**

- Create: `crates/strukt-language/src/bin/language-fixture.rs`
- Create: `crates/strukt-language/tests/native_contract.rs`
- Modify: `crates/strukt-language/Cargo.toml`

- [ ] **Step 1: Write the failing native contract**

Spawn the repository fixture through `StdioLanguageTransport`, initialize it,
open a Unicode/CRLF document, and assert diagnostics, completion, hover, definition,
cancellation observation, bounded stderr, shutdown, and exit.

- [ ] **Step 2: Run and confirm the fixture is missing**

Run: `cargo test -p strukt-language --test native_contract --locked --offline`

Expected: fail because `CARGO_BIN_EXE_language-fixture` is unavailable.

- [ ] **Step 3: Implement explicit fixture modes**

The binary accepts exactly one mode:

```rust
enum FixtureMode {
    Healthy,
    Fragmented,
    Delayed,
    Malformed,
    Oversized,
    StderrFlood,
    CrashAfterInitialize,
    IgnoreShutdown,
}
```

`Healthy` implements the approved subset and returns stable markers. Other modes
exercise one bounded failure. The fixture never reads workspace files or uses the
network.

- [ ] **Step 4: Run native and cross-target checks**

Run: `cargo test -p strukt-language --test native_contract --locked --offline`

Run: `cargo check -p strukt-language --target x86_64-unknown-linux-gnu --locked --offline`

Run: `cargo clippy -p strukt-language --target x86_64-pc-windows-msvc --all-targets --locked --offline -- -D warnings`

Expected: pass; hosted Windows later executes the native process contract.

- [ ] **Step 5: Commit the fake server contract**

```bash
git add crates/strukt-language
git commit -m "test: add native language server contract"
```

## Task 6: Persist Selection and Approval Without Runtime Content

**Files:**

- Modify: `crates/strukt-persistence/Cargo.toml`
- Modify: `crates/strukt-persistence/src/lib.rs`
- Create: `crates/strukt-persistence/src/language_store.rs`
- Create: `crates/strukt-persistence/tests/language_store.rs`
- Modify: `crates/strukt-workspace/src/state.rs`

- [ ] **Step 1: Write failing privacy and round-trip tests**

Require selection, enablement, approval fingerprint, and Problems visibility to
round trip while server state and runtime results remain impossible to serialize:

```rust
#[test]
fn language_snapshot_contains_only_configuration_and_presentation() {
    let snapshot = LanguageSessionSnapshot::new(
        vec![LanguageSelectionSnapshot::enabled("rust", "rust-analyzer")],
        vec![ApprovalSnapshot::new("rust", command_fingerprint())],
        true,
    );
    let json = serde_json::to_string(&snapshot).unwrap();

    assert!(!json.contains("diagnostic"));
    assert!(!json.contains("source_text"));
    assert!(!json.contains("process"));
    assert!(!json.contains("stderr"));
}
```

- [ ] **Step 2: Run and confirm snapshot types are missing**

Run: `cargo test -p strukt-persistence --test language_store --locked --offline`

Expected: compile failure.

- [ ] **Step 3: Implement versioned language contribution snapshots**

Use the existing opaque workspace contribution pattern. Validate unique language
IDs, descriptor IDs, 32-byte fingerprints, bounded entry counts, and schema version.
Preserve unknown siblings and fall back from corrupt current snapshots.

- [ ] **Step 4: Test stopped restoration and approval invalidation**

Restoration returns selection and presentation only. The app must rediscover and
revalidate canonical commands after a matching document opens; a saved fingerprint
alone never authorizes a changed command.

- [ ] **Step 5: Verify and commit persistence**

Run: `cargo test -p strukt-persistence --locked --offline`

Run: `cargo clippy -p strukt-persistence --all-targets --locked --offline -- -D warnings`

Expected: pass.

```bash
git add crates/strukt-persistence crates/strukt-workspace
git commit -m "feat: persist language preferences safely"
```

## Task 7: Integrate Language Lifecycle and Document Synchronization

**Files:**

- Modify: `crates/strukt-app/Cargo.toml`
- Create: `crates/strukt-app/src/language.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/editor.rs`

- [ ] **Step 1: Write failing reducer scheduling tests**

Require workspace restore to remain stopped, matching document open to schedule
discovery, blocking process work to occur outside the reducer, document changes to
coalesce by revision, and workspace replacement to invalidate all completions.

```rust
#[test]
fn opening_matching_document_schedules_language_start_but_restore_does_not() {
    let mut app = app_with_restored_language_selection();
    assert_eq!(app.language.running_servers(), 0);

    let open = app.update(open_rust_document_message());
    assert!(open.units() > 0);
    assert!(matches!(app.language.state("rust"), LanguageState::Discovering));
}
```

- [ ] **Step 2: Run focused app tests and confirm integration is absent**

Run: `cargo test -p strukt-app --locked --offline language_`

Expected: compile failure for app language state/messages.

- [ ] **Step 3: Implement the app coordinator**

Track workspace identity, per-language generation, synchronized documents,
request guards, normalized snapshots, and persistence dirty state. Convert blocking
jobs into Iced tasks and reduce only their typed completions.

- [ ] **Step 4: Wire open/edit/save/close and override changes**

Document open schedules discovery only for eligible full text. Revision changes
coalesce `didChange`; save and close follow server capabilities. A language override
closes the old pairing and opens the new pairing without mutating document content.

- [ ] **Step 5: Add capability and failure isolation coverage**

Test unavailable, disabled, denied, spawn failure, crash loop, stale response,
workspace replacement, capability disablement, and persistence failure while files,
editor saves, and terminals continue.

- [ ] **Step 6: Verify and commit lifecycle integration**

Run: `cargo test -p strukt-app --locked --offline language_`

Run: `cargo clippy -p strukt-app --all-targets --locked --offline -- -D warnings`

Expected: pass.

```bash
git add crates/strukt-app
git commit -m "feat: coordinate workspace language servers"
```

## Task 8: Add Diagnostics and the Problems Pane

**Files:**

- Modify: `crates/strukt-app/src/language.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/view.rs`
- Modify: `crates/strukt-app/src/editor.rs`
- Modify: `crates/strukt-theme/src/tokens.rs`
- Modify: `crates/strukt-theme/tests/builtin_themes.rs`

- [ ] **Step 1: Write failing diagnostic reducer and theme tests**

Require current-version diagnostics, stale rejection, clearing, severity counts,
confined navigation, external confirmation, and distinct semantic colors in light
and dark modes.

- [ ] **Step 2: Run focused tests and confirm UI state is absent**

Run: `cargo test -p strukt-app --locked --offline diagnostic_`

Run: `cargo test -p strukt-theme --locked --offline diagnostic_`

Expected: fail for missing diagnostic UI types/tokens.

- [ ] **Step 3: Implement immutable diagnostic projection**

Store normalized diagnostics by document and generation. Project current ranges to
editor markers and Problems rows; never store raw protocol JSON. Selecting a row
routes through the existing safe document open and Unicode position conversion.

- [ ] **Step 4: Build the Problems pane and server status actions**

Add a keyboard-focusable Problems pane with severity counts, file grouping,
filtering, and navigation. Add status actions for discover, select, enable/disable,
approve/deny exact command, restart, copy bounded failure, and open documentation.

- [ ] **Step 5: Verify focus, confinement, capability isolation, and themes**

Run: `cargo test -p strukt-app --locked --offline diagnostic_`

Run: `cargo test -p strukt-theme --locked --offline`

Expected: pass.

- [ ] **Step 6: Commit diagnostics**

```bash
git add crates/strukt-app crates/strukt-theme
git commit -m "feat: show language diagnostics and problems"
```

## Task 9: Add Completion, Hover, and Definition

**Files:**

- Modify: `crates/strukt-app/src/language.rs`
- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/view.rs`
- Modify: `crates/strukt-app/src/editor.rs`
- Modify: `crates/strukt-app/src/main.rs`

- [ ] **Step 1: Write failing request guard and editor transaction tests**

Require explicit/trigger completion, 200-item cap, safe snippet flattening, one undo
boundary, bounded sanitized hover, single/multiple/external definition handling,
navigation back, cancellation, and focus isolation from terminals.

- [ ] **Step 2: Run focused tests and confirm features are absent**

Run: `cargo test -p strukt-app --locked --offline completion_`

Run: `cargo test -p strukt-app --locked --offline hover_`

Run: `cargo test -p strukt-app --locked --offline definition_`

Expected: fail for missing messages and projections.

- [ ] **Step 3: Implement request scheduling and stale guards**

Create one guard per transient feature containing workspace, document, revision,
position, server generation, and request ID. New requests invalidate old guards and
schedule `$/cancelRequest`. Timeouts clear only their own overlay.

- [ ] **Step 4: Implement completion transactions**

Normalize text edits into current-document scalar ranges, reject stale/overlapping
or external edits, flatten snippet placeholders to safe text, and apply all accepted
edits as one editor transaction and undo entry.

- [ ] **Step 5: Implement hover and definition UI**

Render sanitized plain text/Markdown without HTML, images, scripts, or remote fetch.
Open one confined definition directly; use a bounded picker for multiple results;
require confirmation for external files; display unsupported URIs without opening.

- [ ] **Step 6: Verify and commit language actions**

Run: `cargo test -p strukt-app --locked --offline`

Run: `cargo clippy -p strukt-app --all-targets --locked --offline -- -D warnings`

Expected: pass.

```bash
git add crates/strukt-app
git commit -m "feat: add core language actions"
```

## Task 10: Add Language Smoke and Final M2 Integration Gate

**Files:**

- Modify: `crates/strukt-app/src/app.rs`
- Modify: `crates/strukt-app/src/main.rs`
- Modify: `.github/workflows/ci.yml`
- Create: `docs/evidence/m2-language-intelligence-validation.md`

- [ ] **Step 1: Write failing launch-mode and integration tests**

Require exact `--language-smoke <existing-root>` and `--m2-integration-smoke
<existing-root>` parsing, bounded runtime deadlines, exact markers, and rejection of
missing roots, extra arguments, near-match flags, and `.strukt` metadata.

- [ ] **Step 2: Implement `--language-smoke`**

Launch only `language-fixture`, initialize, synchronize Unicode/CRLF text, observe
diagnostics, completion, hover, definition and cancellation, shut down, restore
selection without restart, and print:

```text
strukt language smoke: discovery, sync, diagnostics, completion, hover, definition, cancellation, shutdown, and restore passed
```

- [ ] **Step 3: Implement final `--m2-integration-smoke`**

Compose existing workspace-files, editor, terminal, and language smoke contracts in
one isolated root. Verify persistence coexistence, file/editor progress during noisy
terminal/language work, stopped terminal restoration, stopped language restoration,
capability isolation, no runtime-content persistence, and print:

```text
strukt M2 integration smoke: files, editor, terminal, language, persistence, isolation, and stopped restore passed
```

- [ ] **Step 4: Add all-platform CI steps**

Build both fixture binaries, run both exact smoke modes with isolated roots on
macOS, Ubuntu, and Windows, require exact markers, reject `.strukt`, and keep the
existing native app startup gate.

- [ ] **Step 5: Run the complete local gate**

Run:

```bash
forj check /Users/jessie/Development/strukt
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline
cargo build -p strukt-app --locked --offline
cargo build -p strukt-language --bin language-fixture --locked --offline
cargo build -p strukt-terminal --bin terminal-fixture --locked --offline
target/debug/strukt-app --language-smoke <temporary-root>
target/debug/strukt-app --m2-integration-smoke <temporary-root>
cargo check -p strukt-app --target x86_64-unknown-linux-gnu --locked --offline
cargo clippy -p strukt-language -p strukt-persistence -p strukt-workspace \
  -p strukt-app --target x86_64-pc-windows-msvc --all-targets \
  --locked --offline -- -D warnings
```

Expected: pass; only the documented transitive `block 0.1.6` warning may remain.

- [ ] **Step 6: Commit smoke and draft evidence**

```bash
git add .github/workflows/ci.yml crates/strukt-app \
  docs/evidence/m2-language-intelligence-validation.md
git commit -m "test: validate M2 language workflows"
```

## Task 11: Review, Validate, and Close M2

**Files:**

- Modify: `README.md`
- Modify: `docs/decisions/0001-native-ui-framework.md`
- Modify: `docs/evidence/m2-language-intelligence-validation.md`
- Modify: `docs/plans/0006-m2-language-intelligence.md`
- Modify: `docs/roadmap.md`
- Modify: `docs/tracker.md`

- [ ] **Step 1: Complete the native macOS walkthrough**

Use a temporary native app bundle and isolated workspace. Exercise visible
diagnostics, Problems navigation, completion, hover, single/multiple definition,
language status, missing-server guidance, exact workspace approval, restart,
failure isolation, theme contrast, keyboard focus, accessibility exposure, stopped
restoration, and no `.strukt` metadata.

- [ ] **Step 2: Run full-slice agentic review**

Review descriptor validation, executable trust, path/canonicalization races,
protocol bounds, deadlocks, lifecycle ordering, stale generations/revisions,
Unicode positions, Markdown safety, editor transaction integrity, definition
confinement, persistence privacy, capability isolation, cross-platform process
cleanup, fake-server fidelity, M2 integration, and M3/M4 boundary drift. Resolve all
critical and important findings with focused regression tests.

- [ ] **Step 3: Record hosted and manual evidence**

Require the implementation head to pass macOS 14, Ubuntu 24.04, and Windows Server
2022. Record run/job links, fake-server/native results, exact smoke markers, manual
results, review findings, and honest Iced/human Windows limitations.

- [ ] **Step 4: Update milestone and release-roadmap artifacts**

Mark M2.4 and M2 complete. Link the spec, plan, issue, PR, and evidence in tracker
and roadmap. Update README and ADR 0001. Preserve M3 through M5 as the pre-release
critical path and move M6+ to the post-release feature roadmap.

- [ ] **Step 5: Commit completion evidence**

```bash
git add README.md docs/decisions/0001-native-ui-framework.md \
  docs/evidence/m2-language-intelligence-validation.md \
  docs/plans/0006-m2-language-intelligence.md docs/roadmap.md docs/tracker.md
git commit -m "docs: complete M2 local development workspace"
```

- [ ] **Step 6: Require the exact final head before merge**

Push the evidence head, require the complete macOS/Ubuntu/Windows matrix, update the
issue and PR with verification and substantive review findings, mark ready, and
squash-merge only under `docs/process/merge-policy.md`.

## Final Verification

M2.4 is not complete until every acceptance criterion in
`docs/specs/0006-m2-language-intelligence.md` has direct evidence, all review
findings are resolved or explicitly accepted, the issue and PR link every required
artifact, and the exact final PR head is green on macOS, Ubuntu, and Windows. That
merge also closes M2. M3 begins from the merged main branch with no automatic
terminal or language-server restart during restoration.
