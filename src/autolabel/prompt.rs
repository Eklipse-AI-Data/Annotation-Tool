//! Builds the few-shot chat-completions request: per target class an optional
//! text hint plus example images (boxes drawn on, and the same boxes listed
//! as 0–1000 coordinates), then the query image.

use std::io::Cursor;
use std::path::Path;

use base64::Engine as _;
use image::{DynamicImage, Rgb, RgbImage};
use serde_json::{json, Value};

use super::examples::{self, NamedBox};
use super::parse::{to_scale, TargetClass};
use super::settings::AutoLabelSettings;

const JPEG_QUALITY: u8 = 85;

/// Box colors for drawn examples, cycled per target class.
const PALETTE: [(&str, [u8; 3]); 6] = [
    ("red", [255, 40, 40]),
    ("green", [40, 230, 60]),
    ("cyan", [0, 220, 255]),
    ("magenta", [255, 0, 220]),
    ("yellow", [255, 230, 0]),
    ("orange", [255, 140, 0]),
];

#[derive(Debug, Clone)]
pub struct PreparedExample {
    pub data_url: String,
    /// `(class name, [x1,y1,x2,y2])` on the 0–1000 scale.
    pub boxes: Vec<(String, [i64; 4])>,
}

#[derive(Debug, Clone)]
/// A labeling-guide page, encoded once per run.
pub struct PreparedGuide {
    pub data_url: String,
    /// Target classes this page explains.
    pub classes: Vec<String>,
}

/// Guide pages relevant to the target classes (see [`guide::Guide::pages_for_targets`]).
pub fn prepare_guide(
    root: &Path,
    preset: &str,
    targets: &[TargetClass],
    settings: &AutoLabelSettings,
) -> Result<Vec<PreparedGuide>, String> {
    let names: Vec<String> = targets.iter().map(|(_, n)| n.clone()).collect();
    let g = super::guide::Guide::load(root, preset);
    let dir = super::guide::guide_dir(root, preset);
    g.pages_for_targets(&names)
        .into_iter()
        .map(|p| {
            Ok(PreparedGuide {
                data_url: encode_image(&dir.join(&p.file), settings.max_side, &[])?,
                classes: p.classes.iter().filter(|c| names.contains(c)).cloned().collect(),
            })
        })
        .collect()
}

pub struct ClassSpec {
    pub name: String,
    pub description: String,
    pub color: &'static str,
    pub examples: Vec<PreparedExample>,
}

fn color_for(index: usize) -> (&'static str, [u8; 3]) {
    PALETTE[index % PALETTE.len()]
}

/// Decodes, downscales (longest side ≤ `max_side`), draws `overlay` boxes
/// (normalized YOLO boxes with an RGB color) and returns a JPEG data URL.
pub fn encode_image(path: &Path, max_side: u32, overlay: &[(crate::yolo::BBox, [u8; 3])]) -> Result<String, String> {
    let img = image::ImageReader::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?
        .with_guessed_format()
        .map_err(|e| format!("read {}: {e}", path.display()))?
        .decode()
        .map_err(|e| format!("decode {}: {e}", path.display()))?;
    let img = downscale(img, max_side);
    let mut rgb = img.into_rgb8();
    let thickness = (rgb.width().max(rgb.height()) / 400).max(2);
    for (b, color) in overlay {
        draw_rect(&mut rgb, b, Rgb(*color), thickness);
    }
    let mut buf = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY)
        .encode_image(&rgb)
        .map_err(|e| format!("encode {}: {e}", path.display()))?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
    ))
}

fn downscale(img: DynamicImage, max_side: u32) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    let longest = w.max(h);
    if longest <= max_side || longest == 0 {
        return img;
    }
    let scale = max_side as f64 / longest as f64;
    let nw = ((w as f64 * scale).round() as u32).max(1);
    let nh = ((h as f64 * scale).round() as u32).max(1);
    img.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
}

fn draw_rect(img: &mut RgbImage, b: &crate::yolo::BBox, color: Rgb<u8>, thickness: u32) {
    let (w, h) = (img.width() as f64, img.height() as f64);
    if w < 1.0 || h < 1.0 {
        return;
    }
    let (x1, y1, x2, y2) = b.denormalized(w, h);
    let clamp_x = |v: f64| v.round().clamp(0.0, w - 1.0) as u32;
    let clamp_y = |v: f64| v.round().clamp(0.0, h - 1.0) as u32;
    let (x1, x2, y1, y2) = (clamp_x(x1), clamp_x(x2), clamp_y(y1), clamp_y(y2));
    for t in 0..thickness {
        // Draw just outside the box so the object itself stays visible.
        let top = y1.saturating_sub(t);
        let bottom = (y2 + t).min(img.height() - 1);
        let left = x1.saturating_sub(t);
        let right = (x2 + t).min(img.width() - 1);
        for x in left..=right {
            img.put_pixel(x, top, color);
            img.put_pixel(x, bottom, color);
        }
        for y in top..=bottom {
            img.put_pixel(left, y, color);
            img.put_pixel(right, y, color);
        }
    }
}

