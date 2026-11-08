# Annotation Tool (Rust / GPUI)

A GPU-accelerated port of the [Tkinter Annotation-Tool](../Annotation-Tool) to Rust with
[GPUI](https://www.gpui.rs) — the same YOLO annotation workflow, refined themes, real
animations and drop-in compatibility with the Python tool's data.

![Rust](https://img.shields.io/badge/rust-1.98+-orange)
![GPUI](https://img.shields.io/badge/gpui-0.2.2-blue)

## Highlights

- **Same workflow, faster canvas** — the image is decoded once and sampled on the GPU;
  zoom/pan is a pure transform (no CPU rescaling), so it stays smooth at any zoom.
- **Drop-in compatible** — YOLO `.txt` files (6-decimal format), `data/*.txt` class
  presets, `session.json` and `config.json` (Tk keybinding syntax included) are all
  read/written byte-compatible with the Python tool. Point `ANNOTATION_TOOL_HOME` at the
  Python checkout and both apps share the same live state.
- **Light and dark themes** with shared controls and non-blocking notifications.
- **Everything ported** — draw/stamp/move/resize boxes, multi-select, undo/redo
  (snapshot semantics identical to the original, quirks included), copy/paste/duplicate,
  box presets (tap/hold Ctrl+1–9), image filters, class editor with file remapping +
  timestamped backups, batch ID replace, validation with live toolbar indicator,
  filmstrip thumbnails, recent projects, lowres export, configurable keybindings,
  help overlay.

## Building

Requires the Rust MSVC toolchain (Visual Studio C++ Build Tools + Windows SDK).

```
cargo run --release
```

## App home

Session/config/classes resolve from (in order):

1. `ANNOTATION_TOOL_HOME` environment variable
2. `--home <dir>` command-line flag
3. the executable's directory (release) or the crate root (debug)

To share state with the Python tool:

```
set ANNOTATION_TOOL_HOME=D:\path\to\Annotation-Tool
```

## Keyboard shortcuts (config.json defaults)

| Action                    | Shortcut               |
| ------------------------- | ---------------------- |
| Next / Previous Image     | `→` / `←`              |
| Cycle Class               | `↓`                    |
| Delete Box                | `Delete`               |
| Copy / Paste Boxes        | `Ctrl+C` / `Ctrl+V`    |
| Duplicate Boxes           | `Ctrl+D`               |
| Edit Box Class            | `Ctrl+E`               |
| Undo / Redo               | `Ctrl+Z` / `Ctrl+Y`    |
| Select All                | `Ctrl+A`               |
| Zoom In / Out / Fit       | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` or wheel |
| Apply / Preview Preset    | tap / hold `Ctrl+1-9`  |
| Save Preset Slot          | `Ctrl+Shift+1-9`       |
| Unannotated Filter        | `U`                    |
| Help Overlay              | `?`                    |
| Deselect Class            | `Escape`               |

All bindings are customizable in Settings → Keybindings (stored in `config.json` using
the original Tk syntax, so both tools read them).

## Project panel

- **Saved boxes** replaces the project progress bar. Each class shows its saved box
  count, including zero-count classes. The summary shows total boxes and classes used.
- Counts cover label files that match images in the open folder. Unrelated label
  files in the output folder do not contribute. Unsaved edits are not counted.
- Click a class count to show images with that class. Use **Clear filters** to restore
  all images, or **Show unannotated only** to find images without saved boxes.
- Use **Refresh** after another program changes label files. The panel shows loading
  and read errors instead of reporting an incomplete scan as zero boxes.
- The file list shows visible and total image counts. The toolbar has **Previous**,
  **Next**, the current image position, and **Fit image** controls.

Export and class-remapping operations still show task progress.


## Differences from the Python tool

- Zooming re-samples the decoded texture on the GPU instead of the CPU
  proxy/pre-render pipeline (`accel.py` is obsolete here); extremely deep minification
  may alias slightly where the original blurred.
- Blocking message boxes for transient messages are replaced with toasts;
  destructive confirmations remain modal.
- Filmstrip/canvas use free panning rather than scrollbars.

## License

MIT, like the original.
