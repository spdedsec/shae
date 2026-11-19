use shae::run_with_engine;
use shae::value::Value;
use std::sync::Mutex;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn lock_test() -> std::sync::MutexGuard<'static, ()> {
    TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn test_gui_module_rust_api() {
    let _lock = lock_test();
    // Ensure headless for test environment
    shae::gui::init_window_internal(40, 30, "Test Window");
    shae::gui::set_headless_internal(true);
    assert!(shae::gui::is_headless_internal());

    let (w, h) = shae::gui::get_dimensions_internal();
    assert_eq!(w, 40);
    assert_eq!(h, 30);

    // Clear
    shae::gui::clear_internal(0x112233);
    assert_eq!(shae::gui::get_pixel_internal(0, 0), 0x112233);
    assert_eq!(shae::gui::get_pixel_internal(39, 29), 0x112233);

    // Set & Get Pixel
    shae::gui::set_pixel_internal(10, 10, 0xFF0000);
    assert_eq!(shae::gui::get_pixel_internal(10, 10), 0xFF0000);
    assert_eq!(shae::gui::get_pixel_internal(100, 100), 0); // Out of bounds

    // Fill Rect
    shae::gui::fill_rect_internal(5, 5, 10, 10, 0x00FF00);
    assert_eq!(shae::gui::get_pixel_internal(5, 5), 0x00FF00);
    assert_eq!(shae::gui::get_pixel_internal(14, 14), 0x00FF00);
    assert_eq!(shae::gui::get_pixel_internal(4, 5), 0x112233);

    // Draw Rect
    shae::gui::draw_rect_internal(20, 10, 8, 8, 0x0000FF);
    assert_eq!(shae::gui::get_pixel_internal(20, 10), 0x0000FF);
    assert_eq!(shae::gui::get_pixel_internal(27, 10), 0x0000FF);
    assert_eq!(shae::gui::get_pixel_internal(20, 17), 0x0000FF);
    assert_eq!(shae::gui::get_pixel_internal(27, 17), 0x0000FF);
    assert_eq!(shae::gui::get_pixel_internal(23, 13), 0x112233); // Interior not filled

    // Draw Line (Bresenham)
    shae::gui::draw_line_internal(0, 0, 5, 5, 0xFFFF00);
    assert_eq!(shae::gui::get_pixel_internal(0, 0), 0xFFFF00);
    assert_eq!(shae::gui::get_pixel_internal(2, 2), 0xFFFF00);
    assert_eq!(shae::gui::get_pixel_internal(5, 5), 0xFFFF00);

    // Draw Circle & Fill Circle
    shae::gui::draw_circle_internal(30, 20, 4, 0xFF00FF);
    assert_eq!(shae::gui::get_pixel_internal(34, 20), 0xFF00FF);
    assert_eq!(shae::gui::get_pixel_internal(26, 20), 0xFF00FF);

    shae::gui::fill_circle_internal(15, 25, 3, 0x00FFFF);
    assert_eq!(shae::gui::get_pixel_internal(15, 25), 0x00FFFF);
    assert_eq!(shae::gui::get_pixel_internal(14, 25), 0x00FFFF);
    assert_eq!(shae::gui::get_pixel_internal(16, 25), 0x00FFFF);

    // Draw Text
    shae::gui::draw_text_internal(0, 0, "HI", 0xFFFFFF);
    // 'H' at (0, 0), 'I' at (6, 0)
    assert_eq!(shae::gui::get_pixel_internal(0, 0), 0xFFFFFF); // H top-left
    assert_eq!(shae::gui::get_pixel_internal(4, 0), 0xFFFFFF); // H top-right
    assert_eq!(shae::gui::get_pixel_internal(8, 0), 0xFFFFFF); // I top-center

    // Draw Sprite
    let sprite = vec![" █ ".to_string(), "███".to_string()];
    shae::gui::draw_sprite_internal(2, 2, &sprite, 0xFFA500);
    assert_eq!(shae::gui::get_pixel_internal(3, 2), 0xFFA500);
    assert_eq!(shae::gui::get_pixel_internal(2, 3), 0xFFA500);
    assert_eq!(shae::gui::get_pixel_internal(3, 3), 0xFFA500);
    assert_eq!(shae::gui::get_pixel_internal(4, 3), 0xFFA500);

    // Input simulation
    assert!(!shae::gui::is_key_down_internal("space"));
    shae::gui::simulate_key_internal("space", true);
    assert!(shae::gui::is_key_down_internal("space"));
    assert!(shae::gui::is_key_down_internal("Space"));
    shae::gui::simulate_key_internal("SPACE", false);
    assert!(!shae::gui::is_key_down_internal("space"));

    // Render & Delay in Headless mode
    shae::gui::render_frame_internal();
    unsafe {
        std::env::set_var("SHAE_FAST_TEST", "1");
    }
    shae::gui::frame_delay_internal(60);

    // Window lifecycle
    assert!(!shae::gui::should_close_internal());
    shae::gui::close_window_internal();
    assert!(shae::gui::should_close_internal());
}

