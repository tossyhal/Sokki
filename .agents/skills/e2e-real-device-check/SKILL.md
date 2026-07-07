---
name: e2e-real-device-check
description: "Use for user-visible desktop or web flows that need real interaction verification: Playwright checks, Tauri dev runs, screenshots, visual QA, audio/media playback, Windows device checks, WebView asset protocol, installers, GPU/device behavior, and any change whose correctness cannot be proven by unit tests alone."
---

# E2E Real Device Check

Use this skill to validate behavior through the same surface a user sees. Keep a short QA inventory and map every final claim to evidence.

## Workflow

1. Build a QA inventory from the user request, visible controls changed, and claims planned for the final response.
2. Run automated tests first when available; E2E should complement, not replace, unit checks.
3. Start or reuse the dev server/app using the repository's documented command.
4. Use `$playwright` for browser flows, screenshots, and DOM-visible verification.
5. For Tauri or native desktop behavior, run the Tauri app when feasible and verify the actual WebView/window behavior.
6. For device-dependent features, define the exact hardware/OS prerequisite and mark unverifiable items clearly if the environment lacks it.
7. Capture evidence only after the UI is in the intended state.

## Desktop And Device Checks

- Verify launched window size, minimum size, navigation, dialogs, and media playback in the real shell when relevant.
- For audio/video features, check actual recording/playback paths, permissions, levels, and failure states.
- For local files in WebView, verify the app returns backend-generated absolute paths and the UI uses the expected conversion/protocol path.
- For installers, validate build artifact existence and at least one install/launch path when environment allows.

## Signoff

Report what was verified, the environment used, and what could not be verified. Do not claim real-device coverage for checks that only ran in a mocked browser.
