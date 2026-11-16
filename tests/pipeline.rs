use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

use excalidraw_tui::{
    converter::{Converter, Event},
    rasterizer::{Rasterizer, Theme},
};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn runtime_available() -> bool {
    ["bun", "node"].iter().any(|runtime| {
        Command::new(runtime)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

fn next_event(rx: &std::sync::mpsc::Receiver<Event>, timeout: Duration, context: &str) -> Event {
    match rx.recv_timeout(timeout) {
        Ok(event) => event,
        Err(err) => panic!("channel error while {context}: {err}"),
    }
}

#[test]
fn full_pipeline_with_chinese_text_and_error_recovery() {
    if !runtime_available() {
        eprintln!("skipping pipeline test: neither Bun nor Node.js is available");
        return;
    }

    let renderer = crate_root().join("renderer");
    if !renderer.join("node_modules").join("@excalidraw").is_dir() {
        eprintln!("skipping pipeline test: run `npm install --prefix renderer` first");
        return;
    }

    let sample_path = crate_root().join("tests/data/sample.excalidraw");
    let sample = fs::read_to_string(&sample_path).expect("read sample file");
    serde_json::from_str::<serde_json::Value>(&sample).expect("sample must be valid JSON");

    let (converter, rx) =
        Converter::spawn(&renderer.join("worker.mjs")).expect("spawn renderer process");

    converter
        .request(1, sample.clone())
        .expect("queue first conversion");

    let mut svg = None;
    let mut elements = 0;
    while svg.is_none() {
        match next_event(&rx, Duration::from_secs(90), "waiting for first conversion") {
            Event::Ready => {}
            Event::Converted {
                generation: 1,
                svg: converted,
                elements: count,
            } => {
                svg = Some(converted);
                elements = count;
            }
            Event::Failed {
                generation,
                message,
            } => {
                panic!("conversion {generation} failed: {message}")
            }
            Event::Died { message } => panic!("renderer died: {message}"),
            Event::Converted { generation, .. } => {
                panic!("unexpected conversion response for generation {generation}")
            }
        }
    }
    let svg = svg.expect("svg");
    assert!(
        elements >= 6,
        "expected all drawable elements, got {elements}"
    );
    assert!(svg.contains("<text"), "SVG must contain text elements");
    assert!(
        svg.contains("中文起点"),
        "SVG must contain the Chinese label"
    );
    assert!(svg.contains("<path"), "SVG must contain hand-drawn paths");

    // A malformed document must produce a per-request error without killing
    // the resident worker.
    converter
        .request(2, "{ this is not valid JSON".to_string())
        .expect("queue invalid conversion");
    match next_event(
        &rx,
        Duration::from_secs(30),
        "waiting for malformed input error",
    ) {
        Event::Failed { generation, .. } => assert_eq!(generation, 2),
        other => panic!("expected Failed for malformed JSON, got {other:?}"),
    }

    // The same worker must still serve a follow-up request.
    converter
        .request(3, sample)
        .expect("queue second valid conversion");
    loop {
        match next_event(
            &rx,
            Duration::from_secs(30),
            "waiting for recovery conversion",
        ) {
            Event::Converted { generation: 3, .. } => break,
            Event::Failed {
                generation,
                message,
            } => {
                panic!("recovery conversion {generation} failed: {message}")
            }
            Event::Died { message } => panic!("renderer died after error: {message}"),
            _ => {}
        }
    }

    // Rasterize entirely in memory and verify real pixels were produced.
    let rasterizer = Rasterizer::new(Theme::Light);
    let tree = rasterizer.parse_svg(&svg).expect("parse converted SVG");
    let image = rasterizer
        .render_view(&tree, (960, 540), 1.0, (0, 0))
        .expect("rasterize");
    assert_eq!((image.width(), image.height()), (960, 540));

    let rgba = image.to_rgba8();
    let ink = rgba
        .pixels()
        .filter(|pixel| !(pixel.0[0] == 255 && pixel.0[1] == 255 && pixel.0[2] == 255))
        .count();
    assert!(
        ink > 2000,
        "expected a non-empty rendered diagram, got {ink} ink pixels"
    );
}
