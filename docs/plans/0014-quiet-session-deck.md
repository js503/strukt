# Quiet Session Deck implementation plan

Goal: implement approved spec 0012 incrementally in the existing dirty branch.
Architecture: presentation stays in strukt-app; shared text/control styling in
strukt-ui. Existing lifecycle messages and confirmation semantics remain intact.
Tech stack: Rust, iced, native session providers.

Execution is local, without delegates. Preserve unrelated edits.

1. Add regression tests in visual_foundation_contract.rs for explicit application
   typography, monospace editing, and opt-in management controls. Run them red.
2. Set application default text size to 13; explicitly size shared list/button
   labels and use Font::MONOSPACE in local and remote editors.
3. Add session_tools_visible and ToggleSessionTools in app.rs, default false;
   wrap the existing management/input/provider forms behind this state, leaving
   errors and confirmations visible. Keep one compact action row.
4. Add compact editor composition in view/mod.rs and route view/editor.rs drawer
   through it. Combine tabs, overflow, and placement actions into one header.
5. Make status text distinguish operation errors from provider availability;
   trace provider failures before changing lifecycle code.
6. Run focused tests, fmt, clippy, semantic guard; build and inspect native app.
7. Record evidence and remaining interaction/motion checks. Do not declare the
   full polish milestone complete from source guards or screenshots alone.

Progress: implementation started; visual and interaction acceptance pending.
