# Vendored egui-baseview 0.9.0

A copy of [egui-baseview](https://github.com/BillyDM/egui-baseview) 0.9.0 from crates.io
(MIT OR Apache-2.0, licence files kept here), used through `[patch.crates-io]` in the
workspace `Cargo.toml`.

## Why

egui-baseview 0.9.0 ignores baseview's drag-and-drop events, so files dropped on a plugin
window never reach egui. SolPlugs Samples imports folders dropped on its window.

## What changed

Only file drag-and-drop:

- `src/window.rs`: `DragEntered`/`DragMoved` set egui's `hovered_files` and accept the drop
  as a copy; `DragLeft` clears them; `DragDropped` adds the files to `dropped_files`.
- `src/dropped.rs` (new): `PathFile`, egui's `DroppedFile` for a path.
- `Cargo.toml`: examples removed (their files aren't vendored).

Drop this copy when an upstream release forwards drops.
