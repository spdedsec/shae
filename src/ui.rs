use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::gui::{
    draw_line_internal, draw_rect_internal, draw_sprite_internal, draw_text_internal,
    fill_rect_internal, parse_color_value,
};
use crate::value::Value;

// -------------------------------------------------------------
// Formatters & UI Math
// -------------------------------------------------------------

pub fn format_score_internal(score: i64, digits: usize) -> String {
    let d = digits.max(1);
    if score < 0 {
        format!("-{:0>width$}", score.unsigned_abs(), width = d)
    } else {
        format!("{:0>width$}", score, width = d)
    }
}

pub fn menu_select_internal(current: i64, dir: i64, count: i64) -> i64 {
    if count <= 0 {
        return 0;
    }
    let res = (current + dir) % count;
    if res < 0 { res + count } else { res }
}

// -------------------------------------------------------------
// UI Drawing Primitives
// -------------------------------------------------------------

pub fn draw_panel_internal(x: i64, y: i64, w: i64, h: i64, border_color: u32, bg_color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }
    fill_rect_internal(x, y, w, h, bg_color);
    draw_rect_internal(x, y, w, h, border_color);
}

pub fn draw_menu_internal(
    title: &str,
    options: &[String],
    selected: i64,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
) {
    // Background panel with neon border
    draw_panel_internal(x, y, w, h, 0x00FFFF, 0x0A0A14);

    // Title centered
    let title_len = title.len() as i64;
    let title_x = (x + (w - title_len * 6) / 2).max(x + 4);
    draw_text_internal(title_x, y + 4, title, 0xFFFF00);

    // Divider line
    draw_line_internal(x + 4, y + 14, x + w - 5, y + 14, 0x008888);

    // Menu options
    let opt_y_start = y + 18;
    for (idx, opt) in options.iter().enumerate() {
        let opt_y = opt_y_start + (idx as i64) * 9;
        if opt_y + 8 > y + h {
            break;
        }
        if idx as i64 == selected {
            // Highlighted active option
            let text = format!("> {}", opt);
            fill_rect_internal(x + 4, opt_y - 1, w - 8, 9, 0x003344);
            draw_text_internal(x + 6, opt_y, &text, 0x00FF00);
        } else {
            let text = format!("  {}", opt);
            draw_text_internal(x + 6, opt_y, &text, 0xAAAAAA);
        }
    }
}

pub fn draw_score_board_internal(score: i64, high_score: i64, x: i64, y: i64, w: i64) {
    let score_str = format!("SCORE: {}", format_score_internal(score, 6));
    let hi_str = format!("HI-SCORE: {}", format_score_internal(high_score, 6));

    draw_text_internal(x, y, &score_str, 0xFFFFFF);

    let hi_len = hi_str.len() as i64 * 6;
    let hi_x = if w > hi_len { x + w - hi_len } else { x + 100 };
    draw_text_internal(hi_x, y, &hi_str, 0xFFFF00);
}

pub fn draw_lives_internal(lives: i64, _max_lives: i64, x: i64, y: i64) {
    draw_text_internal(x, y, "LIVES:", 0xFFFFFF);

    let mini_ship = [" █ ".to_string(), "███".to_string()];

    let ship_x_start = x + 42;
    let count = lives.clamp(0, 10);
    for i in 0..count {
        draw_sprite_internal(ship_x_start + i * 8, y + 1, &mini_ship, 0x00FF00);
    }
}

pub fn draw_game_over_internal(x: i64, y: i64, score: i64, is_high_score: bool) {
    let w = 48;
    let h = 26;
    draw_panel_internal(x, y, w, h, 0xFF0000, 0x1A0000);

    draw_text_internal(x + 6, y + 3, "GAME OVER", 0xFF0000);

    let score_text = format!("SCORE: {}", format_score_internal(score, 6));
    draw_text_internal(x + 4, y + 11, &score_text, 0xFFFFFF);

    if is_high_score {
        draw_text_internal(x + 4, y + 17, "HI-SCORE!", 0xFFFF00);
    } else {
        draw_text_internal(x + 4, y + 17, "PLAY AGAIN", 0x00FFFF);
    }
}

