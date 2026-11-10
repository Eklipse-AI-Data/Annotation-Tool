//! Labeling engine: one prepared [`Labeler`] (encoded examples + client)
//! shared by a pool of worker threads. Pure std threads + mpsc so the UI
//! layer can pump events exactly like its other background tasks.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use super::client::Client;
use super::parse::{self, TargetClass};
use super::prompt::{self, ClassSpec};
use super::settings::AutoLabelSettings;
use crate::yolo::{self, BBox};

pub struct Labeler {
    client: Client,
    settings: AutoLabelSettings,
    specs: Vec<ClassSpec>,
    guides: Vec<prompt::PreparedGuide>,
    targets: Vec<TargetClass>,
}

impl Labeler {
    /// Validates settings and encodes the example images once.
    pub fn prepare(
        settings: AutoLabelSettings,
        library_root: &Path,
        preset: &str,
        targets: Vec<TargetClass>,
        descriptions: &BTreeMap<String, String>,
    ) -> Result<Self, String> {
        if let Some(msg) = settings.missing() {
            return Err(msg.to_string());
        }
        if targets.is_empty() {
            return Err("Pick at least one target class.".to_string());
        }
        let specs = prompt::prepare_specs(library_root, preset, &targets, descriptions, &settings)?;
        let guides = prompt::prepare_guide(library_root, preset, &targets, &settings)?;
        Ok(Self { client: Client::new(&settings), settings, specs, guides, targets })
    }

