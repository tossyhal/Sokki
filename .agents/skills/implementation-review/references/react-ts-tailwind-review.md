# React / TypeScript / Tailwind Review Reference

Use for frontend components, state stores, events, styling, and user-visible flows.

## Review Checklist

- Event subscriptions are registered once, cleaned up when needed, and safe under StrictMode.
- Store updates preserve existing state and handle async races, stale responses, and errors.
- UI has loading, empty, disabled, success, warning, and error states where the workflow needs them.
- TypeScript types match backend payloads and do not rely on unchecked `any` for boundary data.
- Tailwind classes follow project tokens; accent colors and custom styles are not overused.
- Text and controls fit supported viewport sizes; dialogs and menus remain reachable.
- User actions use accessible buttons/inputs and clear focus/keyboard behavior when relevant.
