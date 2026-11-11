use shae::run;

#[test]
fn test_std_random_module() {
    let code = r#"
        let rand = use "std:random"
        
        let n = rand.int(5, 15)
        assert(n >= 5 and n <= 15)
        
        let f = rand.float()
        assert(f >= 0.0 and f <= 1.0)
        
        let items = [100, 200, 300]
        let picked = rand.choice(items)
        assert(picked == 100 or picked == 200 or picked == 300)
        
        let shuffled = rand.shuffle([1, 2, 3, 4, 5])
        assert_eq(len(shuffled), 5)
        
        rand.seed(12345)
        let b = rand.bool()
        assert(b == true or b == false)
    "#;
    let res = run(code);
    assert!(res.is_ok(), "random test failed: {:?}", res.err());
}

#[test]
fn test_std_math_module() {
    let code = r#"
        let math = use "std:math"
        
        assert_eq(math.sqrt(49.0), 7.0)
        assert_eq(math.abs(-99), 99)
        assert_eq(math.min(15, 42), 15)
        assert_eq(math.max(15, 42), 42)
        assert_eq(math.pow(2, 10), 1024)
        assert_eq(math.floor(8.9), 8)
        assert_eq(math.ceil(8.1), 9)
        assert_eq(math.round(8.5), 9)
        assert_eq(math.clamp(120, 0, 100), 100)
        assert_eq(math.clamp(-5, 0, 100), 0)
        assert(math.PI > 3.1415 and math.PI < 3.1416)
        assert(math.E > 2.718 and math.E < 2.719)
    "#;
    let res = run(code);
    assert!(res.is_ok(), "math test failed: {:?}", res.err());
}

#[test]
fn test_std_io_module() {
    let code = r#"
        let io = use "std:io"
        io.println("Hello from std:io test")
        io.flush()
    "#;
    let res = run(code);
    assert!(res.is_ok(), "io test failed: {:?}", res.err());
}

#[test]
fn test_std_gui_module_headless_primitives() {
    let code = r#"
        let gui = use "std:gui"
        
        let win = gui.window("Test Headless Window", 320, 240)
        assert(win > 0)
        
        assert(gui.is_open(win))
        
        gui.clear(win, 0x101010)
        gui.pixel(win, 15, 15, 0xFFFFFF)
        gui.rect(win, 20, 20, 40, 40, 0x00FF00)
        gui.rect_outline(win, 10, 10, 60, 60, 0xFF0000, 2)
        gui.circle(win, 100, 100, 25, 0x0000FF)
        gui.circle_outline(win, 100, 100, 25, 0xFFFF00)
        gui.line(win, 0, 0, 320, 240, 0xFF00FF)
        gui.text(win, 10, 10, "SHAE 2D CANVAS", 0xFFFFFF, 1)
        
        gui.render(win)
        
        let keys = gui.get_keys(win)
        assert(len(keys) >= 0)
        
        let is_up = gui.is_key_down(win, "Up")
        assert(is_up == false)
        
        let mpos = gui.mouse_pos(win)
        assert(mpos == null or len(mpos) == 2)
        
        gui.close(win)
        assert(gui.is_open(win) == false)
    "#;
    let res = run(code);
    assert!(res.is_ok(), "gui test failed: {:?}", res.err());
}
