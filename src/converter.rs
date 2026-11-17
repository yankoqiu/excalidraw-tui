//! Long-lived Excalidraw -> SVG renderer process management.
//!
//! The JavaScript worker is spawned once and talks to us over newline-delimited
//! JSON on stdin/stdout. Conversion results are delivered through the channel
//! returned by [`Converter::spawn`], so the UI thread never blocks on Node.js.

use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use serde_json::Value;

const READY_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const STDERR_TAIL_LINES: usize = 40;

/// Events emitted by a running [`Converter`].
#[derive(Debug)]
pub enum Event {
    /// The worker finished loading @excalidraw/utils and is ready.
    Ready,
    /// A conversion succeeded.
    Converted {
        generation: u64,
        svg: String,
        elements: usize,
    },
    /// A single conversion failed. The worker stays alive.
    Failed { generation: u64, message: String },
    /// The worker process exited or stopped responding and cannot be reused.
    Died { message: String },
}

struct Request {
    generation: u64,
    json: String,
}

/// Handle to the resident JavaScript renderer.
pub struct Converter {
    request_tx: Option<Sender<Request>>,
    child: Arc<Mutex<Child>>,
    worker_thread: Option<JoinHandle<()>>,
}

impl Converter {
    /// Spawns the worker and returns a handle plus the event receiver.
    pub fn spawn(worker: &Path) -> Result<(Self, Receiver<Event>)> {
        if !worker.is_file() {
            bail!("renderer worker not found at {}", worker.display());
        }
        let renderer_dir = worker
            .parent()
            .context("renderer worker has no parent directory")?;
        let runtime = detect_js_runtime()?;

        let mut child = Command::new(&runtime)
            .arg(worker)
            .current_dir(renderer_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to start JavaScript renderer with `{}` (check EXV_JS_RUNTIME, or install Node.js >= 22.22.2 / Bun)",
                    runtime
                )
            })?;

        let stdin = child.stdin.take().context("renderer stdin unavailable")?;
        let stdout = child.stdout.take().context("renderer stdout unavailable")?;
        let stderr = child.stderr.take().context("renderer stderr unavailable")?;

        let child = Arc::new(Mutex::new(child));
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        spawn_stderr_reader(stderr, Arc::clone(&stderr_tail));

        let (request_tx, request_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let worker_thread = thread::Builder::new()
            .name("exv-renderer".into())
            .spawn({
                let child = Arc::clone(&child);
                move || {
                    worker_loop(stdin, stdout, request_rx, event_tx, child, stderr_tail);
                }
            })
            .context("failed to spawn renderer thread")?;

        Ok((
            Self {
                request_tx: Some(request_tx),
                child,
                worker_thread: Some(worker_thread),
            },
            event_rx,
        ))
    }

    /// Queues a conversion request. Results arrive on the channel returned by
    /// [`Converter::spawn`].
    pub fn request(&self, generation: u64, json: String) -> Result<()> {
        let Some(tx) = &self.request_tx else {
            bail!("renderer has been shut down");
        };
        tx.send(Request { generation, json })
            .map_err(|_| anyhow::anyhow!("renderer is not running"))
    }
}

