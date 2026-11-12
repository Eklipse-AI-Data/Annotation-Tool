//! GPUI action definitions and keymap construction from config.json.
//! Configurable actions are loaded through the Tk→GPUI keystroke converter;
//! fixed shortcuts mirror the Python tool's hardcoded bindings.

use gpui::{actions, KeyBinding};

use crate::state::config::{Config, BINDABLE_ACTIONS};

actions!(
    annot,
    [
        // Configurable via config.json
        DeselectClass,
        NextImage,
        PrevImage,
        CycleClass,
        DeleteBox,
        CopyBoxes,
        PasteBoxes,
        DuplicateBoxes,
        EditBoxClass,
        UndoAction,
        RedoAction,
        // Fixed bindings
        SelectAllBoxes,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        ToggleHelpOverlay,
        ToggleUnannotatedFilter,
        ToggleLeftSidebar,
        ToggleRightSidebar,
        ToggleFilmstrip,
        OpenImages,
        SaveAnnotations,
        AutoLabelCurrent,
    ]
);

/// Key context set on the workspace root element.
pub const WORKSPACE_CONTEXT: &str = "Workspace";
/// Key context set on the Settings panel (it captures raw keystrokes when
/// rebinding shortcuts).
pub const SETTINGS_CONTEXT: &str = "Settings";

/// Predicate for every workspace shortcut: active anywhere in the workspace
/// *except* while a text field has focus or the Settings panel is capturing
/// keys. Scoping by context (instead of guarding inside the action handler)
/// matters: a binding that matches consumes the keystroke, so a global `u`
/// binding used to swallow every "u" typed into a text field even though the
/// handler then did nothing. Unmatched keystrokes fall through to text input.
pub const WORKSPACE_SHORTCUTS: &str = "Workspace && !TextInput && !TextArea && !Settings";

/// Builds all keybindings: config-driven actions plus fixed shortcuts.
pub fn build_keymap(config: &Config) -> Vec<KeyBinding> {
    let mut bindings = Vec::new();
    let ctx = Some(WORKSPACE_SHORTCUTS);

    macro_rules! bind_config {
        ($action_id:expr, $action:path) => {
            if let Some(keys) = config.gpui_binding($action_id) {
                bindings.push(KeyBinding::new(keys.as_str(), $action, ctx));
            }
        };
    }

    bind_config!("deselect", DeselectClass);
    bind_config!("next_image", NextImage);
    bind_config!("prev_image", PrevImage);
    bind_config!("cycle_class", CycleClass);
    bind_config!("delete_box", DeleteBox);
    bind_config!("copy", CopyBoxes);
    bind_config!("paste", PasteBoxes);
    bind_config!("duplicate", DuplicateBoxes);
    bind_config!("edit_class", EditBoxClass);
    bind_config!("undo", UndoAction);
    bind_config!("redo", RedoAction);

    // Fixed shortcuts (Python hardcoded)
    for keys in ["ctrl-=", "ctrl-plus", "ctrl-+"] {
        bindings.push(KeyBinding::new(keys, ZoomIn, ctx));
    }
    for keys in ["ctrl--", "ctrl-minus"] {
        bindings.push(KeyBinding::new(keys, ZoomOut, ctx));
    }
    bindings.push(KeyBinding::new("ctrl-0", ZoomReset, ctx));
    bindings.push(KeyBinding::new("?", ToggleHelpOverlay, ctx));
    bindings.push(KeyBinding::new("shift-/", ToggleHelpOverlay, ctx));
    bindings.push(KeyBinding::new("u", ToggleUnannotatedFilter, ctx));
    bindings.push(KeyBinding::new("ctrl-a", SelectAllBoxes, ctx));
    bindings.push(KeyBinding::new("ctrl-s", SaveAnnotations, ctx));
    bindings.push(KeyBinding::new("ctrl-shift-o", OpenImages, ctx));
    bindings.push(KeyBinding::new("ctrl-shift-a", AutoLabelCurrent, ctx));

    // Left/Right always navigate (duplicated with config bindings in Python)
    if config.gpui_binding("next_image").as_deref() != Some("right") {
        bindings.push(KeyBinding::new("right", NextImage, ctx));
    }
    if config.gpui_binding("prev_image").as_deref() != Some("left") {
        bindings.push(KeyBinding::new("left", PrevImage, ctx));
    }

    bindings.extend(crate::ui::entry::entry_key_bindings());
    bindings.extend(crate::ui::textarea::text_area_key_bindings());
    let _ = BINDABLE_ACTIONS.len();
    bindings
}

/// (Re)builds the whole keymap. Clears first: `bind_keys` appends, so without
/// the clear a rebound shortcut kept firing on its old key too.
pub fn apply_keymap(cx: &mut gpui::App, config: &Config) {
    cx.clear_key_bindings();
    cx.bind_keys(build_keymap(config));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_config_and_fixed_bindings() {
        let mut config = Config {
            raw: Config::defaults(),
            bindings: Default::default(),
            crosshair_color: "#FFFFFF".into(),
            crosshair_dash: crate::state::config::DEFAULT_CROSSHAIR_DASH,
        };
        config.set_binding("next_image", "<d>");
        config.set_binding("delete_box", "<q>");
        let bindings = build_keymap(&config);
        assert!(bindings.len() > 20);
    }

    /// Regression: bare-letter shortcuts must not match inside text fields,
    /// or the keystroke is consumed and the letter never gets typed.
    #[test]
    fn workspace_shortcuts_do_not_match_in_text_fields() {
        use gpui::{KeyBindingContextPredicate, KeyContext};
        let pred = KeyBindingContextPredicate::parse(WORKSPACE_SHORTCUTS).unwrap();
        let ws = KeyContext::parse(WORKSPACE_CONTEXT).unwrap();
        let entry = KeyContext::parse("TextInput").unwrap();
        let area = KeyContext::parse("TextArea").unwrap();
        let settings = KeyContext::parse(SETTINGS_CONTEXT).unwrap();
        assert!(pred.depth_of(&[ws.clone()]).is_some());
        assert!(pred.depth_of(&[ws.clone(), entry]).is_none());
        assert!(pred.depth_of(&[ws.clone(), area]).is_none());
        assert!(pred.depth_of(&[ws, settings]).is_none());
    }
}
