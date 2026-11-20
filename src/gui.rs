use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use minifb::{Key, MouseButton, MouseMode, Scale, Window, WindowOptions};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

// -------------------------------------------------------------
// Embedded 8x8 Retro Bitmap Font (ASCII 32 ' ' to 126 '~')
// -------------------------------------------------------------
use crate::font_8x8::get_glyph_8x8;

pub struct SendWindow(pub Window);
unsafe impl Send for SendWindow {}
unsafe impl Sync for SendWindow {}

pub enum WindowBackend {
    Real(SendWindow),
    Headless { open: bool, ticks: usize },
}

pub struct ShaeWindow {
    pub backend: WindowBackend,
    pub width: usize,
    pub height: usize,
    pub buffer: Vec<u32>,
    pub keys_down: Vec<String>,
    pub mouse_pos: Option<(usize, usize)>,
    pub mouse_down: (bool, bool, bool), // (left, middle, right)
    pub last_frame: Instant,
}

static WINDOW_REGISTRY: OnceLock<Mutex<HashMap<usize, ShaeWindow>>> = OnceLock::new();
static NEXT_WINDOW_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

pub fn get_window_registry() -> &'static Mutex<HashMap<usize, ShaeWindow>> {
    WINDOW_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_registry() -> &'static Mutex<HashMap<usize, ShaeWindow>> {
    get_window_registry()
}

#[derive(Clone)]
pub struct ShaeImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>, // 0xAARRGGBB format
}

static IMAGE_REGISTRY: OnceLock<Mutex<HashMap<usize, Arc<ShaeImage>>>> = OnceLock::new();
static NEXT_IMAGE_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

pub fn get_image_registry() -> &'static Mutex<HashMap<usize, Arc<ShaeImage>>> {
    IMAGE_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn build_gui_module() -> Value {
    let mut map = IndexMap::new();
    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("window", gui_window),
        ("is_open", gui_is_open),
        ("isOpen", gui_is_open),
        ("close", gui_close),
        ("clear", gui_clear),
        ("pixel", gui_pixel),
        ("rect", gui_rect),
        ("rect_outline", gui_rect_outline),
        ("rectOutline", gui_rect_outline),
        ("circle", gui_circle),
        ("circle_outline", gui_circle_outline),
        ("circleOutline", gui_circle_outline),
        ("line", gui_line),
        ("text", gui_text),
        ("render", gui_render),
        ("is_key_down", gui_is_key_down),
        ("isKeyDown", gui_is_key_down),
        ("get_keys", gui_get_keys),
        ("getKeys", gui_get_keys),
        ("mouse_pos", gui_mouse_pos),
        ("mousePos", gui_mouse_pos),
        ("is_mouse_down", gui_is_mouse_down),
        ("isMouseDown", gui_is_mouse_down),
        ("limit_fps", gui_limit_fps),
        ("limitFps", gui_limit_fps),
        ("sleep", gui_sleep),
        ("load_image", gui_load_image),
        ("loadImage", gui_load_image),
        ("create_image", gui_create_image),
        ("createImage", gui_create_image),
        ("draw_image", gui_draw_image),
        ("drawImage", gui_draw_image),
        ("draw_image_scaled", gui_draw_image_scaled),
        ("drawImageScaled", gui_draw_image_scaled),
        ("draw_sprite", gui_draw_sprite),
        ("drawSprite", gui_draw_sprite),
        ("image_size", gui_image_size),
        ("imageSize", gui_image_size),
        ("get_image_pixel", gui_get_image_pixel),
        ("getImagePixel", gui_get_image_pixel),
        ("set_image_pixel", gui_set_image_pixel),
        ("setImagePixel", gui_set_image_pixel),
        ("init_window", gui_init_window),
        ("close_window", gui_close_window),
        ("should_close", gui_should_close),
        ("set_headless", gui_set_headless),
        ("is_headless", gui_is_headless),
        ("set_target_fps", gui_set_target_fps),
        ("get_dimensions", gui_get_dimensions),
        ("fill_rect", gui_fill_rect),
        ("draw_rect", gui_draw_rect),
        ("draw_line", gui_draw_line),
        ("draw_circle", gui_draw_circle),
        ("fill_circle", gui_fill_circle),
        ("draw_text", gui_draw_text),
        ("get_pixel", gui_get_pixel),
        ("render_frame", gui_render_frame),
        ("frame_delay", gui_frame_delay),
        ("poll_events", gui_poll_events),
        ("simulate_key", gui_simulate_key),
        ("load_font", gui_load_font),
        ("loadFont", gui_load_font),
        ("draw_text_font", gui_draw_text_font),
        ("drawTextFont", gui_draw_text_font),
        ("measure_text_font", gui_measure_text_font),
        ("measureTextFont", gui_measure_text_font),
    ];
    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: name.to_string(),
                func,
            },
        );
    }
    Value::Map(Arc::new(RwLock::new(map)))
}

