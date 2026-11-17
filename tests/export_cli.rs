use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn runtime_available() -> bool {
    if let Some(runtime) = std::env::var_os("EXV_JS_RUNTIME") {
        if !runtime.is_empty() {
            return Command::new(runtime)
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
        }
    }
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

fn renderer_dir() -> PathBuf {
    crate_root().join("renderer")
}

fn run_export(input: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exv"))
        .arg(input)
        .args(args)
        .env("EXV_RENDERER", renderer_dir())
        .output()
        .expect("run exv")
}

#[test]
fn cli_exports_svg_and_png_once() {
    if !runtime_available() {
        eprintln!("skipping export test: neither Bun nor Node.js is available");
        return;
    }
    if !renderer_dir()
        .join("node_modules")
        .join("@excalidraw")
        .is_dir()
    {
        eprintln!("skipping export test: run `npm install --prefix renderer` first");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let input = dir.path().join("sample.excalidraw");
    fs::copy(crate_root().join("tests/data/sample.excalidraw"), &input)
        .expect("copy sample diagram");

    let svg_result = run_export(&input, &["--format", "svg"]);
    assert!(
        svg_result.status.success(),
        "SVG export failed: {}",
        String::from_utf8_lossy(&svg_result.stderr)
    );
    let svg_path = input.with_extension("svg");
    let svg = fs::read_to_string(&svg_path).expect("read exported SVG");
    assert!(svg.contains("<svg"), "expected SVG output");
    assert!(svg.contains("中文起点"), "expected Chinese label in SVG");

    let png_path = dir.path().join("rendered.png");
    let png_result = run_export(&input, &["-o", png_path.to_str().unwrap()]);
    assert!(
        png_result.status.success(),
        "PNG export failed: {}",
        String::from_utf8_lossy(&png_result.stderr)
    );
    let png = fs::read(&png_path).expect("read exported PNG");
    assert!(
        png.starts_with(b"\x89PNG\r\n\x1a\n"),
        "expected PNG signature"
    );
    let image = image::load_from_memory(&png).expect("decode exported PNG");
    assert!(image.width() > 0 && image.height() > 0);

    let watched = run_export(&input, &["--format", "svg", "--watch"]);
    assert!(!watched.status.success(), "watched export must be rejected");
    assert!(
        String::from_utf8_lossy(&watched.stderr)
            .contains("--watch only applies to the terminal preview"),
        "unexpected diagnostic: {}",
        String::from_utf8_lossy(&watched.stderr)
    );
}
