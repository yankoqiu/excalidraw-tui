//! `exv` – live, zero-disk Excalidraw preview in the terminal.

use std::{
    fs,
    io::{self, IsTerminal, Stdout},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use excalidraw_tui::{
    converter::{self, Converter},
    rasterizer::Theme,
    ui::App,
    watcher::Watcher,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use ratatui_image::picker::{Picker, ProtocolType};

const WATCH_DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Parser)]
#[command(
    name = "exv",
    bin_name = "exv",
    version,
    about = "Live, zero-disk Excalidraw preview in your terminal",
    long_about = "Watch an .excalidraw file and render it directly in the terminal via Kitty \
                  graphics, Sixel, or Unicode half-blocks. All conversion and rasterization \
                  happens in memory; no temporary .svg/.png files are written."
)]
struct Cli {
    /// Path to the .excalidraw file (it may be created after startup in watch mode)
    file: PathBuf,

    /// Canvas color theme
    #[arg(long, value_enum, default_value_t = ThemeArg::Light)]
    theme: ThemeArg,

    /// Disable live file watching
    #[arg(long)]
    no_watch: bool,

    /// Terminal graphics protocol (auto-detects by default)
    #[arg(long, value_enum, default_value_t = ProtocolArg::Auto)]
    protocol: ProtocolArg,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ThemeArg {
    Light,
    Dark,
}

impl From<ThemeArg> for Theme {
    fn from(value: ThemeArg) -> Self {
        match value {
            ThemeArg::Light => Theme::Light,
            ThemeArg::Dark => Theme::Dark,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProtocolArg {
    Auto,
    Kitty,
    Sixel,
    Halfblocks,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("exv: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("must run in an interactive terminal (stdin and stdout must be a TTY)");
    }

    if cli.no_watch {
        let contents = fs::read_to_string(&cli.file)
            .with_context(|| format!("failed to read {}", cli.file.display()))?;
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        serde_json::from_str::<serde_json::Value>(contents)
            .with_context(|| format!("{} is not valid JSON", cli.file.display()))?;
    } else {
        let parent = cli
            .file
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            bail!("watch directory {} does not exist", parent.display());
        }
    }

    let worker = converter::find_worker_script()?;
    let (converter, converter_rx) = Converter::spawn(&worker)?;

    let watcher = if cli.no_watch {
        None
    } else {
        Some(Watcher::new(&cli.file, WATCH_DEBOUNCE)?)
    };

    let mut session = TerminalSession::new()?;
    let picker = build_picker(cli.protocol);
    let mut app = App::new(
        cli.file,
        cli.theme.into(),
        picker,
        converter,
        converter_rx,
        watcher,
    );

    let external_quit = install_signal_flag()?;
    let result = app.run(&mut session.terminal, external_quit);

    drop(app);
    drop(session);
    result
}

fn build_picker(protocol: ProtocolArg) -> Picker {
    // The capability query must run after entering the alternate screen and
    // before terminal events are read.
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    match protocol {
        ProtocolArg::Auto => {}
        ProtocolArg::Kitty => picker.set_protocol_type(ProtocolType::Kitty),
        ProtocolArg::Sixel => picker.set_protocol_type(ProtocolType::Sixel),
        ProtocolArg::Halfblocks => picker.set_protocol_type(ProtocolType::Halfblocks),
    }
    picker
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn new() -> Result<Self> {
        enable_raw_mode().context("failed to enable terminal raw mode (is stdout a TTY?)")?;

        let mut stdout = io::stdout();
        if let Err(err) = execute!(stdout, EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(err).context("failed to enter alternate screen");
        }

        let backend = CrosstermBackend::new(stdout);
        match Terminal::new(backend) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(err) => {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
                Err(err).context("failed to initialize terminal")
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
    }
}

#[cfg(unix)]
fn install_signal_flag() -> Result<Arc<AtomicBool>> {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};

    let flag = Arc::new(AtomicBool::new(false));
    for signal in [SIGHUP, SIGINT, SIGQUIT, SIGTERM] {
        signal_hook::flag::register(signal, Arc::clone(&flag))
            .with_context(|| format!("failed to install handler for signal {signal}"))?;
    }
    Ok(flag)
}

#[cfg(not(unix))]
fn install_signal_flag() -> Result<Arc<AtomicBool>> {
    Ok(Arc::new(AtomicBool::new(false)))
}
