//! Class list handling, golden-ratio class colors, and natural filename
//! sorting — a faithful port of the matching parts of `src/utils.py`.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Class {
    pub id: i64,
    pub name: String,
    /// "#rrggbb" hex string, identical to the Python tool's output.
    pub color_hex: String,
    pub color: (u8, u8, u8),
    pub default_w: f64,
    pub default_h: f64,
}

impl Class {
    fn new(id: i64, name: String) -> Self {
        let color = generate_color(id);
        Self {
            id,
            name,
            color_hex: format!("#{:02x}{:02x}{:02x}", color.0, color.1, color.2),
            color,
            default_w: 100.0,
            default_h: 100.0,
        }
    }
}

/// Loads classes from a text file, one per line. The class id equals the
/// 0-based line index; blank lines are skipped but still consume an index
/// (Python parity — `for i, line in enumerate(f)`).
pub fn load_classes(file_path: &Path) -> Vec<Class> {
    let mut classes = Vec::new();
    let Ok(text) = std::fs::read_to_string(file_path) else {
        return classes;
    };
    for (i, line) in text.lines().enumerate() {
        let name = line.trim();
        if !name.is_empty() {
            classes.push(Class::new(i as i64, name.to_string()));
        }
    }
    classes
}

/// Writes class names one per line, in id order.
pub fn save_classes(file_path: &Path, names: &[String]) -> std::io::Result<()> {
    let mut text = String::new();
    for name in names {
        text.push_str(name);
        text.push('\n');
    }
    std::fs::write(file_path, text)
}

/// Builds a `{old_id: new_id}` mapping by matching names. Classes that were
/// removed map to `None`. Port of `create_class_mapping`.
pub fn create_class_mapping(
    old_classes: &[String],
    new_classes: &[String],
) -> std::collections::HashMap<i64, Option<i64>> {
    let mut mapping = std::collections::HashMap::new();
    for (old_id, old_name) in old_classes.iter().enumerate() {
        match new_classes.iter().position(|n| n == old_name) {
            Some(new_id) => {
                mapping.insert(old_id as i64, Some(new_id as i64));
            }
            None => {
                mapping.insert(old_id as i64, None); // class was removed
            }
        }
    }
    mapping
}

/// Golden-ratio hue spread color for a class id — port of `generate_color`.
/// `hue = (id * 0.618033988749895) % 1.0`, HSV with s=0.8, v=0.95, then the
/// same integer truncation `colorsys`/`int()` perform.
pub fn generate_color(class_id: i64) -> (u8, u8, u8) {
    let golden_ratio_conjugate: f64 = 0.618_033_988_749_895;
    let h = (class_id as f64 * golden_ratio_conjugate) % 1.0;
    let s = 0.8;
    let v = 0.95;

    // Port of colorsys.hsv_to_rgb (h in [0,1])
    let i = (h * 6.0).floor() as i32;
    let f = h * 6.0 - (i as f64);
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        5 => (v, p, q),
        _ => (v, t, p),
    };
    // Python int() truncates toward zero; values here are all >= 0.
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// One chunk of a natural-sort key: either a digit run (compared as an
/// arbitrary-precision integer, like Python) or a lowercased text run.
#[derive(Debug, PartialEq, Eq)]
pub enum NatChunk {
    /// Normalized digits (leading zeros stripped) — compare by (length, lexicographic).
    Num(String),
    /// Lowercased text run.
    Text(String),
}

impl PartialOrd for NatChunk {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NatChunk {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (NatChunk::Text(a), NatChunk::Text(b)) => a.cmp(b),
            (NatChunk::Num(a), NatChunk::Num(b)) => {
                // Arbitrary-precision non-negative integer comparison.
                let len = a.len().cmp(&b.len());
                if len != Ordering::Equal {
                    return len;
                }
                a.cmp(b)
            }
            // Python would raise TypeError comparing int with str at the same
            // position; `re.split` keeps chunk parity so this is unreachable
            // in practice. Define a total order anyway.
            (NatChunk::Num(_), NatChunk::Text(_)) => Ordering::Less,
            (NatChunk::Text(_), NatChunk::Num(_)) => Ordering::Greater,
        }
    }
}