// -------------------------------------------------------------
// Shae Standard Library std:ui Bindings
// -------------------------------------------------------------

fn val_to_i64(val: &Value) -> i64 {
    match val {
        Value::Int(n) => *n,
        Value::Float(f) | Value::Number(f) => *f as i64,
        _ => 0,
    }
}

pub fn build_ui_module() -> Value {
    crate::stdlib::make_map(vec![
        ("draw_menu", ui_draw_menu),
        ("menu_select", ui_menu_select),
        ("draw_score_board", ui_draw_score_board),
        ("draw_lives", ui_draw_lives),
        ("format_score", ui_format_score),
        ("draw_panel", ui_draw_panel),
        ("draw_game_over", ui_draw_game_over),
    ])
}

fn ui_draw_menu(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("ui.draw_menu", 7, &args, span)?;
    let title = args[0].to_display();
    let mut options = Vec::new();
    if let Value::Array(arr) = &args[1] {
        let guard = arr.read().unwrap();
        for item in guard.iter() {
            options.push(item.to_display());
        }
    } else {
        return Err(RuntimeError::new("ui.draw_menu expects array for options".into()).at(span));
    }
    let selected = val_to_i64(&args[2]);
    let x = val_to_i64(&args[3]);
    let y = val_to_i64(&args[4]);
    let w = val_to_i64(&args[5]);
    let h = val_to_i64(&args[6]);

    draw_menu_internal(&title, &options, selected, x, y, w, h);
    Ok(Value::Null)
}

fn ui_menu_select(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("ui.menu_select", 3, &args, span)?;
    let current = val_to_i64(&args[0]);
    let dir = val_to_i64(&args[1]);
    let count = val_to_i64(&args[2]);
    let next = menu_select_internal(current, dir, count);
    Ok(Value::Int(next))
}

fn ui_draw_score_board(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("ui.draw_score_board", 5, &args, span)?;
    let score = val_to_i64(&args[0]);
    let high_score = val_to_i64(&args[1]);
    let x = val_to_i64(&args[2]);
    let y = val_to_i64(&args[3]);
    let w = val_to_i64(&args[4]);

    draw_score_board_internal(score, high_score, x, y, w);
    Ok(Value::Null)
}

fn ui_draw_lives(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("ui.draw_lives", 4, &args, span)?;
    let lives = val_to_i64(&args[0]);
    let max_lives = val_to_i64(&args[1]);
    let x = val_to_i64(&args[2]);
    let y = val_to_i64(&args[3]);

    draw_lives_internal(lives, max_lives, x, y);
    Ok(Value::Null)
}

fn ui_format_score(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("ui.format_score", 2, &args, span)?;
    let score = val_to_i64(&args[0]);
    let digits = val_to_i64(&args[1]).max(1) as usize;
    let s = format_score_internal(score, digits);
    Ok(Value::String(s))
}

fn ui_draw_panel(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("ui.draw_panel", 6, &args, span)?;
    let x = val_to_i64(&args[0]);
    let y = val_to_i64(&args[1]);
    let w = val_to_i64(&args[2]);
    let h = val_to_i64(&args[3]);
    let border_color = parse_color_value(&args[4]);
    let bg_color = parse_color_value(&args[5]);

    draw_panel_internal(x, y, w, h, border_color, bg_color);
    Ok(Value::Null)
}

fn ui_draw_game_over(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("ui.draw_game_over", 4, &args, span)?;
    let x = val_to_i64(&args[0]);
    let y = val_to_i64(&args[1]);
    let score = val_to_i64(&args[2]);
    let is_high_score = args[3].is_truthy();

    draw_game_over_internal(x, y, score, is_high_score);
    Ok(Value::Null)
}