/// Loads and encodes up to `max_examples_per_class` examples for each target
/// class from the preset's library. Only boxes of target classes are drawn
/// and listed.
pub fn prepare_specs(
    root: &Path,
    preset: &str,
    targets: &[TargetClass],
    descriptions: &std::collections::BTreeMap<String, String>,
    settings: &AutoLabelSettings,
) -> Result<Vec<ClassSpec>, String> {
    let color_of = |name: &str| -> Option<[u8; 3]> {
        targets.iter().position(|(_, n)| n == name).map(|i| color_for(i).1)
    };
    let mut specs = Vec::new();
    for (index, (_, name)) in targets.iter().enumerate() {
        let mut prepared = Vec::new();
        for ex in examples::list_examples(root, preset, name)
            .into_iter()
            .take(settings.max_examples_per_class)
        {
            let relevant: Vec<&NamedBox> =
                ex.boxes.iter().filter(|b| color_of(&b.class).is_some()).collect();
            let overlay: Vec<(crate::yolo::BBox, [u8; 3])> = relevant
                .iter()
                .map(|b| (b.to_bbox(0), color_of(&b.class).unwrap_or([255, 0, 0])))
                .collect();
            let data_url = encode_image(&ex.image, settings.max_side, &overlay)?;
            prepared.push(PreparedExample {
                data_url,
                boxes: relevant.iter().map(|b| (b.class.clone(), to_scale(&b.to_bbox(0)))).collect(),
            });
        }
        specs.push(ClassSpec {
            name: name.clone(),
            description: descriptions.get(name).cloned().unwrap_or_default(),
            color: color_for(index).0,
            examples: prepared,
        });
    }
    Ok(specs)
}

pub fn system_prompt(specs: &[ClassSpec]) -> String {
    let names: Vec<String> = specs.iter().map(|s| format!("\"{}\"", s.name)).collect();
    format!(
        "You are an expert annotator creating object-detection labels (YOLO dataset) for video-game screenshots.\n\
         Your job: find every instance of the requested classes in the final image and return a tight bounding box for each.\n\n\
         Rules:\n\
         - Allowed classes (use these exact names): {}.\n\
         - Coordinates are [x1, y1, x2, y2] integers from 0 to 1000, normalized to the image width (x) and height (y); (0,0) is the top-left corner.\n\
         - Box each element exactly the way the reference examples do: same extent, same tightness, same parts included or excluded.\n\
         - Labeling-guide pages (if given) are slides from the project's documentation: elements are outlined by hand and named with text or arrows. Use them to learn what each class covers; their outlines are not exact coordinates.\n\
         - Only label what is actually visible in the final image. Never copy positions from the examples without checking.\n\
         - If no requested class is present, return {{\"boxes\": []}}.\n\
         - Reply with JSON only, no prose: {{\"boxes\": [{{\"class\": \"<name>\", \"box_2d\": [x1, y1, x2, y2]}}]}}",
        names.join(", ")
    )
}

fn ground_truth(boxes: &[(String, [i64; 4])]) -> String {
    let items: Vec<Value> = boxes
        .iter()
        .map(|(class, b)| json!({"class": class, "box_2d": b}))
        .collect();
    json!({ "boxes": items }).to_string()
}

