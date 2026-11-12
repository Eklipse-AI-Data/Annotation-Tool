//! Background image decoding into GPUI `RenderImage`s with LRU caching.
//! Replaces the Python preloader; no CPU-side rescaling is needed because the
//! GPU samples the full-resolution texture during zoom/pan (the whole
//! `accel.py` proxy pipeline is unnecessary in this port).
//!
//! # Memory model (why this module looks the way it does)
//!
//! Two resources are bounded here, and both used to be unbounded:
//!
//! * **Decode concurrency (RAM).** GPUI's Windows background executor is the
//!   OS thread pool, which grows its thread count when work items block. Every
//!   decode briefly holds a full-resolution bitmap (≈33 MB for 4K RGBA), so
//!   "one task per request" let fast filmstrip scrolling or a held arrow key
//!   run dozens of decodes at once and run low-end machines out of memory.
//!   All decodes now go through a [`Lane`] with a fixed number of permits,
//!   and queued work that has been superseded is skipped before decoding.
//! * **GPU textures (VRAM).** GPUI uploads each `RenderImage` into the window's
//!   sprite atlas the first time it is painted and never frees it on its own.
//!   Evicting an image from the LRU therefore freed the CPU copy but leaked
//!   the texture. Evicted images are now *retired*, and their textures are
//!   released once nothing else holds the image (see [`ImageStore::collect`]).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use gpui::{App, AppContext, Context, Entity, RenderImage};
use image::{DynamicImage, Frame, RgbaImage};
use smol::lock::Semaphore;

/// Tiny error alias so this module doesn't drag in `anyhow`.
pub type DecodeResult<T> = std::result::Result<T, String>;

pub const THUMB_DECODE_W: u32 = 96;
pub const THUMB_DECODE_H: u32 = 54;

/// Decodes an image file into a BGRA8 GPUI render image plus its pixel size.
pub fn decode_render_image(path: &Path) -> DecodeResult<(Arc<RenderImage>, (u32, u32))> {
    let img: DynamicImage = image::ImageReader::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?
        .decode()
        .map_err(|e| format!("decode {}: {e}", path.display()))?;
    let (w, h) = (img.width(), img.height());
    let rgba = img.into_rgba8();
    let render = rgba_to_render_image(rgba);
    Ok((Arc::new(render), (w, h)))
}

/// RGBA8 buffer → BGRA `RenderImage` (GPUI expects BGRA byte order).
pub fn rgba_to_render_image(mut rgba: RgbaImage) -> RenderImage {
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    RenderImage::new(vec![Frame::new(rgba)])
}

/// Decodes and fits a thumbnail inside `max_w x max_h` (like PIL's
/// `Image.thumbnail` — aspect preserved, never upscaled).
pub fn decode_thumbnail(path: &Path, max_w: u32, max_h: u32) -> DecodeResult<Arc<RenderImage>> {
    let img: DynamicImage = image::ImageReader::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?
        .decode()
        .map_err(|e| format!("decode {}: {e}", path.display()))?;
    let thumb = img.thumbnail(max_w, max_h);
    drop(img); // release the full-resolution bitmap before converting
    let rgba = thumb.into_rgba8();
    Ok(Arc::new(rgba_to_render_image(rgba)))
}

// ----------------------------------------------------------------------------
// LRU
// ----------------------------------------------------------------------------

/// A minimal LRU cache (recency order in a deque, values in a map).
pub struct Lru<V> {
    map: HashMap<PathBuf, V>,
    order: VecDeque<PathBuf>,
    cap: usize,
}

impl<V: Clone> Lru<V> {
    pub fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap,
        }
    }

    pub fn get(&mut self, key: &Path) -> Option<V> {
        self.map.get(key).cloned().map(|value| {
            if let Some(pos) = self.order.iter().position(|k| k == key) {
                self.order.remove(pos);
                self.order.push_back(key.to_path_buf());
            }
            value
        })
    }

    pub fn contains(&self, key: &Path) -> bool {
        self.map.contains_key(key)
    }

    /// Inserts `value`; returns whatever was displaced (a replaced value for
    /// the same key and/or entries evicted over capacity).
    pub fn put(&mut self, key: PathBuf, value: V) -> Vec<V> {
        let mut displaced = Vec::new();
        if let Some(old) = self.map.remove(&key) {
            displaced.push(old);
            if let Some(pos) = self.order.iter().position(|k| *k == key) {
                self.order.remove(pos);
            }
        }
        self.map.insert(key.clone(), value);
        self.order.push_back(key);
        while self.order.len() > self.cap {
            if let Some(oldest) = self.order.pop_front() {
                displaced.extend(self.map.remove(&oldest));
            }
        }
        displaced
    }

    /// Empties the cache, returning every value it held.
    pub fn drain(&mut self) -> Vec<V> {
        self.order.clear();
        self.map.drain().map(|(_, v)| v).collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

// ----------------------------------------------------------------------------
// Decode lanes (bounded concurrency + supersession)
// ----------------------------------------------------------------------------

/// A class of decode work with its own concurrency budget.
///
/// Each lane is a semaphore plus a generation counter. Callers capture the
/// generation when they enqueue; [`Lane::supersede`] bumps it, and queued
/// work from an older generation is skipped once it gets a permit, so a
/// burst of requests collapses to the latest one instead of decoding all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// The image the canvas is about to show. One at a time; newer requests
    /// supersede older queued ones.
    Current,
    /// Neighbor preloads. One at a time; each navigation supersedes the
    /// previous batch.
    Preload,
    /// Filmstrip thumbnails. The filmstrip drops off-screen requests itself
    /// (see `ThumbLoad` in the filmstrip); this caps the decodes in flight.
    Thumb,
}