/// Splits into digit/non-digit runs like Python's `re.split('([0-9]+)', s)`.
pub fn natural_sort_key(s: &str) -> Vec<NatChunk> {
    let mut chunks = Vec::new();
    let mut current_text = String::new();
    let mut current_num = String::new();
    let mut in_num = false;

    for ch in s.chars() {
        let is_digit = ch.is_ascii_digit();
        if is_digit != in_num {
            // Switch run type; flush the finished run.
            if in_num {
                let normalized = current_num.trim_start_matches('0').to_string();
                chunks.push(NatChunk::Num(normalized));
                current_num.clear();
            } else {
                chunks.push(NatChunk::Text(current_text.to_lowercase()));
                current_text.clear();
            }
            in_num = is_digit;
        }
        if in_num {
            current_num.push(ch);
        } else {
            current_text.push(ch);
        }
    }
    if in_num {
        let normalized = current_num.trim_start_matches('0').to_string();
        chunks.push(NatChunk::Num(normalized));
    } else if !current_text.is_empty() {
        chunks.push(NatChunk::Text(current_text.to_lowercase()));
    }
    chunks
}

/// `true` when the filename is one this tool loads (case-insensitive
/// extension match on .png/.jpg/.jpeg — Python parity).
pub fn is_image_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_match_python() {
        // Ground truth from Python: generate_color(i) for i in 0..6
        // '#f23030', '#3069f2', '#a1f230', '#f230da', '#30f2d1', '#f29930'
        assert_eq!(generate_color(0), (0xf2, 0x30, 0x30));
        assert_eq!(generate_color(1), (0x30, 0x69, 0xf2));
        assert_eq!(generate_color(2), (0xa1, 0xf2, 0x30));
        assert_eq!(generate_color(3), (0xf2, 0x30, 0xda));
        assert_eq!(generate_color(4), (0x30, 0xf2, 0xd1));
        assert_eq!(generate_color(5), (0xf2, 0x99, 0x30));
    }

    #[test]
    fn natural_sort_orders_numbers_inside() {
        let mut names = vec![
            "img10.jpg".to_string(),
            "img2.jpg".to_string(),
            "IMG1.png".to_string(),
            "img1.jpg".to_string(),
            "a2b.png".to_string(),
            "a10b.png".to_string(),
        ];
        names.sort_by(|a, b| natural_sort_key(a).cmp(&natural_sort_key(b)));
        assert_eq!(
            names,
            vec![
                "a2b.png".to_string(),
                "a10b.png".to_string(),
                "img1.jpg".to_string(),
                "IMG1.png".to_string(), // tie broken by stable sort: img1 < IMG1 (lowercase run equal, 'IMG1' was after in input)
                "img2.jpg".to_string(),
                "img10.jpg".to_string(),
            ]
        );
    }

    #[test]
    fn leading_zeros_compare_as_integers() {
        // "img007" == "img7" numerically; Python: [img, 7, .jpg]
        let a = natural_sort_key("img007.jpg");
        let b = natural_sort_key("img7.jpg");
        assert_eq!(a, b);
        // and "img08" < "img9"
        assert!(natural_sort_key("img08.jpg") < natural_sort_key("img9.jpg"));
    }

    #[test]
    fn blank_lines_consume_ids() {
        let dir = std::env::temp_dir().join("at_classes_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("classes.txt");
        std::fs::write(&path, "enemy\n\nally\n   \nweapon\n").unwrap();
        let classes = load_classes(&path);
        assert_eq!(classes.len(), 3);
        assert_eq!(classes[0].id, 0);
        assert_eq!(classes[0].name, "enemy");
        assert_eq!(classes[1].id, 2); // blank line consumed id 1
        assert_eq!(classes[2].id, 4); // whitespace line consumed id 3
        std::fs::remove_file(&path).ok();
    }
}