#[test]
fn test_audio_module_rust_api() {
    let _lock = lock_test();
    shae::audio::clear_log_internal();

    // Volume & Mute
    let mut engine = shae::audio::get_audio_engine().write().unwrap();
    engine.volume = 0.5;
    engine.muted = false;
    drop(engine);

    // Play sounds
    shae::audio::play_laser_internal();
    shae::audio::play_explosion_internal();
    shae::audio::play_hit_internal();
    shae::audio::play_jingle_internal("victory");
    shae::audio::play_jingle_internal("game_over");
    shae::audio::play_tone_internal(440.0, 50, "sine");

    let engine = shae::audio::get_audio_engine().read().unwrap();
    assert_eq!(engine.log.len(), 6);
    assert_eq!(engine.log[0].sound, "laser");
    assert_eq!(engine.log[1].sound, "explosion");
    assert_eq!(engine.log[2].sound, "hit");
    assert_eq!(engine.log[3].sound, "victory");
    assert_eq!(engine.log[4].sound, "game_over");
    assert_eq!(engine.log[5].sound, "tone");
    drop(engine);

    // Muted behavior
    let mut engine = shae::audio::get_audio_engine().write().unwrap();
    engine.muted = true;
    drop(engine);

    shae::audio::play_laser_internal();
    let mut engine = shae::audio::get_audio_engine().write().unwrap();
    assert_eq!(engine.log.len(), 6); // Not added when muted
    engine.muted = false;
    drop(engine);

    // Synthesizer waveform tests
    let sq = shae::audio::synthesize_waveform(440.0, 10, "square", 1.0);
    assert!(!sq.is_empty());
    let tri = shae::audio::synthesize_waveform(440.0, 10, "triangle", 1.0);
    assert!(!tri.is_empty());
    let saw = shae::audio::synthesize_waveform(440.0, 10, "saw", 1.0);
    assert!(!saw.is_empty());
    let noise = shae::audio::synthesize_waveform(440.0, 10, "noise", 1.0);
    assert!(!noise.is_empty());
}

#[test]
fn test_ui_module_rust_api() {
    let _lock = lock_test();
    shae::gui::init_window_internal(50, 40, "UI Test");
    shae::gui::set_headless_internal(true);

    // Format score
    assert_eq!(shae::ui::format_score_internal(0, 6), "000000");
    assert_eq!(shae::ui::format_score_internal(1420, 6), "001420");
    assert_eq!(shae::ui::format_score_internal(999999, 6), "999999");
    assert_eq!(shae::ui::format_score_internal(-50, 4), "-0050");

    // Menu select wrap-around
    assert_eq!(shae::ui::menu_select_internal(0, 1, 3), 1);
    assert_eq!(shae::ui::menu_select_internal(1, 1, 3), 2);
    assert_eq!(shae::ui::menu_select_internal(2, 1, 3), 0);
    assert_eq!(shae::ui::menu_select_internal(0, -1, 3), 2);
    assert_eq!(shae::ui::menu_select_internal(2, -1, 3), 1);

    // Draw components without panic
    shae::ui::draw_panel_internal(5, 5, 40, 30, 0x00FF00, 0x000000);
    let options = vec![
        "PLAY".to_string(),
        "OPTIONS".to_string(),
        "EXIT".to_string(),
    ];
    shae::ui::draw_menu_internal("SPACE INVADERS", &options, 0, 5, 5, 40, 30);
    shae::ui::draw_score_board_internal(1200, 5000, 2, 2, 45);
    shae::ui::draw_lives_internal(3, 3, 2, 35);
    shae::ui::draw_game_over_internal(10, 10, 1200, true);
}

