use shae::font::{load_font_file, get_font};
use shae::run;
use shae::run_vm;
use shae::value::Value;

const TEST_FONT_PATH: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";

#[test]
fn test_font_loading_and_rasterization() {
    let font_id = load_font_file(TEST_FONT_PATH, 24.0)
        .expect("Loading DejaVuSans.ttf should succeed");

    let shae_font = get_font(font_id).expect("Font must exist in registry");
    assert_eq!(shae_font.default_size, 24.0);

    let glyph_a = shae_font.get_glyph('A', 24.0);
    assert!(glyph_a.metrics.width > 0);
    assert!(glyph_a.metrics.height > 0);
    assert_eq!(glyph_a.bitmap.len(), glyph_a.metrics.width * glyph_a.metrics.height);
    assert!(glyph_a.bitmap.iter().any(|&cov| cov > 0));

    // Glyph caching test
    let cached_glyph = shae_font.get_glyph('A', 24.0);
    assert!(std::sync::Arc::ptr_eq(&glyph_a, &cached_glyph));
}

#[test]
fn test_font_measurement() {
    let font_id = load_font_file(TEST_FONT_PATH, 16.0).unwrap();
    let shae_font = get_font(font_id).unwrap();

    let (w1, h1) = shae_font.measure_text("Hi", 16.0);
    let (w2, h2) = shae_font.measure_text("Hello World!", 16.0);
    assert!(w2 > w1);
    assert!(h1 > 0.0);
    assert!(h2 > 0.0);

    // Multiline test
    let (_w_multi, h_multi) = shae_font.measure_text("Line 1\nLine 2\nLine 3", 16.0);
    assert!(h_multi > h1 * 2.0);
}

#[test]
fn test_font_buffer_rendering_and_alpha_blend() {
    let font_id = load_font_file(TEST_FONT_PATH, 20.0).unwrap();
    let shae_font = get_font(font_id).unwrap();

    let width = 120;
    let height = 60;
    let mut buffer = vec![0xFF112233u32; width * height]; // Dark slate background

    shae_font.draw_text_to_buffer(
        &mut buffer,
        width,
        height,
        10,
        10,
        "Shae",
        0xFF00FF88, // Mint green
        20.0,
    );

    // Verify that rendered text modified the buffer
    let altered_pixels = buffer.iter().filter(|&&p| p != 0xFF112233).count();
    assert!(altered_pixels > 20, "Glyphs should be blitted to buffer");
}

#[test]
fn test_shae_script_vector_font_measurement_ast() {
    let script = format!(
        r#"
        let gui = use "std:gui"
        let font = gui.load_font("{}", 22.0)
        let dims = gui.measure_text_font(font, "Shae Vector Engine")
        [font["id"] > 0, dims[0] > 50, dims[1] > 15]
        "#,
        TEST_FONT_PATH
    );

    let res = run(&script).expect("Script font measurement should succeed");
    if let Value::Array(arr) = res {
        let g = arr.read().unwrap();
        assert_eq!(g[0], Value::Bool(true));
        assert_eq!(g[1], Value::Bool(true));
        assert_eq!(g[2], Value::Bool(true));
    } else {
        panic!("Expected array result");
    }
}

#[test]
fn test_shae_script_vector_font_measurement_vm() {
    let script = format!(
        r#"
        let gui = use "std:gui"
        let font = gui.load_font("{}", 18.0)
        let dims_short = gui.measure_text_font(font, "A")
        let dims_long = gui.measure_text_font(font, "A much longer sentence for layout testing")
        [dims_long[0] > dims_short[0], dims_short[1] > 0]
        "#,
        TEST_FONT_PATH
    );

    let res = run_vm(&script).expect("VM script font measurement should succeed");
    if let Value::Array(arr) = res {
        let g = arr.read().unwrap();
        assert_eq!(g[0], Value::Bool(true));
        assert_eq!(g[1], Value::Bool(true));
    } else {
        panic!("Expected array result");
    }
}
