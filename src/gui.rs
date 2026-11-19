use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::HashSet;
use std::io::Write;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

// -------------------------------------------------------------
// Canvas & Window State
// -------------------------------------------------------------

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>, // 0x00RRGGBB
    pub title: String,
    pub is_open: bool,
    pub headless: bool,
    pub last_frame_time: Option<Instant>,
    pub target_fps: u32,
    pub frame_counter: usize,
    pub max_frames: Option<usize>,
    pub keys_down: HashSet<String>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, title: String) -> Self {
        let headless_env = std::env::var("SHAE_HEADLESS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let max_frames_env = std::env::var("SHAE_MAX_FRAMES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok());
        let w = width.max(1);
        let h = height.max(1);
        Canvas {
            width: w,
            height: h,
            pixels: vec![0; w * h],
            title,
            is_open: true,
            headless: headless_env,
            last_frame_time: None,
            target_fps: 60,
            frame_counter: 0,
            max_frames: max_frames_env,
            keys_down: HashSet::new(),
        }
    }
}

pub fn get_canvas() -> &'static RwLock<Canvas> {
    static CANVAS: OnceLock<RwLock<Canvas>> = OnceLock::new();
    CANVAS.get_or_init(|| RwLock::new(Canvas::new(80, 40, "Shae Space Invaders".to_string())))
}

// -------------------------------------------------------------
// Internal Helper Functions
// -------------------------------------------------------------

pub fn init_window_internal(width: usize, height: usize, title: &str) {
    let mut canvas = get_canvas().write().unwrap();
    let w = width.max(1);
    let h = height.max(1);
    canvas.width = w;
    canvas.height = h;
    canvas.pixels = vec![0; w * h];
    canvas.title = title.to_string();
    canvas.is_open = true;
    canvas.frame_counter = 0;
    canvas.last_frame_time = None;
    if let Ok(v) = std::env::var("SHAE_HEADLESS") {
        canvas.headless = v == "1" || v.eq_ignore_ascii_case("true");
    }
    if let Ok(v) = std::env::var("SHAE_MAX_FRAMES") {
        canvas.max_frames = v.parse::<usize>().ok();
    }
}

pub fn close_window_internal() {
    let mut canvas = get_canvas().write().unwrap();
    canvas.is_open = false;
}

pub fn should_close_internal() -> bool {
    let canvas = get_canvas().read().unwrap();
    if !canvas.is_open {
        return true;
    }
    if let Some(max) = canvas.max_frames {
        if canvas.frame_counter >= max {
            return true;
        }
    }
    false
}

pub fn set_headless_internal(headless: bool) {
    let mut canvas = get_canvas().write().unwrap();
    canvas.headless = headless;
}

pub fn is_headless_internal() -> bool {
    get_canvas().read().unwrap().headless
}

pub fn get_dimensions_internal() -> (usize, usize) {
    let canvas = get_canvas().read().unwrap();
    (canvas.width, canvas.height)
}

pub fn clear_internal(color: u32) {
    let mut canvas = get_canvas().write().unwrap();
    let c = color & 0x00FFFFFF;
    canvas.pixels.fill(c);
}

pub fn set_pixel_internal(x: i64, y: i64, color: u32) {
    let mut canvas = get_canvas().write().unwrap();
    if x >= 0 && (x as usize) < canvas.width && y >= 0 && (y as usize) < canvas.height {
        let idx = (y as usize) * canvas.width + (x as usize);
        canvas.pixels[idx] = color & 0x00FFFFFF;
    }
}

pub fn get_pixel_internal(x: i64, y: i64) -> u32 {
    let canvas = get_canvas().read().unwrap();
    if x >= 0 && (x as usize) < canvas.width && y >= 0 && (y as usize) < canvas.height {
        let idx = (y as usize) * canvas.width + (x as usize);
        canvas.pixels[idx]
    } else {
        0
    }
}

pub fn fill_rect_internal(x: i64, y: i64, w: i64, h: i64, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let mut canvas = get_canvas().write().unwrap();
    let c = color & 0x00FFFFFF;
    let cw = canvas.width as i64;
    let ch = canvas.height as i64;

    let x_start = x.clamp(0, cw);
    let x_end = (x + w).clamp(0, cw);
    let y_start = y.clamp(0, ch);
    let y_end = (y + h).clamp(0, ch);

    for py in y_start..y_end {
        let row_offset = (py as usize) * canvas.width;
        for px in x_start..x_end {
            canvas.pixels[row_offset + (px as usize)] = c;
        }
    }
}

