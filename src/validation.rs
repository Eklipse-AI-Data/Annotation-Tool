//! Annotation validation — a faithful port of `validate_annotations` /
//! `calculate_iou` from `src/utils.py`, including message text.

use crate::classes::Class;
use crate::yolo::BBox;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {}

#[derive(Debug, Clone)]
pub struct ValidationIssue {
    #[allow(dead_code)]
    pub kind: &'static str, // "invalid_class" | "out_of_bounds" | ...
    pub message: String,
    pub box_index: usize,
    pub severity: Severity,
}

/// Intersection over Union between two normalized boxes.
pub fn calculate_iou(a: &BBox, b: &BBox) -> f64 {
    let x1_1 = a.x_center - a.w / 2.0;
    let y1_1 = a.y_center - a.h / 2.0;
    let x2_1 = a.x_center + a.w / 2.0;
    let y2_1 = a.y_center + a.h / 2.0;

    let x1_2 = b.x_center - b.w / 2.0;
    let y1_2 = b.y_center - b.h / 2.0;
    let x2_2 = b.x_center + b.w / 2.0;
    let y2_2 = b.y_center + b.h / 2.0;

    let xi1 = x1_1.max(x1_2);
    let yi1 = y1_1.max(y1_2);
    let xi2 = x2_1.min(x2_2);
    let yi2 = y2_1.min(y2_2);

    if xi2 <= xi1 || yi2 <= yi1 {
        return 0.0;
    }

    let inter_area = (xi2 - xi1) * (yi2 - yi1);
    let union_area = a.w * a.h + b.w * b.h - inter_area;
    if union_area <= 0.0 {
        return 0.0;
    }
    inter_area / union_area
}

/// Validates the given boxes, producing issue messages identical to Python's.
pub fn validate_annotations(boxes: &[BBox], classes: &[Class]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let class_ids: std::collections::HashSet<i64> = classes.iter().map(|c| c.id).collect();

    for (i, b) in boxes.iter().enumerate() {
        if !class_ids.contains(&b.class_id) && b.class_id != -1 {
            issues.push(ValidationIssue {
                kind: "invalid_class",
                message: format!("Box {}: Unknown class ID {}", i, b.class_id),
                box_index: i,
                severity: Severity::Error,
            });
        }

        if !(0.0..=1.0).contains(&b.x_center) || !(0.0..=1.0).contains(&b.y_center) {
            issues.push(ValidationIssue {
                kind: "out_of_bounds",
                message: format!("Box {}: Center position out of bounds", i),
                box_index: i,
                severity: Severity::Error,
            });
        }

        let x1 = b.x_center - b.w / 2.0;
        let y1 = b.y_center - b.h / 2.0;
        let x2 = b.x_center + b.w / 2.0;
        let y2 = b.y_center + b.h / 2.0;
        if x1 < -0.01 || y1 < -0.01 || x2 > 1.01 || y2 > 1.01 {
            issues.push(ValidationIssue {
                kind: "extends_outside",
                message: format!("Box {}: Extends outside image boundaries", i),
                box_index: i,
                severity: Severity::Warning,
            });
        }

        let area = b.w * b.h;
        if area < 0.0001 {
            issues.push(ValidationIssue {
                kind: "tiny_box",
                message: format!("Box {i}: Extremely small ({:.4}% of image)", area * 100.0),
                box_index: i,
                severity: Severity::Warning,
            });
        }
        if area > 0.9 {
            issues.push(ValidationIssue {
                kind: "huge_box",
                message: format!("Box {i}: Covers {:.1}% of image", area * 100.0),
                box_index: i,
                severity: Severity::Info,
            });
        }

        if b.w <= 0.0 || b.h <= 0.0 {
            issues.push(ValidationIssue {
                kind: "zero_dimension",
                message: format!("Box {}: Has zero or negative dimension", i),
                box_index: i,
                severity: Severity::Error,
            });
        }
    }

    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            if boxes[i].class_id == boxes[j].class_id {
                let iou = calculate_iou(&boxes[i], &boxes[j]);
                if iou > 0.9 {
                    issues.push(ValidationIssue {
                        kind: "duplicate",
                        message: format!("Boxes {} and {}: Possible duplicates (IoU={:.2})", i, j, iou),
                        box_index: i,
                        severity: Severity::Warning,
                    });
                }
            }
        }
    }

    issues
}

/// Summary counts used by the toolbar indicator: (errors, warnings, infos).
pub fn summarize(issues: &[ValidationIssue]) -> (usize, usize, usize) {
    let mut counts = (0, 0, 0);
    for issue in issues {
        match issue.severity {
            Severity::Error => counts.0 += 1,
            Severity::Warning => counts.1 += 1,
            Severity::Info => counts.2 += 1,
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes() -> Vec<Class> {
        crate::classes::load_classes(std::path::Path::new("nonexistent"))
            .into_iter()
            .chain([Class { id: 0, name: "a".into(), color_hex: "#000000".into(), color: (0,0,0), default_w: 100.0, default_h: 100.0 }])
            .collect()
    }

    #[test]
    fn iou_basics() {
        let a = BBox::new(0, 0.5, 0.5, 0.5, 0.5);
        let b = BBox::new(0, 0.5, 0.5, 0.5, 0.5);
        assert!((calculate_iou(&a, &b) - 1.0).abs() < 1e-9);
        let c = BBox::new(0, 0.1, 0.1, 0.1, 0.1);
        assert_eq!(calculate_iou(&a, &c), 0.0);
        // Values verified against Python's calculate_iou:
        let d = BBox::new(0, 0.625, 0.5, 0.25, 0.5);
        assert!((calculate_iou(&a, &d) - 0.5).abs() < 1e-9);
        let e = BBox::new(0, 0.5, 0.5, 0.4, 0.4);
        assert!((calculate_iou(&a, &e) - 0.64).abs() < 1e-9);
    }

    #[test]
    fn message_formats_match_python() {
        let classes = vec![classes().pop().unwrap()];
        let boxes = vec![
            BBox::new(7, 0.5, 0.5, 0.2, 0.2),   // unknown class 7
            BBox::new(0, 1.5, 0.5, 0.2, 0.2),   // out of bounds center
            BBox::new(0, 0.5, 0.5, 0.005, 0.005), // tiny (area 0.000025)
            BBox::new(0, 0.5, 0.5, 0.96, 0.96), // huge (area 0.9216)
        ];
        let issues = validate_annotations(&boxes, &classes);
        let msgs: Vec<&str> = issues.iter().map(|i| i.message.as_str()).collect();
        assert!(msgs.contains(&"Box 0: Unknown class ID 7"));
        assert!(msgs.contains(&"Box 1: Center position out of bounds"));
        assert!(msgs.contains(&"Box 2: Extremely small (0.0025% of image)"));
        assert!(msgs.contains(&"Box 3: Covers 92.2% of image"));
        // duplicate pair 3 is >0.9 IoU with itself? No — pairs are distinct indices.
    }

    #[test]
    fn duplicate_detection() {
        let classes = vec![Class { id: 0, name: "a".into(), color_hex: "#000000".into(), color: (0,0,0), default_w: 100.0, default_h: 100.0 }];
        let boxes = vec![BBox::new(0, 0.5, 0.5, 0.4, 0.4), BBox::new(0, 0.5, 0.5, 0.4, 0.4)];
        let issues = validate_annotations(&boxes, &classes);
        assert!(issues.iter().any(|i| i.kind == "duplicate"
            && i.message == "Boxes 0 and 1: Possible duplicates (IoU=1.00)"));
    }
}