    /// Asks the model for boxes on one image.
    pub fn label(&self, image: &Path) -> Result<Vec<BBox>, String> {
        let query = prompt::encode_image(image, self.settings.max_side, &[])?;
        let body = prompt::build_request(&self.settings, &self.specs, &self.guides, &query);
        let reply = self.client.chat(&body)?;
        parse::parse_reply(&reply, &self.targets)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Label file written with this many boxes.
    Written(usize),
    /// Model found nothing; no file written.
    Empty,
    /// The image got labels from somewhere else meanwhile; left untouched.
    Skipped,
    Failed(String),
}

#[derive(Debug)]
pub enum JobEvent {
    ImageDone { filename: String, outcome: Outcome },
    Finished { cancelled: bool },
    /// The run could not start (e.g. examples failed to load).
    Aborted(String),
}

/// True when `label` exists and holds at least one valid YOLO row.
pub fn has_labels(label: &Path) -> bool {
    std::fs::read_to_string(label)
        .map(|t| !crate::state::model::count_yolo_rows(&t).is_empty())
        .unwrap_or(false)
}

pub fn label_path(label_dir: &Path, filename: &str) -> Option<PathBuf> {
    let stem = Path::new(filename).file_stem()?;
    let mut name = stem.to_os_string();
    name.push(".txt");
    Some(label_dir.join(name))
}

/// Labels `filenames` (inside `image_dir`) with `concurrency` workers and
/// writes non-empty results to `label_dir`. Sends one `ImageDone` per
/// processed image, then `Finished`. Images still queued when `cancel` is
/// set are not processed.
pub fn run_batch(
    labeler: Arc<Labeler>,
    image_dir: PathBuf,
    label_dir: PathBuf,
    filenames: Vec<String>,
    concurrency: usize,
    cancel: Arc<AtomicBool>,
    tx: Sender<JobEvent>,
) {
    let queue = Arc::new(Mutex::new(filenames.into_iter().collect::<VecDeque<_>>()));
    let workers: Vec<_> = (0..concurrency.max(1))
        .map(|_| {
            let queue = Arc::clone(&queue);
            let labeler = Arc::clone(&labeler);
            let cancel = Arc::clone(&cancel);
            let tx = tx.clone();
            let image_dir = image_dir.clone();
            let label_dir = label_dir.clone();
            std::thread::spawn(move || loop {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let Some(filename) = queue.lock().ok().and_then(|mut q| q.pop_front()) else {
                    return;
                };
                let outcome = process_one(&labeler, &image_dir, &label_dir, &filename);
                if tx.send(JobEvent::ImageDone { filename, outcome }).is_err() {
                    return;
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    let _ = tx.send(JobEvent::Finished { cancelled: cancel.load(Ordering::Relaxed) });
}

fn process_one(labeler: &Labeler, image_dir: &Path, label_dir: &Path, filename: &str) -> Outcome {
    let Some(label) = label_path(label_dir, filename) else {
        return Outcome::Failed("Bad file name".into());
    };
    if has_labels(&label) {
        return Outcome::Skipped;
    }
    match labeler.label(&image_dir.join(filename)) {
        Ok(boxes) if boxes.is_empty() => Outcome::Empty,
        Ok(boxes) => {
            // Re-check: the user may have labeled it while we waited.
            if has_labels(&label) {
                return Outcome::Skipped;
            }
            match yolo::save_yolo(&label, &boxes) {
                Ok(()) => Outcome::Written(boxes.len()),
                Err(e) => Outcome::Failed(format!("Cannot write {}: {e}", label.display())),
            }
        }
        Err(e) => Outcome::Failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "autolabel_job_{tag}_{}_{}",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Serves the same completion reply to every connection.
    fn stub_endpoint(reply: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).ok();
                let content = serde_json::json!({"choices":[{"message":{"content": reply}}]}).to_string();
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{content}",
                    content.len()
                );
            }
        });
        format!("http://{addr}/v1")
    }

    #[test]
    fn batch_writes_skips_and_reports() {
        let base = stub_endpoint(r#"{"boxes":[{"class":"kill_banner","box_2d":[100,100,300,200]}]}"#);
        let lib = temp_dir("lib");
        let images = temp_dir("imgs");
        for name in ["a.png", "b.png", "c.png"] {
            image::RgbImage::new(32, 32).save(images.join(name)).unwrap();
        }
        // b already has labels → must be skipped untouched.
        std::fs::write(images.join("b.txt"), "0 0.5 0.5 0.1 0.1\n").unwrap();

        let settings = AutoLabelSettings { base_url: base, model: "m".into(), timeout_secs: 10, ..Default::default() };
        let labeler = Labeler::prepare(settings, &lib, "wz", vec![(2, "kill_banner".into())], &BTreeMap::new()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        run_batch(
            Arc::new(labeler),
            images.clone(),
            images.clone(),
            vec!["a.png".into(), "b.png".into(), "c.png".into()],
            2,
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let mut done = BTreeMap::new();
        let mut finished = None;
        for ev in rx.iter() {
            match ev {
                JobEvent::ImageDone { filename, outcome } => {
                    done.insert(filename, outcome);
                }
                JobEvent::Finished { cancelled } => finished = Some(cancelled),
                JobEvent::Aborted(e) => panic!("aborted: {e}"),
            }
        }
        assert_eq!(finished, Some(false));
        assert_eq!(done["a.png"], Outcome::Written(1));
        assert_eq!(done["b.png"], Outcome::Skipped);
        assert_eq!(done["c.png"], Outcome::Written(1));
        assert_eq!(std::fs::read_to_string(images.join("a.txt")).unwrap(), "2 0.200000 0.150000 0.200000 0.100000\n");
        assert_eq!(std::fs::read_to_string(images.join("b.txt")).unwrap(), "0 0.5 0.5 0.1 0.1\n");
        for d in [lib, images] {
            std::fs::remove_dir_all(d).ok();
        }
    }

    #[test]
    fn cancelled_before_start_processes_nothing() {
        let settings = AutoLabelSettings { base_url: "http://127.0.0.1:9/v1".into(), model: "m".into(), ..Default::default() };
        let lib = temp_dir("lib2");
        let labeler = Labeler::prepare(settings, &lib, "wz", vec![(0, "x".into())], &BTreeMap::new()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        run_batch(Arc::new(labeler), lib.clone(), lib.clone(), vec!["a.png".into()], 1, Arc::new(AtomicBool::new(true)), tx);
        let events: Vec<JobEvent> = rx.iter().collect();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], JobEvent::Finished { cancelled: true }));
        std::fs::remove_dir_all(lib).ok();
    }

    #[test]
    fn prepare_validates() {
        let lib = temp_dir("lib3");
        let no_model = AutoLabelSettings::default();
        assert!(Labeler::prepare(no_model, &lib, "wz", vec![(0, "x".into())], &BTreeMap::new()).is_err());
        let ok = AutoLabelSettings { model: "m".into(), ..Default::default() };
        assert!(Labeler::prepare(ok, &lib, "wz", vec![], &BTreeMap::new()).is_err());
        std::fs::remove_dir_all(lib).ok();
    }
}