pub fn draw_rect_internal(x: i64, y: i64, w: i64, h: i64, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }
    draw_line_internal(x, y, x + w - 1, y, color);
    draw_line_internal(x, y + h - 1, x + w - 1, y + h - 1, color);
    draw_line_internal(x, y, x, y + h - 1, color);
    draw_line_internal(x + w - 1, y, x + w - 1, y + h - 1, color);
}

pub fn draw_line_internal(x0: i64, y0: i64, x1: i64, y1: i64, color: u32) {
    let mut x0 = x0;
    let mut y0 = y0;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        set_pixel_internal(x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

pub fn draw_circle_internal(cx: i64, cy: i64, r: i64, color: u32) {
    if r < 0 {
        return;
    }
    let mut x = 0;
    let mut y = r;
    let mut d = 1 - r;

    let plot8 = |x: i64, y: i64| {
        set_pixel_internal(cx + x, cy + y, color);
        set_pixel_internal(cx - x, cy + y, color);
        set_pixel_internal(cx + x, cy - y, color);
        set_pixel_internal(cx - x, cy - y, color);
        set_pixel_internal(cx + y, cy + x, color);
        set_pixel_internal(cx - y, cy + x, color);
        set_pixel_internal(cx + y, cy - x, color);
        set_pixel_internal(cx - y, cy - x, color);
    };

    plot8(x, y);
    while x < y {
        x += 1;
        if d < 0 {
            d += 2 * x + 1;
        } else {
            y -= 1;
            d += 2 * (x - y) + 1;
        }
        plot8(x, y);
    }
}

pub fn fill_circle_internal(cx: i64, cy: i64, r: i64, color: u32) {
    if r < 0 {
        return;
    }
    let r_sq = r * r;
    for dy in -r..=r {
        let dx = ((r_sq - dy * dy) as f64).sqrt().round() as i64;
        draw_line_internal(cx - dx, cy + dy, cx + dx, cy + dy, color);
    }
}

// -------------------------------------------------------------
// 5x7 Retro Bitmap Font Table
// -------------------------------------------------------------

fn glyph_for_char(c: char) -> [u8; 7] {
    let uc = c.to_ascii_uppercase();
    match uc {
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x06, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0E, 0x11, 0x10, 0x0E, 0x01, 0x11, 0x0E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '=' => [0x00, 0x1F, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '>' => [0x10, 0x08, 0x04, 0x02, 0x04, 0x08, 0x10],
        '<' => [0x01, 0x02, 0x04, 0x08, 0x04, 0x02, 0x01],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '?' => [0x0E, 0x11, 0x01, 0x06, 0x04, 0x00, 0x04],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x08],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '[' => [0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E],
        ']' => [0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E],
        '*' => [0x00, 0x15, 0x0E, 0x1F, 0x0E, 0x15, 0x00],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        '#' => [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A],
        '%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
        _ => [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F],
    }
}

pub fn draw_text_internal(x: i64, y: i64, text: &str, color: u32) {
    let mut cur_x = x;
    let mut cur_y = y;
    for ch in text.chars() {
        if ch == '\n' {
            cur_x = x;
            cur_y += 8;
            continue;
        }
        let glyph = glyph_for_char(ch);
        for (row_idx, &row_bits) in glyph.iter().enumerate() {
            for col_idx in 0..5 {
                if (row_bits & (1 << (4 - col_idx))) != 0 {
                    set_pixel_internal(cur_x + col_idx as i64, cur_y + row_idx as i64, color);
                }
            }
        }
        cur_x += 6;
    }
}

pub fn draw_sprite_internal(x: i64, y: i64, rows: &[String], color: u32) {
    for (r_idx, row) in rows.iter().enumerate() {
        for (c_idx, ch) in row.chars().enumerate() {
            if ch != ' ' && ch != '.' && ch != '0' {
                set_pixel_internal(x + c_idx as i64, y + r_idx as i64, color);
            }
        }
    }
}

// -------------------------------------------------------------
// Rendering & Delay
// -------------------------------------------------------------

pub fn render_frame_internal() {
    let mut canvas = get_canvas().write().unwrap();
    canvas.frame_counter += 1;
    canvas.last_frame_time = Some(Instant::now());

    if canvas.headless {
        return;
    }

    // Terminal Half-block renderer (▀)
    let w = canvas.width;
    let h = canvas.height;
    let mut out = String::with_capacity(w * h * 12);
    out.push_str("\x1b[H"); // Cursor to home position

    for y in (0..h).step_by(2) {
        for x in 0..w {
            let top_pixel = canvas.pixels[y * w + x];
            let bot_pixel = if y + 1 < h {
                canvas.pixels[(y + 1) * w + x]
            } else {
                0
            };

            let tr = (top_pixel >> 16) & 0xFF;
            let tg = (top_pixel >> 8) & 0xFF;
            let tb = top_pixel & 0xFF;

            let br = (bot_pixel >> 16) & 0xFF;
            let bg = (bot_pixel >> 8) & 0xFF;
            let bb = bot_pixel & 0xFF;

            use std::fmt::Write as _;
            let _ = write!(
                out,
                "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m▀",
                tr, tg, tb, br, bg, bb
            );
        }
        out.push_str("\x1b[0m\n");
    }

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = handle.write_all(out.as_bytes());
    let _ = handle.flush();
}

pub fn frame_delay_internal(fps: u32) {
    let fast_test = std::env::var("SHAE_FAST_TEST")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if fast_test {
        return;
    }

    let target_fps = fps.max(1);
    let frame_dur = Duration::from_micros(1_000_000 / target_fps as u64);

    let last_time = {
        let canvas = get_canvas().read().unwrap();
        canvas.last_frame_time
    };

    if let Some(prev) = last_time {
        let elapsed = prev.elapsed();
        if elapsed < frame_dur {
            std::thread::sleep(frame_dur - elapsed);
        }
    } else {
        std::thread::sleep(frame_dur);
    }

    let mut canvas = get_canvas().write().unwrap();
    canvas.last_frame_time = Some(Instant::now());
}

// -------------------------------------------------------------
// Input System
// -------------------------------------------------------------

fn normalize_key(k: &str) -> String {
    let s = k.trim().to_lowercase();
    match s.as_str() {
        " " | "spacebar" => "space".to_string(),
        "arrowleft" | "left_arrow" => "left".to_string(),
        "arrowright" | "right_arrow" => "right".to_string(),
        "arrowup" | "up_arrow" => "up".to_string(),
        "arrowdown" | "down_arrow" => "down".to_string(),
        "return" => "enter".to_string(),
        "esc" => "escape".to_string(),
        other => other.to_string(),
    }
}

pub fn is_key_down_internal(key: &str) -> bool {
    let canvas = get_canvas().read().unwrap();
    let norm = normalize_key(key);
    canvas.keys_down.contains(&norm)
}

pub fn simulate_key_internal(key: &str, is_down: bool) {
    let mut canvas = get_canvas().write().unwrap();
    let norm = normalize_key(key);
    if is_down {
        canvas.keys_down.insert(norm);
    } else {
        canvas.keys_down.remove(&norm);
    }
}

pub fn poll_events_internal() {
    // In headless mode or automated tests, keys are supplied via simulate_key.
}

// -------------------------------------------------------------
// Value / Color Extraction Helpers
// -------------------------------------------------------------

pub fn parse_color_value(val: &Value) -> u32 {
    match val {
        Value::Int(n) => (*n as u32) & 0x00FFFFFF,
        Value::Float(f) | Value::Number(f) => (*f as u32) & 0x00FFFFFF,
        Value::String(s) => parse_color_str(s),
        _ => 0xFFFFFF,
    }
}

pub fn parse_color_str(s: &str) -> u32 {
    let trimmed = s.trim();
    if let Some(hex) = trimmed.strip_prefix('#') {
        u32::from_str_radix(hex, 16).unwrap_or(0xFFFFFF) & 0x00FFFFFF
    } else if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).unwrap_or(0xFFFFFF) & 0x00FFFFFF
    } else {
        match trimmed.to_lowercase().as_str() {
            "black" => 0x000000,
            "white" => 0xFFFFFF,
            "red" => 0xFF0000,
            "green" => 0x00FF00,
            "blue" => 0x0000FF,
            "yellow" => 0xFFFF00,
            "cyan" => 0x00FFFF,
            "magenta" => 0xFF00FF,
            "orange" => 0xFFA500,
            "purple" => 0x800080,
            "gray" | "grey" => 0x888888,
            "darkgray" | "darkgrey" => 0x444444,
            _ => 0xFFFFFF,
        }
    }
}