impl Drop for Converter {
    fn drop(&mut self) {
        // Dropping the sender lets the worker thread exit once idle.
        self.request_tx.take();

        // Kill the child first: if the thread is blocked waiting for a
        // response, closing the pipe unblocks it. Then reap the process.
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = self.worker_thread.take() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(
    mut stdin: ChildStdin,
    stdout: ChildStdout,
    request_rx: Receiver<Request>,
    event_tx: Sender<Event>,
    child: Arc<Mutex<Child>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
) {
    let (line_tx, line_rx) = mpsc::channel::<String>();
    let reader_thread = thread::Builder::new()
        .name("exv-renderer-out".into())
        .spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        if line_tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    if let Err(err) = reader_thread {
        let _ = event_tx.send(Event::Died {
            message: format!("failed to read renderer output: {err}"),
        });
        stop_child(&child);
        return;
    }

    if !wait_until_ready(&line_rx, &event_tx, &child, &stderr_tail) {
        return;
    }

    while let Ok(request) = request_rx.recv() {
        let generation = request.generation;
        let payload = serde_json::json!({
            "id": generation,
            "json": request.json,
        })
        .to_string();

        let write_result = stdin
            .write_all(payload.as_bytes())
            .and_then(|()| stdin.write_all(b"\n"))
            .and_then(|()| stdin.flush());
        if let Err(err) = write_result {
            let _ = event_tx.send(Event::Died {
                message: with_stderr_tail(
                    &format!("failed to write to renderer: {err}"),
                    &stderr_tail,
                ),
            });
            stop_child(&child);
            return;
        }

        match wait_for_response(&line_rx, generation, &stderr_tail, &child) {
            ResponseOutcome::Converted { svg, elements } => {
                let _ = event_tx.send(Event::Converted {
                    generation,
                    svg,
                    elements,
                });
            }
            ResponseOutcome::Failed { message } => {
                let _ = event_tx.send(Event::Failed {
                    generation,
                    message,
                });
            }
            ResponseOutcome::Fatal { message } => {
                let _ = event_tx.send(Event::Died { message });
                stop_child(&child);
                return;
            }
        }
    }

    // Request channel closed: closing stdin asks the worker to exit.
    drop(stdin);
}

enum ResponseOutcome {
    Converted { svg: String, elements: usize },
    Failed { message: String },
    Fatal { message: String },
}

fn wait_until_ready(
    line_rx: &Receiver<String>,
    event_tx: &Sender<Event>,
    child: &Arc<Mutex<Child>>,
    stderr_tail: &Arc<Mutex<VecDeque<String>>>,
) -> bool {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match line_rx.recv_timeout(remaining) {
            Ok(line) => {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if value.get("ready").and_then(Value::as_bool) == Some(true) {
                    let _ = event_tx.send(Event::Ready);
                    return true;
                }
                if let Some(fatal) = value.get("fatal").and_then(Value::as_str) {
                    let _ = event_tx.send(Event::Died {
                        message: format!("renderer failed to start: {fatal}"),
                    });
                    stop_child(child);
                    return false;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = event_tx.send(Event::Died {
                    message: with_stderr_tail(
                        "timed out waiting for the renderer to start",
                        stderr_tail,
                    ),
                });
                stop_child(child);
                return false;
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = event_tx.send(Event::Died {
                    message: with_stderr_tail("renderer exited during startup", stderr_tail),
                });
                stop_child(child);
                return false;
            }
        }
    }
}

fn wait_for_response(
    line_rx: &Receiver<String>,
    generation: u64,
    stderr_tail: &Arc<Mutex<VecDeque<String>>>,
    child: &Arc<Mutex<Child>>,
) -> ResponseOutcome {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let message = with_stderr_tail("renderer timed out during conversion", stderr_tail);
            stop_child(child);
            return ResponseOutcome::Fatal { message };
        }
        match line_rx.recv_timeout(remaining) {
            Ok(line) => {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if let Some(fatal) = value.get("fatal").and_then(Value::as_str) {
                    let message = format!("renderer failed: {fatal}");
                    stop_child(child);
                    return ResponseOutcome::Fatal { message };
                }
                if value.get("id").and_then(Value::as_u64) != Some(generation) {
                    continue;
                }
                if let Some(message) = value.get("error").and_then(Value::as_str) {
                    return ResponseOutcome::Failed {
                        message: message.to_string(),
                    };
                }
                if let Some(svg) = value.get("svg").and_then(Value::as_str) {
                    let elements =
                        value.get("elements").and_then(Value::as_u64).unwrap_or(0) as usize;
                    return ResponseOutcome::Converted {
                        svg: svg.to_string(),
                        elements,
                    };
                }
                return ResponseOutcome::Failed {
                    message: "renderer returned a malformed response".to_string(),
                };
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                let message = with_stderr_tail("renderer process exited unexpectedly", stderr_tail);
                stop_child(child);
                return ResponseOutcome::Fatal { message };
            }
        }
    }
}

fn stop_child(child: &Arc<Mutex<Child>>) {
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn spawn_stderr_reader(stderr: ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) {
    let _ = thread::Builder::new()
        .name("exv-renderer-err".into())
        .spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if let Ok(mut tail) = tail.lock() {
                    if tail.len() >= STDERR_TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            }
        });
}

fn with_stderr_tail(message: &str, tail: &Arc<Mutex<VecDeque<String>>>) -> String {
    let tail = tail
        .lock()
        .map(|tail| tail.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    if tail.trim().is_empty() {
        message.to_string()
    } else {
        format!("{message}\n{tail}")
    }
}

fn detect_js_runtime() -> Result<String> {
    if let Some(runtime) = std::env::var_os("EXV_JS_RUNTIME") {
        if !runtime.is_empty() {
            return Ok(runtime.to_string_lossy().into_owned());
        }
    }
    for runtime in ["bun", "node"] {
        let available = Command::new(runtime)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if available {
            return Ok(runtime.to_string());
        }
    }
    bail!("no JavaScript runtime found; install Bun or Node.js >= 22.22.2, or set EXV_JS_RUNTIME")
}

/// Locates `renderer/worker.mjs` for development and installed layouts.
pub fn find_worker_script() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("EXV_RENDERER") {
        let path = PathBuf::from(path);
        let candidate = if path.is_dir() {
            path.join("worker.mjs")
        } else {
            path
        };
        if candidate.is_file() {
            return Ok(candidate);
        }
        bail!(
            "EXV_RENDERER points to {}, but worker.mjs was not found there",
            candidate.display()
        );
    }

    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        let mut dirs = Vec::new();
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
        }
        // When invoked through a symlink, current_exe may still name the link.
        // Resolve it so an installed layout under share/ is found either way.
        if let Ok(canonical) = fs::canonicalize(&exe) {
            if let Some(dir) = canonical.parent() {
                if !dirs.iter().any(|candidate| candidate == dir) {
                    dirs.push(dir.to_path_buf());
                }
            }
        }
        for dir in dirs {
            candidates.push(dir.join("renderer").join("worker.mjs"));
            candidates.push(
                dir.join("..")
                    .join("share")
                    .join("excalidraw-tui")
                    .join("renderer")
                    .join("worker.mjs"),
            );
            candidates.push(
                dir.join("..")
                    .join("lib")
                    .join("excalidraw-tui")
                    .join("renderer")
                    .join("worker.mjs"),
            );
        }
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("renderer")
            .join("worker.mjs"),
    );
    candidates.push(PathBuf::from("renderer").join("worker.mjs"));

    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    bail!(
        "could not locate renderer/worker.mjs (looked in: {}). Reinstall excalidraw-tui or set EXV_RENDERER.",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}