#[test]
fn test_gui_audio_ui_pure_shae_evaluator_and_vm() {
    let _lock = lock_test();
    shae::audio::get_audio_engine().write().unwrap().muted = false;
    unsafe {
        std::env::set_var("SHAE_HEADLESS", "1");
        std::env::set_var("SHAE_FAST_TEST", "1");
    }

    let code = r#"
        let gui = use "std:gui"
        let audio = use "std:audio"
        let ui = use "std:ui"

        // Initialize GUI
        let win = gui.init_window(60, 40, "Shae Test")
        assert(win.open == true)
        assert(gui.is_headless() == true)

        let dims = gui.get_dimensions()
        assert(dims[0] == 60)
        assert(dims[1] == 40)

        // Clear and primitive drawing
        gui.clear(0x000000)
        assert(gui.get_pixel(10, 10) == 0)

        gui.fill_rect(10, 10, 5, 5, 0x00FF00)
        assert(gui.get_pixel(12, 12) == 0x00FF00)

        gui.draw_line(0, 0, 10, 0, 0xFF0000)
        assert(gui.get_pixel(5, 0) == 0xFF0000)

        gui.draw_rect(20, 20, 6, 6, 0x0000FF)
        assert(gui.get_pixel(20, 20) == 0x0000FF)
        assert(gui.get_pixel(22, 22) == 0x000000)

        gui.draw_text(0, 15, "TEST", 0xFFFFFF)
        assert(gui.get_pixel(0, 15) != 0)

        gui.draw_sprite(30, 10, [" █ ", "███"], 0xFFFF00)
        assert(gui.get_pixel(31, 10) == 0xFFFF00)
        assert(gui.get_pixel(30, 11) == 0xFFFF00)

        // Simulated Input
        assert(gui.is_key_down("space") == false)
        gui.simulate_key("space", true)
        assert(gui.is_key_down("space") == true)
        gui.simulate_key("space", false)
        assert(gui.is_key_down("space") == false)

        // Audio System
        audio.clear_log()
        audio.set_volume(0.9)
        assert(audio.get_volume() == 0.9)

        audio.play_laser()
        audio.play_explosion()
        audio.play_hit()
        audio.play_jingle("game_over")

        let log = audio.get_audio_log()
        assert(len(log) == 4)
        assert(log[0].sound == "laser")
        assert(log[1].sound == "explosion")
        assert(log[2].sound == "hit")
        assert(log[3].sound == "game_over")
        assert(audio.last_sound() == "game_over")

        // UI System
        let s = ui.format_score(1420, 6)
        assert(s == "001420")

        let next_idx = ui.menu_select(0, 1, 3)
        assert(next_idx == 1)
        let prev_idx = ui.menu_select(0, -1, 3)
        assert(prev_idx == 2)

        ui.draw_panel(2, 2, 20, 10, 0x00FFFF, 0x001122)
        ui.draw_score_board(1420, 9990, 2, 2, 50)
        ui.draw_lives(3, 3, 2, 35)
        ui.draw_menu("MENU", ["START", "QUIT"], 0, 10, 10, 30, 20)

        gui.render_frame()
        gui.frame_delay(60)

        "success"
    "#;

    // Run on AST interpreter
    let res_ast = run_with_engine(code, false).expect("AST evaluation failed");
    if let Value::String(s) = res_ast {
        assert_eq!(s, "success");
    } else {
        panic!("Expected string result on AST");
    }

    // Run on Bytecode VM
    let res_vm = run_with_engine(code, true).expect("VM evaluation failed");
    if let Value::String(s) = res_vm {
        assert_eq!(s, "success");
    } else {
        panic!("Expected string result on VM");
    }
}
