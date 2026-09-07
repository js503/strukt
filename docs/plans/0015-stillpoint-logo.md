# Stillpoint logo D

Approved: user selected D and requested implementation on 2026-09-05.
Supersedes the Bolt Structure geometry/motion in spec 0011 only.

Goal: match the selected cel-shaded cube with an inset, recognizable bolt on
the right face and subtle inner pulse. Keep the 29px mark/42px header slot.

Local execution; no delegated work or changes to activity icons.

1. Add geometry regression for the six-point closed inset bolt and tiny node.
2. Replace the wireframe rendering with three theme-derived flat faces,
   sharp silhouette, seams, inset dark channel and single rim highlight.
3. Remove traveling current; retain a stationary smooth pulse, idle intervals,
   focus pausing and zero animation redraws for reduced motion.
4. Run UI tests, app build and clippy; launch and inspect a native capture.

All colors derive from semantic theme tokens. No new timers or dependencies.

Implemented: native filled faces, closed inset bolt, rim, and stationary pulse.
Eight UI tests passed, UI clippy passed, app built and launched. Native rest-frame
capture inspected: [Stillpoint D](../evidence/m5-5-stillpoint-d.png).
Existing 8.4-second cadence retained; pulse replaces traveling current. Focus and
reduced-motion behavior remains tested; no live animation recording was made.
