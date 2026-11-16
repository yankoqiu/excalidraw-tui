//! In-memory SVG rasterization with system font fallback (CJK included).
//!
//! Zooming re-renders the vector tree at the requested scale, so the image
//! stays sharp instead of being upscaled. Panning renders only the visible
//! viewport into a pixmap that matches the terminal area in pixels.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use image::{DynamicImage, RgbaImage};
use resvg::{
    tiny_skia::{Color, Pixmap, Transform},
    usvg::{self, Tree},
};

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 32.0;
const MAX_PIXELS: u64 = 40_000_000;

const CJK_FONT_PREFERENCES: &[&str] = &[
    "Microsoft YaHei",
    "Microsoft YaHei UI",
    "PingFang SC",
    "Hiragino Sans GB",
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "Source Han Sans SC",
    "Source Han Sans CN",
    "WenQuanYi Micro Hei",
    "Droid Sans Fallback",
    "SimHei",
    "SimSun",
    "Arial Unicode MS",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

pub struct Rasterizer {
    fontdb: Arc<fontdb::Database>,
    default_family: String,
    theme: Theme,
}

impl Rasterizer {
    pub fn new(theme: Theme) -> Self {
        let mut fontdb = fontdb::Database::new();
        fontdb.load_system_fonts();

        let default_family = pick_default_family(&fontdb);
        fontdb.set_sans_serif_family(default_family.clone());
        fontdb.set_serif_family(default_family.clone());

        Self {
            fontdb: Arc::new(fontdb),
            default_family,
            theme,
        }
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }

    pub fn default_family(&self) -> &str {
        &self.default_family
    }

    /// Parses an SVG string into a reusable vector tree.
    pub fn parse_svg(&self, svg: &str) -> Result<Tree> {
        let options = usvg::Options {
            fontdb: Arc::clone(&self.fontdb),
            font_family: self.default_family.clone(),
            dpi: 96.0,
            ..usvg::Options::default()
        };

        Tree::from_str(svg, &options).context("failed to parse SVG from renderer")
    }

    /// Intrinsic size of the SVG in CSS pixels.
    pub fn tree_size(tree: &Tree) -> (f32, f32) {
        let size = tree.size();
        (size.width(), size.height())
    }

    /// Renders the visible viewport.
    ///
    /// `zoom` is relative to "fit the whole diagram inside the area" (1.0 ==
    /// fit). `pan` shifts the viewport by terminal pixels; positive x moves the
    /// view right, positive y moves it down.
    pub fn render_view(
        &self,
        tree: &Tree,
        area_px: (u32, u32),
        zoom: f32,
        pan: (i32, i32),
    ) -> Result<DynamicImage> {
        let (svg_w, svg_h) = Self::tree_size(tree);
        if !(svg_w.is_finite() && svg_h.is_finite()) || svg_w <= 0.0 || svg_h <= 0.0 {
            bail!("diagram has an empty or invalid size ({svg_w}x{svg_h})");
        }

        let aw = area_px.0.max(1);
        let ah = area_px.1.max(1);
        if u64::from(aw) * u64::from(ah) > MAX_PIXELS {
            bail!("terminal image area is too large ({aw}x{ah})");
        }

        let base_scale = (aw as f32 / svg_w).min(ah as f32 / svg_h);
        let scale = base_scale * zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if !scale.is_finite() || scale <= 0.0 {
            bail!("invalid render scale");
        }

        let image_w = svg_w * scale;
        let image_h = svg_h * scale;
        let dx = viewport_origin(image_w, aw as f32, pan.0);
        let dy = viewport_origin(image_h, ah as f32, pan.1);

        let mut pixmap =
            Pixmap::new(aw, ah).context("failed to allocate in-memory image buffer")?;
        pixmap.fill(Color::WHITE);

        let transform = Transform::from_row(scale, 0.0, 0.0, scale, dx, dy);
        resvg::render(tree, transform, &mut pixmap.as_mut());

        let mut pixels = pixmap.take();
        if self.theme == Theme::Dark {
            // The SVG is exported with an opaque white background and dark ink;
            // inverting RGB yields a dark canvas with light ink while keeping
            // alpha untouched. Pixels are opaque, so premultiplied data from
            // tiny-skia is already straight.
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel[0] = 255 - pixel[0];
                pixel[1] = 255 - pixel[1];
                pixel[2] = 255 - pixel[2];
            }
        }

        let image = RgbaImage::from_raw(aw, ah, pixels).context("invalid RGBA pixel buffer")?;
        Ok(DynamicImage::ImageRgba8(image))
    }
}

