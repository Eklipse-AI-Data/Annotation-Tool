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
        SetOutputDir,
        SaveAnnotations,
    ]
);

/// Builds all keybindings: config-driven actions plus fixed shortcuts.
/// Bindings are global (no context) — the workspace guards them while an
/// entry is focused, exactly like the Python tool's Entry-focus guard.
pub fn build_keymap(config: &Config) -> Vec<KeyBinding> {
    let mut bindings = Vec::new();

    macro_rules! bind_config {
        ($action_id:expr, $action:path) => {
            if let Some(keys) = config.gpui_binding($action_id) {
                bindings.push(KeyBinding::new(keys.as_str(), $action, None));
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
        bindings.push(KeyBinding::new(keys, ZoomIn, None));
    }
    for keys in ["ctrl--", "ctrl-minus"] {
        bindings.push(KeyBinding::new(keys, ZoomOut, None));
    }
    bindings.push(KeyBinding::new("ctrl-0", ZoomReset, None));
    bindings.push(KeyBinding::new("?", ToggleHelpOverlay, None));
    bindings.push(KeyBinding::new("shift-/", ToggleHelpOverlay, None));
    bindings.push(KeyBinding::new("u", ToggleUnannotatedFilter, None));
    bindings.push(KeyBinding::new("ctrl-a", SelectAllBoxes, None));
    bindings.push(KeyBinding::new("ctrl-s", SaveAnnotations, None));
    bindings.push(KeyBinding::new("ctrl-shift-o", OpenImages, None));
    bindings.push(KeyBinding::new("ctrl-shift-s", SetOutputDir, None));

    // Left/Right always navigate (duplicated with config bindings in Python)
    if config.gpui_binding("next_image").as_deref() != Some("right") {
        bindings.push(KeyBinding::new("right", NextImage, None));
    }
    if config.gpui_binding("prev_image").as_deref() != Some("left") {
        bindings.push(KeyBinding::new("left", PrevImage, None));
    }

    bindings.extend(crate::ui::entry::entry_key_bindings());
    let _ = BINDABLE_ACTIONS.len();
    bindings
}

/// Re-binds the keymap after settings change.
pub fn apply_keymap(cx: &mut gpui::App, config: &Config) {
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
}
