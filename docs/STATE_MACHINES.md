# State machines

This app is mostly event-driven UI, so much of its logic lives in states and
transitions. This page lists every one of them: where it lives, what the
states mean, and which functions may change it. When you add a mode, a busy
flag or an async result, give it an entry here (and prefer an `enum` to a
new `bool`).

**Async rule used everywhere:** a result that arrives later must prove it is
still wanted before it is applied. It does that with a token captured when
the work started: a generation counter, an epoch or a path compared on
arrival.

## Explicit (enum-backed)

| Machine | Where | States | Transitions |
|---|---|---|---|
| Canvas image | `ImageLoad` in `ui/canvas.rs` | `Empty`, `Loading{path}`, `Ready{image}`, `Failed{path,error}` | `Canvas::request_image`, `Canvas::clear_image`, decode callback (applies only if still `Loading` the same path) |
| Canvas gesture | `Gesture` in `ui/canvas.rs` | `None`, `Drawing`, `Moving`, `Resizing`, `Panning` | mouse down/move/up, `on_mouse_up_out`; reset on any image change; a right press is ignored during a left gesture; a missed release (no button held on move) finishes the gesture |
| Filmstrip pointer | `Pointer` in `ui/filmstrip.rs` | `Idle`, `Hovering{slot}`, `Pressed{..}`, `Dragging{..}` | `on_down` → `Pressed`; moving more than 4px → `Dragging`; `on_up` on `Pressed` = click; `on_up_out` / missed release → back to `Idle`/`Hovering` |
| Thumbnail decode | `ThumbLoad` in `ui/filmstrip.rs` (Ready = present in `ImageStore`) | not requested, `Loading`, `Failed` | `start_visible_loads` (visible slots only, within `thumb_budget()`); completion; `Filmstrip::clear` bumps `epoch` |
| Decode lanes | `Lane` in `images.rs` | `Current`, `Preload`, `Thumb`, each with a semaphore and a generation | `Lane::supersede` drops queued work; `begin_preload_batch` once per navigation |
| GPU texture lifetime | `ImageStore::{retire, collect}` in `images.rs` | cached → retired → released | LRU eviction / `clear_all` → `retire`; `collect` releases it once nothing holds the image and one cycle has passed |
| Preset key | `PresetKey` in `ui/workspace.rs` | `Idle`, `Pressed{slot,ticket}`, `Previewing{slot}` | `on_root_key_down` / `preset_hold_elapsed(ticket)` / `on_root_key_up` / `on_root_modifiers_changed` in `ui/workspace_actions.rs`; blocked while any dialog is open |
| Background task | `Workspace::{begin_task, end_task}` in `ui/workspace.rs` | Idle ⇄ Running (`task_running` + `Modal::Progress`) | only `begin_task` / `end_task` change them; `alert()` queues instead of replacing Progress |
| Dialog slot + alert queue | `Workspace::modal` and `queued_alerts` | one `Modal` or none, plus a FIFO of waiting alerts | `alert()` queues while Progress, Alert or Confirm is up; OK, Confirm buttons, the backdrop and Escape call `show_next_queued_alert` |
| Escape | `Workspace::escape_cascade` | closes the top layer: Settings → project popovers → Auto Label popovers → Alert/Confirm/EditClass → help → validation → else deselect | raw Escape **and** the configurable `DeselectClass` action |
| Model list fetch | `ModelFetch` in `ui/autolabel_ui.rs` | `NotLoaded`, `Loading`, `Loaded`, `Failed(msg)` | `refresh_models` (generation-checked); `ensure_models` also retries after `Failed` |
| Proxy process | `ProxyState` in `autolabel/proxy.rs` | `Off`, `Starting`, `Running{pid}`, `External`, `Failed` | `start` (its decide/stop/spawn section is serialized by the `START` lock), `stop`, `state()` (notices a child that exited) |
| Review status | `review::Status` in `autolabel/review.rs` | (none), `Pending`, `Empty`, `Error` | batch pump `set`; `mark_reviewed` |

## Keyboard routing (key contexts)

The workspace root sets key context `Workspace`, text fields set `TextInput`
or `TextArea`, and the Settings panel sets `Settings`. Every workspace
shortcut is bound with `actions::WORKSPACE_SHORTCUTS`
(`Workspace && !TextInput && !TextArea && !Settings`).

In GPUI, a binding that matches **consumes** the keystroke, even if its
handler then returns early. A context-free `u` binding therefore ate every
"u" typed into a text field. Scope a shortcut with a context; don't guard it
inside the handler. `apply_keymap` clears the keymap before rebinding.

## Still implicit (candidates for the next pass)

- **Current image**: `AppState::current_image_index: i64` uses `-1` to mean
  "no image". It is also set to `-1` on purpose to make `load_image` skip
  autosave (`finish_class_changes`, `autolabel_finish_batch`, filter scan).
  About 46 call sites read it. The fix is an
  `enum CurrentImage { None, Open { index } }` plus an explicit
  `load_image_without_autosave`.
- **Undo history** is not cleared on image change. This matches the Python
  tool on purpose ("quirks included").
- **Image filter**: `Workspace::filter_class_id` holds both "class picked in
  the dropdown" and "filter applied". `unannotated_filter_active` lives in
  `AppState`. These belong in one
  `enum ImageFilter { None, Scanning(kind), Class(id), Unannotated }`.
- **Save errors** are reported through `AppState::class_counts_error`. A unit
  test pins this behavior, and it is how autosave failures become visible.
- **Popovers**: `recents_open`, `filter_open` and the Auto Label
  `model_open`/`reasoning_open` are separate bools sharing one
  `popover_closed_at` debounce.
- **Proxy UI**: render calls `proxy::state()` and filesystem probes on
  every frame.
