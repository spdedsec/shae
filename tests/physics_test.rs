use indexmap::IndexMap;
use shae::physics::{AABB, Circle, Ray, Vec2, integrate_map_body, resolve_map_collision};
use shae::run;
use shae::run_vm;
use shae::value::Value;

#[test]
fn test_vec2_operations() {
    let v1 = Vec2::new(3.0, 4.0);
    assert!((v1.length() - 5.0).abs() < 1e-6);

    let v_norm = v1.normalized();
    assert!((v_norm.length() - 1.0).abs() < 1e-6);
    assert!((v_norm.x - 0.6).abs() < 1e-6);
    assert!((v_norm.y - 0.8).abs() < 1e-6);

    let v2 = Vec2::new(1.0, 2.0);
    assert!((v1.dot(&v2) - 11.0).abs() < 1e-6);
}

#[test]
fn test_aabb_geometry_and_intersections() {
    let box1 = AABB {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    let box2 = AABB {
        x: 5.0,
        y: 5.0,
        w: 10.0,
        h: 10.0,
    };
    let box3 = AABB {
        x: 20.0,
        y: 20.0,
        w: 5.0,
        h: 5.0,
    };

    assert!(box1.intersects(&box2));
    assert!(box2.intersects(&box1));
    assert!(!box1.intersects(&box3));

    assert!(box1.contains_point(5.0, 5.0));
    assert!(!box1.contains_point(15.0, 5.0));
}

#[test]
fn test_circle_geometry_and_intersections() {
    let c1 = Circle {
        cx: 0.0,
        cy: 0.0,
        r: 10.0,
    };
    let c2 = Circle {
        cx: 15.0,
        cy: 0.0,
        r: 10.0,
    };
    let c3 = Circle {
        cx: 50.0,
        cy: 50.0,
        r: 5.0,
    };

    assert!(c1.intersects(&c2));
    assert!(!c1.intersects(&c3));

    assert!(c1.contains_point(5.0, 0.0));
    assert!(!c1.contains_point(12.0, 0.0));

    let box1 = AABB {
        x: 8.0,
        y: -5.0,
        w: 10.0,
        h: 10.0,
    };
    assert!(c1.intersects_aabb(&box1));

    let box2 = AABB {
        x: 30.0,
        y: 30.0,
        w: 10.0,
        h: 10.0,
    };
    assert!(!c1.intersects_aabb(&box2));
}

#[test]
fn test_raycast_intersections() {
    let ray = Ray {
        origin: Vec2::new(-10.0, 5.0),
        dir: Vec2::new(1.0, 0.0),
    };
    let aabb = AABB {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };

    let hit = ray.intersects_aabb(&aabb);
    assert!(hit.hit);
    assert!((hit.dist - 10.0).abs() < 1e-6);
    assert_eq!(hit.normal, Vec2::new(-1.0, 0.0));

    let ray_miss = Ray {
        origin: Vec2::new(-10.0, 25.0),
        dir: Vec2::new(1.0, 0.0),
    };
    assert!(!ray_miss.intersects_aabb(&aabb).hit);

    let circle = Circle {
        cx: 10.0,
        cy: 0.0,
        r: 5.0,
    };
    let ray_circle = Ray {
        origin: Vec2::new(0.0, 0.0),
        dir: Vec2::new(1.0, 0.0),
    };
    let hit_c = ray_circle.intersects_circle(&circle);
    assert!(hit_c.hit);
    assert!((hit_c.dist - 5.0).abs() < 1e-6);
}

#[test]
fn test_rigid_body_integration_and_collision_resolution() {
    let mut body_a = IndexMap::new();
    body_a.insert("pos_x".into(), Value::Float(0.0));
    body_a.insert("pos_y".into(), Value::Float(0.0));
    body_a.insert("vel_x".into(), Value::Float(10.0));
    body_a.insert("vel_y".into(), Value::Float(0.0));
    body_a.insert("acc_x".into(), Value::Float(0.0));
    body_a.insert("acc_y".into(), Value::Float(0.0));
    body_a.insert("mass".into(), Value::Float(1.0));
    body_a.insert("friction".into(), Value::Float(0.0));

    integrate_map_body(&mut body_a, 0.1, 9.8);
    let px = match body_a.get("pos_x").unwrap() {
        Value::Float(f) => *f,
        _ => panic!(),
    };
    let vy = match body_a.get("vel_y").unwrap() {
        Value::Float(f) => *f,
        _ => panic!(),
    };
    assert!((px - 1.0).abs() < 1e-6);
    assert!((vy - 0.98).abs() < 1e-6);

    // Collision resolution
    let mut crate1 = IndexMap::new();
    crate1.insert("pos_x".into(), Value::Float(0.0));
    crate1.insert("pos_y".into(), Value::Float(0.0));
    crate1.insert("w".into(), Value::Float(10.0));
    crate1.insert("h".into(), Value::Float(10.0));
    crate1.insert("vel_x".into(), Value::Float(5.0));
    crate1.insert("vel_y".into(), Value::Float(0.0));
    crate1.insert("mass".into(), Value::Float(1.0));
    crate1.insert("restitution".into(), Value::Float(1.0));
    crate1.insert("is_static".into(), Value::Bool(false));

    let mut crate2 = IndexMap::new();
    crate2.insert("pos_x".into(), Value::Float(8.0));
    crate2.insert("pos_y".into(), Value::Float(0.0));
    crate2.insert("w".into(), Value::Float(10.0));
    crate2.insert("h".into(), Value::Float(10.0));
    crate2.insert("vel_x".into(), Value::Float(-5.0));
    crate2.insert("vel_y".into(), Value::Float(0.0));
    crate2.insert("mass".into(), Value::Float(1.0));
    crate2.insert("restitution".into(), Value::Float(1.0));
    crate2.insert("is_static".into(), Value::Bool(false));

    let col = resolve_map_collision(&mut crate1, &mut crate2);
    assert!(col.is_some());
    let (nx, _, depth) = col.unwrap();
    assert!((depth - 2.0).abs() < 1e-6);
    assert_eq!(nx, 1.0);

    let v1x = match crate1.get("vel_x").unwrap() {
        Value::Float(f) => *f,
        _ => panic!(),
    };
    let v2x = match crate2.get("vel_x").unwrap() {
        Value::Float(f) => *f,
        _ => panic!(),
    };
    assert!(v1x < 0.0);
    assert!(v2x > 0.0);
}

#[test]
fn test_shae_script_physics_integration_ast() {
    let script = r#"
        let physics = use "std:physics"
        let b1 = physics.aabb(0.0, 0.0, 20.0, 20.0)
        let b2 = physics.aabb(10.0, 10.0, 20.0, 20.0)
        let b3 = physics.aabb(100.0, 100.0, 10.0, 10.0)

        let hit1 = physics.aabb_intersects(b1, b2)
        let hit2 = physics.aabb_intersects(b1, b3)
        let inside = physics.aabb_contains_point(b1, 5.0, 5.0)

        let ray = physics.ray(-10.0, 10.0, 1.0, 0.0)
        let ray_res = physics.ray_intersects_aabb(ray, b1)

        [hit1, hit2, inside, ray_res["hit"], ray_res["dist"]]
    "#;

    let res = run(script).expect("Script physics execution should succeed");
    if let Value::Array(arr) = res {
        let g = arr.read().unwrap();
        assert_eq!(g[0], Value::Bool(true));
        assert_eq!(g[1], Value::Bool(false));
        assert_eq!(g[2], Value::Bool(true));
        assert_eq!(g[3], Value::Bool(true));
        if let Value::Float(dist) = g[4] {
            assert!((dist - 10.0).abs() < 1e-5);
        } else {
            panic!("Expected float dist");
        }
    } else {
        panic!("Expected array result");
    }
}

#[test]
fn test_shae_script_physics_world_vm() {
    let script = r#"
        let physics = use "std:physics"
        let world = physics.world_new(100.0)

        let ground = physics.body({
            "pos_x": 0.0, "pos_y": 100.0,
            "w": 200.0, "h": 20.0,
            "is_static": true,
            "friction": 0.5,
            "restitution": 0.0
        })
        physics.world_add_body(world, ground)

        let crate_body = physics.body({
            "pos_x": 50.0, "pos_y": 50.0,
            "w": 10.0, "h": 10.0,
            "mass": 1.0,
            "is_static": false
        })
        physics.world_add_body(world, crate_body)

        // Step world 5 times
        let i = 0
        while i < 5 {
            physics.world_step(world, 0.1)
            i = i + 1
        }

        let bodies = physics.world_get_bodies(world)
        let dropped = bodies[1]
        [bodies.len, dropped["pos_y"] > 50.0]
    "#;

    let res = run_vm(script).expect("Script physics world execution should succeed");
    if let Value::Array(arr) = res {
        let g = arr.read().unwrap();
        assert_eq!(g[0], Value::Int(2));
        assert_eq!(g[1], Value::Bool(true));
    } else {
        panic!("Expected array result");
    }
}