fn viewport_origin(image_extent: f32, area_extent: f32, pan: i32) -> f32 {
    if image_extent <= area_extent {
        (area_extent - image_extent) / 2.0
    } else {
        let max_offset = (image_extent - area_extent) / 2.0;
        let pan = (pan as f32).clamp(-max_offset, max_offset);
        -(max_offset + pan)
    }
}

fn pick_default_family(fontdb: &fontdb::Database) -> String {
    let is_available = |name: &str| {
        fontdb.faces().any(|face| {
            face.families
                .iter()
                .any(|(family, _)| family.eq_ignore_ascii_case(name))
        })
    };

    for family in CJK_FONT_PREFERENCES {
        if is_available(family) {
            return (*family).to_string();
        }
    }
    for face in fontdb.faces() {
        if let Some((family, _)) = face.families.first() {
            return family.clone();
        }
    }
    "sans-serif".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_svg() -> &'static str {
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="420" height="160" viewBox="0 0 420 160">
  <rect x="0" y="0" width="420" height="160" fill="#ffffff"/>
  <rect x="20" y="30" width="150" height="80" rx="12" fill="#a5d8ff" stroke="#1971c2" stroke-width="2"/>
  <text x="35" y="78" font-family="Virgil, Segoe UI Emoji" font-size="26" fill="#1971c2">中文测试</text>
  <path d="M190 70 H 380" stroke="#1e1e1e" stroke-width="2" fill="none"/>
  <path d="M370 62 L 380 70 L 370 78" stroke="#1e1e1e" stroke-width="2" fill="none"/>
</svg>"##
    }

    #[test]
    fn renders_chinese_text_and_arrow_in_memory() {
        let rasterizer = Rasterizer::new(Theme::Light);
        let tree = rasterizer.parse_svg(sample_svg()).expect("parse svg");
        let image = rasterizer
            .render_view(&tree, (840, 320), 1.0, (0, 0))
            .expect("render");

        assert_eq!(image.width(), 840);
        assert_eq!(image.height(), 320);

        let rgba = image.to_rgba8();
        let ink = rgba
            .pixels()
            .filter(|pixel| pixel.0[0] < 240 || pixel.0[1] < 240 || pixel.0[2] < 240)
            .count();
        assert!(ink > 500, "expected visible diagram content, got {ink}");
    }

    #[test]
    fn dark_theme_inverts_light_background() {
        let rasterizer = Rasterizer::new(Theme::Dark);
        let tree = rasterizer.parse_svg(sample_svg()).expect("parse svg");
        let image = rasterizer
            .render_view(&tree, (420, 160), 1.0, (0, 0))
            .expect("render");
        let rgba = image.to_rgba8();

        let corner = rgba.get_pixel(1, 1);
        assert!(
            corner.0[0] < 20 && corner.0[1] < 20 && corner.0[2] < 20,
            "dark theme background should be dark, got {corner:?}"
        );

        let light_pixels = rgba
            .pixels()
            .filter(|pixel| pixel.0[0] > 200 && pixel.0[1] > 200 && pixel.0[2] > 200)
            .count();
        assert!(light_pixels > 100, "expected inverted (light) ink");
    }

    #[test]
    fn zoom_and_pan_keep_viewport_size() {
        let rasterizer = Rasterizer::new(Theme::Light);
        let tree = rasterizer.parse_svg(sample_svg()).expect("parse svg");
        let image = rasterizer
            .render_view(&tree, (400, 200), 3.0, (120, -40))
            .expect("render");
        assert_eq!((image.width(), image.height()), (400, 200));
    }
}
