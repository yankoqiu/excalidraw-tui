//! TUI canvas, status bar, key handling and the live refresh state machine.

use std::{
    fs, io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
        Arc,
    },
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame, Terminal,
};
use ratatui_image::{
    picker::{Picker, ProtocolType},
    protocol::Protocol,
    Image as TerminalImage, Resize,
};
use resvg::usvg::Tree;
use serde_json::Value;

use crate::{
    converter::{self, Converter},
    rasterizer::{Rasterizer, Theme},
    watcher::Watcher,
};

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 8.0;
const ZOOM_STEP: f32 = 1.2;
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const STATUS_WIDTH: usize = 80;

#[derive(Clone, Copy, PartialEq, Eq)]
struct ImageSignature {
    area_width: u32,
    area_height: u32,
    zoom_bits: u32,
    pan_x: i32,
    pan_y: i32,
}

enum Status {
    Syncing,
    Ready,
    Waiting(String),
    Error(String),
}

pub struct App {
    path: PathBuf,
    display_name: String,
    picker: Picker,
    rasterizer: Rasterizer,
    converter: Converter,
    converter_rx: Receiver<converter::Event>,
    watcher: Option<Watcher>,

    tree: Option<Tree>,
    element_count: usize,
    last_file_contents: Option<String>,
    latest_generation: u64,

    zoom: f32,
    pan: (i32, i32),
    protocol: Option<Protocol>,
    image_signature: Option<ImageSignature>,
    last_image_area: Option<Rect>,

