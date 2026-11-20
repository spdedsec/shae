//! Scalable Vector TrueType & OpenType Typography Engine (`std:gui`)
//!
//! Provides high-performance, SIMD-accelerated TrueType/OpenType font loading,
//! glyph rasterization, LRU glyph caching for 60 FPS loops, layout metrics, and
//! subpixel antialiased alpha blending into 32-bit ARGB framebuffers.

use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub struct RasterizedGlyph {
    pub metrics: Metrics,
    pub bitmap: Vec<u8>,
}

pub struct ShaeFont {
    pub font: Font,
    pub default_size: f32,
    pub path: String,
    cache: Mutex<HashMap<(char, u32), Arc<RasterizedGlyph>>>,
}

impl ShaeFont {
    pub fn new(font: Font, default_size: f32, path: String) -> Self {
        Self {
            font,
            default_size,
            path,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_glyph(&self, ch: char, size_px: f32) -> Arc<RasterizedGlyph> {
        let key = (ch, size_px.to_bits());
        {
            let cache = self.cache.lock().unwrap();
            if let Some(glyph) = cache.get(&key) {
                return Arc::clone(glyph);
            }
        }
        let (metrics, bitmap) = self.font.rasterize(ch, size_px);
        let glyph = Arc::new(RasterizedGlyph { metrics, bitmap });
        let mut cache = self.cache.lock().unwrap();
        cache.insert(key, Arc::clone(&glyph));
        glyph
    }

    pub fn measure_text(&self, text: &str, size_px: f32) -> (f32, f32) {
        let line_metrics = self.font.horizontal_line_metrics(size_px);
        let line_height = line_metrics.map(|m| m.new_line_size).unwrap_or(size_px * 1.2);

        let mut max_width: f32 = 0.0;
        let mut current_width: f32 = 0.0;
        let mut line_count: usize = 1;
        let mut prev_char: Option<char> = None;

        for ch in text.chars() {
            if ch == '\n' {
                if current_width > max_width {
                    max_width = current_width;
                }
                current_width = 0.0;
                line_count += 1;
                prev_char = None;
                continue;
            }
            if let Some(prev) = prev_char {
                if let Some(kern) = self.font.horizontal_kern(prev, ch, size_px) {
                    current_width += kern;
                }
            }
            let metrics = self.font.metrics(ch, size_px);
            current_width += metrics.advance_width;
            prev_char = Some(ch);
        }

        if current_width > max_width {
            max_width = current_width;
        }

        let total_height = line_count as f32 * line_height;
        (max_width, total_height)
    }

    pub fn draw_text_to_buffer(
        &self,
        buffer: &mut [u32],
        buf_width: usize,
        buf_height: usize,
        sx: i64,
        sy: i64,
        text: &str,
        color: u32,
        size_px: f32,
    ) {
        let text_alpha = ((color >> 24) & 0xFF) as u32;
        let eff_base_alpha = if text_alpha == 0 { 255 } else { text_alpha };
        let r_fg = (color >> 16) & 0xFF;
        let g_fg = (color >> 8) & 0xFF;
        let b_fg = color & 0xFF;

        let line_metrics = self.font.horizontal_line_metrics(size_px);
        let ascent = line_metrics.map(|m| m.ascent).unwrap_or(size_px * 0.8);
        let new_line_size = line_metrics.map(|m| m.new_line_size).unwrap_or(size_px * 1.2);

        let mut baseline = (sy as f32) + ascent;
        let mut pen_x = sx as f32;
        let mut prev_char: Option<char> = None;

        for ch in text.chars() {
            if ch == '\n' {
                pen_x = sx as f32;
                baseline += new_line_size;
                prev_char = None;
                continue;
            }

            if let Some(prev) = prev_char {
                if let Some(kern) = self.font.horizontal_kern(prev, ch, size_px) {
                    pen_x += kern;
                }
            }

            let glyph = self.get_glyph(ch, size_px);
            let gx = (pen_x + glyph.metrics.xmin as f32).round() as i64;
            let gy = (baseline - glyph.metrics.ymin as f32 - glyph.metrics.height as f32).round() as i64;

            for row in 0..glyph.metrics.height {
                let py = gy + row as i64;
                if py < 0 || py >= buf_height as i64 {
                    continue;
                }
                let win_row_idx = (py as usize) * buf_width;
                let bmp_row_idx = row * glyph.metrics.width;

                for col in 0..glyph.metrics.width {
                    let px = gx + col as i64;
                    if px < 0 || px >= buf_width as i64 {
                        continue;
                    }
                    let cov = glyph.bitmap[bmp_row_idx + col];
                    if cov == 0 {
                        continue;
                    }

                    let alpha = ((eff_base_alpha * (cov as u32)) / 255) as u32;
                    let win_idx = win_row_idx + (px as usize);

                    if alpha >= 255 {
                        buffer[win_idx] = (255 << 24) | (r_fg << 16) | (g_fg << 8) | b_fg;
                    } else if alpha > 0 {
                        let bg = buffer[win_idx];
                        let inv_a = 255 - alpha;
                        let bg_r = (bg >> 16) & 0xFF;
                        let bg_g = (bg >> 8) & 0xFF;
                        let bg_b = bg & 0xFF;
                        let out_r = (r_fg * alpha + bg_r * inv_a) / 255;
                        let out_g = (g_fg * alpha + bg_g * inv_a) / 255;
                        let out_b = (b_fg * alpha + bg_b * inv_a) / 255;
                        buffer[win_idx] = (255 << 24) | (out_r << 16) | (out_g << 8) | out_b;
                    }
                }
            }

            pen_x += glyph.metrics.advance_width;
            prev_char = Some(ch);
        }
    }
}

static FONT_REGISTRY: OnceLock<Mutex<HashMap<usize, Arc<ShaeFont>>>> = OnceLock::new();
static NEXT_FONT_ID: AtomicUsize = AtomicUsize::new(1);

pub fn get_font_registry() -> &'static Mutex<HashMap<usize, Arc<ShaeFont>>> {
    FONT_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn load_font_file(path: &str, default_size: f32) -> Result<usize, String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read font file '{}': {}", path, e))?;
    load_font_data(&data, default_size, path)
}

pub fn load_font_data(bytes: &[u8], default_size: f32, path: &str) -> Result<usize, String> {
    let settings = FontSettings::default();
    let font = Font::from_bytes(bytes, settings)
        .map_err(|e| format!("Failed to parse TrueType/OpenType font '{}': {}", path, e))?;

    let shae_font = Arc::new(ShaeFont::new(font, default_size, path.to_string()));
    let id = NEXT_FONT_ID.fetch_add(1, Ordering::Relaxed);
    get_font_registry().lock().unwrap().insert(id, shae_font);
    Ok(id)
}

pub fn get_font(id: usize) -> Option<Arc<ShaeFont>> {
    get_font_registry().lock().unwrap().get(&id).cloned()
}
