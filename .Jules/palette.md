## 2024-05-23 - Persistent Clear Pattern in Polling UIs
**Learning:** In polling-based networked UIs, simple local state clearing is insufficient as background pollers will immediately repopulate the UI with old data from the backend. A `clear_timestamp` must be tracked and used to filter incoming data.
**Action:** Always implement a timestamp-based filter when adding "clear" functionality to views that poll a persistent store.

## 2024-05-23 - GPUI Keyboard Accessibility
**Learning:** Adding `tab_index(0)` and focus styles makes an element focusable but doesn't automatically handle activation. Keyboard listeners for Enter and Space must be explicitly added to maintain parity with mouse interactions.
**Action:** Pair `tab_index(0)` with `on_key_down` handlers to ensure keyboard-only users can trigger actions.
