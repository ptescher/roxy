## 2025-05-14 - Persistent Clear Pattern for Polling UIs
**Learning:** In a UI that constantly polls a backend (like ClickHouse) for the latest data, simply clearing the local state is insufficient as the next poll will repopulate it. A `clear_timestamp` should be stored in the app state and used to filter incoming data.
**Action:** Always implement a `clear_timestamp` filter when adding 'Clear' functionality to polling-based networked applications.

## 2025-05-14 - GPUI 0.2.x Downcasting and Actions
**Learning:** `AnyView::downcast::<V>()` returns `Result<Entity<V>, AnyView>` instead of `Option`. Also, type inference in nested `update` closures can sometimes fail, requiring explicit variable naming or type annotations.
**Action:** Use `if let Ok(entity) = view.downcast::<T>()` for safe downcasting in GPUI 0.2.x.
