## 2025-05-15 - Persistent Clear Pattern in Polling UIs
**Learning:** In applications that use polling to update UI state from a backend (like ClickHouse), a simple client-side clear of state vectors is insufficient as the next poll will immediately repopulate the data. A "clear timestamp" must be tracked in the application state and used to filter incoming messages.
**Action:** Implement a `clear_ts` field in state, update it on clear actions, and use `retain()` or `filter()` in message handlers to ignore records with older timestamps.
