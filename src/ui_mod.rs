use crate::ast::Span;
use crate::eval::{Evaluator, RuntimeError};
use crate::font_8x8::get_glyph_8x8;
use crate::gui::ShaeWindow;
use crate::value::Value;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

#[allow(dead_code)]
pub struct UiState {
    pub active_id: Option<String>,
    pub hot_id: Option<String>,
    pub last_mouse_down: bool,
}

#[allow(dead_code)]
static UI_STATE: OnceLock<Mutex<UiState>> = OnceLock::new();

#[allow(dead_code)]
fn get_ui_state() -> &'static Mutex<UiState> {
    UI_STATE.get_or_init(|| {
        Mutex::new(UiState {
            active_id: None,
            hot_id: None,
            last_mouse_down: false,
        })
    })
}

pub fn build_ui_module() -> Value {
    let mut map = IndexMap::new();
    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("button", ui_button),
        ("slider", ui_slider),
        ("checkbox", ui_checkbox),
        ("panel", ui_panel),
        ("label", ui_label),
        ("progress_bar", ui_progress_bar),
        ("progressBar", ui_progress_bar),
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

fn to_f64(val: &Value) -> Option<f64> {
    match val {
        Value::Float(f) | Value::Number(f) => Some(*f),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn to_color(val: &Value) -> Option<u32> {
    match val {
        Value::Int(i) => Some(*i as u32),
        Value::Float(f) | Value::Number(f) => Some(*f as u32),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Drawing Primitives Helpers on ShaeWindow Buffer
// ---------------------------------------------------------------------------

fn raw_rect(win: &mut ShaeWindow, x: i64, y: i64, w: i64, h: i64, color: u32) {
    let width = win.width as i64;
    let height = win.height as i64;

    let x0 = x.max(0).min(width);
    let x1 = (x + w).max(0).min(width);
    let y0 = y.max(0).min(height);
    let y1 = (y + h).max(0).min(height);

    for py in y0..y1 {
        let row_start = (py * width) as usize;
        for px in x0..x1 {
            let idx = row_start + px as usize;
            if idx < win.buffer.len() {
                win.buffer[idx] = color;
            }
        }
    }
}

fn raw_rect_outline(win: &mut ShaeWindow, x: i64, y: i64, w: i64, h: i64, color: u32, border: i64) {
    raw_rect(win, x, y, w, border, color); // Top
    raw_rect(win, x, y + h - border, w, border, color); // Bottom
    raw_rect(win, x, y, border, h, color); // Left
    raw_rect(win, x + w - border, y, border, h, color); // Right
}

fn raw_text(win: &mut ShaeWindow, x: i64, y: i64, text: &str, color: u32, scale: usize) {
    let scale = scale.max(1);
    let mut cur_x = x;

    for ch in text.chars() {
        if ch == '\n' {
            cur_x = x;
            continue;
        }
        let glyph = get_glyph_8x8(ch);
        for row in 0..8 {
            for col in 0..8 {
                if (glyph[row] & (1 << col)) != 0 {
                    let px = cur_x + (col * scale) as i64;
                    let py = y + (row * scale) as i64;
                    raw_rect(win, px, py, scale as i64, scale as i64, color);
                }
            }
        }
        cur_x += (8 * scale) as i64;
    }
}

// ---------------------------------------------------------------------------
// UI Widget Implementations
// ---------------------------------------------------------------------------

fn ui_button(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(
            RuntimeError::new("ui.button expects (window, text, [x, y, w, h, bg_color, text_color])".into())
                .at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Bool(false)),
    };
    let text = match &args[1] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("ui.button text must be string".into()).at(span)),
    };

    let x = if args.len() > 2 { to_i64(&args[2]).unwrap_or(0) } else { 10 };
    let y = if args.len() > 3 { to_i64(&args[3]).unwrap_or(0) } else { 10 };
    let text_len = text.len() as i64;
    let default_w = (text_len * 8 + 20).max(60);
    let w = if args.len() > 4 { to_i64(&args[4]).unwrap_or(default_w) } else { default_w };
    let h = if args.len() > 5 { to_i64(&args[5]).unwrap_or(28) } else { 28 };
    let base_bg = if args.len() > 6 { to_color(&args[6]).unwrap_or(0x313244) } else { 0x313244 };
    let text_color = if args.len() > 7 { to_color(&args[7]).unwrap_or(0xCDD6F4) } else { 0xCDD6F4 };

    let mut reg = crate::gui::get_window_registry().lock().unwrap();
    let win = match reg.get_mut(&win_id) {
        Some(w) => w,
        None => return Ok(Value::Bool(false)),
    };

    let (mouse_x, mouse_y) = win.mouse_pos.map(|(mx, my)| (mx as i64, my as i64)).unwrap_or((-1, -1));
    let mouse_left_down = win.mouse_down.0;

    let is_hover = mouse_x >= x && mouse_x < x + w && mouse_y >= y && mouse_y < y + h;
    let is_clicked = is_hover && mouse_left_down;

    // Visual styles: hover (lightened), active/clicked (accent), normal
    let bg = if is_clicked {
        0x89B4FA // Blue accent when pressed
    } else if is_hover {
        0x45475A // Lighter gray on hover
    } else {
        base_bg
    };

    let border_color = if is_hover { 0x89B4FA } else { 0x585B70 };

    raw_rect(win, x, y, w, h, bg);
    raw_rect_outline(win, x, y, w, h, border_color, 1);

    // Center text vertically and horizontally
    let text_w = text_len * 8;
    let text_x = x + ((w - text_w) / 2).max(4);
    let text_y = y + ((h - 8) / 2).max(2);
    let final_text_color = if is_clicked { 0x11111B } else { text_color };
    raw_text(win, text_x, text_y, &text, final_text_color, 1);

    Ok(Value::Bool(is_clicked))
}

fn ui_slider(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 7 {
        return Err(
            RuntimeError::new("ui.slider expects (window, label, x, y, w, min, max, current_value)".into())
                .at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Float(0.0)),
    };
    let label = match &args[1] {
        Value::String(s) => s.clone(),
        _ => "".to_string(),
    };
    let x = to_i64(&args[2]).unwrap_or(10);
    let y = to_i64(&args[3]).unwrap_or(10);
    let w = to_i64(&args[4]).unwrap_or(150).max(40);
    let min_val = to_f64(&args[5]).unwrap_or(0.0);
    let max_val = to_f64(&args[6]).unwrap_or(100.0);
    let mut current = to_f64(&args[7]).unwrap_or(min_val).max(min_val).min(max_val);

    let mut reg = crate::gui::get_window_registry().lock().unwrap();
    let win = match reg.get_mut(&win_id) {
        Some(w) => w,
        None => return Ok(Value::Float(current)),
    };

    let (mouse_x, mouse_y) = win.mouse_pos.map(|(mx, my)| (mx as i64, my as i64)).unwrap_or((-1, -1));
    let mouse_left_down = win.mouse_down.0;

    let track_y = y + 14;
    let track_h = 6;
    let knob_w = 12;
    let knob_h = 16;

    // Check interaction
    let is_interacting = mouse_left_down && mouse_x >= x - 5 && mouse_x <= x + w + 5 && mouse_y >= y && mouse_y <= y + 24;
    if is_interacting {
        let t = ((mouse_x - x) as f64 / w as f64).max(0.0).min(1.0);
        current = min_val + t * (max_val - min_val);
    }

    // Draw label and value
    let val_str = format!("{}: {:.1}", label, current);
    raw_text(win, x, y, &val_str, 0xCDD6F4, 1);

    // Draw track
    raw_rect(win, x, track_y, w, track_h, 0x313244);
    raw_rect_outline(win, x, track_y, w, track_h, 0x585B70, 1);

    // Draw filled portion
    let progress = if max_val > min_val { ((current - min_val) / (max_val - min_val)).max(0.0).min(1.0) } else { 0.0 };
    let fill_w = (progress * w as f64) as i64;
    if fill_w > 0 {
        raw_rect(win, x, track_y, fill_w, track_h, 0x89B4FA);
    }

    // Draw knob
    let knob_x = x + fill_w - (knob_w / 2);
    let knob_y = track_y + (track_h / 2) - (knob_h / 2);
    raw_rect(win, knob_x, knob_y, knob_w, knob_h, 0xF5E0DC);
    raw_rect_outline(win, knob_x, knob_y, knob_w, knob_h, 0x11111B, 1);

    Ok(Value::Float(current))
}

fn ui_checkbox(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 4 {
        return Err(
            RuntimeError::new("ui.checkbox expects (window, label, x, y, [checked])".into())
                .at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Bool(false)),
    };
    let label = match &args[1] {
        Value::String(s) => s.clone(),
        _ => "".to_string(),
    };
    let x = to_i64(&args[2]).unwrap_or(10);
    let y = to_i64(&args[3]).unwrap_or(10);
    let mut checked = if args.len() > 4 {
        match &args[4] {
            Value::Bool(b) => *b,
            _ => false,
        }
    } else {
        false
    };

    let box_size = 14;
    let mut reg = crate::gui::get_window_registry().lock().unwrap();
    let win = match reg.get_mut(&win_id) {
        Some(w) => w,
        None => return Ok(Value::Bool(checked)),
    };

    let (mouse_x, mouse_y) = win.mouse_pos.map(|(mx, my)| (mx as i64, my as i64)).unwrap_or((-1, -1));
    let mouse_left_down = win.mouse_down.0;

    let is_hover = mouse_x >= x && mouse_x <= x + box_size + 8 * label.len() as i64 + 10 && mouse_y >= y && mouse_y <= y + box_size;
    if is_hover && mouse_left_down {
        checked = !checked;
    }

    // Draw box
    raw_rect(win, x, y, box_size, box_size, 0x313244);
    raw_rect_outline(win, x, y, box_size, box_size, if is_hover { 0x89B4FA } else { 0x585B70 }, 1);

    // Draw check mark
    if checked {
        raw_rect(win, x + 3, y + 3, box_size - 6, box_size - 6, 0xA6E3A1);
    }

    // Draw label
    raw_text(win, x + box_size + 8, y + 3, &label, 0xCDD6F4, 1);

    Ok(Value::Bool(checked))
}

fn ui_panel(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 5 {
        return Err(
            RuntimeError::new("ui.panel expects (window, title, x, y, w, h, [bg_color], [border_color])".into())
                .at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let title = match &args[1] {
        Value::String(s) => s.clone(),
        _ => "".to_string(),
    };
    let x = to_i64(&args[2]).unwrap_or(0);
    let y = to_i64(&args[3]).unwrap_or(0);
    let w = to_i64(&args[4]).unwrap_or(100);
    let h = if args.len() > 5 { to_i64(&args[5]).unwrap_or(100) } else { 100 };
    let bg_color = if args.len() > 6 { to_color(&args[6]).unwrap_or(0x181825) } else { 0x181825 };
    let border_color = if args.len() > 7 { to_color(&args[7]).unwrap_or(0x45475A) } else { 0x45475A };

    let mut reg = crate::gui::get_window_registry().lock().unwrap();
    let win = match reg.get_mut(&win_id) {
        Some(w) => w,
        None => return Ok(Value::Null),
    };

    // Body
    raw_rect(win, x, y, w, h, bg_color);
    raw_rect_outline(win, x, y, w, h, border_color, 1);

    // Title bar
    if !title.is_empty() {
        raw_rect(win, x, y, w, 20, border_color);
        raw_text(win, x + 6, y + 6, &title, 0xCDD6F4, 1);
    }

    Ok(Value::Null)
}

fn ui_label(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 4 {
        return Err(
            RuntimeError::new("ui.label expects (window, text, x, y, [color], [scale])".into())
                .at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let text = match &args[1] {
        Value::String(s) => s.clone(),
        _ => return Ok(Value::Null),
    };
    let x = to_i64(&args[2]).unwrap_or(0);
    let y = to_i64(&args[3]).unwrap_or(0);
    let color = if args.len() > 4 { to_color(&args[4]).unwrap_or(0xCDD6F4) } else { 0xCDD6F4 };
    let scale = if args.len() > 5 { to_i64(&args[5]).unwrap_or(1).max(1) as usize } else { 1 };

    let mut reg = crate::gui::get_window_registry().lock().unwrap();
    let win = match reg.get_mut(&win_id) {
        Some(w) => w,
        None => return Ok(Value::Null),
    };

    raw_text(win, x, y, &text, color, scale);
    Ok(Value::Null)
}

fn ui_progress_bar(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 6 {
        return Err(
            RuntimeError::new("ui.progress_bar expects (window, x, y, w, h, progress_0_to_1, [fill_color], [bg_color])".into())
                .at(span),
        );
    }
    let win_id = match to_win_id(&args[0]) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let x = to_i64(&args[1]).unwrap_or(0);
    let y = to_i64(&args[2]).unwrap_or(0);
    let w = to_i64(&args[3]).unwrap_or(100);
    let h = to_i64(&args[4]).unwrap_or(16);
    let progress = to_f64(&args[5]).unwrap_or(0.0).max(0.0).min(1.0);
    let fill_color = if args.len() > 6 { to_color(&args[6]).unwrap_or(0xA6E3A1) } else { 0xA6E3A1 };
    let bg_color = if args.len() > 7 { to_color(&args[7]).unwrap_or(0x313244) } else { 0x313244 };

    let mut reg = crate::gui::get_window_registry().lock().unwrap();
    let win = match reg.get_mut(&win_id) {
        Some(w) => w,
        None => return Ok(Value::Null),
    };

    raw_rect(win, x, y, w, h, bg_color);
    raw_rect_outline(win, x, y, w, h, 0x585B70, 1);

    let fill_w = (progress * w as f64) as i64;
    if fill_w > 0 {
        raw_rect(win, x, y, fill_w, h, fill_color);
    }

    Ok(Value::Null)
}