/// The full `/chat/completions` body.
pub fn build_request(
    settings: &AutoLabelSettings,
    specs: &[ClassSpec],
    guides: &[PreparedGuide],
    query_data_url: &str,
) -> Value {
    let mut parts: Vec<Value> = Vec::new();
    if !guides.is_empty() {
        parts.push(json!({
            "type": "text",
            "text": "Labeling guide pages from the project's documentation (what each class is; outlines drawn by hand):",
        }));
        for g in guides {
            parts.push(json!({
                "type": "text",
                "text": format!("Guide page explaining: {}", g.classes.join(", ")),
            }));
            parts.push(json!({"type": "image_url", "image_url": {"url": g.data_url, "detail": "high"}}));
        }
    }
    let legend: Vec<String> = specs.iter().map(|s| format!("{} = {}", s.color, s.name)).collect();
    parts.push(json!({
        "type": "text",
        "text": format!(
            "Reference examples follow. In example images the correct boxes are drawn as colored rectangles ({}), and the exact expected answer is given as JSON.",
            legend.join(", ")
        ),
    }));
    for spec in specs {
        let mut header = format!("Class \"{}\"", spec.name);
        if !spec.description.trim().is_empty() {
            header.push_str(&format!(": {}", spec.description.trim()));
        }
        if spec.examples.is_empty() {
            let in_guide = guides.iter().any(|g| g.classes.contains(&spec.name));
            header.push_str(if in_guide {
                " (no labeled examples; rely on the guide page and description)"
            } else {
                " (no example images; rely on the description and class name)"
            });
        }
        parts.push(json!({"type": "text", "text": header}));
        for (i, ex) in spec.examples.iter().enumerate() {
            parts.push(json!({
                "type": "text",
                "text": format!("Example {} for \"{}\" — expected answer: {}", i + 1, spec.name, ground_truth(&ex.boxes)),
            }));
            parts.push(json!({"type": "image_url", "image_url": {"url": ex.data_url, "detail": "high"}}));
        }
    }
    let names: Vec<String> = specs.iter().map(|s| format!("\"{}\"", s.name)).collect();
    parts.push(json!({
        "type": "text",
        "text": format!(
            "Now label this image (no boxes drawn on it). Find every instance of: {}. Reply with the JSON only.",
            names.join(", ")
        ),
    }));
    parts.push(json!({"type": "image_url", "image_url": {"url": query_data_url, "detail": "high"}}));

    let mut body = json!({
        "model": settings.model.trim(),
        "messages": [
            {"role": "system", "content": system_prompt(specs)},
            {"role": "user", "content": parts},
        ],
        "stream": false,
    });
    let effort = settings.reasoning_effort.trim();
    if !effort.is_empty() {
        body["reasoning_effort"] = Value::String(effort.to_string());
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yolo::BBox;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "autolabel_prompt_{tag}_{}_{}",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn decode_data_url(url: &str) -> RgbImage {
        let b64 = url.strip_prefix("data:image/jpeg;base64,").expect("jpeg data url");
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        image::load_from_memory(&bytes).unwrap().into_rgb8()
    }

    #[test]
    fn encode_downscales_and_draws() {
        let dir = temp_dir("enc");
        let path = dir.join("img.png");
        RgbImage::from_pixel(2000, 1000, Rgb([0, 0, 0])).save(&path).unwrap();
        let overlay = vec![(BBox::from_corners(500.0, 250.0, 1500.0, 750.0, 2000.0, 1000.0, 0), [255, 0, 0])];
        let img = decode_data_url(&encode_image(&path, 1000, &overlay).unwrap());
        assert_eq!((img.width(), img.height()), (1000, 500));
        // Top edge of the box (y≈125) is red-ish; the center stays dark.
        let edge = img.get_pixel(500, 125);
        assert!(edge[0] > 100 && edge[0] > edge[1] + 50, "edge {edge:?}");
        let center = img.get_pixel(500, 250);
        assert!(center[0] < 60, "center {center:?}");
        // Small images are not upscaled.
        let small = dir.join("small.png");
        RgbImage::new(100, 50).save(&small).unwrap();
        let img = decode_data_url(&encode_image(&small, 1000, &[]).unwrap());
        assert_eq!((img.width(), img.height()), (100, 50));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn specs_and_request_shape() {
        let root = temp_dir("lib");
        let src = temp_dir("src");
        let img = src.join("a.png");
        RgbImage::new(64, 32).save(&img).unwrap();
        examples::add_example(
            &root,
            "wz",
            "kill_banner",
            &img,
            &[
                ("kill_banner".into(), BBox::new(0, 0.5, 0.5, 0.5, 0.5)),
                ("not_a_target".into(), BBox::new(5, 0.1, 0.1, 0.1, 0.1)),
            ],
        )
        .unwrap();
        let targets = vec![(0, "kill_banner".to_string()), (4, "down_banner".to_string())];
        let mut desc = std::collections::BTreeMap::new();
        desc.insert("down_banner".to_string(), "red text when downed".to_string());
        let settings = AutoLabelSettings { model: "gpt-test".into(), reasoning_effort: "low".into(), ..Default::default() };
        let specs = prepare_specs(&root, "wz", &targets, &desc, &settings).unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].examples.len(), 1);
        assert_eq!(specs[0].examples[0].boxes, vec![("kill_banner".to_string(), [250, 250, 750, 750])]);
        assert!(specs[1].examples.is_empty());

        let body = build_request(&settings, &specs, &[], "data:image/jpeg;base64,QUFB");
        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["reasoning_effort"], "low");
        let parts = body["messages"][1]["content"].as_array().unwrap();
        let images: Vec<&Value> = parts.iter().filter(|p| p["type"] == "image_url").collect();
        assert_eq!(images.len(), 2, "one example + the query");
        assert_eq!(images.last().unwrap()["image_url"]["url"], "data:image/jpeg;base64,QUFB");
        let text: String = parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("red text when downed"));
        assert!(!text.contains("not_a_target"));
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("\"down_banner\""));

        let no_effort = AutoLabelSettings { model: "m".into(), ..Default::default() };
        assert!(build_request(&no_effort, &specs, &[], "x").get("reasoning_effort").is_none());

        // Guide pages go first and are named.
        let guides = vec![PreparedGuide { data_url: "data:image/png;base64,R1VJREU=".into(), classes: vec!["down_banner".into()] }];
        let body = build_request(&settings, &specs, &guides, "data:image/jpeg;base64,QUFB");
        let parts = body["messages"][1]["content"].as_array().unwrap();
        let images: Vec<&Value> = parts.iter().filter(|p| p["type"] == "image_url").collect();
        assert_eq!(images.len(), 3, "guide + example + query");
        assert_eq!(images[0]["image_url"]["url"], "data:image/png;base64,R1VJREU=");
        let text: String = parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("Guide page explaining: down_banner"));
        assert!(text.contains("rely on the guide page"));

        for d in [root, src] {
            std::fs::remove_dir_all(d).ok();
        }
    }
}
