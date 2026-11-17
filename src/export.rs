//! One-shot diagram export to SVG or PNG.

use std::{
    fs,
    io::{self, Cursor},
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use image::ImageFormat;

use crate::{
    converter::{Converter, Event},
    rasterizer::{Rasterizer, Theme},
};

const CONVERSION_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Svg,
    Png,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Png => "png",
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "svg" => Some(Self::Svg),
            "png" => Some(Self::Png),
            _ => None,
        }
    }
}

pub fn resolve_format(explicit: Option<Format>, output: Option<&Path>) -> Result<Format> {
    let inferred = output.and_then(Format::from_path);
    match (explicit, inferred) {
        (Some(format), Some(inferred)) if format != inferred => bail!(
            "output extension .{} conflicts with --format {}",
            inferred.extension(),
            format.extension()
        ),
        (Some(format), _) => Ok(format),
        (None, Some(format)) => Ok(format),
        (None, None) => {
            bail!("specify --format svg|png (or use an .svg/.png output path)")
        }
    }
}

pub fn default_output_path(input: &Path, format: Format) -> Result<PathBuf> {
    let mut output = input.to_path_buf();
    output.set_extension(format.extension());
    if output == input {
        bail!(
            "input already has the .{} extension; pass --output <PATH>",
            format.extension()
        );
    }
    Ok(output)
}

/// Reads and validates an Excalidraw document. `Ok(None)` means not found.
pub fn read_source(path: &Path) -> Result<Option<String>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
    serde_json::from_str::<serde_json::Value>(contents)
        .with_context(|| format!("{} is not valid JSON", path.display()))?;
    Ok(Some(contents.to_owned()))
}

pub struct Rendered {
    pub bytes: Vec<u8>,
    pub elements: usize,
}

pub struct ExportPipeline {
    format: Format,
    scale: f32,
    rasterizer: Option<Rasterizer>,
    session: RendererSession,
}

impl ExportPipeline {
    pub fn new(worker: &Path, format: Format, scale: f32, theme: Theme) -> Result<Self> {
        let rasterizer = match format {
            Format::Svg => None,
            Format::Png => Some(Rasterizer::new(theme)),
        };
        Ok(Self {
            format,
            scale,
            rasterizer,
            session: RendererSession::new(worker)?,
        })
    }

    pub fn render(&mut self, json: String) -> Result<Rendered> {
        let Conversion { svg, elements } = self.session.convert(json)?;
        let bytes = match self.format {
            Format::Svg => svg.into_bytes(),
            Format::Png => {
                let rasterizer = self
                    .rasterizer
                    .as_ref()
                    .context("PNG rasterizer is unavailable")?;
                let tree = rasterizer.parse_svg(&svg)?;
                let image = rasterizer.render_full(&tree, self.scale)?;
                let mut bytes = Vec::new();
                image
                    .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
                    .context("failed to encode PNG")?;
                bytes
            }
        };
        Ok(Rendered { bytes, elements })
    }
}

struct Conversion {
    svg: String,
    elements: usize,
}

struct RendererSession {
    converter: Converter,
    events: Receiver<Event>,
    generation: u64,
}

impl RendererSession {
    fn new(worker: &Path) -> Result<Self> {
        let (converter, events) = Converter::spawn(worker)?;
        Ok(Self {
            converter,
            events,
            generation: 0,
        })
    }

    fn convert(&mut self, json: String) -> Result<Conversion> {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.converter.request(generation, json)?;

        let deadline = Instant::now() + CONVERSION_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("timed out waiting for the renderer");
            }
            match self.events.recv_timeout(remaining) {
                Ok(Event::Ready) => {}
                Ok(Event::Converted {
                    generation: response_generation,
                    svg,
                    elements,
                }) if response_generation == generation => {
                    return Ok(Conversion { svg, elements });
                }
                Ok(Event::Failed {
                    generation: response_generation,
                    message,
                }) if response_generation == generation => bail!("{message}"),
                Ok(Event::Died { message }) => bail!("{message}"),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => bail!("timed out waiting for the renderer"),
                Err(RecvTimeoutError::Disconnected) => {
                    bail!("renderer stopped unexpectedly")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_format_from_output_extension() {
        assert_eq!(
            resolve_format(None, Some(Path::new("diagram.svg"))).unwrap(),
            Format::Svg
        );
        assert_eq!(
            resolve_format(None, Some(Path::new("diagram.PNG"))).unwrap(),
            Format::Png
        );
        assert!(resolve_format(None, Some(Path::new("diagram.txt"))).is_err());
        assert!(resolve_format(Some(Format::Svg), Some(Path::new("diagram.png"))).is_err());
    }

    #[test]
    fn derives_default_output_path() {
        assert_eq!(
            default_output_path(Path::new("diagram.excalidraw"), Format::Png).unwrap(),
            PathBuf::from("diagram.png")
        );
        assert_eq!(
            default_output_path(Path::new("diagram"), Format::Svg).unwrap(),
            PathBuf::from("diagram.svg")
        );
        assert!(default_output_path(Path::new("diagram.svg"), Format::Svg).is_err());
    }

    #[test]
    fn reads_bom_prefixed_json_and_reports_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diagram.excalidraw");
        fs::write(&path, "\u{feff}{\"elements\":[]}").unwrap();

        assert_eq!(
            read_source(&path).unwrap().as_deref(),
            Some("{\"elements\":[]}")
        );
        assert!(read_source(&dir.path().join("missing.excalidraw"))
            .unwrap()
            .is_none());
    }
}
