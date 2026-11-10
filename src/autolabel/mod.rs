//! Auto Label: few-shot LLM labeling through an OpenAI-compatible endpoint
//! (CLIProxyAPI in front of a Codex subscription).
//!
//! - [`settings`]: endpoint/model config stored in config.json
//! - [`examples`]: per-preset, per-class labelled example library
//! - [`guide`]: imported labeling-guide pages (PDF/images) per preset
//! - [`prompt`]: image encoding + few-shot request building
//! - [`client`]: blocking HTTP client
//! - [`parse`]: model reply → YOLO boxes
//! - [`job`]: labeler + multi-threaded batch runner
//! - [`review`]: "needs review" sidecar next to the images

pub mod client;
pub mod examples;
pub mod guide;
pub mod job;
pub mod parse;
pub mod prompt;
pub mod proxy;
pub mod review;
pub mod settings;
