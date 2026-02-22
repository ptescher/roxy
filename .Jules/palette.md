## 2025-02-12 - Persistent Clear Pattern in Polling UIs
**Learning:** In networked UIs that poll backend storage (like ClickHouse) for real-time updates, a simple `clear()` of local state is insufficient because the next poll will immediately repopulate the UI with old data.
**Action:** Implement a `clear_timestamp` in the application state. When clearing, set this timestamp to `now()`. Filter all incoming polled data to only include records with `timestamp >= clear_timestamp`. This ensures the "Clear" action feels persistent to the user without requiring destructive backend API calls.

## 2025-02-12 - GPUI 0.2.2 Accessibility and Closure Patterns
**Learning:** GPUI 0.2.2 requires explicit type annotations for context parameters in `update` and `on_action` closures (e.g., `cx: &mut Context<View>`) when type inference fails. Additionally, `gpui::Div` in this version lacks an `.active()` method, and keyboard accessibility must be manually wired using `.tab_index(0)` and `.on_key_down()`.
**Action:** Always provide explicit types for `cx` in complex closures to avoid compiler errors. Use `.hover()` for visual feedback and ensure `on_key_down` handles `Space` and `Enter` for focusable elements.
