//! Labeling guide: the project's "what to label" documentation (a PDF of
//! slides, or annotated screenshots) attached to a class preset. Each page is
//! stored as an image under `data/examples/<preset>/guide/`, tagged with the
//! classes it explains, and sent with the prompt as a visual reference for
//! those classes.
//!
//! Tagging is done by the model at import time (one small request per page):
//! it reads the class names written on the page next to the outlined
//! elements, and writes a one-line hint for each.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::examples;
use super::settings::AutoLabelSettings;

pub const DIR: &str = "guide";
const INDEX: &str = "guide.json";
/// Pages are rendered this wide (px); prompts downscale them to `max_side`.
const RENDER_WIDTH: u32 = 1600;
/// Guide pages sent per class / per request at most.
pub const MAX_PAGES_PER_CLASS: usize = 2;
pub const MAX_PAGES_PER_REQUEST: usize = 4;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GuidePage {
    /// Image file name inside the guide folder.
    pub file: String,
    /// Original document name and page number (1-based) it came from.
    pub source: String,
    pub page: u32,
    /// Class names this page shows / explains.
    pub classes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Guide {
    pub pages: Vec<GuidePage>,
}

pub fn guide_dir(root: &Path, preset: &str) -> PathBuf {
    examples::preset_dir(root, preset).join(DIR)
}

impl Guide {
    pub fn load(root: &Path, preset: &str) -> Self {
        std::fs::read_to_string(guide_dir(root, preset).join(INDEX))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path, preset: &str) -> Result<(), String> {
        let dir = guide_dir(root, preset);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(INDEX), text).map_err(|e| e.to_string())
    }

    /// Pages that explain `class`, in document order.
    pub fn pages_for(&self, class: &str) -> Vec<&GuidePage> {
        self.pages.iter().filter(|p| p.classes.iter().any(|c| c == class)).collect()
    }

    /// Pages to send for these target classes: up to
    /// [`MAX_PAGES_PER_CLASS`] per class, each page once, at most
    /// [`MAX_PAGES_PER_REQUEST`] in total. Prefers pages about fewer classes
    /// (more focused).
    pub fn pages_for_targets(&self, targets: &[String]) -> Vec<&GuidePage> {
        let mut chosen: Vec<&GuidePage> = Vec::new();
        for class in targets {
            let mut pages = self.pages_for(class);
            pages.sort_by_key(|p| p.classes.len());
            for page in pages.into_iter().take(MAX_PAGES_PER_CLASS) {
                if chosen.len() < MAX_PAGES_PER_REQUEST && !chosen.iter().any(|c| c.file == page.file) {
                    chosen.push(page);
                }
            }
        }
        chosen
    }
}

/// Removes the whole guide of a preset.
pub fn clear(root: &Path, preset: &str) -> Result<(), String> {
    let dir = guide_dir(root, preset);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Turns the chosen documents into page images in the guide folder.
/// PDFs are rendered page by page; images are copied. Returns the new pages
/// (untagged).
pub fn import_sources(root: &Path, preset: &str, sources: &[PathBuf]) -> Result<Vec<GuidePage>, String> {
    let dir = guide_dir(root, preset);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Continue after the highest page number on disk (pages that explained
    // nothing were deleted, so the count of kept pages can collide).
    let mut next = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_prefix("page_")?.strip_suffix(".png")?.parse::<usize>().ok()
        })
        .max()
        .unwrap_or(0)
        + 1;
    let mut out = Vec::new();
    for src in sources {
        let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if ext == "pdf" {
            let pages = render_pdf(src, RENDER_WIDTH)?;
            for (i, png) in pages.into_iter().enumerate() {
                let file = format!("page_{next:03}.png");
                std::fs::write(dir.join(&file), png).map_err(|e| format!("Can't save {file}: {e}"))?;
                out.push(GuidePage { file, source: name.clone(), page: i as u32 + 1, classes: Vec::new() });
                next += 1;
            }
        } else if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp") {
            // Re-encode to PNG so every page decodes the same way.
            let img = image::open(src).map_err(|e| format!("Can't read {name}: {e}"))?;
            let file = format!("page_{next:03}.png");
            img.save(dir.join(&file)).map_err(|e| format!("Can't save {file}: {e}"))?;
            out.push(GuidePage { file, source: name.clone(), page: 1, classes: Vec::new() });
            next += 1;
        } else {
            return Err(format!("{name}: only PDF and image files can be imported."));
        }
    }
    Ok(out)
}

