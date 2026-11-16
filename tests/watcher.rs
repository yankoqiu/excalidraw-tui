use std::{fs, thread, time::Duration, time::Instant};

use excalidraw_tui::watcher::Watcher;
use tempfile::tempdir;

#[test]
fn detects_atomic_rename_saves() {
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("diagram.excalidraw");
    fs::write(&target, r#"{"type":"excalidraw","elements":[]}"#).expect("seed target");

    let watcher = Watcher::new(&target, Duration::from_millis(100)).expect("start watcher");

    // Emulate editors/agents that write a temp file and rename it over the
    // target: watching the file inode alone would stop working here.
    let temp = dir.path().join(".diagram.excalidraw.tmp");
    fs::write(
        &temp,
        r#"{"type":"excalidraw","elements":[{"type":"text","text":"你好"}]}"#,
    )
    .expect("write temp file");
    fs::rename(&temp, &target).expect("atomic rename");

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if watcher.try_recv().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("watcher did not report the atomic rename within 10s");
}
