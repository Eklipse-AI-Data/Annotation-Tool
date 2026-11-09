//! Auto Label UI: the left-sidebar tab (targets, per-class examples and
//! hints, test on the current image, batch run with progress/cancel) and the
//! "AI – needs review" flags shown in the filmstrip, file list and status bar.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};

use gpui::{div, prelude::*, px, App, Context, Entity, Focusable, Window};

use crate::autolabel::job::{self, JobEvent, Labeler, Outcome};
use crate::autolabel::parse::TargetClass;
use crate::autolabel::review::{self, ReviewFile, Status};
use crate::autolabel::examples;
use crate::autolabel::guide::{self, Guide};
use crate::autolabel::proxy::{self, ProxyState};
use crate::autolabel::settings::{self as al_settings, AutoLabelSettings, REASONING_LEVELS};
use crate::state::model::PreviewSource;
use crate::state::paths;
use crate::yolo::BBox;

use super::canvas::{app_state, app_state_mut};
use super::entry::{Entry, EntryChanged};
use super::theme::ThemeState;
use super::widgets::{self, BtnVariant};
use super::workspace::{Modal, Workspace};

// ---------------------------------------------------------------------------
// Review flags (global so the filmstrip can read them)
// ---------------------------------------------------------------------------

pub struct AutoLabelReview(pub ReviewFile);
impl gpui::Global for AutoLabelReview {}

pub fn review_file(cx: &App) -> Option<&ReviewFile> {
    cx.try_global::<AutoLabelReview>().map(|r| &r.0)
}

pub fn review_status(cx: &App, filename: &str) -> Option<Status> {
    review_file(cx).and_then(|r| r.status(filename))
}

fn review_mut(cx: &mut App) -> &mut ReviewFile {
    if !cx.has_global::<AutoLabelReview>() {
        cx.set_global(AutoLabelReview(ReviewFile::default()));
    }
    &mut cx.global_mut::<AutoLabelReview>().0
}

pub fn library_root() -> PathBuf {
    paths::data_dir().join("examples")
}

const SESSION_PRESET_KEY: &str = "active_class_preset";

// ---------------------------------------------------------------------------
// Model list (shared by the Auto Label tab and Settings → Auto Label)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ModelList {
    /// Models that can label images, newest-looking first.
    pub models: Vec<String>,
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    generation: u64,
}
impl gpui::Global for ModelList {}

pub fn model_list(cx: &App) -> Option<&ModelList> {
    cx.try_global::<ModelList>()
}

