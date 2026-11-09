//! Background image decoding into GPUI `RenderImage`s with LRU caching.
//! Replaces the Python preloader; no CPU-side rescaling is needed because the
//! GPU samples the full-resolution texture during zoom/pan (the whole
//! `accel.py` proxy pipeline is unnecessary in this port).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, AppContext, Context, Entity, RenderImage};
use image::{DynamicImage, Frame, RgbaImage};

/// Tiny error alias so this module doesn't drag in `anyhow`.
pub type DecodeResult<T> = std::result::Result<T, String>;

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
    let rgba = thumb.into_rgba8();
    Ok(Arc::new(rgba_to_render_image(rgba)))
}

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

    pub fn put(&mut self, key: PathBuf, value: V) {
        if self.map.contains_key(&key) {
            if let Some(pos) = self.order.iter().position(|k| *k == key) {
                self.order.remove(pos);
            }
        }
        self.map.insert(key.clone(), value);
        self.order.push_back(key);
        while self.order.len() > self.cap {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

/// Global decoded-image store shared by the canvas and the filmstrip.
pub struct ImageStore {
    /// Full-resolution decoded frames (LRU 7, like the Python preloader).
    full: Lru<Arc<RenderImage>>,
    /// 96×54 thumbnails (LRU 300).
    thumbs: Lru<Arc<RenderImage>>,
}

impl gpui::Global for ImageStore {}

impl ImageStore {
    pub fn init(cx: &mut App) {
        cx.set_global(Self {
            full: Lru::new(7),
            thumbs: Lru::new(300),
        });
    }

    pub fn full_cached(cx: &mut App, path: &Path) -> Option<Arc<RenderImage>> {
        cx.global_mut::<Self>().full.get(path)
    }

    pub fn thumb_cached(cx: &mut App, path: &Path) -> Option<Arc<RenderImage>> {
        cx.global_mut::<Self>().thumbs.get(path)
    }

    pub fn cache_full(cx: &mut App, path: &Path, image: Arc<RenderImage>) {
        cx.global_mut::<Self>().full.put(path.to_path_buf(), image);
    }

    pub fn cache_thumb(cx: &mut App, path: &Path, image: Arc<RenderImage>) {
        cx.global_mut::<Self>().thumbs.put(path.to_path_buf(), image);
    }

    pub fn clear_all(cx: &mut App) {
        let store = cx.global_mut::<Self>();
        store.full.clear();
        store.thumbs.clear();
    }
}

/// Decodes `path` on the background pool; on completion updates `entity` via
/// `apply` on the main thread (the path is handed back with the result).
/// Check the cache first with `full_cached`.
pub fn load_full<E, F>(
    cx: &mut App,
    path: PathBuf,
    entity: Entity<E>,
    apply: F,
) where
    E: 'static,
    F: Fn(&mut E, (PathBuf, DecodeResult<(Arc<RenderImage>, (u32, u32))>), &mut Context<E>) + 'static,
{
    let p = path.clone();
    let task = cx.background_spawn(async move { decode_render_image(&p) });
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let result = task.await;
        let _ = cx.update(|cx| {
            entity.update(cx, |el, cx| apply(el, (path, result), cx));
        });
    })
    .detach();
}

/// Preloads a path into the store without touching any entity.
pub fn preload_full(cx: &mut App, path: PathBuf) {
    if ImageStore::full_cached(cx, &path).is_some() {
        return;
    }
    let p = path.clone();
    let task = cx.background_spawn(async move { decode_render_image(&p) });
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        if let Ok((image, _)) = task.await {
            let _ = cx.update(|cx| {
                ImageStore::cache_full(cx, &path, image);
            });
        }
    })
    .detach();
}

/// Decodes a 96×54 thumbnail on the background pool and updates `entity`.
pub fn load_thumb<E, F>(
    cx: &mut App,
    path: PathBuf,
    entity: Entity<E>,
    apply: F,
) where
    E: 'static,
    F: Fn(&mut E, DecodeResult<Arc<RenderImage>>, &mut Context<E>) + 'static,
{
    let p = path.clone();
    let task = cx.background_spawn(async move { decode_thumbnail(&p, 96, 54) });
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let result = task.await;
        let _ = cx.update(|cx| {
            entity.update(cx, |el, cx| apply(el, result, cx));
        });
    })
    .detach();
}
