//! Shape original scripts with bundled fonts, including Indic ligatures and RTL.
use anyhow::{Context, Result, ensure};
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use eframe::egui::{Color32, ColorImage};
use std::path::Path;

const FONTS: [&str; 5] = [
    "notosans.ttf",
    "notosanssc.ttf",
    "notosansdevanagari.ttf",
    "notosansbengali.ttf",
    "notonaskharabic.ttf",
];

pub struct TextRenderer {
    fonts: FontSystem,
    cache: SwashCache,
}

pub struct RenderedText {
    pub image: ColorImage,
    pub missing_glyphs: usize,
    pub rtl_lines: usize,
    pub clipped: bool,
}

impl TextRenderer {
    pub fn bundled() -> Result<Self> {
        let exe = std::env::current_exe()?;
        let directory = exe
            .parent()
            .context("Missing application directory")?
            .join("assets/speech-fonts");
        Self::from_directory(&directory)
    }

    pub fn from_directory(directory: &Path) -> Result<Self> {
        let mut database = cosmic_text::fontdb::Database::new();
        for filename in FONTS {
            database
                .load_font_file(directory.join(filename))
                .with_context(|| {
                    format!("Bundled speech font {filename} is missing or unreadable")
                })?;
        }
        database.set_sans_serif_family("Noto Sans");
        Ok(Self {
            fonts: FontSystem::new_with_locale_and_db("en-US".into(), database),
            cache: SwashCache::new(),
        })
    }

    /// Bounded rasterization for a transcript paragraph. The JSON and exports
    /// always retain the full original text; the UI must label a clipped preview.
    pub fn render(
        &mut self,
        text: &str,
        width: usize,
        pixels_per_point: f32,
    ) -> Result<RenderedText> {
        ensure!(
            pixels_per_point.is_finite() && pixels_per_point > 0.,
            "Invalid display scale"
        );
        let scale = pixels_per_point.clamp(0.5, 3.);
        let width = width.clamp(160, 2048);
        let preview: String = text.chars().take(4000).collect();
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(16. * scale, 25. * scale));
        buffer.set_size(Some(width as f32), None);
        buffer.set_text(
            &preview,
            &Attrs::new().family(Family::Name("Noto Sans")),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.fonts, false);
        let mut missing_glyphs = 0;
        let mut rtl_lines = 0;
        let mut desired_height = 1.;
        for run in buffer.layout_runs() {
            missing_glyphs += run.glyphs.iter().filter(|g| g.glyph_id == 0).count();
            rtl_lines += usize::from(run.rtl);
            desired_height = f32::max(desired_height, run.line_top + run.line_height);
        }
        let height = (desired_height.ceil() as usize).clamp(1, 2048);
        let mut image =
            ColorImage::new([width, height], vec![Color32::TRANSPARENT; width * height]);
        buffer.draw(
            &mut self.fonts,
            &mut self.cache,
            Color::rgb(225, 231, 240),
            |x, y, w, h, color| {
                let [r, g, b, a] = color.as_rgba();
                let pixel = Color32::from_rgba_unmultiplied(r, g, b, a);
                for row in y.max(0)..(y + h as i32).min(height as i32) {
                    for column in x.max(0)..(x + w as i32).min(width as i32) {
                        let destination = &mut image.pixels[row as usize * width + column as usize];
                        if destination.a() < a {
                            *destination = pixel;
                        }
                    }
                }
            },
        );
        Ok(RenderedText {
            image,
            missing_glyphs,
            rtl_lines,
            clipped: preview.len() != text.len() || desired_height > height as f32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_shape_original_scripts_including_right_to_left() {
        let directory = std::env::var_os("SOUNDGEO_TEST_SPEECH_FONTS").expect(
            "Prepare the bundled speech fonts before running the speech font qualification",
        );
        let mut renderer = TextRenderer::from_directory(Path::new(&directory)).unwrap();
        for (text, rtl) in [
            ("English français español português bahasa Indonesia", false),
            ("这是中文语音转写。", false),
            ("हिन्दी में मूल प्रतिलेख", false),
            ("বাংলা ভাষায় মূল প্রতিলিপি", false),
            ("النص الأصلي باللغة العربية", true),
            ("اردو میں اصل نقل لاہور", true),
        ] {
            let rendered = renderer.render(text, 500, 1.).unwrap();
            assert_eq!(rendered.missing_glyphs, 0, "{text}");
            assert!(!rendered.clipped, "{text}");
            assert_eq!(rendered.rtl_lines > 0, rtl, "{text}");
            assert!(
                rendered.image.pixels.iter().any(|pixel| pixel.a() > 0),
                "{text}"
            );
        }
    }
}