/// Fetches `GET /models` in the background. Retries an empty list a few
/// times: a freshly started proxy registers its models a moment later.
pub fn refresh_models(settings: AutoLabelSettings, cx: &mut App) {
    let generation = {
        let list = cx.default_global::<ModelList>();
        list.loading = true;
        list.generation += 1;
        list.generation
    };
    cx.refresh_windows();
    let (tx, rx) = smol::channel::bounded::<Result<Vec<String>, String>>(1);
    std::thread::spawn(move || {
        let client = crate::autolabel::client::Client::new(&settings);
        let mut result = Ok(Vec::new());
        for attempt in 0..4 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(1500));
            }
            result = client.list_models().map(|ids| {
                let mut ids: Vec<String> = ids.into_iter().filter(|m| al_settings::is_labeling_model(m)).collect();
                ids.sort_by(|a, b| b.cmp(a));
                ids
            });
            if !matches!(&result, Ok(ids) if ids.is_empty()) {
                break;
            }
        }
        let _ = tx.send_blocking(result);
    });
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let result = rx.recv().await.unwrap_or_else(|_| Err("Model list request stopped".into()));
        let _ = cx.update(|cx| {
            let list = cx.default_global::<ModelList>();
            if list.generation != generation {
                return;
            }
            list.loading = false;
            list.loaded = true;
            match result {
                Ok(models) => {
                    list.error = models
                        .is_empty()
                        .then(|| "No models listed. Sign in to Codex in Settings → Auto Label.".to_string());
                    list.models = models;
                }
                Err(e) => {
                    list.error = Some(e);
                    list.models.clear();
                }
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

/// Loads the model list once (if it was never loaded and isn't loading).
pub fn ensure_models(cx: &mut App) {
    let needed = model_list(cx).is_none_or(|l| !l.loaded && !l.loading);
    if needed {
        refresh_models(AutoLabelSettings::from_config(&app_state(cx).config.raw), cx);
    }
}

/// Changes saved Auto Label settings (from the tab's quick dropdowns).
fn update_saved_settings(cx: &mut App, change: impl FnOnce(&mut AutoLabelSettings)) {
    let state = app_state_mut(cx);
    let mut s = AutoLabelSettings::from_config(&state.config.raw);
    change(&mut s);
    s.store(&mut state.config.raw);
    state.config.save();
}

/// Popover rows for a model dropdown: status, models, the current model if
/// it isn't listed, and "Refresh list".
pub fn model_rows<V: 'static>(
    id_prefix: &'static str,
    current: &str,
    theme: &super::theme::Theme,
    cx: &mut Context<V>,
    pick: fn(&mut V, String, &mut Context<V>),
    refresh: fn(&mut V, &mut Context<V>),
) -> Vec<gpui::AnyElement> {
    let (models, loading, error) = model_list(cx)
        .map(|l| (l.models.clone(), l.loading, l.error.clone()))
        .unwrap_or_default();
    let mut rows = Vec::new();
    if loading {
        rows.push(widgets::dropdown_note("Loading models…", false, theme).into_any_element());
    } else if let Some(e) = error {
        rows.push(widgets::dropdown_note(e, true, theme).into_any_element());
    }
    let mut names = models;
    if !current.is_empty() && !names.iter().any(|m| m == current) {
        names.insert(0, current.to_string());
    }
    for (i, name) in names.into_iter().enumerate() {
        let selected = name == current;
        let value = name.clone();
        rows.push(
            widgets::dropdown_row((id_prefix, i), name, selected, theme)
                .on_click(cx.listener(move |view: &mut V, _, _, cx| pick(view, value.clone(), cx)))
                .into_any_element(),
        );
    }
    rows.push(
        widgets::dropdown_row((id_prefix, usize::MAX), "↻  Refresh list", false, theme)
            .text_color(theme.rgb(theme.fg_muted))
            .on_click(cx.listener(move |view: &mut V, _, _, cx| refresh(view, cx)))
            .into_any_element(),
    );
    rows
}

/// Popover rows for the reasoning-effort dropdown.
pub fn reasoning_rows<V: 'static>(
    id_prefix: &'static str,
    current: &str,
    theme: &super::theme::Theme,
    cx: &mut Context<V>,
    pick: fn(&mut V, String, &mut Context<V>),
) -> Vec<gpui::AnyElement> {
    REASONING_LEVELS
        .iter()
        .enumerate()
        .map(|(i, (value, label))| {
            let label = if value.is_empty() { "Default (model decides)".to_string() } else { label.to_string() };
            let value = value.to_string();
            widgets::dropdown_row((id_prefix, i), label, value == current, theme)
                .on_click(cx.listener(move |view: &mut V, _, _, cx| pick(view, value.clone(), cx)))
                .into_any_element()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// CLIProxyAPI lifetime (started with the app, stopped on quit)
// ---------------------------------------------------------------------------

/// Called once at launch: starts the proxy if enabled and stops it on quit.
pub fn proxy_install(cx: &mut App) {
    cx.on_app_quit(|_cx| {
        proxy::shutdown();
        async {}
    })
    .detach();
    proxy_apply(cx);
    if !AutoLabelSettings::from_config(&app_state(cx).config.raw).proxy_managed {
        ensure_models(cx);
    }
}

/// Brings the proxy in line with the saved settings: (re)starts it when
/// "Start with the app" is on, stops it when off. Non-blocking.
pub fn proxy_apply(cx: &mut App) {
    let mut settings = AutoLabelSettings::from_config(&app_state(cx).config.raw);
    if !settings.proxy_managed {
        proxy::stop();
        cx.refresh_windows();
        return;
    }
    // The app writes the proxy config itself, so a key can be made up.
    if settings.proxy_config.trim().is_empty() && settings.api_key.trim().is_empty() {
        settings.api_key = proxy::generate_key();
        let state = app_state_mut(cx);
        settings.store(&mut state.config.raw);
        state.config.save();
    }
    proxy_start_with(settings, cx);
}

/// Starts (or restarts, if its launch settings changed) the proxy with these
/// settings in the background; windows re-render when it's up or failed.
pub fn proxy_start_with(settings: AutoLabelSettings, cx: &mut App) {
    let (tx, rx) = smol::channel::bounded::<()>(1);
    let for_models = settings.clone();
    std::thread::spawn(move || {
        proxy::start(&settings);
        let _ = tx.send_blocking(());
    });
    cx.refresh_windows();
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let _ = rx.recv().await;
        let _ = cx.update(|cx| {
            // Proxy is up: (re)load its model list for the dropdowns.
            if matches!(proxy::state(), ProxyState::Running { .. } | ProxyState::External) {
                refresh_models(for_models, cx);
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

/// Restarts a managed proxy (e.g. after a Codex sign-in) — stop, then apply.
pub fn proxy_restart(cx: &mut App) {
    proxy::stop();
    proxy_apply(cx);
}

/// Opens the Codex sign-in console; restarts the proxy once it closes so the
/// new account is picked up.
pub fn proxy_codex_login(settings: &AutoLabelSettings, cx: &mut App) -> Result<(), String> {
    proxy::start_codex_login(settings)?;
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        while proxy::login_running() {
            smol::Timer::after(std::time::Duration::from_millis(700)).await;
        }
        let _ = cx.update(|cx| {
            if AutoLabelSettings::from_config(&app_state(cx).config.raw).proxy_managed {
                proxy_restart(cx);
            } else {
                cx.refresh_windows();
            }
        });
    })
    .detach();
    Ok(())
}

/// Saved settings, if a model is set and the (managed) proxy is up.
fn endpoint_ready(cx: &mut App) -> Result<AutoLabelSettings, String> {
    let settings = AutoLabelSettings::from_config(&app_state(cx).config.raw);
    if let Some(msg) = settings.missing() {
        return Err(msg.to_string());
    }
    if settings.proxy_managed {
        match proxy::state() {
            ProxyState::Starting => {
                return Err("CLIProxyAPI is still starting. Try again in a few seconds.".into());
            }
            ProxyState::Off => {
                proxy_apply(cx);
                return Err("Starting CLIProxyAPI… try again in a few seconds.".into());
            }
            ProxyState::Failed(e) => return Err(e),
            ProxyState::Running { .. } | ProxyState::External => {}
        }
    }
    Ok(settings)
}

enum GuideMsg {
    Progress(String),
    Done(Result<(Vec<guide::GuidePage>, Vec<(String, String)>, usize, Option<String>), String>),
}

/// Renders/copies the guide pages, then tags each with the classes it
/// explains (a few pages in parallel). Pages that explain nothing are
/// deleted. Returns `(kept pages, hints, skipped, first error)`.
fn import_and_tag(
    root: &Path,
    preset: &str,
    sources: &[PathBuf],
    classes: &[String],
    settings: &AutoLabelSettings,
    tx: &smol::channel::Sender<GuideMsg>,
) -> Result<(Vec<guide::GuidePage>, Vec<(String, String)>, usize, Option<String>), String> {
    let pages = guide::import_sources(root, preset, sources)?;
    let total = pages.len();
    let dir = guide::guide_dir(root, preset);
    let client = crate::autolabel::client::Client::new(settings);
    let queue = std::sync::Mutex::new(pages.into_iter().enumerate().collect::<std::collections::VecDeque<_>>());
    let results = std::sync::Mutex::new(Vec::new());
    let done = std::sync::atomic::AtomicUsize::new(0);
    let _ = tx.send_blocking(GuideMsg::Progress(format!("Matching {total} page(s) to classes…")));
    std::thread::scope(|scope| {
        for _ in 0..settings.concurrency.clamp(1, 4) {
            scope.spawn(|| loop {
                let Some((index, page)) = queue.lock().unwrap().pop_front() else { break };
                let tagged = crate::autolabel::prompt::encode_image(&dir.join(&page.file), settings.max_side, &[])
                    .and_then(|url| client.chat(&guide::tag_request(settings, classes, &url)))
                    .and_then(|reply| guide::parse_tags(&reply, classes));
                results.lock().unwrap().push((index, page, tagged));
                let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                let _ = tx.send_blocking(GuideMsg::Progress(format!("Matching pages to classes… {n}/{total}")));
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(i, _, _)| *i);
    let (mut kept, mut hints, mut skipped, mut first_error) = (Vec::new(), Vec::new(), 0, None);
    for (_, mut page, tagged) in results {
        match tagged {
            Ok(tags) if !tags.is_empty() => {
                page.classes = tags.iter().map(|(c, _)| c.clone()).collect();
                for (class, hint) in tags {
                    if !hints.iter().any(|(c, _): &(String, String)| *c == class) {
                        hints.push((class, hint));
                    }
                }
                kept.push(page);
            }
            Ok(_) => {
                skipped += 1;
                let _ = std::fs::remove_file(dir.join(&page.file));
            }
            Err(e) => {
                first_error.get_or_insert(e);
                let _ = std::fs::remove_file(dir.join(&page.file));
            }
        }
    }
    if kept.is_empty() {
        if let Some(e) = first_error {
            return Err(e);
        }
    }
    Ok((kept, hints, skipped, first_error))
}

/// One-line proxy status: `(is_error, text)`.
pub fn proxy_status_line(state: &ProxyState) -> (bool, String) {
    match state {
        ProxyState::Off => (false, "CLIProxyAPI: stopped".into()),
        ProxyState::Starting => (false, "CLIProxyAPI: starting…".into()),
        ProxyState::Running { pid } => (false, format!("CLIProxyAPI: running (pid {pid})")),
        ProxyState::External => (false, "CLIProxyAPI: already running outside the app (left alone)".into()),
        ProxyState::Failed(e) => (true, format!("CLIProxyAPI: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Panel state
// ---------------------------------------------------------------------------

pub struct AutoLabelPanel {
    pub preset: String,
    pub targets: BTreeSet<i64>,
    /// `image_dir + class names` the panel was last synced for.
    synced_key: Option<String>,
    /// Folder the target pre-selection was computed for.
    targets_dir: Option<String>,
    pub descriptions: BTreeMap<String, String>,
    pub desc_entries: BTreeMap<String, Entity<Entry>>,
    pub examples: BTreeMap<String, Vec<examples::Example>>,
    pub testing: bool,
    /// `(is_error, text)` shown above the action buttons.
    pub message: Option<(bool, String)>,
    pub collapse_targets: bool,
    pub collapse_examples: bool,
    pub model_open: bool,
    pub reasoning_open: bool,
    /// Labeling guide of the active preset.
    pub guide: Guide,
    /// Progress text while a guide import runs.
    pub guide_busy: Option<String>,
}

impl AutoLabelPanel {
    pub fn new() -> Self {
        Self {
            preset: examples::CUSTOM_PRESET.to_string(),
            targets: BTreeSet::new(),
            synced_key: None,
            targets_dir: None,
            descriptions: BTreeMap::new(),
            desc_entries: BTreeMap::new(),
            examples: BTreeMap::new(),
            testing: false,
            message: None,
            collapse_targets: true,
            collapse_examples: true,
            model_open: false,
            reasoning_open: false,
            guide: Guide::default(),
            guide_busy: None,
        }
    }

    pub fn any_entry_focused(&self, window: &Window, cx: &App) -> bool {
        self.desc_entries
            .values()
            .any(|e| e.read(cx).focus_handle(cx).is_focused(window))
    }
}

#[derive(Default)]
struct BatchStats {
    written: usize,
    boxes: usize,
    empty: usize,
    skipped: usize,
    failed: usize,
    first_error: Option<String>,
    done: usize,
    total: usize,
}

impl Workspace {
    // ------------------------------------------------------------------
    // Sync
    // ------------------------------------------------------------------

    fn autolabel_sync_key(cx: &App) -> String {
        let s = app_state(cx);
        let names: Vec<&str> = s.classes.iter().map(|c| c.name.as_str()).collect();
        format!("{}|{}", s.image_dir, names.join("\u{1f}"))
    }

    /// Re-reads preset, targets, hints and examples when the folder or the
    /// class list changed (or when `force`).
    pub fn autolabel_sync(&mut self, force: bool, cx: &mut Context<Self>) {
        let key = Self::autolabel_sync_key(cx);
        if !force && self.autolabel.synced_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.autolabel.synced_key = Some(key);

        let remembered = app_state(cx)
            .session
            .get(SESSION_PRESET_KEY)
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let preset = examples::detect_active_preset(
            &paths::data_dir(),
            &paths::classes_file(),
            remembered.as_deref(),
        );
        if preset != self.autolabel.preset {
            self.autolabel.desc_entries.clear();
        }
        self.autolabel.preset = preset;
        let root = library_root();
        self.autolabel.descriptions = examples::load_descriptions(&root, &self.autolabel.preset);

        let classes: Vec<TargetClass> = app_state(cx)
            .classes
            .iter()
            .map(|c| (c.id, c.name.clone()))
            .collect();
        let image_dir = app_state(cx).image_dir.clone();
        if self.autolabel.targets_dir.as_deref() != Some(image_dir.as_str()) {
            self.autolabel.targets_dir = Some(image_dir.clone());
            let folder = Path::new(&image_dir)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let matched = examples::classes_matching_folder(&folder, &classes);
            if !matched.is_empty() {
                self.autolabel.targets = matched.into_iter().collect();
            }
        }
        let valid: BTreeSet<i64> = classes.iter().map(|(id, _)| *id).collect();
        self.autolabel.targets.retain(|id| valid.contains(id));
        self.autolabel_reload_examples(cx);
    }

    fn autolabel_targets(&self, cx: &App) -> Vec<TargetClass> {
        app_state(cx)
            .classes
            .iter()
            .filter(|c| self.autolabel.targets.contains(&c.id))
            .map(|c| (c.id, c.name.clone()))
            .collect()
    }

    fn autolabel_reload_examples(&mut self, cx: &mut Context<Self>) {
        let root = library_root();
        self.autolabel.guide = Guide::load(&root, &self.autolabel.preset);
        let targets = self.autolabel_targets(cx);
        self.autolabel.examples = targets
            .iter()
            .map(|(_, name)| (name.clone(), examples::list_examples(&root, &self.autolabel.preset, name)))
            .collect();
        for (_, name) in &targets {
            if self.autolabel.desc_entries.contains_key(name) {
                continue;
            }
            let text = self.autolabel.descriptions.get(name).cloned().unwrap_or_default();
            let entry = cx.new(|cx| {
                let mut e = Entry::new("Hint, e.g. \"red banner, top center\"", cx);
                e.content = text.into();
                e
            });
            let class_name = name.clone();
            cx.subscribe(&entry, move |ws: &mut Workspace, entry, _: &EntryChanged, cx| {
                let text = entry.read(cx).text();
                ws.autolabel.descriptions.insert(class_name.clone(), text);
                let _ = examples::save_descriptions(&library_root(), &ws.autolabel.preset, &ws.autolabel.descriptions);
            })
            .detach();
            self.autolabel.desc_entries.insert(name.clone(), entry);
        }
    }

    /// Remembers which preset file the class list was loaded from.
    pub fn autolabel_remember_preset(preset_file: &str, cx: &mut Context<Self>) {
        let stem = Path::new(preset_file)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut map = serde_json::Map::new();
        map.insert(SESSION_PRESET_KEY.into(), serde_json::Value::String(stem.clone()));
        crate::state::session::save_session(&paths::session_file(), &map);
        app_state_mut(cx)
            .session
            .insert(SESSION_PRESET_KEY.into(), serde_json::Value::String(stem));
    }

    pub fn autolabel_open_tab(&mut self, cx: &mut Context<Self>) {
        app_state_mut(cx).show_left_sidebar = true;
        self.left_tab = super::workspace::LeftTab::AutoLabel;
        self.autolabel_sync(true, cx);
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Review flags
    // ------------------------------------------------------------------

    /// Loads the review sidecar for the current folder.
    pub fn autolabel_on_folder_loaded(&mut self, cx: &mut Context<Self>) {
        let dir = app_state(cx).image_dir.clone();
        let file = if dir.is_empty() { ReviewFile::default() } else { ReviewFile::load(Path::new(&dir)) };
        cx.set_global(AutoLabelReview(file));
    }

    fn current_filename(cx: &App) -> Option<String> {
        let s = app_state(cx);
        if s.current_image_index < 0 {
            return None;
        }
        s.image_list.get(s.current_image_index as usize).cloned()
    }

    /// Clears the AI flag for the current image (after the user saved it).
    pub fn autolabel_mark_current_reviewed(&mut self, cx: &mut Context<Self>) {
        let Some(name) = Self::current_filename(cx) else { return };
        let review = review_mut(cx);
        if review.status(&name) == Some(Status::Pending) && review.mark_reviewed(&name) {
            let _ = review.save();
            self.filmstrip.update(cx, |_, cx| cx.notify());
        }
    }

    pub fn autolabel_current_pending(&self, cx: &App) -> bool {
        Self::current_filename(cx).is_some_and(|n| review_status(cx, &n) == Some(Status::Pending))
    }

    pub fn autolabel_next_pending(&mut self, cx: &mut Context<Self>) {
        let (list, current) = {
            let s = app_state(cx);
            (s.image_list.clone(), s.current_image_index)
        };
        let Some(review) = review_file(cx) else { return };
        let n = list.len();
        let start = if current < 0 { 0 } else { current as usize + 1 };
        let next = (0..n)
            .map(|k| (start + k) % n)
            .find(|&i| review.is_pending(&list[i]));
        match next {
            Some(ix) => self.guard_unsaved(move |ws, cx| ws.load_image(ix, cx), cx),
            None => self.toast("No images left to review", cx),
        }
    }

    // ------------------------------------------------------------------
    // Examples
    // ------------------------------------------------------------------

    pub fn autolabel_add_example(&mut self, class_name: String, cx: &mut Context<Self>) {
        let Some(filename) = Self::current_filename(cx) else {
            self.toast("Open an image first", cx);
            return;
        };
        let (image_path, named_boxes, class_box_count) = {
            let s = app_state(cx);
            let name_of = |id: i64| s.classes.iter().find(|c| c.id == id).map(|c| c.name.clone());
            let named: Vec<(String, BBox)> = s
                .boxes
                .iter()
                .filter_map(|b| name_of(b.class_id).map(|n| (n, *b)))
                .collect();
            let count = named.iter().filter(|(n, _)| *n == class_name).count();
            (PathBuf::from(&s.image_dir).join(&filename), named, count)
        };
        match examples::add_example(&library_root(), &self.autolabel.preset, &class_name, &image_path, &named_boxes) {
            Ok(_) => {
                let msg = if class_box_count == 0 {
                    format!("Added {filename} as a negative example for {class_name} (no {class_name} boxes on it)")
                } else {
                    format!("Added {filename} as an example for {class_name} ({class_box_count} box(es))")
                };
                self.autolabel.message = Some((false, msg.clone()));
                self.toast(msg, cx);
            }
            Err(e) => {
                self.autolabel.message = Some((true, format!("Could not add example: {e}")));
            }
        }
        self.autolabel_reload_examples(cx);
        cx.notify();
    }

    /// Adds labeled frames from the open folder as examples for `class_name`
    /// (up to "Examples per class"), spread across the folder.
    fn autolabel_pick_examples(&mut self, class_name: String, cx: &mut Context<Self>) {
        let settings = AutoLabelSettings::from_config(&app_state(cx).config.raw);
        let have = self.autolabel.examples.get(&class_name).map(Vec::len).unwrap_or(0);
        let want = settings.max_examples_per_class.max(1).saturating_sub(have);
        if want == 0 {
            self.autolabel.message = Some((false, format!(
                "{class_name} already has {have} example(s), the limit set in Settings → Auto Label → Examples per class. Remove one to pick others."
            )));
            cx.notify();
            return;
        }
        let (image_dir, label_dir, files, classes) = {
            let s = app_state(cx);
            (PathBuf::from(&s.image_dir), PathBuf::from(&s.output_dir), s.full_image_list.clone(), s.classes.clone())
        };
        let Some(class_id) = classes.iter().find(|c| c.name == class_name).map(|c| c.id) else {
            return;
        };
        let used: BTreeSet<String> = self
            .autolabel
            .examples
            .get(&class_name)
            .map(|v| v.iter().map(|e| e.source.clone()).collect())
            .unwrap_or_default();
        // Labeled frames that contain this class and aren't examples yet.
        let candidates: Vec<(String, Vec<BBox>)> = files
            .iter()
            .filter(|f| !used.contains(&image_dir.join(f).to_string_lossy().to_string()))
            .filter_map(|f| {
                let label = job::label_path(&label_dir, f)?;
                let boxes = crate::yolo::parse_yolo(&label);
                boxes.iter().any(|b| b.class_id == class_id).then(|| (f.clone(), boxes))
            })
            .collect();
        if candidates.is_empty() {
            self.autolabel.message = Some((true, format!(
                "No labeled image in this folder has a {class_name} box yet. Label one by hand first, or import the guide."
            )));
            cx.notify();
            return;
        }
        // Evenly spaced picks so examples cover the whole folder.
        let n = want.min(candidates.len());
        let picks: Vec<&(String, Vec<BBox>)> =
            (0..n).map(|i| &candidates[(i * candidates.len() + candidates.len() / (2 * n)) / n]).collect();
        let name_of = |id: i64| classes.iter().find(|c| c.id == id).map(|c| c.name.clone());
        let mut added = Vec::new();
        let mut error = None;
        for (file, boxes) in picks {
            let named: Vec<(String, BBox)> = boxes.iter().filter_map(|b| name_of(b.class_id).map(|n| (n, *b))).collect();
            match examples::add_example(&library_root(), &self.autolabel.preset, &class_name, &image_dir.join(file), &named) {
                Ok(_) => added.push(file.clone()),
                Err(e) => error = Some(e),
            }
        }
        self.autolabel.message = Some(match error {
            Some(e) => (true, format!("Could not add example: {e}")),
            None => (false, format!("Added {} example(s) for {class_name}: {}", added.len(), added.join(", "))),
        });
        self.autolabel_reload_examples(cx);
        cx.notify();
    }

    /// Imports a labeling guide (PDF / screenshots) for the active preset:
    /// renders the pages, asks the model which classes each page explains,
    /// and fills empty class hints from it.
    fn autolabel_import_guide(&mut self, cx: &mut Context<Self>) {
        if self.autolabel.guide_busy.is_some() {
            return;
        }
        let settings = match endpoint_ready(cx) {
            Ok(s) => s,
            Err(e) => {
                self.autolabel.message = Some((true, format!("Guide import needs the model: {e}")));
                cx.notify();
                return;
            }
        };
        let classes: Vec<String> = app_state(cx).classes.iter().map(|c| c.name.clone()).collect();
        if classes.is_empty() {
            self.autolabel.message = Some((true, "Load the class list first; the guide is matched against it.".into()));
            cx.notify();
            return;
        }
        let preset = self.autolabel.preset.clone();
        cx.spawn(async move |this: gpui::WeakEntity<Workspace>, cx: &mut gpui::AsyncApp| {
            let Some(files) = rfd::AsyncFileDialog::new()
                .set_title("Import labeling guide (PDF or annotated screenshots)")
                .add_filter("Guide", &["pdf", "png", "jpg", "jpeg", "webp", "bmp"])
                .pick_files()
                .await
            else {
                return;
            };
            let sources: Vec<PathBuf> = files.iter().map(|f| f.path().to_path_buf()).collect();
            let (tx, rx) = smol::channel::unbounded::<GuideMsg>();
            let root = library_root();
            let thread_preset = preset.clone();
            std::thread::spawn(move || {
                let _ = tx.send_blocking(GuideMsg::Progress("Reading the guide…".into()));
                let result = import_and_tag(&root, &thread_preset, &sources, &classes, &settings, &tx);
                let _ = tx.send_blocking(GuideMsg::Done(result));
            });
            let _ = this.update(cx, |ws, cx| {
                ws.autolabel.guide_busy = Some("Reading the guide…".into());
                ws.autolabel.message = None;
                cx.notify();
            });
            while let Ok(msg) = rx.recv().await {
                let done = matches!(msg, GuideMsg::Done(_));
                let _ = this.update(cx, |ws, cx| {
                    match msg {
                        GuideMsg::Progress(text) => ws.autolabel.guide_busy = Some(text),
                        GuideMsg::Done(result) => {
                            ws.autolabel.guide_busy = None;
                            ws.autolabel_finish_guide(&preset, result, cx);
                        }
                    }
                    cx.notify();
                });
                if done {
                    break;
                }
            }
        })
        .detach();
    }

    fn autolabel_finish_guide(
        &mut self,
        preset: &str,
        result: Result<(Vec<guide::GuidePage>, Vec<(String, String)>, usize, Option<String>), String>,
        cx: &mut Context<Self>,
    ) {
        let (pages, hints, skipped, first_error) = match result {
            Ok(v) => v,
            Err(e) => {
                self.autolabel.message = Some((true, format!("Guide import failed: {e}")));
                return;
            }
        };
        let root = library_root();
        let mut g = Guide::load(&root, preset);
        let added = pages.len();
        g.pages.extend(pages);
        if let Err(e) = g.save(&root, preset) {
            self.autolabel.message = Some((true, format!("Could not save the guide: {e}")));
            return;
        }
        // Fill empty hints (never overwrite what the user wrote).
        let mut descriptions = examples::load_descriptions(&root, preset);
        let filled = guide::merge_hints(&mut descriptions, &hints);
        if filled > 0 {
            let _ = examples::save_descriptions(&root, preset, &descriptions);
        }
        if preset == self.autolabel.preset {
            for (class, text) in &descriptions {
                if let Some(entry) = self.autolabel.desc_entries.get(class) {
                    if entry.read(cx).text().trim().is_empty() && !text.is_empty() {
                        entry.update(cx, |e, cx| e.set_text(text, cx));
                    }
                }
            }
            self.autolabel.descriptions = descriptions;
            self.autolabel_reload_examples(cx);
        }
        let covered: BTreeSet<&String> = g.pages.iter().flat_map(|p| p.classes.iter()).collect();
        let mut msg = format!(
            "Guide imported: {added} page(s) kept, {} class(es) covered, {filled} hint(s) filled.",
            covered.len()
        );
        if skipped > 0 {
            msg.push_str(&format!(" {skipped} page(s) explained no class and were skipped."));
        }
        if let Some(e) = &first_error {
            msg.push_str(&format!(" Some pages failed: {e}"));
        }
        self.autolabel.message = Some((first_error.is_some() && added == 0, msg));
    }

    fn autolabel_clear_guide(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = guide::clear(&library_root(), &self.autolabel.preset) {
            self.autolabel.message = Some((true, format!("Could not remove the guide: {e}")));
        } else {
            self.autolabel.message = Some((false, "Guide removed.".into()));
        }
        self.autolabel_reload_examples(cx);
        cx.notify();
    }

    fn autolabel_remove_example(&mut self, image: PathBuf, cx: &mut Context<Self>) {
        if let Err(e) = examples::remove_example(&image) {
            self.autolabel.message = Some((true, format!("Could not remove example: {e}")));
        }
        self.autolabel_reload_examples(cx);
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Test on the current image (preview)
    // ------------------------------------------------------------------

    fn autolabel_prepare_inputs(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(AutoLabelSettings, Vec<TargetClass>, String, BTreeMap<String, String>), String> {
        self.autolabel_sync(false, cx);
        let settings = endpoint_ready(cx)?;
        let targets = self.autolabel_targets(cx);
        if targets.is_empty() {
            return Err("Tick at least one target class in the Auto Label tab.".to_string());
        }
        Ok((settings, targets, self.autolabel.preset.clone(), self.autolabel.descriptions.clone()))
    }

    pub fn autolabel_test_current(&mut self, cx: &mut Context<Self>) {
        if self.autolabel.testing || self.task_running {
            return;
        }
        let Some(filename) = Self::current_filename(cx) else {
            self.toast("Open an image first", cx);
            return;
        };
        let (settings, targets, preset, descriptions) = match self.autolabel_prepare_inputs(cx) {
            Ok(v) => v,
            Err(msg) => {
                self.autolabel.message = Some((true, msg.clone()));
                self.toast(msg, cx);
                cx.notify();
                return;
            }
        };
        let path = PathBuf::from(&app_state(cx).image_dir).join(&filename);
        self.autolabel.testing = true;
        self.autolabel.message = Some((false, format!("Asking {} about {filename}…", settings.model)));
        self.set_status(format!("Auto Label: asking {}…", settings.model), cx);

        let (tx, rx) = smol::channel::bounded(1);
        std::thread::spawn(move || {
            let result = Labeler::prepare(settings, &library_root(), &preset, targets, &descriptions)
                .and_then(|labeler| labeler.label(&path));
            let _ = tx.send_blocking(result);
        });
        let ws = cx.entity();
        cx.spawn(async move |_this: gpui::WeakEntity<Workspace>, cx: &mut gpui::AsyncApp| {
            let result = rx.recv().await.unwrap_or_else(|_| Err("Auto Label worker stopped".into()));
            let _ = cx.update(|cx| {
                ws.update(cx, |ws, cx| ws.autolabel_test_done(filename, result, cx));
            });
        })
        .detach();
        cx.notify();
    }

    fn autolabel_test_done(&mut self, filename: String, result: Result<Vec<BBox>, String>, cx: &mut Context<Self>) {
        self.autolabel.testing = false;
        let still_here = Self::current_filename(cx).as_deref() == Some(filename.as_str());
        match result {
            Ok(boxes) if boxes.is_empty() => {
                let msg = format!("Model found no target classes on {filename}");
                self.autolabel.message = Some((false, msg.clone()));
                self.set_status(msg, cx);
            }
            Ok(boxes) => {
                let n = boxes.len();
                if still_here {
                    app_state_mut(cx).set_preview(&boxes, PreviewSource::AutoLabel, None);
                    self.autolabel.message =
                        Some((false, format!("{n} suggestion(s) shown dashed on the canvas — Accept or Discard.")));
                    self.set_status(format!("Auto Label: {n} suggestion(s)"), cx);
                } else {
                    self.autolabel.message = Some((false, format!("{n} suggestion(s) for {filename} (image changed, not shown)")));
                }
            }
            Err(e) => {
                self.autolabel.message = Some((true, e.clone()));
                self.set_status("Auto Label failed", cx);
                self.toast("Auto Label failed — see the Auto Label tab", cx);
            }
        }
        cx.notify();
    }

    pub fn autolabel_preview_active(&self, cx: &App) -> bool {
        let s = app_state(cx);
        s.preview_visible && s.preview_source == Some(PreviewSource::AutoLabel)
    }

    pub fn autolabel_accept_preview(&mut self, cx: &mut Context<Self>) {
        if !self.autolabel_preview_active(cx) {
            return;
        }
        let n = {
            let state = app_state_mut(cx);
            let boxes = state.preview_boxes.clone();
            state.save_state();
            state.boxes.extend(boxes.iter().copied());
            state.clear_preview();
            boxes.len()
        };
        self.autolabel.message = Some((false, format!("Accepted {n} box(es). Adjust them, then save.")));
        self.set_status(format!("Accepted {n} AI box(es)"), cx);
        self.refresh_validation(cx);
        cx.notify();
    }

    pub fn autolabel_discard_preview(&mut self, cx: &mut Context<Self>) {
        if self.autolabel_preview_active(cx) {
            app_state_mut(cx).clear_preview();
            self.autolabel.message = None;
            self.set_status("AI suggestions discarded", cx);
            cx.notify();
        }
    }

    // ------------------------------------------------------------------
    // Batch run
    // ------------------------------------------------------------------

    pub fn autolabel_run_batch(&mut self, cx: &mut Context<Self>) {
        if self.task_running || self.autolabel.testing {
            return;
        }
        if app_state(cx).image_dir.is_empty() {
            self.alert("Auto Label", "Open an image folder first.");
            cx.notify();
            return;
        }
        if let Err(msg) = self.autolabel_prepare_inputs(cx) {
            self.alert("Auto Label", msg);
            cx.notify();
            return;
        }
        self.guard_unsaved(|ws, cx| ws.autolabel_confirm_batch(cx), cx);
    }

    fn autolabel_confirm_batch(&mut self, cx: &mut Context<Self>) {
        // Flush the current image so it is judged by what's on disk.
        if app_state(cx).auto_save && app_state(cx).is_dirty() {
            app_state_mut(cx).save_annotations();
        }
        let (settings, targets, _, _) = match self.autolabel_prepare_inputs(cx) {
            Ok(v) => v,
            Err(msg) => {
                self.alert("Auto Label", msg);
                cx.notify();
                return;
            }
        };
        let (image_dir, label_dir, all) = {
            let s = app_state(cx);
            (s.image_dir.clone(), s.output_dir.clone(), s.full_image_list.clone())
        };
        let review = review_file(cx).cloned().unwrap_or_default();
        let label_dir_path = PathBuf::from(&label_dir);
        let mut previously_empty = 0;
        let candidates: Vec<String> = all
            .into_iter()
            .filter(|name| {
                let labeled = job::label_path(&label_dir_path, name).is_some_and(|p| job::has_labels(&p));
                if labeled {
                    return false;
                }
                if review.status(name) == Some(Status::Empty) {
                    previously_empty += 1;
                    return false;
                }
                true
            })
            .collect();
        if candidates.is_empty() {
            let mut msg = "Every image in this folder already has labels.".to_string();
            if previously_empty > 0 {
                msg += &format!("\n\n{previously_empty} image(s) were skipped because the model found nothing last time. Use \"Retry empty/failed\" in the Auto Label tab to try them again.");
            }
            self.alert("Auto Label", msg);
            cx.notify();
            return;
        }
        let example_total: usize = self.autolabel.examples.values().map(Vec::len).sum();
        let target_names: Vec<String> = targets.iter().map(|(_, n)| n.clone()).collect();
        let folder = Path::new(&image_dir)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut message = format!(
            "Label {} unlabeled image(s) in \"{folder}\"?\n\nTargets: {}\nExamples: {example_total} (max {} per class)\nModel: {} · {} parallel request(s)\n\nLabels are written next to the images and flagged \"AI\" until you review them. Images that already have labels are never touched; a backup of existing labels is made first.",
            candidates.len(),
            target_names.join(", "),
            settings.max_examples_per_class,
            settings.model,
            settings.concurrency,
        );
        if example_total == 0 {
            message += "\n\n⚠ No example images for these classes — results will be much less accurate. Add a few examples first.";
        }
        if previously_empty > 0 {
            message += &format!("\n\n{previously_empty} image(s) where the model found nothing before are skipped.");
        }
        let candidates = std::rc::Rc::new(candidates);
        self.confirm(
            "Auto Label",
            message,
            Box::new(move |ws, _window, cx| ws.autolabel_start_batch(candidates.to_vec(), cx)),
        );
        cx.notify();
    }

    fn autolabel_start_batch(&mut self, candidates: Vec<String>, cx: &mut Context<Self>) {
        if self.task_running {
            return;
        }
        let (settings, targets, preset, descriptions) = match self.autolabel_prepare_inputs(cx) {
            Ok(v) => v,
            Err(msg) => {
                self.alert("Auto Label", msg);
                cx.notify();
                return;
            }
        };
        let (image_dir, label_dir) = {
            let s = app_state(cx);
            (PathBuf::from(&s.image_dir), PathBuf::from(&s.output_dir))
        };
        let total = candidates.len();
        let cancel = Arc::new(AtomicBool::new(false));
        self.task_running = true;
        self.modal = Some(Modal::Progress {
            title: "Auto Labeling".into(),
            label: "Preparing examples…".into(),
            current: 0,
            total,
            cancel: Some(Arc::clone(&cancel)),
        });
        cx.notify();

        let (tx, rx) = mpsc::channel::<JobEvent>();
        let concurrency = settings.concurrency;
        std::thread::spawn(move || {
            // Safety net: back up existing labels (nothing is overwritten,
            // but the backup makes the run trivially reversible).
            let _ = crate::yolo::backup_annotations(&label_dir, |_, _, _| {});
            match Labeler::prepare(settings, &library_root(), &preset, targets, &descriptions) {
                Err(e) => {
                    let _ = tx.send(JobEvent::Aborted(e));
                }
                Ok(labeler) => {
                    job::run_batch(Arc::new(labeler), image_dir, label_dir, candidates, concurrency, cancel, tx);
                }
            }
        });

        let model = AutoLabelSettings::from_config(&app_state(cx).config.raw).model;
        let ws = cx.entity();
        cx.spawn(async move |_this: gpui::WeakEntity<Workspace>, cx: &mut gpui::AsyncApp| {
            let mut stats = BatchStats { total, ..Default::default() };
            loop {
                match rx.try_recv() {
                    Ok(JobEvent::ImageDone { filename, outcome }) => {
                        stats.done += 1;
                        let entry = match &outcome {
                            Outcome::Written(n) => {
                                stats.written += 1;
                                stats.boxes += n;
                                Some(review::Entry::new(Status::Pending, &model, *n, None))
                            }
                            Outcome::Empty => {
                                stats.empty += 1;
                                Some(review::Entry::new(Status::Empty, &model, 0, None))
                            }
                            Outcome::Skipped => {
                                stats.skipped += 1;
                                None
                            }
                            Outcome::Failed(e) => {
                                stats.failed += 1;
                                stats.first_error.get_or_insert_with(|| e.clone());
                                Some(review::Entry::new(Status::Error, &model, 0, Some(e.clone())))
                            }
                        };
                        let label = format!(
                            "{}/{} · {} labeled · {} empty · {} failed — {filename}",
                            stats.done, stats.total, stats.written, stats.empty, stats.failed
                        );
                        let done = stats.done;
                        let _ = cx.update(|cx| {
                            if let Some(entry) = entry {
                                let review = review_mut(cx);
                                review.set(&filename, entry);
                                let _ = review.save();
                            }
                            ws.update(cx, |ws, cx| {
                                if let Some(Modal::Progress { label: l, current, .. }) = &mut ws.modal {
                                    *l = label.into();
                                    *current = done;
                                }
                                ws.filmstrip.update(cx, |_, cx| cx.notify());
                                cx.notify();
                            });
                        });
                    }
                    Ok(JobEvent::Finished { cancelled }) => {
                        let _ = cx.update(|cx| {
                            ws.update(cx, |ws, cx| ws.autolabel_finish_batch(stats, cancelled, None, cx));
                        });
                        return;
                    }
                    Ok(JobEvent::Aborted(e)) => {
                        let _ = cx.update(|cx| {
                            ws.update(cx, |ws, cx| ws.autolabel_finish_batch(stats, false, Some(e), cx));
                        });
                        return;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        smol::Timer::after(std::time::Duration::from_millis(100)).await;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = cx.update(|cx| {
                            ws.update(cx, |ws, cx| {
                                ws.autolabel_finish_batch(stats, false, Some("Auto Label worker stopped unexpectedly".into()), cx)
                            });
                        });
                        return;
                    }
                }
            }
        })
        .detach();
    }

    fn autolabel_finish_batch(
        &mut self,
        stats: BatchStats,
        cancelled: bool,
        aborted: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(Modal::Progress { .. }) = self.modal {
            self.modal = None;
        }
        self.task_running = false;
        // Reload the current image without autosaving stale in-memory boxes.
        let index = app_state(cx).current_image_index;
        if index != -1 {
            app_state_mut(cx).current_image_index = -1;
            self.load_image(index as usize, cx);
        }
        self.refresh_class_counts(cx);
        self.filmstrip.update(cx, |_, cx| cx.notify());

        if let Some(err) = aborted {
            self.autolabel.message = Some((true, err.clone()));
            self.alert("Auto Label failed", err);
            cx.notify();
            return;
        }
        let title = if cancelled { "Auto Label cancelled" } else { "Auto Label finished" };
        let mut msg = format!(
            "Processed {} of {} image(s).\n\nLabeled: {} ({} boxes)\nNothing found: {}\nFailed: {}",
            stats.done, stats.total, stats.written, stats.boxes, stats.empty, stats.failed
        );
        if stats.skipped > 0 {
            msg += &format!("\nSkipped (labeled meanwhile): {}", stats.skipped);
        }
        if let Some(e) = &stats.first_error {
            msg += &format!("\n\nFirst error: {}", crate::autolabel::client::truncate(e, 240));
        }
        if stats.written > 0 {
            msg += "\n\nAI-labeled images show an \"AI\" badge. Review each one (\"Next to review\" in the Auto Label tab) and press Ctrl+S to accept it.";
        }
        self.autolabel.message = Some((stats.failed > 0, format!(
            "{}: {} labeled, {} empty, {} failed",
            title, stats.written, stats.empty, stats.failed
        )));
        self.alert(title, msg);
        cx.notify();
    }

    fn autolabel_retry_flags(&mut self, cx: &mut Context<Self>) {
        let review = review_mut(cx);
        let n = review.clear_non_pending();
        let _ = review.save();
        self.toast(format!("Cleared {n} empty/failed flag(s) — they'll be retried next run"), cx);
        self.filmstrip.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Render
    // ------------------------------------------------------------------

    pub fn render_autolabel_tab(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        self.autolabel_sync(false, cx);
        let theme = ThemeState::current(cx);
        let settings = AutoLabelSettings::from_config(&app_state(cx).config.raw);
        let classes: Vec<(i64, String, (u8, u8, u8))> = app_state(cx)
            .classes
            .iter()
            .map(|c| (c.id, c.name.clone(), c.color))
            .collect();
        let has_image = app_state(cx).current_image_index != -1;
        let has_folder = !app_state(cx).image_dir.is_empty();
        let preview_active = self.autolabel_preview_active(cx);
        let busy = self.task_running || self.autolabel.testing;
        let (pending, empty, failed) = review_file(cx)
            .map(|r| (r.count(Status::Pending), r.count(Status::Empty), r.count(Status::Error)))
            .unwrap_or((0, 0, 0));

        // --- Setup ---------------------------------------------------------
        let field_label = |text: &'static str| {
            div().w(px(64.)).flex_none().text_size(px(11.)).text_color(theme.rgb(theme.fg_muted)).child(text)
        };

        let mut model_dd = div().id("al-model-dd").flex_1().min_w_0().child(
            widgets::dropdown_button(
                "al-model-btn",
                if settings.model.is_empty() { "Select a model…".to_string() } else { settings.model.clone() },
                settings.model.is_empty(),
                self.autolabel.model_open,
                theme,
            )
            .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                if !ws.popover_just_closed() {
                    ws.autolabel.model_open = !ws.autolabel.model_open;
                    ws.autolabel.reasoning_open = false;
                    if ws.autolabel.model_open {
                        ensure_models(cx);
                    }
                }
                cx.notify();
            })),
        );
        if self.autolabel.model_open {
            let rows = model_rows(
                "al-tab-model",
                &settings.model,
                theme,
                cx,
                |ws: &mut Workspace, model, cx| {
                    update_saved_settings(cx, |s| s.model = model);
                    ws.autolabel.model_open = false;
                    ws.autolabel.message = None;
                    cx.notify();
                },
                |_ws: &mut Workspace, cx| {
                    refresh_models(AutoLabelSettings::from_config(&app_state(cx).config.raw), cx);
                    cx.notify();
                },
            );
            model_dd = model_dd.child(widgets::popover("al-tab-model-pop", px(250.), px(320.), theme, rows,
                cx.listener(|ws: &mut Workspace, _, _, cx| {
                    ws.autolabel.model_open = false;
                    ws.popover_closed_at = Some(std::time::Instant::now());
                    cx.notify();
                })));
        }

        let mut reasoning_dd = div().id("al-reasoning-dd").flex_1().min_w_0().child(
            widgets::dropdown_button(
                "al-reasoning-btn",
                al_settings::reasoning_label(&settings.reasoning_effort),
                settings.reasoning_effort.is_empty(),
                self.autolabel.reasoning_open,
                theme,
            )
            .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                if !ws.popover_just_closed() {
                    ws.autolabel.reasoning_open = !ws.autolabel.reasoning_open;
                    ws.autolabel.model_open = false;
                }
                cx.notify();
            })),
        );
        if self.autolabel.reasoning_open {
            let rows = reasoning_rows("al-tab-reasoning", &settings.reasoning_effort, theme, cx,
                |ws: &mut Workspace, level, cx| {
                    update_saved_settings(cx, |s| s.reasoning_effort = level);
                    ws.autolabel.reasoning_open = false;
                    cx.notify();
                });
            reasoning_dd = reasoning_dd.child(widgets::popover("al-tab-reasoning-pop", px(200.), px(300.), theme, rows,
                cx.listener(|ws: &mut Workspace, _, _, cx| {
                    ws.autolabel.reasoning_open = false;
                    ws.popover_closed_at = Some(std::time::Instant::now());
                    cx.notify();
                })));
        }

        let (endpoint_err, endpoint_line) = if settings.proxy_managed {
            proxy_status_line(&proxy::state())
        } else {
            (false, settings.base().to_string())
        };
        let can_start = settings.proxy_managed
            && matches!(proxy::state(), ProxyState::Off | ProxyState::Failed(_));

        let setup = div()
            .flex_none()
            .px(px(12.))
            .py(px(10.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(11.)).text_color(theme.rgb(theme.fg_muted)).child("Class preset"))
                    .child(div().text_size(px(12.)).font_weight(gpui::FontWeight::SEMIBOLD).child(self.autolabel.preset.clone())),
            )
            .child(div().flex().items_center().gap(px(8.)).child(field_label("Model")).child(model_dd))
            .child(div().flex().items_center().gap(px(8.)).child(field_label("Reasoning")).child(reasoning_dd))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .text_color(if endpoint_err { theme.rgb(theme.error) } else { theme.rgb(theme.fg_muted) })
                            .child(endpoint_line),
                    )
                    .when(can_start, |el| {
                        el.child(widgets::header_link("al-proxy-start", "Start", theme).on_click(cx.listener(
                            |_ws: &mut Workspace, _, _, cx| proxy_restart(cx),
                        )))
                    })
                    .child(
                        widgets::header_link("al-settings", "Endpoint…", theme).on_click(cx.listener(
                            |ws: &mut Workspace, _, window, cx| {
                                ws.open_settings(window, cx);
                                if let Some(Modal::Settings(view)) = &ws.modal {
                                    view.update(cx, |v, cx| v.select_tab(super::dialogs::SettingsTab::AutoLabel, cx));
                                }
                            },
                        )),
                    ),
            );

        // --- Target classes -----------------------------------------------
        let n_targets = self.autolabel.targets.len();
        let targets_header = widgets::pane_header_collapsible(
            "al-col-targets",
            "Target classes",
            self.autolabel.collapse_targets,
            Some(div().child(format!("{n_targets} selected")).into_any_element()),
            theme,
        )
        .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.autolabel.collapse_targets = !ws.autolabel.collapse_targets;
            cx.notify();
        }));
        let target_rows: Vec<gpui::AnyElement> = classes
            .iter()
            .map(|(id, name, color)| {
                let id = *id;
                let checked = self.autolabel.targets.contains(&id);
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().flex_1().min_w_0().child(
                        widgets::checkbox(("al-target", id as usize), name.clone(), checked, theme).on_click(
                            cx.listener(move |ws: &mut Workspace, _, _, cx| {
                                if !ws.autolabel.targets.remove(&id) {
                                    ws.autolabel.targets.insert(id);
                                }
                                ws.autolabel_reload_examples(cx);
                                cx.notify();
                            }),
                        ),
                    ))
                    .child(widgets::color_swatch(*color, px(9.), theme))
                    .into_any_element()
            })
            .collect();
        let targets_content = div()
            .flex_none()
            .px(px(12.))
            .py(px(8.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(widgets::caption("Classes the model should find. A class named like the folder is ticked automatically.", theme))
            .when(!classes.is_empty(), |el| {
                let all_ids: Vec<i64> = classes.iter().map(|(id, _, _)| *id).collect();
                let all_selected = all_ids.iter().all(|id| self.autolabel.targets.contains(id));
                el.child(
                    div()
                        .mt(px(4.))
                        .pb(px(4.))
                        .border_b_1()
                        .border_color(theme.rgb(theme.divider))
                        .child(
                            widgets::checkbox(
                                "al-target-all",
                                format!("Select all ({})", all_ids.len()),
                                all_selected,
                                theme,
                            )
                            .on_click(cx.listener(move |ws: &mut Workspace, _, _, cx| {
                                if all_selected {
                                    ws.autolabel.targets.clear();
                                } else {
                                    ws.autolabel.targets.extend(all_ids.iter().copied());
                                }
                                ws.autolabel_reload_examples(cx);
                                cx.notify();
                            })),
                        ),
                )
            })
            .child(
                div()
                    .id("al-target-list")
                    .mt(px(4.))
                    .max_h(px(170.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(target_rows),
            );

        // --- Examples per target ----------------------------------------------
        let examples_header = widgets::pane_header_collapsible(
            "al-col-examples",
            "Examples & hints",
            self.autolabel.collapse_examples,
            None,
            theme,
        )
        .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.autolabel.collapse_examples = !ws.autolabel.collapse_examples;
            cx.notify();
        }));
        let mut class_blocks: Vec<gpui::AnyElement> = Vec::new();
        for (ci, (id, name, color)) in classes.iter().enumerate() {
            if !self.autolabel.targets.contains(id) {
                continue;
            }
            let examples = self.autolabel.examples.get(name).cloned().unwrap_or_default();
            let entry = self.autolabel.desc_entries.get(name).cloned();
            let add_name = name.clone();
            let pick_name = name.clone();
            let guide_pages: Vec<String> = self
                .autolabel
                .guide
                .pages_for(name)
                .iter()
                .map(|p| format!("p.{}", p.page))
                .collect();
            let rows: Vec<gpui::AnyElement> = examples
                .iter()
                .enumerate()
                .map(|(ei, ex)| {
                    let file = ex.image.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                    let n_boxes = ex.boxes.iter().filter(|b| b.class == *name).count();
                    let path = ex.image.clone();
                    div()
                        .id(("al-ex", ci * 1000 + ei))
                        .h(px(24.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .rounded(px(5.))
                        .text_size(px(11.5))
                        .hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.08)))
                        .child(super::icons::icon("image", 12.0, theme.rgb(theme.fg_muted)))
                        .child(div().flex_1().min_w_0().truncate().child(file))
                        .child(div().flex_none().text_color(theme.rgb(theme.fg_muted)).child(
                            if n_boxes == 0 { "negative".to_string() } else { format!("{n_boxes} box") },
                        ))
                        .child(
                            div()
                                .id(("al-ex-rm", ci * 1000 + ei))
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.rgba(theme.danger, 0.15)))
                                .child(super::icons::icon("trash", 12.0, theme.rgb(theme.fg_muted)))
                                .on_click(cx.listener(move |ws: &mut Workspace, _, _, cx| {
                                    ws.autolabel_remove_example(path.clone(), cx);
                                })),
                        )
                        .into_any_element()
                })
                .collect();
            class_blocks.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .pb(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(widgets::color_swatch(*color, px(10.), theme))
                            .child(div().flex_1().min_w_0().truncate().text_size(px(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD).child(name.clone()))
                            .child(div().text_size(px(11.))
                                .text_color(if examples.is_empty() && guide_pages.is_empty() {
                                    theme.rgb(theme.warning)
                                } else {
                                    theme.rgb(theme.fg_muted)
                                })
                                .child(format!("{} example(s)", examples.len()))),
                    )
                    .when(!guide_pages.is_empty(), |el| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_size(px(11.))
                                .text_color(theme.rgb(theme.fg_muted))
                                .child(super::icons::icon("file-text", 11.0, theme.rgb(theme.fg_muted)))
                                .child(format!("Guide {}", guide_pages.join(", "))),
                        )
                    })
                    .when_some(entry, |el, entry| el.child(entry))
                    .children(rows)
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(
                                widgets::button_with_icon(
                                    ("al-add", ci),
                                    "Add current",
                                    Some("image"),
                                    BtnVariant::Default,
                                    theme,
                                )
                                .flex_1()
                                .h(px(26.))
                                .text_size(px(11.5))
                                .when(!has_image, |el| el.opacity(0.5))
                                .on_click(cx.listener(move |ws: &mut Workspace, _, _, cx| {
                                    ws.autolabel_add_example(add_name.clone(), cx);
                                })),
                            )
                            .child(
                                widgets::button_with_icon(
                                    ("al-pick", ci),
                                    "Pick from folder",
                                    Some("folder-open"),
                                    BtnVariant::Default,
                                    theme,
                                )
                                .flex_1()
                                .h(px(26.))
                                .text_size(px(11.5))
                                .when(!has_folder, |el| el.opacity(0.5))
                                .on_click(cx.listener(move |ws: &mut Workspace, _, _, cx| {
                                    ws.autolabel_pick_examples(pick_name.clone(), cx);
                                })),
                            ),
                    )
                    .into_any_element(),
            );
        }
        // --- Labeling guide ----------------------------------------------------
        let g = &self.autolabel.guide;
        let guide_box = {
            let busy = self.autolabel.guide_busy.clone();
            let mut sources: Vec<&str> = g.pages.iter().map(|p| p.source.as_str()).collect();
            sources.dedup();
            let covered: BTreeSet<&String> = g.pages.iter().flat_map(|p| p.classes.iter()).collect();
            let summary = if g.pages.is_empty() {
                "No guide yet. Import your \"what to label\" doc (PDF slides or annotated screenshots): the model then sees the right page for each class, and empty hints get filled from it.".to_string()
            } else {
                format!("{} page(s) covering {} class(es) · {}", g.pages.len(), covered.len(), sources.join(", "))
            };
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .p(px(8.))
                .mb(px(6.))
                .rounded(px(8.))
                .bg(theme.rgba(theme.fg_muted, 0.06))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(super::icons::icon("file-text", 12.0, theme.rgb(theme.fg_muted)))
                        .child(div().flex_1().text_size(px(12.)).font_weight(gpui::FontWeight::SEMIBOLD).child("Labeling guide"))
                        .when(!g.pages.is_empty() && busy.is_none(), |el| {
                            el.child(widgets::header_link("al-guide-clear", "Remove", theme).on_click(
                                cx.listener(|ws: &mut Workspace, _, _, cx| ws.autolabel_clear_guide(cx)),
                            ))
                        }),
                )
                .child(widgets::caption(summary, theme))
                .when_some(busy.clone(), |el, text| {
                    el.child(div().text_size(px(11.)).text_color(theme.rgb(theme.accent)).child(text))
                })
                .when(busy.is_none(), |el| {
                    el.child(
                        widgets::button_with_icon(
                            "al-guide-import",
                            if g.pages.is_empty() { "Import guide…" } else { "Add more pages…" },
                            Some("file-text"),
                            BtnVariant::Default,
                            theme,
                        )
                        .h(px(26.))
                        .text_size(px(11.5))
                        .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| ws.autolabel_import_guide(cx))),
                    )
                })
        };

        let examples_content = div()
            .id("al-examples")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(12.))
            .py(px(8.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(guide_box)
            .child(widgets::caption(
                "Examples: \"Pick from folder\" takes frames you already labeled for the class; \"Add current\" adds the open image (with no box of the class it teaches what not to label).",
                theme,
            ))
            .when(class_blocks.is_empty(), |el| el.child(widgets::caption("Tick a target class first.", theme)))
            .children(class_blocks);

        // --- Actions ----------------------------------------------------------
        let message = self.autolabel.message.clone();
        let actions = div()
            .flex_none()
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .bg(theme.rgb(theme.bg_chrome))
            .when_some(message, |el, (is_err, text)| {
                el.child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .text_color(if is_err { theme.rgb(theme.error) } else { theme.rgb(theme.fg_muted) })
                        .child(crate::autolabel::client::truncate(&text, 400)),
                )
            })
            .when(pending > 0 || empty > 0 || failed > 0, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(11.))
                        .child(div().flex_1().text_color(theme.rgb(theme.fg_muted)).child(format!(
                            "{pending} to review · {empty} empty · {failed} failed"
                        )))
                        .when(empty + failed > 0, |el| {
                            el.child(widgets::header_link("al-retry", "Retry empty/failed", theme).on_click(
                                cx.listener(|ws: &mut Workspace, _, _, cx| ws.autolabel_retry_flags(cx)),
                            ))
                        })
                        .when(pending > 0, |el| {
                            el.child(widgets::header_link("al-next", "Next to review", theme).on_click(
                                cx.listener(|ws: &mut Workspace, _, _, cx| ws.autolabel_next_pending(cx)),
                            ))
                        }),
                )
            })
            .when(preview_active, |el| {
                el.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            widgets::button("al-accept", "Accept suggestions", BtnVariant::Accent, theme)
                                .flex_1()
                                .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| ws.autolabel_accept_preview(cx))),
                        )
                        .child(
                            widgets::button("al-discard", "Discard", BtnVariant::Default, theme)
                                .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| ws.autolabel_discard_preview(cx))),
                        ),
                )
            })
            .child(
                widgets::button_with_icon(
                    "al-test",
                    if self.autolabel.testing { "Asking the model…" } else { "Test on current image" },
                    Some("eye"),
                    BtnVariant::Default,
                    theme,
                )
                .w_full()
                .when(busy || !has_image, |el| el.opacity(0.5))
                .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| ws.autolabel_test_current(cx))),
            )
            .child(
                widgets::button_with_icon(
                    "al-run",
                    "Auto-label unlabeled images",
                    Some("sparkles"),
                    BtnVariant::Accent,
                    theme,
                )
                .w_full()
                .when(busy || !has_folder, |el| el.opacity(0.5))
                .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| ws.autolabel_run_batch(cx))),
            );

        div()
            .id("autolabel-tab")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(setup)
            .child(widgets::h_divider(theme))
            .child(targets_header)
            .when(self.autolabel.collapse_targets, |el| el.child(targets_content))
            .child(widgets::h_divider(theme))
            .child(examples_header)
            .when(self.autolabel.collapse_examples, |el| el.child(examples_content))
            .when(!self.autolabel.collapse_examples, |el| el.child(div().flex_1()))
            .child(widgets::h_divider(theme))
            .child(actions)
            .into_any_element()
    }
}
