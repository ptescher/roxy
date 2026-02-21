## 2026-02-21 - Persistent Clear Pattern in Polling UIs
**Learning:** In applications that poll backend storage (like ClickHouse) for real-time data, simply clearing the UI's local state is insufficient because the next poll will re-populate the UI with the same old data. A "persistent clear" must be implemented by tracking a `clear_timestamp` in the application state and filtering out any incoming records with a timestamp older than this value.
**Action:** Always implement timestamp-based filtering when adding "Clear" functionality to polling-based monitoring tools to ensure a consistent and flicker-free user experience.

## 2026-02-21 - GPUI Keyboard Accessibility
**Learning:** GPUI components (as of v0.2.2) do not automatically handle keyboard focus or ARIA labels. Interactive elements implemented as `div()` blocks require explicit `.tab_index(0)` to be focusable and `.focus(|style| { ... })` to provide visual feedback for keyboard navigation. Additionally, when native tooltips are unavailable, adding keyboard shortcut hints directly to labels (e.g., "Clear (Cmd+K)") is an effective way to improve discoverability.
**Action:** Ensure all custom interactive elements have a defined tab index and visual focus state. Use explicit shortcut hints in labels to aid power users and improve overall accessibility.
