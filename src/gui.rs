use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use minifb::{Key, MouseButton, MouseMode, Scale, Window, WindowOptions};
use std::collections::HashMap;
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

fn get_registry() -> &'static Mutex<HashMap<usize, ShaeWindow>> {
    WINDOW_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
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

fn gui_clear(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
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
        return Err(RuntimeError::new("gui.circle expects (window, cx, cy, r, color)".into()).at(span));
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
        return Err(
            RuntimeError::new("gui.text expects (window, x, y, text, color, [scale])".into())
                .at(span),
        );
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
                                if px >= 0 && (px as usize) < width && py >= 0 && (py as usize) < height {
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
        let is_down = win
            .keys_down
            .iter()
            .any(|k| k.to_lowercase() == target_key);
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