/// Request asking which of `classes` a guide page explains, plus a short hint each.
pub fn tag_request(settings: &AutoLabelSettings, classes: &[String], page_data_url: &str) -> Value {
    let list = classes.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
    let mut body = json!({
        "model": settings.model.trim(),
        "messages": [
            {"role": "system", "content": format!(
                "You read pages of a labeling guide for an object-detection dataset of video-game screenshots. \
                 A page shows screenshots where on-screen elements are outlined with colored rectangles and named \
                 with text labels, arrows or a heading.\n\
                 Known classes: {list}.\n\
                 For the page you get, list every known class that the page outlines or explains. For each, write a \
                 one-sentence hint (max 20 words) describing what to box: the element's look, text and usual \
                 screen position, as shown on the page.\n\
                 Only use names from the known list, exactly as written. If the page explains none (title page, \
                 table of contents, blank), return an empty list.\n\
                 Reply with JSON only: {{\"classes\": [{{\"name\": \"<class>\", \"hint\": \"<hint>\"}}]}}"
            )},
            {"role": "user", "content": [
                {"type": "text", "text": "Guide page:"},
                {"type": "image_url", "image_url": {"url": page_data_url, "detail": "high"}},
            ]},
        ],
        "stream": false,
    });
    let effort = settings.reasoning_effort.trim();
    if !effort.is_empty() {
        body["reasoning_effort"] = Value::String(effort.to_string());
    }
    body
}

/// Parses a tagging reply into `(class, hint)` pairs restricted to `classes`.
pub fn parse_tags(reply: &str, classes: &[String]) -> Result<Vec<(String, String)>, String> {
    let start = reply.find('{').ok_or("No JSON in the reply")?;
    let end = reply.rfind('}').ok_or("No JSON in the reply")?;
    let value: Value =
        serde_json::from_str(&reply[start..=end]).map_err(|e| format!("Unreadable reply: {e}"))?;
    let items = value.get("classes").and_then(Value::as_array).cloned().unwrap_or_default();
    let norm = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let mut out: Vec<(String, String)> = Vec::new();
    for item in items {
        let (name, hint) = match &item {
            Value::String(s) => (s.clone(), String::new()),
            Value::Object(o) => (
                o.get("name").or_else(|| o.get("class")).and_then(Value::as_str).unwrap_or_default().to_string(),
                o.get("hint").and_then(Value::as_str).unwrap_or_default().trim().to_string(),
            ),
            _ => continue,
        };
        if let Some(known) = classes.iter().find(|c| norm(c) == norm(&name)) {
            if !out.iter().any(|(c, _)| c == known) {
                out.push((known.clone(), hint));
            }
        }
    }
    Ok(out)
}

/// Merges guide hints into the class descriptions without overwriting any
/// the user wrote. Returns how many were filled.
pub fn merge_hints(descriptions: &mut BTreeMap<String, String>, hints: &[(String, String)]) -> usize {
    let mut filled = 0;
    for (class, hint) in hints {
        if hint.is_empty() {
            continue;
        }
        let slot = descriptions.entry(class.clone()).or_default();
        if slot.trim().is_empty() {
            *slot = hint.clone();
            filled += 1;
        }
    }
    filled
}

/// Renders every page of a PDF to PNG bytes with the Windows PDF renderer.
#[cfg(windows)]
pub fn render_pdf(path: &Path, width: u32) -> Result<Vec<Vec<u8>>, String> {
    use windows::core::HSTRING;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    let err = |what: &str, e: windows::core::Error| format!("{what}: {}", e.message());
    // Fine if already initialized on this thread.
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    let abs = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let abs = abs.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(abs.as_str()))
        .and_then(|op| op.get())
        .map_err(|e| err("Can't open the PDF", e))?;
    let doc = PdfDocument::LoadFromFileAsync(&file)
        .and_then(|op| op.get())
        .map_err(|e| err("Can't read the PDF", e))?;
    let count = doc.PageCount().map_err(|e| err("PDF page count", e))?;
    let mut pages = Vec::with_capacity(count as usize);
    for i in 0..count {
        let render = || -> windows::core::Result<Vec<u8>> {
            let page = doc.GetPage(i)?;
            let stream = InMemoryRandomAccessStream::new()?;
            let options = PdfPageRenderOptions::new()?;
            options.SetDestinationWidth(width)?;
            page.RenderWithOptionsToStreamAsync(&stream, &options)?.get()?;
            let size = stream.Size()? as u32;
            let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
            reader.LoadAsync(size)?.get()?;
            let mut buf = vec![0u8; size as usize];
            reader.ReadBytes(&mut buf)?;
            Ok(buf)
        };
        pages.push(render().map_err(|e| err(&format!("Can't render PDF page {}", i + 1), e))?);
    }
    Ok(pages)
}

