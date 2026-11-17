//! `exv` – live, zero-disk Excalidraw preview in the terminal.

use std::{
    fs,
    io::{self, IsTerminal, Stdout, Write},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EndSynchronizedUpdate, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use excalidraw_tui::{
    converter::{self, Converter},
    export::{self, ExportPipeline, Format},
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
    about = "Preview or convert Excalidraw diagrams",
    long_about = "Preview an .excalidraw file in the terminal via Kitty graphics, Sixel, or \
                  Unicode half-blocks, or export it to SVG/PNG. Pass --watch to keep the \
                  terminal preview synchronized with the source. All conversion and \
                  rasterization happens in memory; no temporary files are written."
)]
struct Cli {
    /// Path to the .excalidraw file (it may be created after startup in watch mode)
    file: PathBuf,

    /// Canvas color theme (preview and PNG export)
    #[arg(long, value_enum, default_value_t = ThemeArg::Light)]
    theme: ThemeArg,

    /// Watch the source and refresh the terminal preview on changes
    #[arg(short = 'w', long)]
    watch: bool,

    /// Export to SVG or PNG instead of opening the terminal preview
    #[arg(short = 'f', long, value_enum)]
    format: Option<FormatArg>,

    /// Export path; use "-" for stdout (defaults to <input>.svg or <input>.png)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// PNG resolution multiplier (default 1)
    #[arg(long)]
    scale: Option<f32>,

    /// Terminal graphics protocol (preview only; auto-detects by default)
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
enum FormatArg {
    Svg,
    Png,
}

impl From<FormatArg> for Format {
    fn from(value: FormatArg) -> Self {
        match value {
            FormatArg::Svg => Format::Svg,
            FormatArg::Png => Format::Png,
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

    let format = if cli.format.is_some() || cli.output.is_some() {
        let explicit = cli.format.map(Format::from);
        let output_hint = cli.output.as_deref().filter(|path| *path != Path::new("-"));
        Some(export::resolve_format(explicit, output_hint)?)
    } else {
        None
    };

    if cli.scale.is_some() && !matches!(format, Some(Format::Png)) {
        bail!("--scale is only valid with --format png");
    }
    if format.is_some() && cli.watch {
        bail!("--watch only applies to the terminal preview");
    }
    if format.is_some() && !matches!(cli.protocol, ProtocolArg::Auto) {
        bail!("--protocol only applies to the terminal preview");
    }

    let output = match format {
        Some(format) => Some(match &cli.output {
            Some(path) => path.clone(),
            None => export::default_output_path(&cli.file, format)?,
        }),
        None => None,
    };

    match (format, output.as_deref()) {
        (Some(format), Some(output)) => run_export(&cli, format, output),
        _ => run_preview(&cli),
    }
}

fn run_preview(cli: &Cli) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("must run in an interactive terminal (stdin and stdout must be a TTY)");
    }

    if cli.watch {
        let parent = cli
            .file
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            bail!("watch directory {} does not exist", parent.display());
        }
    } else if export::read_source(&cli.file)?.is_none() {
        bail!("{} does not exist", cli.file.display());
    }

    let worker = converter::find_worker_script()?;
    let (converter, converter_rx) = Converter::spawn(&worker)?;

    let watcher = if cli.watch {
        Some(Watcher::new(&cli.file, WATCH_DEBOUNCE)?)
    } else {
        None
    };

    let mut session = TerminalSession::new()?;
    let picker = build_picker(cli.protocol);
    let mut app = App::new(
        cli.file.clone(),
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

fn run_export(cli: &Cli, format: Format, output: &Path) -> Result<()> {
    let to_stdout = output == Path::new("-");
    if !to_stdout {
        if paths_refer_to_same_file(&cli.file, output) {
            bail!("output path must differ from the input file");
        }
        let parent = output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            bail!("output directory {} does not exist", parent.display());
        }
    }

    let scale = cli.scale.unwrap_or(1.0);
    if !scale.is_finite() || scale <= 0.0 {
        bail!("--scale must be a positive number");
    }

    let contents = export::read_source(&cli.file)?
        .with_context(|| format!("{} does not exist", cli.file.display()))?;
    let worker = converter::find_worker_script()?;
    let mut pipeline = ExportPipeline::new(&worker, format, scale, cli.theme.into())?;
    let rendered = pipeline.render(contents)?;
    write_output(output, &rendered.bytes, to_stdout)?;
    if !to_stdout {
        eprintln!(
            "wrote {} ({} elements)",
            output.display(),
            rendered.elements
        );
    }
    Ok(())
}

fn write_output(path: &Path, bytes: &[u8], to_stdout: bool) -> Result<()> {
    if to_stdout {
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(bytes)
            .context("failed to write output to stdout")?;
        stdout.flush().context("failed to flush stdout")
    } else {
        fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
    }
}

fn paths_refer_to_same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
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
        let _ = execute!(
            io::stdout(),
            EndSynchronizedUpdate,
            Show,
            LeaveAlternateScreen
        );
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