struct LaneState {
    permits: Arc<Semaphore>,
    generation: Arc<AtomicU64>,
}

/// Thumbnail decodes allowed in flight at once (each one briefly holds a
/// full-resolution bitmap, so this stays small on low-core machines).
pub fn thumb_budget() -> usize {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    if cores >= 8 { 2 } else { 1 }
}

fn lanes() -> &'static [LaneState; 3] {
    static LANES: OnceLock<[LaneState; 3]> = OnceLock::new();
    LANES.get_or_init(|| {
        let lane = |n| LaneState {
            permits: Arc::new(Semaphore::new(n)),
            generation: Arc::new(AtomicU64::new(0)),
        };
        [lane(1), lane(1), lane(thumb_budget())]
    })
}

impl Lane {
    fn state(self) -> &'static LaneState {
        &lanes()[self as usize]
    }

    /// Invalidates everything queued on this lane so far.
    pub fn supersede(self) {
        self.state().generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Runs `decode` on the background pool once this lane has a permit.
    /// Returns `None` without decoding if the lane was superseded meanwhile.
    fn run<T: Send + 'static>(
        self,
        cx: &App,
        decode: impl FnOnce() -> T + Send + 'static,
    ) -> gpui::Task<Option<T>> {
        let LaneState { permits, generation } = self.state();
        let (permits, generation) = (permits.clone(), generation.clone());
        let enqueued_at = generation.load(Ordering::SeqCst);
        cx.background_spawn(async move {
            let _permit = permits.acquire_arc().await;
            if generation.load(Ordering::SeqCst) != enqueued_at {
                return None;
            }
            Some(decode())
        })
    }
}

// ----------------------------------------------------------------------------
// Store
// ----------------------------------------------------------------------------

/// Global decoded-image store shared by the canvas and the filmstrip.
pub struct ImageStore {
    /// Full-resolution decoded frames (LRU 7, like the Python preloader).
    full: Lru<Arc<RenderImage>>,
    /// 96×54 thumbnails (LRU 300).
    thumbs: Lru<Arc<RenderImage>>,
    /// Images evicted from the caches whose GPU textures are not released
    /// yet, tagged with the collection cycle they were retired in.
    retired: Vec<(u64, Arc<RenderImage>)>,
    cycle: u64,
}

impl gpui::Global for ImageStore {}

impl ImageStore {
    pub fn init(cx: &mut App) {
        cx.set_global(Self {
            full: Lru::new(7),
            thumbs: Lru::new(300),
            retired: Vec::new(),
            cycle: 0,
        });
    }

    pub fn full_cached(cx: &mut App, path: &Path) -> Option<Arc<RenderImage>> {
        cx.global_mut::<Self>().full.get(path)
    }

    pub fn thumb_cached(cx: &mut App, path: &Path) -> Option<Arc<RenderImage>> {
        cx.global_mut::<Self>().thumbs.get(path)
    }

    pub fn cache_full(cx: &mut App, path: &Path, image: Arc<RenderImage>) {
        let store = cx.global_mut::<Self>();
        let displaced = store.full.put(path.to_path_buf(), image);
        store.retire(displaced);
        Self::collect(cx);
    }

    pub fn cache_thumb(cx: &mut App, path: &Path, image: Arc<RenderImage>) {
        let store = cx.global_mut::<Self>();
        let displaced = store.thumbs.put(path.to_path_buf(), image);
        store.retire(displaced);
        Self::collect(cx);
    }

