use shae::run_file_with_engine;
use shae::run_with_engine;
use std::sync::Mutex;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn lock_test() -> std::sync::MutexGuard<'static, ()> {
    TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn test_space_invaders_file_execution_vm_and_ast() {
    let _lock = lock_test();
    unsafe {
        std::env::set_var("SHAE_HEADLESS", "1");
        std::env::set_var("SHAE_FAST_TEST", "1");
        std::env::set_var("SHAE_MAX_FRAMES", "50");
    }

    // Run file on VM
    let res_vm = run_file_with_engine("examples/space_invaders.shae", true);
    assert!(res_vm.is_ok(), "VM execution failed: {:?}", res_vm.err());

    // Run file on AST
    let res_ast = run_file_with_engine("examples/space_invaders.shae", false);
    assert!(res_ast.is_ok(), "AST execution failed: {:?}", res_ast.err());
}

#[test]
fn test_space_invaders_simulated_gameplay_and_audio_log() {
    let _lock = lock_test();
    unsafe {
        std::env::set_var("SHAE_HEADLESS", "1");
        std::env::set_var("SHAE_FAST_TEST", "1");
    }
    shae::audio::clear_log_internal();
    shae::audio::get_audio_engine().write().unwrap().muted = false;

    let script = r#"
        let gui = use "std:gui"
        let audio = use "std:audio"
        let ui = use "std:ui"

        gui.init_window(80, 40, "Test Invaders Gameplay")
        audio.clear_log()

        // 1. Setup Game State
        let score = 0
        let high_score = 1000
        let lives = 3
        let player_x = 30
        let player_y = 30
        let state = 0 // STATE_MENU

        // Start from menu by pressing Space
        gui.simulate_key("space", true)
        if gui.is_key_down("space") {
            state = 1 // STATE_PLAYING
        }
        gui.simulate_key("space", false)
        assert(state == 1)

        // 2. Setup a target alien directly above player
        let alien = {
            x: 30,
            y: 10,
            w: 5,
            h: 3,
            alive: true,
            points: 30,
            color: 0x00FFFF
        }

        // 3. Player shoots laser
        gui.simulate_key("space", true)
        let laser = null
        if gui.is_key_down("space") {
            laser = { x: player_x + 2, y: player_y - 1, active: true }
            audio.play_laser()
        }
        gui.simulate_key("space", false)
        assert(laser != null)
        assert(audio.last_sound() == "laser")

        // 4. Simulate frames until laser reaches alien
        let frame = 0
        let hit = false
        while frame < 25 and not hit {
            laser.y -= 1
            // Check collision
            if laser.x >= alien.x and laser.x < alien.x + alien.w and laser.y >= alien.y and laser.y < alien.y + alien.h {
                alien.alive = false
                laser.active = false
                score += alien.points
                audio.play_explosion()
                hit = true
            }
            frame += 1
        }

        assert(hit == true)
        assert(alien.alive == false)
        assert(score == 30)
        assert(audio.last_sound() == "explosion")

        // Verify audio log has both laser and explosion
        let log = audio.get_audio_log()
        assert(len(log) >= 2)
        assert(log[0].sound == "laser")
        assert(log[1].sound == "explosion")

        // 5. Test bunker collision and damage
        let bunker = { x: 28, y: 20, w: 8, h: 3, hp: 4 }
        let bomb = { x: 30, y: 15, active: true }
        let b_hit = false
        while bomb.y < 25 and not b_hit {
            bomb.y += 1
            if bomb.x >= bunker.x and bomb.x < bunker.x + bunker.w and bomb.y >= bunker.y and bomb.y < bunker.y + bunker.h {
                bomb.active = false
                bunker.hp -= 1
                audio.play_hit()
                b_hit = true
            }
        }
        assert(b_hit == true)
        assert(bunker.hp == 3)
        assert(audio.last_sound() == "hit")

        // 6. Test game over jingle
        lives = 1
        lives -= 1
        if lives <= 0 {
            audio.play_jingle("game_over")
            state = 3 // STATE_GAME_OVER
        }
        assert(state == 3)
        assert(audio.last_sound() == "game_over")

        // 7. Test victory jingle
        audio.play_jingle("victory")
        assert(audio.last_sound() == "victory")

        "ok"
    "#;

    // Run on VM
    let vm_res = run_with_engine(script, true).expect("VM simulation failed");
    assert_eq!(vm_res.to_display(), "ok");

    // Run on AST
    let ast_res = run_with_engine(script, false).expect("AST simulation failed");
    assert_eq!(ast_res.to_display(), "ok");
}

#[test]
fn test_space_invaders_movement_physics_and_screen_bounds() {
    let _lock = lock_test();
    unsafe {
        std::env::set_var("SHAE_HEADLESS", "1");
        std::env::set_var("SHAE_FAST_TEST", "1");
    }

    let script = r#"
        let gui = use "std:gui"
        gui.init_window(80, 40, "Physics Test")

        let player_x = 2
        let player_w = 5
        let SCREEN_W = 80

        // Test left boundary clamping
        gui.simulate_key("left", true)
        if (gui.is_key_down("left") or gui.is_key_down("a")) and player_x > 2 {
            player_x -= 1
        }
        assert(player_x == 2) // Should not move past left bound

        // Test right boundary movement
        gui.simulate_key("left", false)
        gui.simulate_key("right", true)
        let steps = 0
        while steps < 100 {
            if (gui.is_key_down("right") or gui.is_key_down("d")) and player_x < SCREEN_W - player_w - 2 {
                player_x += 1
            }
            steps += 1
        }
        gui.simulate_key("right", false)
        assert(player_x == SCREEN_W - player_w - 2) // Clamped at right bound

        // Test fleet bounce & descent logic
        let fleet_dir = 1
        let alien_x = SCREEN_W - 7
        let alien_y = 10
        let alien_w = 5

        // Alien reaches right edge
        if alien_x + fleet_dir * 2 + alien_w >= SCREEN_W - 2 {
            fleet_dir = -fleet_dir
            alien_y += 1
        }
        assert(fleet_dir == -1)
        assert(alien_y == 11)

        "physics_ok"
    "#;

    let vm_res = run_with_engine(script, true).expect("VM physics test failed");
    assert_eq!(vm_res.to_display(), "physics_ok");

    let ast_res = run_with_engine(script, false).expect("AST physics test failed");
    assert_eq!(ast_res.to_display(), "physics_ok");
}
