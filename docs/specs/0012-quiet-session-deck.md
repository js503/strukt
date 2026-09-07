# Quiet Session Deck

Status: Approved by the user on 2026-09-05.

North star: quiet by default, terminal-first, consistent native typography,
details on demand. Refines spec 0011 without replacing session/editor lifecycles.

- Use native system UI type at 13px, metadata at 11–12px, explicit monospace
  for editable code and terminals. Avoid inherited 16px controls.
- Supporting editor uses one header with tabs, actions, and placement controls;
  do not repeat its path in multiple stacked bars.
- Session management is opt-in, not a permanent form below the terminal.
  Preserve all actions, capability checks, and destructive confirmations.
- Provider health and operation errors must not give contradictory success
  feedback. Keep the actual error visible; investigate lifecycle failures.
- Use one selected leaf in session navigation and restrained separators.
- Motion must be subtle and respect reduced motion; no decorative perpetual
  movement beyond the approved brand animation. Verify layout before adding motion.

Acceptance: focused tests, native captures, retained editor/session controls,
and no claims of full visual completion until interactive checks are complete.