fn val_to_i64(val: &Value) -> i64 {
    match val {
        Value::Int(n) => *n,
        Value::Float(f) | Value::Number(f) => *f as i64,
        _ => 0,
    }
}

// -------------------------------------------------------------
// Shae Standard Library std:gui Bindings
// -------------------------------------------------------------

pub fn build_gui_module() -> Value {
    crate::stdlib::make_map(vec![
        ("init_window", gui_init_window),
        ("close_window", gui_close_window),
        ("should_close", gui_should_close),
        ("set_headless", gui_set_headless),
        ("is_headless", gui_is_headless),
        ("set_target_fps", gui_set_target_fps),
        ("get_dimensions", gui_get_dimensions),
        ("clear", gui_clear),
        ("fill_rect", gui_fill_rect),
        ("draw_rect", gui_draw_rect),
        ("draw_line", gui_draw_line),
        ("draw_circle", gui_draw_circle),
        ("fill_circle", gui_fill_circle),
        ("draw_text", gui_draw_text),
        ("draw_sprite", gui_draw_sprite),
        ("get_pixel", gui_get_pixel),
        ("render_frame", gui_render_frame),
        ("frame_delay", gui_frame_delay),
        ("poll_events", gui_poll_events),
        ("is_key_down", gui_is_key_down),
        ("simulate_key", gui_simulate_key),
    ])
}

