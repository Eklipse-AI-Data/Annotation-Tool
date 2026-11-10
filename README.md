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
  filmstrip thumbnails, recent projects, configurable keybindings,
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
| Save Annotations          | `Ctrl+S`               |
| Select All                | `Ctrl+A`               |
| Zoom In / Out / Fit       | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` or wheel |
| Apply / Preview Preset    | tap / hold `Ctrl+1-9`  |
| Save Preset Slot          | `Ctrl+Shift+1-9`       |
| Unannotated Filter        | `U`                    |
| Help Overlay              | `?`                    |
| Auto Label current image  | `Ctrl+Shift+A`         |
| Deselect Class            | `Escape`               |

All bindings are customizable in Settings → Keybindings (stored in `config.json` using
the original Tk syntax, so both tools read them).

## Auto Label (LLM few-shot labeling)

The **Auto Label** tab (sparkles button in the toolbar) lets an LLM label a folder from a
few examples you label by hand. It talks to any OpenAI-compatible endpoint; to use a
**Codex / ChatGPT subscription** it goes through [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI),
which the app can run for you: it starts when the app opens and stops when the app closes
(it is tied to the app process, so it also stops if the app crashes).

1. Download the CLIProxyAPI Windows release (`CLIProxyAPI_<version>_windows_amd64.zip`) and
   put `cli-proxy-api.exe` in `cliproxy\` next to the app (or anywhere, and browse to it).
2. **Settings → Auto Label → CLIProxyAPI**: tick **Start CLIProxyAPI when the app opens…**,
   click **Sign in to Codex…** and finish the ChatGPT login in the browser (once; the login
   is kept in `%USERPROFILE%\.cli-proxy-api`).
3. Keep the endpoint `http://127.0.0.1:8317/v1`, click **Test connection**, pick a model,
   **Save & Close**.

The app writes the proxy's config itself (`cliproxy\config.yaml`: loopback only, the port
from the endpoint URL, the API key field as the client key, generated if empty) and logs to
`cliproxy\cli-proxy-api.log`. To use your own `config.yaml`, set **Proxy config**; then
the endpoint URL and API key must match it. If something is already listening on the port,
the app leaves it alone.

Workflow:

- **Target classes** — the classes the model should find. A class named like the image
  folder (`images/triple_kill` → `triple_kill`) is ticked automatically.
- **Labeling guide** — **Import guide…** takes your "what to label" doc (a PDF of slides,
  or annotated screenshots). Pages are rendered with Windows' built-in PDF renderer; the
  model then tags which classes each page explains (one small request per page) and fills
  any empty class hint from it. Pages that explain nothing (title, tables, blank) are
  dropped. From then on every request includes the guide page(s) for the target classes.
  Stored in `data/examples/<preset>/guide/`; **Add more pages…** / **Remove** manage it.
- **Examples & hints** — per class, **Pick from folder** adds frames you already labeled
  that contain the class (spread across the folder, up to "Examples per class"); **Add
  current** adds the open image as labeled (an image with none of that class's boxes is a
  *negative* example). The one-line hint per class can be edited. Examples are stored per
  class preset in `data/examples/<preset>/<class>/`, so they're reused by every folder that
  uses that preset (the preset is detected from `data/*.txt`).
- **Test on current image** — shows the model's boxes dashed on the canvas; **Accept** or
  **Discard**. Use this to tune examples before a batch.
- **Auto-label unlabeled images** — labels every image in the folder that has no label
  file yet (existing labels are never touched; a backup is made first). Progress can be
  cancelled. Results are written as normal YOLO `.txt` files and flagged **AI** in the
  filmstrip/file list via `.autolabel.json` in the image folder.
- **Review** — **Next to review** jumps to the next AI-labeled image; fix the boxes if
  needed and press `Ctrl+S` to accept (editing + autosave also counts). Images where the
  model found nothing get no label file; **Retry empty/failed** re-queues them.

Settings (`auto_label` in `config.json`): endpoint, key, model, optional
`reasoning_effort`, parallel requests (default 3), max image side sent (default 1280px)
and examples per class (default 3). LLM boxes are approximate — always review them.

## Saving

- With **Auto Save** on, the current image is written when you move to another image,
  every 30 seconds, and on close.
- With Auto Save off, press `Ctrl+S` (or the toolbar save button). Unsaved edits show as a
  dot on the save button, a `•` in the window title and a warning in the status bar.
  Switching image, folder, filter or class preset, or closing the window, asks
  **Save / Discard / Cancel** first.

## Project panel

- **Saved boxes** replaces the project progress bar. Each class shows its saved box
  count, including zero-count classes. The summary shows total boxes and classes used.
- Counts cover label files that match images in the open folder. Unrelated label
  files in the image folder do not contribute. Unsaved edits are not counted.
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