    /// Drops every cached image (project switch). Textures are released as
    /// soon as the views stop showing them.
    pub fn clear_all(cx: &mut App) {
        Lane::Current.supersede();
        Lane::Preload.supersede();
        let store = cx.global_mut::<Self>();
        let mut displaced = store.full.drain();
        displaced.extend(store.thumbs.drain());
        store.retire(displaced);
        Self::collect(cx);
    }

    fn retire(&mut self, images: Vec<Arc<RenderImage>>) {
        let cycle = self.cycle;
        self.retired.extend(images.into_iter().map(|image| (cycle, image)));
    }

    /// Releases the GPU textures of retired images that are safe to free.
    ///
    /// "Safe" means two things, because GPUI's renderer unwraps the atlas
    /// texture for every sprite in the frame it presents:
    /// 1. nothing else holds the image (`strong_count == 1`) — e.g. the
    ///    canvas still showing an evicted frame keeps it alive; and
    /// 2. it was retired in an *earlier* cycle, so at least one frame was
    ///    built without it before its texture disappears.
    ///
    /// The release itself is deferred until the current update finishes,
    /// because `App::drop_image` skips the window that is mid-update.
    fn collect(cx: &mut App) {
        let store = cx.global_mut::<Self>();
        store.cycle += 1;
        let cycle = store.cycle;
        let (free, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut store.retired)
            .into_iter()
            .partition(|(retired_at, image)| *retired_at + 1 < cycle && Arc::strong_count(image) == 1);
        store.retired = keep;
        if free.is_empty() {
            return;
        }
        cx.defer(move |cx| {
            for (_, image) in free {
                cx.drop_image(image, None);
            }
        });
    }
}

// ----------------------------------------------------------------------------
// Loaders
// ----------------------------------------------------------------------------

/// Decodes `path` on the [`Lane::Current`] lane; on completion updates
/// `entity` via `apply` on the main thread (the path is handed back with the
/// result). A newer `load_full` supersedes this one while it is still queued,
/// in which case `apply` is never called. Check the cache first with
/// `full_cached`.
pub fn load_full<E, F>(
    cx: &mut App,
    path: PathBuf,
    entity: Entity<E>,
    apply: F,
) where
    E: 'static,
    F: Fn(&mut E, (PathBuf, DecodeResult<(Arc<RenderImage>, (u32, u32))>), &mut Context<E>) + 'static,
{
    Lane::Current.supersede();
    let p = path.clone();
    let task = Lane::Current.run(cx, move || decode_render_image(&p));
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let Some(result) = task.await else { return };
        let _ = cx.update(|cx| {
            entity.update(cx, |el, cx| apply(el, (path, result), cx));
        });
    })
    .detach();
}

/// Starts a new preload batch: queued preloads from the previous batch are
/// dropped before they decode.
pub fn begin_preload_batch() {
    Lane::Preload.supersede();
}

/// Preloads a path into the store without touching any entity. Queued
/// preloads are dropped by the next [`begin_preload_batch`].
pub fn preload_full(cx: &mut App, path: PathBuf) {
    if cx.global::<ImageStore>().full.contains(&path) {
        return;
    }
    let p = path.clone();
    let task = Lane::Preload.run(cx, move || decode_render_image(&p));
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let result = task.await;
        let _ = cx.update(|cx| {
            if let Some(Ok((image, _))) = result {
                ImageStore::cache_full(cx, &path, image);
            }
        });
    })
    .detach();
}

/// Decodes a 96×54 thumbnail on the [`Lane::Thumb`] lane and updates `entity`.
pub fn load_thumb<E, F>(
    cx: &mut App,
    path: PathBuf,
    entity: Entity<E>,
    apply: F,
) where
    E: 'static,
    F: Fn(&mut E, DecodeResult<Arc<RenderImage>>, &mut Context<E>) + 'static,
{
    let task = Lane::Thumb.run(cx, move || decode_thumbnail(&path, THUMB_DECODE_W, THUMB_DECODE_H));
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let result = task.await.unwrap_or_else(|| Err("superseded".into()));
        let _ = cx.update(|cx| {
            entity.update(cx, |el, cx| apply(el, result, cx));
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_reports_evictions_and_replacements() {
        let mut lru = Lru::new(2);
        assert!(lru.put("a".into(), 1).is_empty());
        assert!(lru.put("b".into(), 2).is_empty());
        assert_eq!(lru.get(Path::new("a")), Some(1)); // a is now MRU
        assert_eq!(lru.put("c".into(), 3), vec![2]); // evicts b
        assert_eq!(lru.put("a".into(), 9), vec![1]); // replaces a
        assert_eq!(lru.len(), 2);
        let mut drained = lru.drain();
        drained.sort();
        assert_eq!(drained, vec![3, 9]);
        assert_eq!(lru.len(), 0);
    }
}