    status: Status,
    dirty: bool,
    quit: bool,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: PathBuf,
        theme: Theme,
        picker: Picker,
        converter: Converter,
        converter_rx: Receiver<converter::Event>,
        watcher: Option<Watcher>,
    ) -> Self {
        let display_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| path.display().to_string());

        let mut app = Self {
            path,
            display_name,
            picker,
            rasterizer: Rasterizer::new(theme),
            converter,
            converter_rx,
            watcher,
            tree: None,
            element_count: 0,
            last_file_contents: None,
            latest_generation: 0,
            zoom: 1.0,
            pan: (0, 0),
            protocol: None,
            image_signature: None,
            last_image_area: None,
            status: Status::Syncing,
            dirty: true,
            quit: false,
        };
        app.reload_file();
        app
    }

    pub fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        external_quit: Arc<AtomicBool>,
    ) -> Result<()> {
        while !self.quit && !external_quit.load(Ordering::Relaxed) {
            self.poll_background();
            if self.quit {
                break;
            }

            let timeout = if self.dirty {
                Duration::ZERO
            } else {
                POLL_INTERVAL
            };
            if event::poll(timeout).context("failed to poll terminal events")? {
                let event = event::read().context("failed to read terminal event")?;
                self.handle_terminal_event(event);
                // A held key queues repeats while the previous frame is
                // rasterized and encoded; fold a burst into one final frame.
                for _ in 0..64 {
                    if self.quit || !event::poll(Duration::ZERO).context("failed to poll events")? {
                        break;
                    }
                    let event = event::read().context("failed to read terminal event")?;
                    self.handle_terminal_event(event);
                }
            }

            if self.quit {
                break;
            }
            self.poll_background();
            if self.dirty {
                self.dirty = false;
                draw_synchronized(terminal, |frame| self.draw(frame))?;
            }
        }
        Ok(())
    }

    fn poll_background(&mut self) {
        while let Ok(event) = self.converter_rx.try_recv() {
            self.handle_converter_event(event);
        }

        let mut changed = false;
        if let Some(watcher) = &self.watcher {
            while watcher.try_recv().is_some() {
                changed = true;
            }
        }
        if changed {
            self.reload_file();
        }
    }

    fn handle_converter_event(&mut self, event: converter::Event) {
        match event {
            converter::Event::Ready => {}
            converter::Event::Converted {
                generation,
                svg,
                elements,
            } => {
                if generation != self.latest_generation {
                    return;
                }
                match self.rasterizer.parse_svg(&svg) {
                    Ok(tree) => {
                        self.tree = Some(tree);
                        self.element_count = elements;
                        self.protocol = None;
                        self.image_signature = None;
                        self.status = Status::Ready;
                        self.clamp_pan();
                        self.dirty = true;
                    }
                    Err(err) => {
                        self.status = Status::Error(format!("invalid SVG: {err}"));
                        self.dirty = true;
                    }
                }
            }
            converter::Event::Failed {
                generation,
                message,
            } => {
                if generation != self.latest_generation {
                    return;
                }
                self.status = Status::Error(message);
                self.dirty = true;
            }
            converter::Event::Died { message } => {
                self.status = Status::Error(message);
                self.dirty = true;
            }
        }
    }

    fn reload_file(&mut self) {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                self.last_file_contents = None;
                self.status =
                    Status::Waiting(format!("waiting for {} to be created", self.display_name));
                self.dirty = true;
                return;
            }
            Err(err) => {
                self.status = Status::Error(format!("failed to read file: {err}"));
                self.dirty = true;
                return;
            }
        };
        let contents = contents
            .strip_prefix('\u{feff}')
            .unwrap_or(&contents)
            .to_owned();

        if self.last_file_contents.as_deref() == Some(contents.as_str()) {
            return;
        }

        let parsed: Value = match serde_json::from_str(&contents) {
            Ok(value) => value,
            Err(_) => {
                // Streaming writes can leave truncated JSON on disk. Keep the
                // previous good frame until a debounced, valid version lands.
                self.status = Status::Syncing;
                self.dirty = true;
                return;
            }
        };

        self.element_count = count_elements(&parsed);
        self.last_file_contents = Some(contents.clone());
        self.latest_generation = self.latest_generation.wrapping_add(1);
        let generation = self.latest_generation;
        self.status = Status::Syncing;
        self.dirty = true;

        if let Err(err) = self.converter.request(generation, contents) {
            self.status = Status::Error(format!("renderer unavailable: {err}"));
        }
    }

    fn handle_terminal_event(&mut self, event: CrosstermEvent) {
        match event {
            CrosstermEvent::Key(key) => self.handle_key(key),
            CrosstermEvent::Resize(_, _) => {
                self.dirty = true;
            }
            CrosstermEvent::FocusGained => self.dirty = true,
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let (h_step, v_step) = self.pan_steps();

        match key.code {
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('h') | KeyCode::Left => self.pan_by(-h_step, 0),
            KeyCode::Char('l') | KeyCode::Right => self.pan_by(h_step, 0),
            KeyCode::Char('k') | KeyCode::Up => self.pan_by(0, -v_step),
            KeyCode::Char('j') | KeyCode::Down => self.pan_by(0, v_step),
            KeyCode::Char('+') | KeyCode::Char('=') => self.zoom_by(ZOOM_STEP),
            KeyCode::Char('-') | KeyCode::Char('_') => self.zoom_by(1.0 / ZOOM_STEP),
            KeyCode::Char('0') | KeyCode::Char('r') => self.reset_view(),
            _ => {}
        }
    }

    fn pan_steps(&self) -> (i32, i32) {
        let Some(area) = self.last_image_area else {
            return (32, 32);
        };
        let font = self.picker.font_size();
        let area_w = f32::from(area.width) * f32::from(font.width);
        let area_h = f32::from(area.height) * f32::from(font.height);
        let step = (area_w.min(area_h) / 20.0).max(24.0) as i32;
        (step, step)
    }

    fn pan_by(&mut self, dx: i32, dy: i32) {
        self.pan.0 = self.pan.0.saturating_add(dx);
        self.pan.1 = self.pan.1.saturating_add(dy);
        self.clamp_pan();
        self.dirty = true;
    }

    fn zoom_by(&mut self, factor: f32) {
        let next = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (next - self.zoom).abs() < f32::EPSILON {
            return;
        }
        self.zoom = next;
        self.clamp_pan();
        self.dirty = true;
    }

    fn reset_view(&mut self) {
        self.zoom = 1.0;
        self.pan = (0, 0);
        self.dirty = true;
    }

    fn clamp_pan(&mut self) {
        let (Some(tree), Some(area)) = (self.tree.as_ref(), self.last_image_area) else {
            return;
        };
        let font = self.picker.font_size();
        let area_w = f32::from(area.width) * f32::from(font.width);
        let area_h = f32::from(area.height) * f32::from(font.height);
        if area_w <= 0.0 || area_h <= 0.0 {
            return;
        }
        let (svg_w, svg_h) = Rasterizer::tree_size(tree);
        if svg_w <= 0.0 || svg_h <= 0.0 {
            return;
        }
        let base_scale = (area_w / svg_w).min(area_h / svg_h);
        let scale = base_scale * self.zoom;
        let max_x = ((svg_w * scale - area_w).max(0.0)) / 2.0;
        let max_y = ((svg_h * scale - area_h).max(0.0)) / 2.0;
        self.pan.0 = (self.pan.0 as f32).clamp(-max_x, max_x) as i32;
        self.pan.1 = (self.pan.1 as f32).clamp(-max_y, max_y) as i32;
    }

    fn draw(&mut self, frame: &mut Frame) {
        let chunks =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(frame.area());
        let image_area = chunks[0];
        let footer_area = chunks[1];

        self.last_image_area = Some(image_area);
        self.sync_image(image_area);

        if let Some(protocol) = self.protocol.as_ref() {
            frame.render_widget(TerminalImage::new(protocol), image_area);
        } else {
            self.draw_placeholder(frame, image_area);
        }

        frame.render_widget(self.footer(), footer_area);
    }

    fn sync_image(&mut self, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let Some(tree) = self.tree.as_ref() else {
            return;
        };
        let font = self.picker.font_size();
        let area_w = u32::from(area.width) * u32::from(font.width);
        let area_h = u32::from(area.height) * u32::from(font.height);
        if area_w == 0 || area_h == 0 {
            return;
        }

        let signature = ImageSignature {
            area_width: area_w,
            area_height: area_h,
            zoom_bits: self.zoom.to_bits(),
            pan_x: self.pan.0,
            pan_y: self.pan.1,
        };
        if self.image_signature == Some(signature) {
            return;
        }

        let image = match self
            .rasterizer
            .render_view(tree, (area_w, area_h), self.zoom, self.pan)
        {
            Ok(image) => image,
            Err(err) => {
                self.protocol = None;
                self.image_signature = None;
                self.status = Status::Error(format!("render failed: {err}"));
                return;
            }
        };

        let size = ratatui::layout::Size::new(area.width, area.height);
        match self.picker.new_protocol(image, size, Resize::Fit(None)) {
            Ok(protocol) => {
                self.protocol = Some(protocol);
                self.image_signature = Some(signature);
            }
            Err(err) => {
                self.protocol = None;
                self.image_signature = None;
                self.status = Status::Error(format!("image encoding failed: {err}"));
            }
        }
    }

    fn draw_placeholder(&self, frame: &mut Frame, area: Rect) {
        if area.height == 0 {
            return;
        }
        let (text, color) = match &self.status {
            Status::Syncing => ("rendering diagram…".to_string(), Color::Yellow),
            Status::Ready => ("no diagram to display".to_string(), Color::Yellow),
            Status::Waiting(message) => (message.clone(), Color::Yellow),
            Status::Error(message) => (
                format!("error: {}", one_line(message, STATUS_WIDTH)),
                Color::Red,
            ),
        };
        let mut target = area;
        if target.height > 1 {
            target.y += (target.height - 1) / 2;
            target.height = 1;
        }
        frame.render_widget(
            Paragraph::new(text)
                .alignment(Alignment::Center)
                .style(Style::default().fg(color)),
            target,
        );
    }

    fn footer(&self) -> Paragraph<'_> {
        let dim = Style::default().fg(Color::DarkGray);
        let separator = || Span::styled("  |  ", dim);
        let protocol = match self.picker.protocol_type() {
            ProtocolType::Kitty => "kitty",
            ProtocolType::Sixel => "sixel",
            ProtocolType::Halfblocks => "halfblocks",
            ProtocolType::Iterm2 => "iterm2",
        };
        let status = match &self.status {
            Status::Syncing => Span::styled("syncing…", Style::default().fg(Color::Yellow)),
            Status::Ready => Span::styled("ready", Style::default().fg(Color::Green)),
            Status::Waiting(message) => Span::styled(
                one_line(message, STATUS_WIDTH),
                Style::default().fg(Color::Yellow),
            ),
            Status::Error(message) => Span::styled(
                format!("error: {}", one_line(message, STATUS_WIDTH)),
                Style::default().fg(Color::Red),
            ),
        };

        let line = Line::from(vec![
            Span::styled(
                self.display_name.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            separator(),
            Span::raw(format!("zoom {:.0}%", self.zoom * 100.0)),
            separator(),
            Span::raw(format!("{} elements", self.element_count)),
            separator(),
            Span::raw(protocol.to_string()),
            separator(),
            status,
            separator(),
            Span::styled("hjkl pan  +/- zoom  0 reset  q quit", dim),
        ]);

        Paragraph::new(line)
    }
}

fn draw_synchronized(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    draw: impl FnOnce(&mut Frame),
) -> Result<()> {
    // DEC mode 2026 lets terminals present the whole frame atomically, so
    // large image updates do not flash intermediate cell states.
    execute!(terminal.backend_mut(), BeginSynchronizedUpdate)
        .context("failed to begin synchronized update")?;
    let result = terminal.draw(draw).map(|_| ());
    let end = execute!(terminal.backend_mut(), EndSynchronizedUpdate)
        .context("failed to end synchronized update");
    result?;
    end
}

fn count_elements(value: &Value) -> usize {
    value
        .get("elements")
        .and_then(Value::as_array)
        .map(|elements| {
            elements
                .iter()
                .filter(|element| element.get("isDeleted").and_then(Value::as_bool) != Some(true))
                .count()
        })
        .unwrap_or(0)
}

fn one_line(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let mut truncated: String = collapsed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect();
    truncated.push('…');
    truncated
}
