## 2025-01-24 - Project Structure and Tooltip Limitations

**Learning:** The UI implementation in this project is currently centered in `main.rs`, with some components in `crates/roxy-ui/src/components/` being unused (e.g., `toolbar.rs`). Additionally, the `.tooltip()` method on GPUI elements was unavailable or incompatible with the current environment's trait scope.

**Action:** When modifying the UI, prioritize checking `main.rs` for active render logic before assuming component files are in use. Use keyboard shortcut hints in button labels as a fallback for missing tooltips.
