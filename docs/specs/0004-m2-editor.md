# M2.2 Editor

- Status: Approved design
- Milestone: M2 — Local development workspace
- Parent spec: [`0003-local-development-workspace.md`](0003-local-development-workspace.md)
- Depends on: M2.1 workspace and files slice
- Tracking issue: [#5 — M2.2: Native editor](https://github.com/js503/strukt/issues/5)

## Summary

M2.2 turns the center canvas into a real local source editor. A developer can open
workspace files from the explorer or Quick Open, work across preview and pinned
tabs, edit and save text, undo and redo changes, find and replace content, recover
unsaved work, and respond safely to external filesystem changes.

The editor is local-first and useful without AI, a cloud account, a language
server, or a terminal. Syntax highlighting ships in this slice, while Language
Server Protocol features remain the separate M2 language-intelligence slice.

## Goals

- Provide native Unicode editing with IME, selection, clipboard, keyboard, focus,
  and accessibility behavior through the Iced application surface.
- Keep document identity, content changes, history, dirty state, conflicts,
  recovery, and editor-facing state in a UI-independent `strukt-editor` module.
- Support one replaceable preview tab and multiple pinned document tabs.
- Save through the retained workspace capability without writing metadata into the
  opened repository.
- Handle external edits, moves, and deletions without silently discarding local
  work.
- Provide document-scoped undo, redo, find, replace, indentation, line numbers,
  bracket matching, and bundled syntax highlighting.
- Restore open documents and editor view state across application restarts.
- Recover unsaved content automatically when protected per-user key storage is
  available.
- Keep large, binary, and pathological files from making the application
  unresponsive.
- Preserve boundaries that allow a later custom GPU editor surface without
  rewriting document behavior.

## Non-goals

- Language-server discovery, lifecycle, diagnostics, completion, hover, or
  go-to-definition.
- Multi-cursor user commands or rectangular-selection UI. The edit model must be
  able to add them later without changing document identity or persistence.
- Split editor groups. M2.2 has one center editor group; layout integration is a
  later M2 slice.
- Remote files, SSH workspaces, collaborative editing, Git diff editing, notebooks,
  or rich binary previews.
- Downloading grammars or language tools at runtime.
- Autosave by default. Delayed autosave exists as an opt-in preference.
- A fully custom text renderer. M2.2 validates the Iced editor surface behind an
  adapter before strukt considers replacing it.

## Product Decisions

### Preview and pinned tabs

Single-clicking a file opens it in the editor group's preview slot. Opening another
file by single click replaces an unchanged preview. Double-clicking, editing,
pinning, or explicitly opening to the side promotes the preview to a permanent tab.
Because split groups are out of scope, “open to the side” is represented in the
command contract but is not exposed in M2.2.

A dirty, conflicted, missing, or recovery-backed document is always pinned and is
never replaced implicitly.

### Local unsaved recovery

Unsaved recovery is enabled automatically when a platform-protected per-user key
is available. Recovery content is encrypted before it reaches application-local
storage. The repository receives no recovery files, lockfiles, or editor metadata.

If protected key storage is unavailable, editing and ordinary saves continue, but
recovery is disabled and the editor presents a clear explanation. Plaintext
recovery is never used as a fallback. Disabling recovery deletes the stored
recovery payloads for that workspace.

### Large and binary files

The default normal-edit threshold is 4 MiB per file. A larger text file opens in a
bounded read-only preview that loads at most the first 1 MiB and reports the full
file size. **Edit Anyway** is an explicit consent action that loads the full file
and promotes the tab to pinned. The warning becomes stronger above 64 MiB, but the
user remains in control.

A file with a NUL byte in its initial 8 KiB is treated as binary. Binary files show
path, type, and size metadata rather than entering the text editor. Lines longer
than 1 MiB disable syntax highlighting for that document and display a performance
notice; plain-text editing remains available when the normal size rules allow it.

All thresholds are application preferences stored outside the workspace. M2.2 need
not expose preference UI beyond the **Edit Anyway** action.

### Manual save and optional autosave

Manual save is the default. Delayed autosave is an opt-in application preference
and uses a one-second idle delay. Autosave never resolves an external-change
conflict and never closes a recovery record until the save is confirmed.

## Architecture

### Hybrid domain core and native surface

M2.2 uses a hybrid architecture:

- `strukt-editor` owns document IDs, workspace-relative paths, canonical content,
  revisions, edit transactions, document-scoped history, dirty state, search,
  conflicts, recovery metadata, tab state, and serializable editor view state.
- `strukt-app` owns Iced editor widgets, focus, native clipboard interaction, IME,
  pointer hit testing, and rendering.
- A narrow adapter translates Iced actions into UI-independent cursor, selection,
  and edit transactions. Iced types do not cross into `strukt-editor`.
- `strukt-fs` provides capability-confined document reads, disk revisions, and
  staged save publication.
- `strukt-persistence` stores versioned editor layout and encrypted recovery
  envelopes in the platform application-data directory.
- `strukt-theme` supplies semantic editor, selection, gutter, conflict, and syntax
  colors.

The Iced text surface remains replaceable. Document tests do not instantiate Iced,
and replacing the widget must not change persistence or file-operation contracts.

### Document identity and revisions

Each open document has:

- a process-local `DocumentId` that is never reused;
- the owning `WorkspaceId`;
- a normalized workspace-relative path;
- a monotonically increasing in-memory revision;
- a disk revision captured at open or successful save;
- canonical text content and line-ending metadata;
- tab, dirty, read-only, conflict, missing, and recovery state;
- cursor, selection, scroll, and find state.

A disk revision includes stable metadata available on the platform plus a content
digest when needed. Background results carry the document ID and expected in-memory
revision. Results for a closed, replaced, or advanced document are discarded.

### Edit transactions and history

An edit transaction contains one or more non-overlapping range replacements in the
same document revision. This supports single-cursor editing now and multiple
cursors later. The domain core validates ordering and UTF-8 boundaries, applies the
transaction, records its inverse, advances the revision, and publishes new view
state.

Typing, deletion, paste, indentation, replace, undo, and redo all use this contract.
Adjacent compatible typing operations coalesce into one history entry. History is
bounded by both entry count and retained byte cost; the defaults are 10,000 entries
and 64 MiB per document. Eviction removes the oldest complete transaction without
changing current content.

The Iced surface performs cursor and pointer interaction. For edits, the adapter
captures the pre-edit selection, emits the matching domain transaction, applies the
native action, and checks the resulting cursor state. Undo, redo, bulk replace, and
external reload may rebuild the surface from the domain snapshot; ordinary typing
must not copy the whole document per keystroke.

### Grammar registry

A data-backed grammar registry maps file names, extensions, and explicit language
overrides to a syntax identifier. M2.2 ships support for Rust, JavaScript,
TypeScript, Python, JSON, TOML, Markdown, shell, YAML, HTML, CSS, and plain text.

The registry and syntax token model live outside `strukt-app`. The Iced adapter may
use its bundled highlighter implementation, but the document model consumes only
normalized token spans and never depends on a specific parser. Unknown or failed
grammars fall back to plain text. Highlighting work is cancelable, revision-bound,
and limited to the visible region plus a bounded look-ahead.

### Recovery key provider

A `RecoveryKeyProvider` contract supplies, creates, or deletes a per-user recovery
key through protected platform storage. Production adapters use the macOS Keychain,
Windows Credential Manager, and Linux Secret Service where available. Tests use an
in-memory provider. No key is stored beside its ciphertext.

Recovery envelopes are authenticated and include schema version, workspace ID,
document path, document revision, and save baseline. Writes are coalesced after two
seconds of inactivity and use atomic replacement with a last-valid fallback.

## User Experience

### Opening documents

Explorer selection and Quick Open emit the same `OpenDocument` command. The
application validates that the path belongs to the active workspace, reads it
through the retained capability, performs binary and size checks, and returns a
normalized open result.

The center canvas contains:

- a tab strip with file name, dirty indicator, conflict/missing status, and close;
- the editor or safe metadata/large-file preview;
- an optional find/replace bar;
- a compact status row for language mode, line ending, encoding, cursor position,
  recovery state, and large-file notices.

Opening an already-open path focuses the existing document instead of creating a
duplicate.

### Editing and navigation

M2.2 supports:

- Unicode insertion and IME composition;
- mouse and keyboard cursor movement and selection;
- copy, cut, paste, select all, undo, and redo;
- line numbers, indentation and unindentation;
- configurable visible whitespace;
- matching-bracket indication;
- document find and replace with case-sensitive, whole-word, and regular-expression
  modes;
- next and previous match navigation;
- syntax highlighting and a manual language-mode override.

Platform-standard shortcuts use `Cmd` on macOS and `Ctrl` on Windows and Linux.
Every command is also addressable through the application command model so a later
command palette or plugin can invoke it without synthesizing keys.

### Saving and closing

`Cmd/Ctrl+S` submits the current content, expected disk revision, line-ending mode,
and target path to the capability-confined save adapter. A successful save updates
the disk baseline, clears dirty and conflict state, and deletes the corresponding
recovery payload.

Closing a dirty tab presents **Save**, **Discard**, and **Cancel**. Discard is
explicit and deletes recovery content only after the tab closes. Closing the
application presents one consolidated list of dirty documents rather than a stack
of independent dialogs.

### External changes

Watcher events never mutate editor content directly. They request a bounded disk
revision check and then emit one normalized document event:

- a clean document changed on disk reloads automatically and preserves the nearest
  valid cursor and scroll position;
- a dirty document changed on disk keeps local content and enters conflict state;
- **Compare** opens a read-only local-versus-disk comparison in the center canvas;
- **Reload from Disk** replaces local content only after explicit confirmation and
  creates an undo boundary;
- **Keep Editing** acknowledges the conflict but does not change the save baseline;
- a deleted or moved path remains open, pinned, dirty, and recoverable, with Save As
  reserved for a later file-dialog refinement if the original path cannot return.

The save adapter rechecks the expected disk revision immediately before publishing
the staged file. A known external change returns `SaveConflict` instead of
overwriting. Filesystems do not provide a universal compare-and-swap primitive, so
an uncooperative writer can still race the final publication window; the adapter
rechecks after publication and reports a new conflict if observed. strukt never
silently proceeds after a change it has detected.

### Restoration

Workspace persistence records pinned document paths, the current preview path,
active tab, tab order, cursor, selection, scroll, find settings, language override,
and read-only choice. On restart, clean documents reload from disk. Recovery-backed
documents restore encrypted unsaved content and clearly identify that state.

Missing files restore as recoverable missing documents only when recovery content
exists; otherwise the tab shows a removable missing-file placeholder. Restoration
is asynchronous and cannot block the native window from appearing.

## Failure Handling

- **Read denied or path escaped:** keep the current editor state and show a scoped
  open error; never retry through an ambient path.
- **Binary file:** show metadata view, not malformed text.
- **Oversized file:** show bounded read-only preview and explicit override.
- **Invalid UTF-8:** show a safe encoding error and metadata; lossless alternate
  encodings are deferred.
- **Save conflict:** preserve local content and require Compare, Reload, or explicit
  Force Save.
- **Save IO failure:** preserve dirty state and recovery; allow retry.
- **External deletion or move:** preserve content as a pinned recoverable document.
- **Grammar failure or excessive line:** fall back to plain text.
- **Recovery key unavailable:** disable recovery visibly; never store plaintext.
- **Recovery corruption or authentication failure:** ignore the invalid payload,
  retain the last valid envelope when present, and show a non-destructive warning.
- **Stale task completion:** discard it by document ID and revision.
- **History budget exhausted:** evict oldest complete entries and continue editing.

## Security and Privacy

- All workspace reads and writes use retained capabilities and normalized relative
  paths. Editor commands never accept an ambient absolute target.
- Symlink and workspace-root replacement checks follow the M2.1 authority model.
- Windows staged publication uses one narrowly audited platform adapter around
  `SetFileInformationByHandle`; it renames the already-open staging handle relative
  to the retained destination-directory handle. All other workspace crates retain
  the workspace-wide unsafe-code prohibition.
- Recovery content stays outside the repository and is encrypted and authenticated
  with a platform-protected key.
- Clipboard operations occur only after explicit user commands.
- Syntax definitions are bundled data and do not execute workspace code.
- Opening a file never executes hooks, formatters, language servers, tasks, or shell
  commands.
- Recovery and history bounds prevent an opened workspace from forcing unbounded
  application-data growth.

## Verification Strategy

### Unit tests

- document identity, revisions, UTF-8-safe transactions, and inverse edits;
- undo/redo coalescing and history eviction;
- preview replacement and promotion to pinned tabs;
- dirty, read-only, missing, conflict, and recovery transitions;
- find/replace modes and zero-width regular-expression behavior;
- grammar mapping, token fallback, and stale highlight rejection;
- recovery envelope authentication, versioning, and key-unavailable behavior.

### Contract tests

- capability-confined reads and staged saves;
- path traversal, symlink replacement, and workspace-root replacement rejection;
- expected disk revision and `SaveConflict` behavior;
- atomic-save failure preserves the prior valid file;
- binary detection, normal-edit threshold, bounded preview, and explicit override;
- recovery persistence never creates workspace-local metadata.

### Integration tests

- explorer and Quick Open open the same document once;
- preview replacement, edit promotion, tab closing, and dirty confirmation;
- save followed by watcher notification does not create a false conflict;
- clean external change reloads while a dirty change enters conflict;
- restart restores tabs, active document, view state, and unsaved recovery;
- workspace replacement cancels stale open, save, highlight, and recovery tasks;
- syntax highlighting remains usable without a language server.

### Native and hosted validation

- macOS manual walkthrough covers file opening, tabs, editing, shortcuts, clipboard,
  IME, focus, accessibility, save/conflict flows, recovery, and large files.
- Hosted macOS, Ubuntu, and Windows jobs build the native application and run a
  deterministic editor smoke that opens, edits, saves, restores, and verifies a
  temporary fixture.
- Windows hosted tests cover path, line-ending, atomic-save, protected-key-provider
  contract, and native startup behavior; human Windows IME and accessibility remain
  public-alpha gates if no Windows development machine is available.
- Stress tests cover the 4 MiB normal limit, explicit larger-file editing, long
  lines, repeated edits, rapid watcher events, and background highlighting while
  input remains responsive.

## Acceptance Criteria

M2.2 is complete when:

1. Explorer and Quick Open open real text files into one preview slot, and editing,
   double-clicking, or pinning makes a tab permanent.
2. A user can edit Unicode text, select, copy, cut, paste, indent, undo, redo, find,
   replace, and navigate matches using native keyboard and pointer interaction.
3. Manual save uses the retained workspace capability, reports failures, detects
   known external changes, and never creates repository metadata.
4. Clean external changes reload; dirty changes, moves, and deletions preserve local
   content and expose explicit resolution actions.
5. Bundled syntax highlighting works for the listed languages and falls back safely
   to plain text without a language server.
6. Binary and oversized files cannot force unbounded editor allocation without
   explicit user consent.
7. Restart restores tab and view state, and encrypted unsaved recovery restores
   content when protected key storage is available.
8. The domain editor, save, recovery, integration, native smoke, and cross-platform
   gates pass with no unresolved critical or important review findings.
9. The implementation does not include LSP, terminals, remote workspaces, editor
   splits, or a custom GPU text renderer.

Completing M2.2 does not complete M2. The local terminal, language intelligence,
integration/restoration, and final Iced revalidation slices remain.

## Related Artifacts

- Parent M2 spec: [`0003-local-development-workspace.md`](0003-local-development-workspace.md)
- M2.1 plan: [`../plans/0003-m2-workspace-files.md`](../plans/0003-m2-workspace-files.md)
- M2 roadmap: [`../roadmap.md`](../roadmap.md)
- Workspace shell reference:
  [`../mockups/workspace-shell/focus-context.html`](../mockups/workspace-shell/focus-context.html)
