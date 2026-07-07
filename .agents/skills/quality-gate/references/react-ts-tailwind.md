# React / TypeScript / Tailwind Quality Reference

Use this reference when validating frontend behavior, state, styling, or TypeScript contracts.

## Checks

- Run the repo's frontend build command, usually `npm run build`, before signoff.
- Run targeted unit/component tests if the repo provides them.
- Run lint/typecheck commands when present; do not invent new scripts unless asked.
- Use browser/E2E verification for user-visible flows, responsive layout, media playback, drag/drop, dialogs, or stateful controls.

## Review Points

- Keep TypeScript API types aligned with backend command payloads and events.
- Centralize event listeners when the app architecture requires it; guard React StrictMode double registration.
- Avoid native `<form>` submit flows in apps that standardize on explicit `onClick` / `onChange` control.
- Prefer design tokens and Tailwind theme values over one-off colors and spacing.
- Confirm text fits at supported viewports and interactive states are visible, disabled, loading, and error-aware.