fn gui_init_window(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let w = if !args.is_empty() {
        val_to_i64(&args[0]) as usize
    } else {
        80
    };
    let h = if args.len() > 1 {
        val_to_i64(&args[1]) as usize
    } else {
        40
    };
    let title = if args.len() > 2 {
        args[2].to_display()
    } else {
        "Shae GUI".to_string()
    };

    init_window_internal(w, h, &title);

    let mut map = IndexMap::new();
    map.insert("width".to_string(), Value::Int(w as i64));
    map.insert("height".to_string(), Value::Int(h as i64));
    map.insert("title".to_string(), Value::String(title));
    map.insert("open".to_string(), Value::Bool(true));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn gui_close_window(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    close_window_internal();
    Ok(Value::Null)
}

fn gui_should_close(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    Ok(Value::Bool(should_close_internal()))
}

fn gui_set_headless(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("gui.set_headless", 1, &args, span)?;
    let headless = args[0].is_truthy();
    set_headless_internal(headless);
    Ok(Value::Null)
}

fn gui_is_headless(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    Ok(Value::Bool(is_headless_internal()))
}

fn gui_set_target_fps(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("gui.set_target_fps", 1, &args, span)?;
    let fps = val_to_i64(&args[0]).max(1) as u32;
    get_canvas().write().unwrap().target_fps = fps;
    Ok(Value::Null)
}

fn gui_get_dimensions(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let (w, h) = get_dimensions_internal();
    let dims = vec![Value::Int(w as i64), Value::Int(h as i64)];
    Ok(Value::Array(Arc::new(RwLock::new(dims))))
}

fn gui_clear(_ev: &mut Evaluator, args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let color = if !args.is_empty() {
        parse_color_value(&args[0])
    } else {
        0
    };
    clear_internal(color);
    Ok(Value::Null)
}

fn gui_fill_rect(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.fill_rect", 5, &args, span)?;
    let x = val_to_i64(&args[0]);
    let y = val_to_i64(&args[1]);
    let w = val_to_i64(&args[2]);
    let h = val_to_i64(&args[3]);
    let color = parse_color_value(&args[4]);
    fill_rect_internal(x, y, w, h, color);
    Ok(Value::Null)
}

fn gui_draw_rect(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.draw_rect", 5, &args, span)?;
    let x = val_to_i64(&args[0]);
    let y = val_to_i64(&args[1]);
    let w = val_to_i64(&args[2]);
    let h = val_to_i64(&args[3]);
    let color = parse_color_value(&args[4]);
    draw_rect_internal(x, y, w, h, color);
    Ok(Value::Null)
}

fn gui_draw_line(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.draw_line", 5, &args, span)?;
    let x0 = val_to_i64(&args[0]);
    let y0 = val_to_i64(&args[1]);
    let x1 = val_to_i64(&args[2]);
    let y1 = val_to_i64(&args[3]);
    let color = parse_color_value(&args[4]);
    draw_line_internal(x0, y0, x1, y1, color);
    Ok(Value::Null)
}

fn gui_draw_circle(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("gui.draw_circle", 4, &args, span)?;
    let cx = val_to_i64(&args[0]);
    let cy = val_to_i64(&args[1]);
    let r = val_to_i64(&args[2]);
    let color = parse_color_value(&args[3]);
    draw_circle_internal(cx, cy, r, color);
    Ok(Value::Null)
}

fn gui_fill_circle(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("gui.fill_circle", 4, &args, span)?;
    let cx = val_to_i64(&args[0]);
    let cy = val_to_i64(&args[1]);
    let r = val_to_i64(&args[2]);
    let color = parse_color_value(&args[3]);
    fill_circle_internal(cx, cy, r, color);
    Ok(Value::Null)
}

fn gui_draw_text(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.draw_text", 4, &args, span)?;
    let x = val_to_i64(&args[0]);
    let y = val_to_i64(&args[1]);
    let text = match &args[2] {
        Value::String(s) => s.clone(),
        other => other.to_display(),
    };
    let color = parse_color_value(&args[3]);
    draw_text_internal(x, y, &text, color);
    Ok(Value::Null)
}

fn gui_draw_sprite(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("gui.draw_sprite", 4, &args, span)?;
    let x = val_to_i64(&args[0]);
    let y = val_to_i64(&args[1]);
    let color = parse_color_value(&args[3]);

    match &args[2] {
        Value::Array(arr) => {
            let guard = arr.read().unwrap();
            let mut string_rows = Vec::new();
            let mut is_strings = true;

            for item in guard.iter() {
                if let Value::String(s) = item {
                    string_rows.push(s.clone());
                } else {
                    is_strings = false;
                    break;
                }
            }

            if is_strings {
                draw_sprite_internal(x, y, &string_rows, color);
            } else {
                // 2D Array of ints / bools / colors
                for (r_idx, row_item) in guard.iter().enumerate() {
                    if let Value::Array(row_arr) = row_item {
                        let row_guard = row_arr.read().unwrap();
                        for (c_idx, cell) in row_guard.iter().enumerate() {
                            let cell_color = match cell {
                                Value::Int(n) if *n > 1 => (*n as u32) & 0x00FFFFFF,
                                Value::Int(n) if *n == 1 => color,
                                Value::Bool(true) => color,
                                Value::Float(f) | Value::Number(f) if *f > 1.0 => {
                                    (*f as u32) & 0x00FFFFFF
                                }
                                Value::Float(f) | Value::Number(f) if *f > 0.0 => color,
                                _ => 0,
                            };
                            if cell_color != 0 {
                                set_pixel_internal(x + c_idx as i64, y + r_idx as i64, cell_color);
                            }
                        }
                    }
                }
            }
        }
        _ => {
            return Err(RuntimeError::new(
                "gui.draw_sprite expects an array of strings or 2D array for sprite matrix".into(),
            )
            .at(span));
        }
    }

    Ok(Value::Null)
}

fn gui_get_pixel(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.get_pixel", 2, &args, span)?;
    let x = val_to_i64(&args[0]);
    let y = val_to_i64(&args[1]);
    let pix = get_pixel_internal(x, y);
    Ok(Value::Int(pix as i64))
}

fn gui_render_frame(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    render_frame_internal();
    Ok(Value::Null)
}

fn gui_frame_delay(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let fps = if !args.is_empty() {
        val_to_i64(&args[0]).max(1) as u32
    } else {
        60
    };
    frame_delay_internal(fps);
    Ok(Value::Null)
}

fn gui_poll_events(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    poll_events_internal();
    Ok(Value::Null)
}

fn gui_is_key_down(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("gui.is_key_down", 1, &args, span)?;
    let key = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "gui.is_key_down expects key name as string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    Ok(Value::Bool(is_key_down_internal(key)))
}

fn gui_simulate_key(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("gui.simulate_key", 2, &args, span)?;
    let key = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(RuntimeError::new(format!(
                "gui.simulate_key expects key name as string, got {}",
                other.type_name()
            ))
            .at(span));
        }
    };
    let is_down = args[1].is_truthy();
    simulate_key_internal(key, is_down);
    Ok(Value::Null)
}
