//! Model reply → YOLO boxes. The model is asked for
//! `{"boxes":[{"class":"<name>","box_2d":[x1,y1,x2,y2]}]}` with integer
//! coordinates normalized to 0–1000; replies are parsed leniently (code
//! fences, prose around the JSON, a bare array, alternate key names).

use serde_json::Value;

use crate::yolo::BBox;

/// Coordinate scale used in prompts and replies.
pub const SCALE: f64 = 1000.0;

/// Smallest accepted box side on the 0–1000 scale.
const MIN_SIDE: f64 = 2.0;

/// A class the model may return: `(class_id, name)`.
pub type TargetClass = (i64, String);

/// Parses a model reply into boxes restricted to `targets`. Unknown classes
/// and degenerate boxes are dropped; out-of-range coordinates are clamped.
/// Returns `Err` only when no JSON can be found at all.
pub fn parse_reply(text: &str, targets: &[TargetClass]) -> Result<Vec<BBox>, String> {
    let json = extract_json(text).ok_or_else(|| {
        format!(
            "Model reply was not JSON: {}",
            super::client::truncate(text.trim(), 200)
        )
    })?;
    let items: Vec<Value> = match &json {
        Value::Array(items) => items.clone(),
        Value::Object(obj) => ["boxes", "objects", "detections", "annotations"]
            .iter()
            .find_map(|k| obj.get(*k).and_then(Value::as_array).cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    let mut out: Vec<BBox> = Vec::new();
    for item in &items {
        let Some(class_id) = item_class(item).and_then(|c| match_class(&c, targets)) else {
            continue;
        };
        let Some(coords) = item_coords(item) else {
            continue;
        };
        let clamp = |v: f64| v.clamp(0.0, SCALE);
        let (x1, x2) = (clamp(coords[0].min(coords[2])), clamp(coords[0].max(coords[2])));
        let (y1, y2) = (clamp(coords[1].min(coords[3])), clamp(coords[1].max(coords[3])));
        if x2 - x1 < MIN_SIDE || y2 - y1 < MIN_SIDE {
            continue;
        }
        let b = BBox::from_corners(x1, y1, x2, y2, SCALE, SCALE, class_id);
        if !out.iter().any(|o| same_box(o, &b)) {
            out.push(b);
        }
    }
    Ok(out)
}

/// A box's corners on the 0–1000 scale (rounded), used in prompts.
pub fn to_scale(b: &BBox) -> [i64; 4] {
    let (x1, y1, x2, y2) = b.denormalized(SCALE, SCALE);
    [x1, y1, x2, y2].map(|v| v.clamp(0.0, SCALE).round() as i64)
}

fn same_box(a: &BBox, b: &BBox) -> bool {
    a.class_id == b.class_id
        && (a.x_center - b.x_center).abs() < 1e-6
        && (a.y_center - b.y_center).abs() < 1e-6
        && (a.w - b.w).abs() < 1e-6
        && (a.h - b.h).abs() < 1e-6
}

/// Finds the first parseable JSON object/array in `text`, ignoring code
/// fences and surrounding prose.
fn extract_json(text: &str) -> Option<Value> {
    for (i, ch) in text.char_indices() {
        if ch != '{' && ch != '[' {
            continue;
        }
        let mut stream = serde_json::Deserializer::from_str(&text[i..]).into_iter::<Value>();
        if let Some(Ok(value)) = stream.next() {
            let useful = match &value {
                Value::Object(_) => true,
                Value::Array(items) => items.is_empty() || items.iter().all(Value::is_object),
                _ => false,
            };
            if useful {
                return Some(value);
            }
        }
    }
    None
}

enum ClassRef {
    Name(String),
    Id(i64),
}

fn item_class(item: &Value) -> Option<ClassRef> {
    for key in ["class", "label", "name", "class_name", "category"] {
        match item.get(key) {
            Some(Value::String(s)) => return Some(ClassRef::Name(s.clone())),
            Some(Value::Number(n)) => return n.as_i64().map(ClassRef::Id),
            _ => {}
        }
    }
    item.get("class_id").and_then(Value::as_i64).map(ClassRef::Id)
}

fn item_coords(item: &Value) -> Option<[f64; 4]> {
    for key in ["box_2d", "bbox", "box", "bbox_2d", "coordinates"] {
        if let Some(Value::Array(arr)) = item.get(key) {
            if arr.len() == 4 {
                let v: Vec<f64> = arr.iter().filter_map(Value::as_f64).collect();
                if v.len() == 4 && v.iter().all(|x| x.is_finite()) {
                    return Some([v[0], v[1], v[2], v[3]]);
                }
            }
        }
    }
    // {"x1":..,"y1":..,"x2":..,"y2":..}
    let get = |k: &str| item.get(k).and_then(Value::as_f64);
    match (get("x1"), get("y1"), get("x2"), get("y2")) {
        (Some(a), Some(b), Some(c), Some(d)) => Some([a, b, c, d]),
        _ => None,
    }
}

fn normalize_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn match_class(c: &ClassRef, targets: &[TargetClass]) -> Option<i64> {
    match c {
        ClassRef::Id(id) => targets.iter().find(|(tid, _)| tid == id).map(|(id, _)| *id),
        ClassRef::Name(name) => {
            let name = name.trim();
            targets
                .iter()
                .find(|(_, n)| n == name)
                .or_else(|| targets.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)))
                .or_else(|| {
                    let key = normalize_name(name);
                    targets.iter().find(|(_, n)| normalize_name(n) == key)
                })
                .map(|(id, _)| *id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> Vec<TargetClass> {
        // Ids with a gap, like a class file with a blank line.
        vec![(0, "kill_banner".into()), (3, "Triple Kill".into())]
    }

    #[test]
    fn parses_plain_object() {
        let r = parse_reply(
            r#"{"boxes":[{"class":"kill_banner","box_2d":[100,200,300,400]}]}"#,
            &targets(),
        )
        .unwrap();
        assert_eq!(r.len(), 1);
        let b = r[0];
        assert_eq!(b.class_id, 0);
        assert!((b.x_center - 0.2).abs() < 1e-9);
        assert!((b.y_center - 0.3).abs() < 1e-9);
        assert!((b.w - 0.2).abs() < 1e-9);
        assert!((b.h - 0.2).abs() < 1e-9);
    }

    #[test]
    fn parses_fenced_and_prose() {
        let text = "Here you go:\n```json\n{\"boxes\": [{\"label\": \"triple kill\", \"bbox\": [10, 10, 50, 40]}]}\n```\nDone.";
        let r = parse_reply(text, &targets()).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].class_id, 3, "fuzzy name match keeps the real id (gap)");
    }

    #[test]
    fn bare_array_and_empty() {
        let r = parse_reply(r#"[{"class":"kill_banner","box_2d":[0,0,10,10]}]"#, &targets()).unwrap();
        assert_eq!(r.len(), 1);
        assert!(parse_reply(r#"{"boxes": []}"#, &targets()).unwrap().is_empty());
        assert!(parse_reply("[]", &targets()).unwrap().is_empty());
    }

    #[test]
    fn drops_unknown_and_degenerate_clamps_range() {
        let text = r#"{"boxes":[
            {"class":"weapon_icon","box_2d":[0,0,100,100]},
            {"class":"kill_banner","box_2d":[500,500,501,600]},
            {"class":"kill_banner","box_2d":[900,-50,1200,100]},
            {"class":"kill_banner","box_2d":"oops"}
        ]}"#;
        let r = parse_reply(text, &targets()).unwrap();
        assert_eq!(r.len(), 1);
        let (x1, y1, x2, y2) = r[0].denormalized(1000.0, 1000.0);
        assert!((x1 - 900.0).abs() < 1e-6 && (x2 - 1000.0).abs() < 1e-6);
        assert!(y1.abs() < 1e-6 && (y2 - 100.0).abs() < 1e-6);
    }

    #[test]
    fn swapped_corners_and_dedupe() {
        let text = r#"{"boxes":[
            {"class":"kill_banner","box_2d":[300,400,100,200]},
            {"class":"kill_banner","box_2d":[100,200,300,400]}
        ]}"#;
        let r = parse_reply(text, &targets()).unwrap();
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn numeric_class_ids() {
        let r = parse_reply(r#"{"boxes":[{"class_id":3,"box_2d":[0,0,10,10]},{"class":1,"box_2d":[0,0,10,10]}]}"#, &targets()).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].class_id, 3);
    }

    #[test]
    fn garbage_is_error() {
        assert!(parse_reply("I cannot see any banner.", &targets()).is_err());
    }

    #[test]
    fn scale_round_trip() {
        let b = BBox::from_corners(120.0, 40.0, 480.0, 90.0, 1920.0, 1080.0, 0);
        let s = to_scale(&b);
        assert_eq!(s, [63, 37, 250, 83]);
    }
}