fn to_win_id(val: &Value) -> Option<usize> {
    match val {
        Value::Int(i) if *i >= 0 => Some(*i as usize),
        Value::Float(f) | Value::Number(f) if *f >= 0.0 => Some(*f as usize),
        _ => None,
    }
}

fn to_i64(val: &Value) -> Option<i64> {
    match val {
        Value::Int(i) => Some(*i),
        Value::Float(f) | Value::Number(f) => Some(*f as i64),
        _ => None,
    }
}

fn to_usize(val: &Value) -> Option<usize> {
    match val {
        Value::Int(i) if *i >= 0 => Some(*i as usize),
        Value::Float(f) | Value::Number(f) if *f >= 0.0 => Some(*f as usize),
        _ => None,
    }
}

fn to_u32_color(val: &Value) -> Option<u32> {
    match val {
        Value::Int(i) => Some(*i as u32),
        Value::Float(f) | Value::Number(f) => Some(*f as u32),
        _ => None,
    }
}

fn parse_color(val: &Value) -> u32 {
    match val {
        Value::Int(i) => (*i as u32) & 0x00FFFFFF,
        Value::Float(n) | Value::Number(n) => (*n as u32) & 0x00FFFFFF,
        Value::String(s) => {
            let clean = s.trim().trim_start_matches('#');
            if let Ok(c) = u32::from_str_radix(clean, 16) {
                c & 0x00FFFFFF
            } else {
                match clean.to_lowercase().as_str() {
                    "black" => 0x000000,
                    "white" => 0xFFFFFF,
                    "red" => 0xFF0000,
                    "green" => 0x00FF00,
                    "blue" => 0x0000FF,
                    "yellow" => 0xFFFF00,
                    "cyan" => 0x00FFFF,
                    "magenta" => 0xFF00FF,
                    "gray" | "grey" => 0x808080,
                    "dark_gray" => 0x303030,
                    "orange" => 0xFFA500,
                    "purple" => 0x800080,
                    _ => 0xFFFFFF,
                }
            }
        }
        _ => 0xFFFFFF,
    }
}

fn gui_window(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 3 {
        return Err(RuntimeError::new("gui.window expects (title, width, height)".into()).at(span));
    }
    let title = match &args[0] {
        Value::String(s) => s.clone(),
        other => other.to_display(),
    };
    let width = to_usize(&args[1]).unwrap_or(640);
    let height = to_usize(&args[2]).unwrap_or(480);

    let mut options = WindowOptions::default();
    options.scale = Scale::X1;
    options.resize = false;

    // Attempt to open real desktop window; fallback to headless if in CI/SSH without display
    let backend = match Window::new(&title, width, height, options) {
        Ok(win) => WindowBackend::Real(SendWindow(win)),
        Err(e) => {
            eprintln!(
                "[Shae GUI] Notice: Could not open native display window ({}). Using virtual headless canvas.",
                e
            );
            WindowBackend::Headless {
                open: true,
                ticks: 0,
            }
        }
    };

    let buffer = vec![0u32; width * height];
    let win_id = NEXT_WINDOW_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let shae_win = ShaeWindow {
        backend,
        width,
        height,
        buffer,
        keys_down: Vec::new(),
        mouse_pos: None,
        mouse_down: (false, false, false),
        last_frame: Instant::now(),
    };

    get_registry().lock().unwrap().insert(win_id, shae_win);
    Ok(Value::Int(win_id as i64))
}

fn gui_is_open(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.is_open", 1, &args, span)?;
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Bool(false)),
    };

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        match &mut win.backend {
            WindowBackend::Real(SendWindow(w)) => {
                let open = w.is_open() && !w.is_key_down(Key::Escape);
                Ok(Value::Bool(open))
            }
            WindowBackend::Headless { open, ticks } => {
                *ticks += 1;
                // In headless tests, keep open for up to 300 frames then close gracefully
                if *ticks > 300 {
                    *open = false;
                }
                Ok(Value::Bool(*open))
            }
        }
    } else {
        Ok(Value::Bool(false))
    }
}

fn gui_close(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.close", 1, &args, span)?;
    if let Some(win_id) = to_win_id(&args[0]) {
        get_registry().lock().unwrap().remove(&win_id);
    }
    Ok(Value::Null)
}

fn gui_clear(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() == 1 {
        return gui_clear_retro(ev, args, span);
    }
    if args.is_empty() {
        return Err(RuntimeError::new("gui.clear expects (window, [color])".into()).at(span));
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let color = if args.len() > 1 {
        parse_color(&args[1])
    } else {
        0x000000
    };

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        win.buffer.fill(color);
    }
    Ok(Value::Null)
}

