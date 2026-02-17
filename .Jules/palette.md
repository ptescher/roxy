## 2025-01-24 - [Persistent Clear Pattern]
**Learning:** In networked/polling UIs, clearing the local state is insufficient as the next poll will likely repopulate the same data. A more robust UX pattern is to track a `clear_timestamp` and filter out data older than this point, effectively "resetting" the session for the user without necessarily truncating the backend database.
**Action:** Always implement local filtering by timestamp when implementing "Clear" in polling-based interfaces.