#[cfg(not(windows))]
pub fn render_pdf(_path: &Path, _width: u32) -> Result<Vec<Vec<u8>>, String> {
    Err("PDF import needs Windows; export the guide pages as images instead.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(file: &str, classes: &[&str]) -> GuidePage {
        GuidePage { file: file.into(), source: "g.pdf".into(), page: 1, classes: classes.iter().map(|c| c.to_string()).collect() }
    }

    #[test]
    fn picks_focused_pages_once() {
        let g = Guide {
            pages: vec![
                page("p1.png", &["a", "b", "c"]),
                page("p2.png", &["a"]),
                page("p3.png", &["b"]),
                page("p4.png", &["a", "b"]),
            ],
        };
        let chosen: Vec<&str> = g.pages_for_targets(&["a".into(), "b".into()]).iter().map(|p| p.file.as_str()).collect();
        assert_eq!(chosen, vec!["p2.png", "p4.png", "p3.png"]);
        assert!(g.pages_for_targets(&["zzz".into()]).is_empty());
    }

    #[test]
    fn parses_tags_leniently() {
        let classes: Vec<String> = ["kill_banner", "red_kill_msg"].iter().map(|s| s.to_string()).collect();
        let reply = "```json\n{\"classes\":[{\"name\":\"Kill Banner\",\"hint\":\"big banner\"},{\"name\":\"red_kill_msg\",\"hint\":\"\"},{\"name\":\"other\"}]}\n```";
        let tags = parse_tags(reply, &classes).unwrap();
        assert_eq!(tags, vec![("kill_banner".to_string(), "big banner".to_string()), ("red_kill_msg".to_string(), String::new())]);
        assert!(parse_tags("{\"classes\":[]}", &classes).unwrap().is_empty());
        assert!(parse_tags("nope", &classes).is_err());
    }

    #[test]
    fn hints_never_overwrite() {
        let mut d = BTreeMap::new();
        d.insert("a".to_string(), "mine".to_string());
        let n = merge_hints(&mut d, &[("a".into(), "guide".into()), ("b".into(), "guide b".into()), ("c".into(), String::new())]);
        assert_eq!(n, 1);
        assert_eq!(d["a"], "mine");
        assert_eq!(d["b"], "guide b");
        assert!(!d.contains_key("c"));
    }

    #[test]
    fn save_load_round_trip() {
        let root = std::env::temp_dir().join(format!("al-guide-{}", std::process::id()));
        let g = Guide { pages: vec![page("page_001.png", &["x"])] };
        g.save(&root, "wz").unwrap();
        assert_eq!(Guide::load(&root, "wz"), g);
        clear(&root, "wz").unwrap();
        assert!(Guide::load(&root, "wz").pages.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn new_pages_never_overwrite_old_ones() {
        let root = std::env::temp_dir().join(format!("al-guide-names-{}", std::process::id()));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let img = src.join("shot.png");
        image::RgbImage::new(8, 8).save(&img).unwrap();
        let first = import_sources(&root, "wz", &[img.clone(), img.clone()]).unwrap();
        assert_eq!(first.iter().map(|p| p.file.as_str()).collect::<Vec<_>>(), ["page_001.png", "page_002.png"]);
        // Page 1 explained nothing and was dropped; only page 2 is kept.
        std::fs::remove_file(guide_dir(&root, "wz").join("page_001.png")).unwrap();
        Guide { pages: vec![first[1].clone()] }.save(&root, "wz").unwrap();
        let second = import_sources(&root, "wz", &[img]).unwrap();
        assert_eq!(second[0].file, "page_003.png");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Renders the real guide when it's present on this machine.
    #[cfg(windows)]
    #[test]
    fn renders_real_pdf_if_present() {
        let pdf = Path::new(r"E:\Label Studio\Docs\Warzone MW4 Label requirement (1).pdf");
        if !pdf.exists() {
            return;
        }
        let pages = render_pdf(pdf, 800).unwrap();
        assert!(pages.len() >= 10, "{} pages", pages.len());
        let img = image::load_from_memory(&pages[2]).unwrap();
        assert_eq!(img.width(), 800);
    }
}