fn gui_pixel(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 4 {
        return Err(RuntimeError::new("gui.pixel expects (window, x, y, color)".into()).at(span));
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let x = match to_i64(&args[1]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let y = match to_i64(&args[2]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let color = parse_color(&args[3]);

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        if x >= 0 && (x as usize) < win.width && y >= 0 && (y as usize) < win.height {
            let idx = (y as usize) * win.width + (x as usize);
            win.buffer[idx] = color;
        }
    }
    Ok(Value::Null)
}

fn gui_rect(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 6 {
        return Err(
            RuntimeError::new("gui.rect expects (window, x, y, w, h, color)".into()).at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let rx = match to_i64(&args[1]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let ry = match to_i64(&args[2]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let rw = match to_i64(&args[3]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let rh = match to_i64(&args[4]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let color = parse_color(&args[5]);

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        let x0 = rx.max(0) as usize;
        let y0 = ry.max(0) as usize;
        let x1 = ((rx + rw).max(0) as usize).min(win.width);
        let y1 = ((ry + rh).max(0) as usize).min(win.height);

        for y in y0..y1 {
            let row_start = y * win.width;
            for x in x0..x1 {
                win.buffer[row_start + x] = color;
            }
        }
    }
    Ok(Value::Null)
}

fn gui_rect_outline(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 6 {
        return Err(RuntimeError::new(
            "gui.rect_outline expects (window, x, y, w, h, color, [thickness])".into(),
        )
        .at(span));
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let rx = match to_i64(&args[1]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let ry = match to_i64(&args[2]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let rw = match to_i64(&args[3]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let rh = match to_i64(&args[4]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let color = parse_color(&args[5]);
    let thick = if args.len() > 6 {
        to_usize(&args[6]).unwrap_or(1).max(1)
    } else {
        1
    };

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        let width = win.width;
        let height = win.height;

        let draw_p = |buf: &mut Vec<u32>, px: i64, py: i64| {
            if px >= 0 && (px as usize) < width && py >= 0 && (py as usize) < height {
                buf[(py as usize) * width + (px as usize)] = color;
            }
        };

        for t in 0..(thick as i64) {
            for x in (rx + t)..=(rx + rw - 1 - t) {
                draw_p(&mut win.buffer, x, ry + t);
                draw_p(&mut win.buffer, x, ry + rh - 1 - t);
            }
            for y in (ry + t)..=(ry + rh - 1 - t) {
                draw_p(&mut win.buffer, rx + t, y);
                draw_p(&mut win.buffer, rx + rw - 1 - t, y);
            }
        }
    }
    Ok(Value::Null)
}

fn gui_circle(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 5 {
        return Err(
            RuntimeError::new("gui.circle expects (window, cx, cy, r, color)".into()).at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let cx = match to_i64(&args[1]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let cy = match to_i64(&args[2]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let r = match to_i64(&args[3]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let color = parse_color(&args[4]);

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        let r2 = r * r;
        let y0 = (cy - r).max(0);
        let y1 = (cy + r).min(win.height as i64 - 1);
        let x0 = (cx - r).max(0);
        let x1 = (cx + r).min(win.width as i64 - 1);

        for y in y0..=y1 {
            let dy = y - cy;
            let dy2 = dy * dy;
            let row_start = (y as usize) * win.width;
            for x in x0..=x1 {
                let dx = x - cx;
                if dx * dx + dy2 <= r2 {
                    win.buffer[row_start + (x as usize)] = color;
                }
            }
        }
    }
    Ok(Value::Null)
}

fn gui_circle_outline(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 5 {
        return Err(RuntimeError::new(
            "gui.circle_outline expects (window, cx, cy, r, color)".into(),
        )
        .at(span));
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let cx = match to_i64(&args[1]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let cy = match to_i64(&args[2]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let r = match to_i64(&args[3]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let color = parse_color(&args[4]);

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        let width = win.width;
        let height = win.height;

        let draw_p = |buf: &mut Vec<u32>, px: i64, py: i64| {
            if px >= 0 && (px as usize) < width && py >= 0 && (py as usize) < height {
                buf[(py as usize) * width + (px as usize)] = color;
            }
        };

        // Midpoint Circle Algorithm
        let mut x = r;
        let mut y = 0i64;
        let mut err = 0i64;

        while x >= y {
            draw_p(&mut win.buffer, cx + x, cy + y);
            draw_p(&mut win.buffer, cx + y, cy + x);
            draw_p(&mut win.buffer, cx - y, cy + x);
            draw_p(&mut win.buffer, cx - x, cy + y);
            draw_p(&mut win.buffer, cx - x, cy - y);
            draw_p(&mut win.buffer, cx - y, cy - x);
            draw_p(&mut win.buffer, cx + y, cy - x);
            draw_p(&mut win.buffer, cx + x, cy - y);

            y += 1;
            err += 1 + 2 * y;
            if 2 * (err - x) + 1 > 0 {
                x -= 1;
                err += 1 - 2 * x;
            }
        }
    }
    Ok(Value::Null)
}

fn gui_line(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 6 {
        return Err(
            RuntimeError::new("gui.line expects (window, x0, y0, x1, y1, color)".into()).at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let mut x0 = match to_i64(&args[1]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let mut y0 = match to_i64(&args[2]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let x1 = match to_i64(&args[3]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let y1 = match to_i64(&args[4]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let color = parse_color(&args[5]);

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        let width = win.width;
        let height = win.height;

        loop {
            if x0 >= 0 && (x0 as usize) < width && y0 >= 0 && (y0 as usize) < height {
                win.buffer[(y0 as usize) * width + (x0 as usize)] = color;
            }
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
    Ok(Value::Null)
}

fn gui_text(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 5 {
        return Err(RuntimeError::new(
            "gui.text expects (window, x, y, text, color, [scale])".into(),
        )
        .at(span));
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let sx = match to_i64(&args[1]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let sy = match to_i64(&args[2]) {
        Some(v) => v,
        None => return Ok(Value::Null),
    };
    let text = match &args[3] {
        Value::String(s) => s.clone(),
        other => other.to_display(),
    };
    let color = parse_color(&args[4]);
    let scale = if args.len() > 5 {
        to_usize(&args[5]).unwrap_or(1).max(1)
    } else {
        1
    };

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        let width = win.width;
        let height = win.height;

        let mut cur_x = sx;
        let mut cur_y = sy;

        for ch in text.chars() {
            if ch == '\n' {
                cur_y += (8 * scale) as i64 + (2 * scale) as i64;
                cur_x = sx;
                continue;
            }
            let glyph = get_glyph_8x8(ch);
            for row in 0..8 {
                let bits = glyph[row];
                for col in 0..8 {
                    if (bits & (1 << (7 - col))) != 0 {
                        for sy_i in 0..scale {
                            for sx_i in 0..scale {
                                let px = cur_x + (col * scale + sx_i) as i64;
                                let py = cur_y + (row * scale + sy_i) as i64;
                                if px >= 0
                                    && (px as usize) < width
                                    && py >= 0
                                    && (py as usize) < height
                                {
                                    win.buffer[(py as usize) * width + (px as usize)] = color;
                                }
                            }
                        }
                    }
                }
            }
            cur_x += (8 * scale) as i64;
        }
    }
    Ok(Value::Null)
}

fn gui_render(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.render", 1, &args, span)?;
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        win.last_frame = Instant::now();
        match &mut win.backend {
            WindowBackend::Real(SendWindow(w)) => {
                let _ = w.update_with_buffer(&win.buffer, win.width, win.height);

                // Update input states
                let mut keys = Vec::new();
                for key in w.get_keys() {
                    let k_str = match key {
                        Key::Up => "Up",
                        Key::Down => "Down",
                        Key::Left => "Left",
                        Key::Right => "Right",
                        Key::Space => "Space",
                        Key::Enter => "Enter",
                        Key::Escape => "Escape",
                        Key::A => "A",
                        Key::B => "B",
                        Key::C => "C",
                        Key::D => "D",
                        Key::E => "E",
                        Key::F => "F",
                        Key::G => "G",
                        Key::H => "H",
                        Key::I => "I",
                        Key::J => "J",
                        Key::K => "K",
                        Key::L => "L",
                        Key::M => "M",
                        Key::N => "N",
                        Key::O => "O",
                        Key::P => "P",
                        Key::Q => "Q",
                        Key::R => "R",
                        Key::S => "S",
                        Key::T => "T",
                        Key::U => "U",
                        Key::V => "V",
                        Key::W => "W",
                        Key::X => "X",
                        Key::Y => "Y",
                        Key::Z => "Z",
                        Key::Key0 => "0",
                        Key::Key1 => "1",
                        Key::Key2 => "2",
                        Key::Key3 => "3",
                        Key::Key4 => "4",
                        Key::Key5 => "5",
                        Key::Key6 => "6",
                        Key::Key7 => "7",
                        Key::Key8 => "8",
                        Key::Key9 => "9",
                        _ => continue,
                    };
                    keys.push(k_str.to_string());
                }
                win.keys_down = keys;

                if let Some((mx, my)) = w.get_mouse_pos(MouseMode::Pass) {
                    win.mouse_pos = Some((mx as usize, my as usize));
                } else {
                    win.mouse_pos = None;
                }

                win.mouse_down = (
                    w.get_mouse_down(MouseButton::Left),
                    w.get_mouse_down(MouseButton::Middle),
                    w.get_mouse_down(MouseButton::Right),
                );
            }
            WindowBackend::Headless { .. } => {
                // Virtual render - no op
            }
        }
    }
    Ok(Value::Null)
}

fn gui_is_key_down(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() == 1 {
        return gui_is_key_down_retro(_ev, args, span);
    }
    if args.len() < 2 {
        return Err(
            RuntimeError::new("gui.is_key_down expects (window, key_name)".into()).at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Bool(false)),
    };
    let target_key = match &args[1] {
        Value::String(s) => s.to_lowercase(),
        _ => return Ok(Value::Bool(false)),
    };

    let reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get(&win_id) {
        let is_down = win.keys_down.iter().any(|k| k.to_lowercase() == target_key);
        Ok(Value::Bool(is_down))
    } else {
        Ok(Value::Bool(false))
    }
}

fn gui_get_keys(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.get_keys", 1, &args, span)?;
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Array(Arc::new(RwLock::new(Vec::new())))),
    };

    let reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get(&win_id) {
        let items: Vec<Value> = win
            .keys_down
            .iter()
            .map(|k| Value::String(k.clone()))
            .collect();
        Ok(Value::Array(Arc::new(RwLock::new(items))))
    } else {
        Ok(Value::Array(Arc::new(RwLock::new(Vec::new()))))
    }
}

fn gui_mouse_pos(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.mouse_pos", 1, &args, span)?;
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };

    let reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get(&win_id) {
        if let Some((x, y)) = win.mouse_pos {
            let coords = vec![Value::Int(x as i64), Value::Int(y as i64)];
            Ok(Value::Array(Arc::new(RwLock::new(coords))))
        } else {
            Ok(Value::Null)
        }
    } else {
        Ok(Value::Null)
    }
}

fn gui_is_mouse_down(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(
            RuntimeError::new("gui.is_mouse_down expects (window, button)".into()).at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Bool(false)),
    };
    let btn_str = match &args[1] {
        Value::String(s) => s.to_lowercase(),
        _ => return Ok(Value::Bool(false)),
    };

    let reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get(&win_id) {
        let pressed = match btn_str.as_str() {
            "left" | "1" => win.mouse_down.0,
            "middle" | "2" => win.mouse_down.1,
            "right" | "3" => win.mouse_down.2,
            _ => false,
        };
        Ok(Value::Bool(pressed))
    } else {
        Ok(Value::Bool(false))
    }
}

fn gui_limit_fps(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(
            RuntimeError::new("gui.limit_fps expects (window, [target_fps])".into()).at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let target_fps = if args.len() > 1 {
        to_usize(&args[1]).unwrap_or(60).max(1) as u64
    } else {
        60
    };

    let target_duration = Duration::from_micros(1_000_000 / target_fps);
    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        let elapsed = win.last_frame.elapsed();
        if elapsed < target_duration {
            std::thread::sleep(target_duration - elapsed);
        }
        win.last_frame = Instant::now();
    }
    Ok(Value::Null)
}

fn gui_sleep(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui.sleep", 1, &args, span)?;
    let ms = to_usize(&args[0]).unwrap_or(0) as u64;
    if ms > 0 {
        std::thread::sleep(Duration::from_millis(ms));
    }
    Ok(Value::Null)
}

// ---------------------------------------------------------------------------
// Image & Sprite Engine Functions
// ---------------------------------------------------------------------------

fn to_image_id(val: &Value) -> Option<usize> {
    match val {
        Value::Int(i) if *i >= 0 => Some(*i as usize),
        Value::Float(f) | Value::Number(f) if *f >= 0.0 => Some(*f as usize),
        Value::Map(m) => {
            let map = m.read().unwrap();
            map.get("id").and_then(|v| match v {
                Value::Int(i) if *i >= 0 => Some(*i as usize),
                Value::Float(f) | Value::Number(f) if *f >= 0.0 => Some(*f as usize),
                _ => None,
            })
        }
        _ => None,
    }
}

#[inline(always)]
fn blend_pixel(bg: u32, fg: u32) -> u32 {
    let alpha = (fg >> 24) & 0xFF;
    if alpha == 255 {
        fg
    } else if alpha == 0 {
        bg
    } else {
        let inv_alpha = 255 - alpha;
        let r = (((fg >> 16) & 0xFF) * alpha + ((bg >> 16) & 0xFF) * inv_alpha) / 255;
        let g = (((fg >> 8) & 0xFF) * alpha + ((bg >> 8) & 0xFF) * inv_alpha) / 255;
        let b = ((fg & 0xFF) * alpha + (bg & 0xFF) * inv_alpha) / 255;
        (255 << 24) | (r << 16) | (g << 8) | b
    }
}

fn gui_load_image(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("gui.load_image", 1, &args, span)?;
    let path = match &args[0] {
        Value::String(s) => s.clone(),
        _ => {
            return Err(
                RuntimeError::new("gui.load_image: expected file path string".into()).at(span),
            );
        }
    };

    let img = match image::open(&path) {
        Ok(img) => img,
        Err(e) => {
            return Err(RuntimeError::new(format!(
                "gui.load_image: failed to load image '{}': {}",
                path, e
            ))
            .at(span));
        }
    };

    let (w, h) = (img.width() as usize, img.height() as usize);
    let rgba = img.to_rgba8();
    let mut pixels = Vec::with_capacity(w * h);
    for pixel in rgba.pixels() {
        let [r, g, b, a] = pixel.0;
        let argb = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        pixels.push(argb);
    }

    let id = NEXT_IMAGE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let shae_img = Arc::new(ShaeImage {
        width: w,
        height: h,
        pixels,
    });
    get_image_registry().lock().unwrap().insert(id, shae_img);

    let mut map = IndexMap::new();
    map.insert("id".to_string(), Value::Int(id as i64));
    map.insert("width".to_string(), Value::Int(w as i64));
    map.insert("height".to_string(), Value::Int(h as i64));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn gui_create_image(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new(
            "gui.create_image expects (width, height, [fill_color])".into(),
        )
        .at(span));
    }
    let w = to_usize(&args[0]).unwrap_or(32).max(1);
    let h = to_usize(&args[1]).unwrap_or(32).max(1);
    let fill = if args.len() > 2 {
        to_u32_color(&args[2]).unwrap_or(0x00000000)
    } else {
        0x00000000
    };

    let pixels = vec![fill; w * h];
    let id = NEXT_IMAGE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let shae_img = Arc::new(ShaeImage {
        width: w,
        height: h,
        pixels,
    });
    get_image_registry().lock().unwrap().insert(id, shae_img);

    let mut map = IndexMap::new();
    map.insert("id".to_string(), Value::Int(id as i64));
    map.insert("width".to_string(), Value::Int(w as i64));
    map.insert("height".to_string(), Value::Int(h as i64));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn gui_draw_image(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 4 {
        return Err(
            RuntimeError::new("gui.draw_image expects (window, image, x, y)".into()).at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let img_id = match to_image_id(&args[1]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let x = to_i64(&args[2]).unwrap_or(0);
    let y = to_i64(&args[3]).unwrap_or(0);

    let img_arc = {
        let img_reg = get_image_registry().lock().unwrap();
        match img_reg.get(&img_id) {
            Some(arc) => Arc::clone(arc),
            None => return Ok(Value::Null),
        }
    };

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        let win_w = win.width as i64;
        let win_h = win.height as i64;
        let img_w = img_arc.width as i64;
        let img_h = img_arc.height as i64;

        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + img_w).min(win_w);
        let y1 = (y + img_h).min(win_h);

        for py in y0..y1 {
            let win_row = (py * win_w) as usize;
            let img_row = ((py - y) * img_w) as usize;
            for px in x0..x1 {
                let win_idx = win_row + px as usize;
                let img_idx = img_row + (px - x) as usize;
                if win_idx < win.buffer.len() && img_idx < img_arc.pixels.len() {
                    let fg = img_arc.pixels[img_idx];
                    let bg = win.buffer[win_idx];
                    win.buffer[win_idx] = blend_pixel(bg, fg);
                }
            }
        }
    }
    Ok(Value::Null)
}

fn gui_draw_image_scaled(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 6 {
        return Err(RuntimeError::new(
            "gui.draw_image_scaled expects (window, image, x, y, target_w, target_h)".into(),
        )
        .at(span));
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let img_id = match to_image_id(&args[1]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let x = to_i64(&args[2]).unwrap_or(0);
    let y = to_i64(&args[3]).unwrap_or(0);
    let tw = to_i64(&args[4]).unwrap_or(1).max(1);
    let th = to_i64(&args[5]).unwrap_or(1).max(1);

    let img_arc = {
        let img_reg = get_image_registry().lock().unwrap();
        match img_reg.get(&img_id) {
            Some(arc) => Arc::clone(arc),
            None => return Ok(Value::Null),
        }
    };

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        let win_w = win.width as i64;
        let win_h = win.height as i64;
        let img_w = img_arc.width as i64;
        let img_h = img_arc.height as i64;

        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + tw).min(win_w);
        let y1 = (y + th).min(win_h);

        for py in y0..y1 {
            let win_row = (py * win_w) as usize;
            let src_y = (((py - y) * img_h) / th).min(img_h - 1);
            let img_row = (src_y * img_w) as usize;

            for px in x0..x1 {
                let win_idx = win_row + px as usize;
                let src_x = (((px - x) * img_w) / tw).min(img_w - 1);
                let img_idx = img_row + src_x as usize;

                if win_idx < win.buffer.len() && img_idx < img_arc.pixels.len() {
                    let fg = img_arc.pixels[img_idx];
                    let bg = win.buffer[win_idx];
                    win.buffer[win_idx] = blend_pixel(bg, fg);
                }
            }
        }
    }
    Ok(Value::Null)
}

fn gui_draw_sprite(
    ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() == 4 {
        return gui_draw_sprite_retro(ev, args, span);
    }
    if args.len() < 10 {
        return Err(RuntimeError::new(
            "gui.draw_sprite expects (window, image, sx, sy, sw, sh, dx, dy, dw, dh)".into(),
        )
        .at(span));
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let img_id = match to_image_id(&args[1]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let sx = to_i64(&args[2]).unwrap_or(0).max(0);
    let sy = to_i64(&args[3]).unwrap_or(0).max(0);
    let sw = to_i64(&args[4]).unwrap_or(0).max(1);
    let sh = to_i64(&args[5]).unwrap_or(0).max(1);
    let dx = to_i64(&args[6]).unwrap_or(0);
    let dy = to_i64(&args[7]).unwrap_or(0);
    let dw = to_i64(&args[8]).unwrap_or(sw).max(1);
    let dh = to_i64(&args[9]).unwrap_or(sh).max(1);

    let img_arc = {
        let img_reg = get_image_registry().lock().unwrap();
        match img_reg.get(&img_id) {
            Some(arc) => Arc::clone(arc),
            None => return Ok(Value::Null),
        }
    };

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        let win_w = win.width as i64;
        let win_h = win.height as i64;
        let img_w = img_arc.width as i64;
        let img_h = img_arc.height as i64;

        let x0 = dx.max(0);
        let y0 = dy.max(0);
        let x1 = (dx + dw).min(win_w);
        let y1 = (dy + dh).min(win_h);

        for py in y0..y1 {
            let win_row = (py * win_w) as usize;
            let src_y = (sy + ((py - dy) * sh) / dh).min(img_h - 1);
            let img_row = (src_y * img_w) as usize;

            for px in x0..x1 {
                let win_idx = win_row + px as usize;
                let src_x = (sx + ((px - dx) * sw) / dw).min(img_w - 1);
                let img_idx = img_row + src_x as usize;

                if win_idx < win.buffer.len() && img_idx < img_arc.pixels.len() {
                    let fg = img_arc.pixels[img_idx];
                    let bg = win.buffer[win_idx];
                    win.buffer[win_idx] = blend_pixel(bg, fg);
                }
            }
        }
    }
    Ok(Value::Null)
}

fn gui_image_size(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("gui.image_size", 1, &args, span)?;
    let img_id = match to_image_id(&args[0]) {
        Some(id) => id,
        None => {
            return Ok(Value::Array(Arc::new(RwLock::new(vec![
                Value::Int(0),
                Value::Int(0),
            ]))));
        }
    };

    let img_reg = get_image_registry().lock().unwrap();
    if let Some(img) = img_reg.get(&img_id) {
        Ok(Value::Array(Arc::new(RwLock::new(vec![
            Value::Int(img.width as i64),
            Value::Int(img.height as i64),
        ]))))
    } else {
        Ok(Value::Array(Arc::new(RwLock::new(vec![
            Value::Int(0),
            Value::Int(0),
        ]))))
    }
}

fn gui_get_image_pixel(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 3 {
        return Err(RuntimeError::new("gui.get_image_pixel expects (image, x, y)".into()).at(span));
    }
    let img_id = match to_image_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Int(0)),
    };
    let x = to_usize(&args[1]).unwrap_or(0);
    let y = to_usize(&args[2]).unwrap_or(0);

    let img_reg = get_image_registry().lock().unwrap();
    if let Some(img) = img_reg.get(&img_id) {
        if x < img.width && y < img.height {
            let idx = y * img.width + x;
            return Ok(Value::Int(img.pixels[idx] as i64));
        }
    }
    Ok(Value::Int(0))
}

fn gui_set_image_pixel(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 4 {
        return Err(
            RuntimeError::new("gui.set_image_pixel expects (image, x, y, color)".into()).at(span),
        );
    }
    let img_id = match to_image_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let x = to_usize(&args[1]).unwrap_or(0);
    let y = to_usize(&args[2]).unwrap_or(0);
    let color = to_u32_color(&args[3]).unwrap_or(0);

    let mut img_reg = get_image_registry().lock().unwrap();
    if let Some(img_arc) = img_reg.get_mut(&img_id) {
        if let Some(img) = Arc::get_mut(img_arc) {
            if x < img.width && y < img.height {
                let idx = y * img.width + x;
                img.pixels[idx] = color;
            }
        } else {
            // Need to clone-on-write if shared
            let mut new_img = (**img_arc).clone();
            if x < new_img.width && y < new_img.height {
                let idx = y * new_img.width + x;
                new_img.pixels[idx] = color;
            }
            *img_arc = Arc::new(new_img);
        }
    }
    Ok(Value::Null)
}

// ============================================================================
// Retro Canvas & Graphics Engine (Milestone 3 Integration)
// ============================================================================
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
    } else if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
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

fn gui_clear_retro(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
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

fn gui_draw_sprite_retro(
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

fn gui_is_key_down_retro(
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

// -------------------------------------------------------------
// Scalable Vector Typography Functions (TrueType / OpenType)
// -------------------------------------------------------------

fn to_f64_opt(val: &Value) -> Option<f64> {
    match val {
        Value::Float(f) | Value::Number(f) => Some(*f),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn extract_font_handle(val: &Value) -> Option<(usize, f32)> {
    match val {
        Value::Int(i) if *i > 0 => Some((*i as usize, 16.0)),
        Value::Map(m) => {
            let map = m.read().unwrap();
            let id = match map.get("id") {
                Some(Value::Int(i)) => *i as usize,
                _ => return None,
            };
            let size = match map.get("size") {
                Some(Value::Float(f)) => *f as f32,
                Some(Value::Int(i)) => *i as f32,
                _ => 16.0,
            };
            Some((id, size))
        }
        _ => None,
    }
}

fn gui_load_font(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("gui.load_font expects (path, [size_px])".into()).at(span));
    }
    let path = match &args[0] {
        Value::String(s) => s.as_str(),
        other => &other.to_display(),
    };
    let size = if args.len() > 1 {
        to_f64_opt(&args[1]).unwrap_or(16.0) as f32
    } else {
        16.0
    };

    match crate::font::load_font_file(path, size) {
        Ok(id) => {
            let mut map = IndexMap::new();
            map.insert("id".to_string(), Value::Int(id as i64));
            map.insert("path".to_string(), Value::String(path.to_string()));
            map.insert("size".to_string(), Value::Float(size as f64));
            Ok(Value::Map(Arc::new(RwLock::new(map))))
        }
        Err(e) => Err(RuntimeError::new(format!("gui.load_font: {}", e)).at(span)),
    }
}

fn gui_measure_text_font(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new(
            "gui.measure_text_font expects (font, text, [size_px])".into(),
        )
        .at(span));
    }
    let (font_id, default_size) = extract_font_handle(&args[0]).ok_or_else(|| {
        RuntimeError::new("gui.measure_text_font: invalid font handle".into()).at(span)
    })?;

    let text = match &args[1] {
        Value::String(s) => s.clone(),
        other => other.to_display(),
    };
    let size = if args.len() > 2 {
        to_f64_opt(&args[2]).unwrap_or(default_size as f64) as f32
    } else {
        default_size
    };

    let font_arc = crate::font::get_font(font_id).ok_or_else(|| {
        RuntimeError::new(format!("gui.measure_text_font: font {} not found", font_id)).at(span)
    })?;

    let (w, h) = font_arc.measure_text(&text, size);
    let arr = vec![Value::Int(w.ceil() as i64), Value::Int(h.ceil() as i64)];
    Ok(Value::Array(Arc::new(RwLock::new(arr))))
}

fn gui_draw_text_font(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.len() < 5 {
        return Err(RuntimeError::new(
            "gui.draw_text_font expects (window, font, text, x, y, color, [size])".into(),
        )
        .at(span));
    }

    let (win_id, font_arg_idx) = if let Some(wid) = to_win_id(&args[0]) {
        (wid, 1)
    } else if extract_font_handle(&args[0]).is_some() {
        let first_win = get_registry()
            .lock()
            .unwrap()
            .keys()
            .next()
            .copied()
            .unwrap_or(1);
        (first_win, 0)
    } else {
        return Err(RuntimeError::new(
            "gui.draw_text_font: invalid window or font argument".into(),
        )
        .at(span));
    };

    let (font_id, default_size) = extract_font_handle(&args[font_arg_idx]).ok_or_else(|| {
        RuntimeError::new("gui.draw_text_font: invalid font handle".into()).at(span)
    })?;

    let text = match &args[font_arg_idx + 1] {
        Value::String(s) => s.clone(),
        other => other.to_display(),
    };
    let x = to_i64(&args[font_arg_idx + 2]).unwrap_or(0);
    let y = to_i64(&args[font_arg_idx + 3]).unwrap_or(0);
    let color = parse_color(&args[font_arg_idx + 4]);

    let size = if args.len() > font_arg_idx + 5 {
        to_f64_opt(&args[font_arg_idx + 5]).unwrap_or(default_size as f64) as f32
    } else {
        default_size
    };

    let font_arc = crate::font::get_font(font_id).ok_or_else(|| {
        RuntimeError::new(format!("gui.draw_text_font: font {} not found", font_id)).at(span)
    })?;

    let mut reg = get_registry().lock().unwrap();
    if let Some(win) = reg.get_mut(&win_id) {
        font_arc.draw_text_to_buffer(
            &mut win.buffer,
            win.width,
            win.height,
            x,
            y,
            &text,
            color,
            size,
        );
    }

    Ok(Value::Null)
}
