//! 2D Physics & Rigid Body Dynamics Engine (`std:physics`)
//!
//! Provides geometric shape constructors, collision detection algorithms (AABB, Circle, Point, Ray),
//! Symplectic Euler rigid-body kinematics, impulse-based contact resolution with restitution and friction,
//! and simulated physics world containers.

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn normalized(&self) -> Self {
        let len = self.length();
        if len > 1e-9 {
            Self {
                x: self.x / len,
                y: self.y / len,
            }
        } else {
            Self { x: 0.0, y: 0.0 }
        }
    }

    pub fn dot(&self, other: &Vec2) -> f64 {
        self.x * other.x + self.y * other.y
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AABB {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl AABB {
    pub fn intersects(&self, other: &AABB) -> bool {
        self.x <= other.x + other.w
            && self.x + self.w >= other.x
            && self.y <= other.y + other.h
            && self.y + self.h >= other.y
    }

    pub fn contains_point(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
}

impl Circle {
    pub fn intersects(&self, other: &Circle) -> bool {
        let dx = self.cx - other.cx;
        let dy = self.cy - other.cy;
        let rad_sum = self.r + other.r;
        (dx * dx + dy * dy) <= (rad_sum * rad_sum)
    }

    pub fn contains_point(&self, px: f64, py: f64) -> bool {
        let dx = px - self.cx;
        let dy = py - self.cy;
        (dx * dx + dy * dy) <= (self.r * self.r)
    }

    pub fn intersects_aabb(&self, aabb: &AABB) -> bool {
        let clamped_x = self.cx.clamp(aabb.x, aabb.x + aabb.w);
        let clamped_y = self.cy.clamp(aabb.y, aabb.y + aabb.h);
        let dx = self.cx - clamped_x;
        let dy = self.cy - clamped_y;
        (dx * dx + dy * dy) <= (self.r * self.r)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    pub origin: Vec2,
    pub dir: Vec2,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RaycastHit {
    pub hit: bool,
    pub dist: f64,
    pub normal: Vec2,
}

impl Ray {
    pub fn intersects_aabb(&self, aabb: &AABB) -> RaycastHit {
        let mut tmin = f64::NEG_INFINITY;
        let mut tmax = f64::INFINITY;
        let mut normal = Vec2::new(0.0, 0.0);

        // X axis slab
        if self.dir.x.abs() > 1e-9 {
            let t1 = (aabb.x - self.origin.x) / self.dir.x;
            let t2 = (aabb.x + aabb.w - self.origin.x) / self.dir.x;
            let (tnear, tfar, n) = if t1 < t2 {
                (t1, t2, Vec2::new(-1.0, 0.0))
            } else {
                (t2, t1, Vec2::new(1.0, 0.0))
            };
            if tnear > tmin {
                tmin = tnear;
                normal = n;
            }
            if tfar < tmax {
                tmax = tfar;
            }
        } else if self.origin.x < aabb.x || self.origin.x > aabb.x + aabb.w {
            return RaycastHit {
                hit: false,
                dist: 0.0,
                normal: Vec2::new(0.0, 0.0),
            };
        }

        // Y axis slab
        if self.dir.y.abs() > 1e-9 {
            let t1 = (aabb.y - self.origin.y) / self.dir.y;
            let t2 = (aabb.y + aabb.h - self.origin.y) / self.dir.y;
            let (tnear, tfar, n) = if t1 < t2 {
                (t1, t2, Vec2::new(0.0, -1.0))
            } else {
                (t2, t1, Vec2::new(0.0, 1.0))
            };
            if tnear > tmin {
                tmin = tnear;
                normal = n;
            }
            if tfar < tmax {
                tmax = tfar;
            }
        } else if self.origin.y < aabb.y || self.origin.y > aabb.y + aabb.h {
            return RaycastHit {
                hit: false,
                dist: 0.0,
                normal: Vec2::new(0.0, 0.0),
            };
        }

        if tmin <= tmax && tmax >= 0.0 {
            let dist = if tmin >= 0.0 { tmin } else { 0.0 };
            RaycastHit {
                hit: true,
                dist,
                normal,
            }
        } else {
            RaycastHit {
                hit: false,
                dist: 0.0,
                normal: Vec2::new(0.0, 0.0),
            }
        }
    }

    pub fn intersects_circle(&self, circle: &Circle) -> RaycastHit {
        let oc = Vec2::new(self.origin.x - circle.cx, self.origin.y - circle.cy);
        let b = 2.0 * oc.dot(&self.dir);
        let c = oc.dot(&oc) - circle.r * circle.r;
        let discriminant = b * b - 4.0 * c;

        if discriminant < 0.0 {
            return RaycastHit {
                hit: false,
                dist: 0.0,
                normal: Vec2::new(0.0, 0.0),
            };
        }

        let sqrt_d = discriminant.sqrt();
        let t1 = (-b - sqrt_d) / 2.0;
        let t2 = (-b + sqrt_d) / 2.0;

        let dist = if t1 >= 0.0 {
            t1
        } else if t2 >= 0.0 {
            t2
        } else {
            return RaycastHit {
                hit: false,
                dist: 0.0,
                normal: Vec2::new(0.0, 0.0),
            };
        };

        let hit_pt = Vec2::new(
            self.origin.x + self.dir.x * dist,
            self.origin.y + self.dir.y * dist,
        );
        let normal = Vec2::new(hit_pt.x - circle.cx, hit_pt.y - circle.cy).normalized();

        RaycastHit {
            hit: true,
            dist,
            normal,
        }
    }
}

// ---------------------------------------------------------------------------
// Rigid Body Struct & World Registry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RigidBody {
    pub id: u64,
    pub pos: Vec2,
    pub vel: Vec2,
    pub acc: Vec2,
    pub w: f64,
    pub h: f64,
    pub mass: f64,
    pub restitution: f64,
    pub friction: f64,
    pub is_static: bool,
}

impl RigidBody {
    pub fn inv_mass(&self) -> f64 {
        if self.is_static || self.mass <= 0.0 {
            0.0
        } else {
            1.0 / self.mass
        }
    }

    pub fn aabb(&self) -> AABB {
        AABB {
            x: self.pos.x,
            y: self.pos.y,
            w: self.w,
            h: self.h,
        }
    }
}

pub struct PhysicsWorld {
    pub gravity_y: f64,
    pub bodies: Vec<Arc<RwLock<IndexMap<String, Value>>>>,
}

static NEXT_BODY_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_WORLD_ID: AtomicU64 = AtomicU64::new(1);
static WORLDS: OnceLock<Mutex<HashMap<u64, Arc<RwLock<PhysicsWorld>>>>> = OnceLock::new();

fn get_worlds() -> &'static Mutex<HashMap<u64, Arc<RwLock<PhysicsWorld>>>> {
    WORLDS.get_or_init(|| Mutex::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// Helper Conversion Functions
// ---------------------------------------------------------------------------

fn to_f64_val(v: &Value) -> Option<f64> {
    match v {
        Value::Float(n) | Value::Number(n) => Some(*n),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn to_bool_val(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        _ => false,
    }
}

fn get_f64_prop(map: &IndexMap<String, Value>, key: &str, default: f64) -> f64 {
    map.get(key).and_then(to_f64_val).unwrap_or(default)
}

fn make_vec2_map(x: f64, y: f64) -> Value {
    let mut map = IndexMap::new();
    map.insert("x".to_string(), Value::Float(x));
    map.insert("y".to_string(), Value::Float(y));
    Value::Map(Arc::new(RwLock::new(map)))
}

fn make_aabb_map(x: f64, y: f64, w: f64, h: f64) -> Value {
    let mut map = IndexMap::new();
    map.insert("x".to_string(), Value::Float(x));
    map.insert("y".to_string(), Value::Float(y));
    map.insert("w".to_string(), Value::Float(w));
    map.insert("h".to_string(), Value::Float(h));
    Value::Map(Arc::new(RwLock::new(map)))
}

fn make_circle_map(cx: f64, cy: f64, r: f64) -> Value {
    let mut map = IndexMap::new();
    map.insert("cx".to_string(), Value::Float(cx));
    map.insert("cy".to_string(), Value::Float(cy));
    map.insert("r".to_string(), Value::Float(r));
    Value::Map(Arc::new(RwLock::new(map)))
}

fn make_ray_map(ox: f64, oy: f64, dx: f64, dy: f64) -> Value {
    let dir = Vec2::new(dx, dy).normalized();
    let mut map = IndexMap::new();
    map.insert("ox".to_string(), Value::Float(ox));
    map.insert("oy".to_string(), Value::Float(oy));
    map.insert("dx".to_string(), Value::Float(dir.x));
    map.insert("dy".to_string(), Value::Float(dir.y));
    Value::Map(Arc::new(RwLock::new(map)))
}

fn extract_aabb(val: &Value) -> Option<AABB> {
    if let Value::Map(m) = val {
        let map = m.read().unwrap();
        let x = get_f64_prop(&map, "x", get_f64_prop(&map, "pos_x", 0.0));
        let y = get_f64_prop(&map, "y", get_f64_prop(&map, "pos_y", 0.0));
        let w = get_f64_prop(&map, "w", get_f64_prop(&map, "width", 0.0));
        let h = get_f64_prop(&map, "h", get_f64_prop(&map, "height", 0.0));
        Some(AABB { x, y, w, h })
    } else {
        None
    }
}

fn extract_circle(val: &Value) -> Option<Circle> {
    if let Value::Map(m) = val {
        let map = m.read().unwrap();
        let cx = get_f64_prop(&map, "cx", get_f64_prop(&map, "x", 0.0));
        let cy = get_f64_prop(&map, "cy", get_f64_prop(&map, "y", 0.0));
        let r = get_f64_prop(&map, "r", get_f64_prop(&map, "radius", 0.0));
        Some(Circle { cx, cy, r })
    } else {
        None
    }
}

fn extract_ray(val: &Value) -> Option<Ray> {
    if let Value::Map(m) = val {
        let map = m.read().unwrap();
        let ox = get_f64_prop(&map, "ox", get_f64_prop(&map, "x", 0.0));
        let oy = get_f64_prop(&map, "oy", get_f64_prop(&map, "y", 0.0));
        let dx = get_f64_prop(&map, "dx", 1.0);
        let dy = get_f64_prop(&map, "dy", 0.0);
        Some(Ray {
            origin: Vec2::new(ox, oy),
            dir: Vec2::new(dx, dy).normalized(),
        })
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Symplectic Euler Kinematics & Manifold Solver
// ---------------------------------------------------------------------------

pub fn integrate_map_body(map: &mut IndexMap<String, Value>, dt: f64, gravity_y: f64) {
    let is_static = map.get("is_static").map(to_bool_val).unwrap_or(false);
    let mass = get_f64_prop(map, "mass", 1.0);

    if is_static || mass <= 0.0 {
        return;
    }

    let mut px = get_f64_prop(map, "pos_x", get_f64_prop(map, "x", 0.0));
    let mut py = get_f64_prop(map, "pos_y", get_f64_prop(map, "y", 0.0));
    let mut vx = get_f64_prop(map, "vel_x", get_f64_prop(map, "vx", 0.0));
    let mut vy = get_f64_prop(map, "vel_y", get_f64_prop(map, "vy", 0.0));
    let mut ax = get_f64_prop(map, "acc_x", 0.0);
    let mut ay = get_f64_prop(map, "acc_y", 0.0);
    let friction = get_f64_prop(map, "friction", 0.1);

    // Apply gravity
    ay += gravity_y;

    // Velocity integration
    vx += ax * dt;
    vy += ay * dt;

    // Friction / damping
    let damping = (1.0 - friction * 0.1 * dt).clamp(0.0, 1.0);
    vx *= damping;

    // Position integration (Symplectic)
    px += vx * dt;
    py += vy * dt;

    // Reset forces
    ax = 0.0;
    ay = 0.0;

    map.insert("pos_x".to_string(), Value::Float(px));
    map.insert("pos_y".to_string(), Value::Float(py));
    map.insert("x".to_string(), Value::Float(px));
    map.insert("y".to_string(), Value::Float(py));
    map.insert("vel_x".to_string(), Value::Float(vx));
    map.insert("vel_y".to_string(), Value::Float(vy));
    map.insert("acc_x".to_string(), Value::Float(ax));
    map.insert("acc_y".to_string(), Value::Float(ay));
}

pub fn resolve_map_collision(
    map_a: &mut IndexMap<String, Value>,
    map_b: &mut IndexMap<String, Value>,
) -> Option<(f64, f64, f64)> {
    let mut ax = get_f64_prop(map_a, "pos_x", get_f64_prop(map_a, "x", 0.0));
    let mut ay = get_f64_prop(map_a, "pos_y", get_f64_prop(map_a, "y", 0.0));
    let aw = get_f64_prop(map_a, "w", get_f64_prop(map_a, "width", 0.0));
    let ah = get_f64_prop(map_a, "h", get_f64_prop(map_a, "height", 0.0));

    let mut bx = get_f64_prop(map_b, "pos_x", get_f64_prop(map_b, "x", 0.0));
    let mut by = get_f64_prop(map_b, "pos_y", get_f64_prop(map_b, "y", 0.0));
    let bw = get_f64_prop(map_b, "w", get_f64_prop(map_b, "width", 0.0));
    let bh = get_f64_prop(map_b, "h", get_f64_prop(map_b, "height", 0.0));

    // Centers
    let ca_x = ax + aw * 0.5;
    let ca_y = ay + ah * 0.5;
    let cb_x = bx + bw * 0.5;
    let cb_y = by + bh * 0.5;

    let delta_x = cb_x - ca_x;
    let delta_y = cb_y - ca_y;

    let ox = (aw + bw) * 0.5 - delta_x.abs();
    let oy = (ah + bh) * 0.5 - delta_y.abs();

    if ox <= 0.0 || oy <= 0.0 {
        return None;
    }

    // Minimum Translation Vector (MTV)
    let (normal_x, normal_y, depth) = if ox < oy {
        let sign = if delta_x >= 0.0 { 1.0 } else { -1.0 };
        (sign, 0.0, ox)
    } else {
        let sign = if delta_y >= 0.0 { 1.0 } else { -1.0 };
        (0.0, sign, oy)
    };

    let static_a = map_a.get("is_static").map(to_bool_val).unwrap_or(false);
    let static_b = map_b.get("is_static").map(to_bool_val).unwrap_or(false);
    let mass_a = get_f64_prop(map_a, "mass", 1.0);
    let mass_b = get_f64_prop(map_b, "mass", 1.0);

    let inv_mass_a = if static_a || mass_a <= 0.0 {
        0.0
    } else {
        1.0 / mass_a
    };
    let inv_mass_b = if static_b || mass_b <= 0.0 {
        0.0
    } else {
        1.0 / mass_b
    };
    let total_inv_mass = inv_mass_a + inv_mass_b;

    if total_inv_mass <= 1e-9 {
        return Some((normal_x, normal_y, depth));
    }

    // Positional separation
    let sep_a = depth * (inv_mass_a / total_inv_mass);
    let sep_b = depth * (inv_mass_b / total_inv_mass);

    ax -= normal_x * sep_a;
    ay -= normal_y * sep_a;
    bx += normal_x * sep_b;
    by += normal_y * sep_b;

    map_a.insert("pos_x".to_string(), Value::Float(ax));
    map_a.insert("pos_y".to_string(), Value::Float(ay));
    map_a.insert("x".to_string(), Value::Float(ax));
    map_a.insert("y".to_string(), Value::Float(ay));

    map_b.insert("pos_x".to_string(), Value::Float(bx));
    map_b.insert("pos_y".to_string(), Value::Float(by));
    map_b.insert("x".to_string(), Value::Float(bx));
    map_b.insert("y".to_string(), Value::Float(by));

    // Velocities & Impulse
    let mut vax = get_f64_prop(map_a, "vel_x", 0.0);
    let mut vay = get_f64_prop(map_a, "vel_y", 0.0);
    let mut vbx = get_f64_prop(map_b, "vel_x", 0.0);
    let mut vby = get_f64_prop(map_b, "vel_y", 0.0);

    let rel_vx = vbx - vax;
    let rel_vy = vby - vay;
    let vn = rel_vx * normal_x + rel_vy * normal_y;

    if vn < 0.0 {
        let rest_a = get_f64_prop(map_a, "restitution", 0.2);
        let rest_b = get_f64_prop(map_b, "restitution", 0.2);
        let e = if vn.abs() < 5.0 {
            0.0 // eliminate micro-jitter for resting contacts
        } else {
            rest_a.min(rest_b).clamp(0.0, 1.0)
        };

        let jn = (-(1.0 + e) * vn) / total_inv_mass;
        let jn_x = jn * normal_x;
        let jn_y = jn * normal_y;

        vax -= jn_x * inv_mass_a;
        vay -= jn_y * inv_mass_a;
        vbx += jn_x * inv_mass_b;
        vby += jn_y * inv_mass_b;

        // Tangential Coulomb friction
        let vt_x = rel_vx - vn * normal_x;
        let vt_y = rel_vy - vn * normal_y;
        let vt_len = (vt_x * vt_x + vt_y * vt_y).sqrt();

        if vt_len > 1e-9 {
            let tx = vt_x / vt_len;
            let ty = vt_y / vt_len;
            let vt = rel_vx * tx + rel_vy * ty;

            let fric_a = get_f64_prop(map_a, "friction", 0.2);
            let fric_b = get_f64_prop(map_b, "friction", 0.2);
            let mu = (fric_a * fric_b).sqrt();

            let max_jt = jn * mu;
            let jt = (-vt / total_inv_mass).clamp(-max_jt, max_jt);

            let jt_x = jt * tx;
            let jt_y = jt * ty;

            vax -= jt_x * inv_mass_a;
            vay -= jt_y * inv_mass_a;
            vbx += jt_x * inv_mass_b;
            vby += jt_y * inv_mass_b;
        }

        map_a.insert("vel_x".to_string(), Value::Float(vax));
        map_a.insert("vel_y".to_string(), Value::Float(vay));
        map_b.insert("vel_x".to_string(), Value::Float(vbx));
        map_b.insert("vel_y".to_string(), Value::Float(vby));
    }

    Some((normal_x, normal_y, depth))
}

// ---------------------------------------------------------------------------
// Shae Builtin Functions
// ---------------------------------------------------------------------------

fn physics_aabb(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("physics.aabb", 4, &args, span)?;
    let x = to_f64_val(&args[0])
        .ok_or_else(|| RuntimeError::new("physics.aabb: expected number for x".into()).at(span))?;
    let y = to_f64_val(&args[1])
        .ok_or_else(|| RuntimeError::new("physics.aabb: expected number for y".into()).at(span))?;
    let w = to_f64_val(&args[2])
        .ok_or_else(|| RuntimeError::new("physics.aabb: expected number for w".into()).at(span))?;
    let h = to_f64_val(&args[3])
        .ok_or_else(|| RuntimeError::new("physics.aabb: expected number for h".into()).at(span))?;
    Ok(make_aabb_map(x, y, w, h))
}

fn physics_circle(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.circle", 3, &args, span)?;
    let cx = to_f64_val(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.circle: expected number for cx".into()).at(span)
    })?;
    let cy = to_f64_val(&args[1]).ok_or_else(|| {
        RuntimeError::new("physics.circle: expected number for cy".into()).at(span)
    })?;
    let r = to_f64_val(&args[2]).ok_or_else(|| {
        RuntimeError::new("physics.circle: expected number for r".into()).at(span)
    })?;
    Ok(make_circle_map(cx, cy, r))
}

fn physics_point(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("physics.point", 2, &args, span)?;
    let x = to_f64_val(&args[0])
        .ok_or_else(|| RuntimeError::new("physics.point: expected number for x".into()).at(span))?;
    let y = to_f64_val(&args[1])
        .ok_or_else(|| RuntimeError::new("physics.point: expected number for y".into()).at(span))?;
    Ok(make_vec2_map(x, y))
}

fn physics_ray(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("physics.ray", 4, &args, span)?;
    let ox = to_f64_val(&args[0])
        .ok_or_else(|| RuntimeError::new("physics.ray: expected number for ox".into()).at(span))?;
    let oy = to_f64_val(&args[1])
        .ok_or_else(|| RuntimeError::new("physics.ray: expected number for oy".into()).at(span))?;
    let dx = to_f64_val(&args[2])
        .ok_or_else(|| RuntimeError::new("physics.ray: expected number for dx".into()).at(span))?;
    let dy = to_f64_val(&args[3])
        .ok_or_else(|| RuntimeError::new("physics.ray: expected number for dy".into()).at(span))?;
    Ok(make_ray_map(ox, oy, dx, dy))
}

fn physics_body(_ev: &mut Evaluator, args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let mut map = IndexMap::new();
    let id = NEXT_BODY_ID.fetch_add(1, Ordering::Relaxed);
    map.insert("id".to_string(), Value::Int(id as i64));

    if !args.is_empty() {
        if let Value::Map(m) = &args[0] {
            let src = m.read().unwrap();
            for (k, v) in src.iter() {
                map.insert(k.clone(), v.clone());
            }
        }
    }

    // Defaults
    let pos_x = get_f64_prop(&map, "pos_x", get_f64_prop(&map, "x", 0.0));
    let pos_y = get_f64_prop(&map, "pos_y", get_f64_prop(&map, "y", 0.0));
    let vel_x = get_f64_prop(&map, "vel_x", get_f64_prop(&map, "vx", 0.0));
    let vel_y = get_f64_prop(&map, "vel_y", get_f64_prop(&map, "vy", 0.0));
    let acc_x = get_f64_prop(&map, "acc_x", 0.0);
    let acc_y = get_f64_prop(&map, "acc_y", 0.0);
    let w = get_f64_prop(&map, "w", get_f64_prop(&map, "width", 16.0));
    let h = get_f64_prop(&map, "h", get_f64_prop(&map, "height", 16.0));
    let mass = get_f64_prop(&map, "mass", 1.0);
    let restitution = get_f64_prop(&map, "restitution", 0.2);
    let friction = get_f64_prop(&map, "friction", 0.2);
    let is_static = map.get("is_static").map(to_bool_val).unwrap_or(false);

    map.insert("pos_x".to_string(), Value::Float(pos_x));
    map.insert("pos_y".to_string(), Value::Float(pos_y));
    map.insert("x".to_string(), Value::Float(pos_x));
    map.insert("y".to_string(), Value::Float(pos_y));
    map.insert("vel_x".to_string(), Value::Float(vel_x));
    map.insert("vel_y".to_string(), Value::Float(vel_y));
    map.insert("acc_x".to_string(), Value::Float(acc_x));
    map.insert("acc_y".to_string(), Value::Float(acc_y));
    map.insert("w".to_string(), Value::Float(w));
    map.insert("h".to_string(), Value::Float(h));
    map.insert("mass".to_string(), Value::Float(mass));
    map.insert("restitution".to_string(), Value::Float(restitution));
    map.insert("friction".to_string(), Value::Float(friction));
    map.insert("is_static".to_string(), Value::Bool(is_static));

    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn physics_aabb_intersects(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.aabb_intersects", 2, &args, span)?;
    let a = extract_aabb(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.aabb_intersects: expected aabb or body for arg 1".into())
            .at(span)
    })?;
    let b = extract_aabb(&args[1]).ok_or_else(|| {
        RuntimeError::new("physics.aabb_intersects: expected aabb or body for arg 2".into())
            .at(span)
    })?;
    Ok(Value::Bool(a.intersects(&b)))
}

fn physics_aabb_contains_point(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.aabb_contains_point", 3, &args, span)?;
    let a = extract_aabb(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.aabb_contains_point: expected aabb or body for arg 1".into())
            .at(span)
    })?;
    let px = to_f64_val(&args[1]).ok_or_else(|| {
        RuntimeError::new("physics.aabb_contains_point: expected number for px".into()).at(span)
    })?;
    let py = to_f64_val(&args[2]).ok_or_else(|| {
        RuntimeError::new("physics.aabb_contains_point: expected number for py".into()).at(span)
    })?;
    Ok(Value::Bool(a.contains_point(px, py)))
}

fn physics_circle_intersects(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.circle_intersects", 2, &args, span)?;
    let c1 = extract_circle(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.circle_intersects: expected circle for arg 1".into()).at(span)
    })?;
    let c2 = extract_circle(&args[1]).ok_or_else(|| {
        RuntimeError::new("physics.circle_intersects: expected circle for arg 2".into()).at(span)
    })?;
    Ok(Value::Bool(c1.intersects(&c2)))
}

fn physics_circle_contains_point(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.circle_contains_point", 3, &args, span)?;
    let c = extract_circle(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.circle_contains_point: expected circle for arg 1".into())
            .at(span)
    })?;
    let px = to_f64_val(&args[1]).ok_or_else(|| {
        RuntimeError::new("physics.circle_contains_point: expected number for px".into()).at(span)
    })?;
    let py = to_f64_val(&args[2]).ok_or_else(|| {
        RuntimeError::new("physics.circle_contains_point: expected number for py".into()).at(span)
    })?;
    Ok(Value::Bool(c.contains_point(px, py)))
}

fn physics_circle_aabb_intersects(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.circle_aabb_intersects", 2, &args, span)?;
    let c = extract_circle(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.circle_aabb_intersects: expected circle for arg 1".into())
            .at(span)
    })?;
    let a = extract_aabb(&args[1]).ok_or_else(|| {
        RuntimeError::new("physics.circle_aabb_intersects: expected aabb for arg 2".into()).at(span)
    })?;
    Ok(Value::Bool(c.intersects_aabb(&a)))
}

fn physics_ray_intersects_aabb(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.ray_intersects_aabb", 2, &args, span)?;
    let ray = extract_ray(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.ray_intersects_aabb: expected ray for arg 1".into()).at(span)
    })?;
    let aabb = extract_aabb(&args[1]).ok_or_else(|| {
        RuntimeError::new("physics.ray_intersects_aabb: expected aabb or body for arg 2".into())
            .at(span)
    })?;
    let hit = ray.intersects_aabb(&aabb);

    let mut map = IndexMap::new();
    map.insert("hit".to_string(), Value::Bool(hit.hit));
    map.insert("dist".to_string(), Value::Float(hit.dist));
    map.insert("normal_x".to_string(), Value::Float(hit.normal.x));
    map.insert("normal_y".to_string(), Value::Float(hit.normal.y));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn physics_ray_intersects_circle(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.ray_intersects_circle", 2, &args, span)?;
    let ray = extract_ray(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.ray_intersects_circle: expected ray for arg 1".into()).at(span)
    })?;
    let circle = extract_circle(&args[1]).ok_or_else(|| {
        RuntimeError::new("physics.ray_intersects_circle: expected circle for arg 2".into())
            .at(span)
    })?;
    let hit = ray.intersects_circle(&circle);

    let mut map = IndexMap::new();
    map.insert("hit".to_string(), Value::Bool(hit.hit));
    map.insert("dist".to_string(), Value::Float(hit.dist));
    map.insert("normal_x".to_string(), Value::Float(hit.normal.x));
    map.insert("normal_y".to_string(), Value::Float(hit.normal.y));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn physics_integrate_body(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("physics.integrate_body: expected body map".into()).at(span));
    }
    let dt = if args.len() > 1 {
        to_f64_val(&args[1]).unwrap_or(1.0 / 60.0)
    } else {
        1.0 / 60.0
    };
    let gravity_y = if args.len() > 2 {
        to_f64_val(&args[2]).unwrap_or(0.0)
    } else {
        0.0
    };

    if let Value::Map(m) = &args[0] {
        let mut map = m.write().unwrap();
        integrate_map_body(&mut map, dt, gravity_y);
        Ok(args[0].clone())
    } else {
        Err(RuntimeError::new("physics.integrate_body: expected body map".into()).at(span))
    }
}

fn physics_resolve_collision(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.resolve_collision", 2, &args, span)?;
    match (&args[0], &args[1]) {
        (Value::Map(ma), Value::Map(mb)) => {
            let mut map_a = ma.write().unwrap();
            let mut map_b = mb.write().unwrap();
            let res = resolve_map_collision(&mut map_a, &mut map_b);
            let mut out = IndexMap::new();
            if let Some((nx, ny, depth)) = res {
                out.insert("collided".to_string(), Value::Bool(true));
                out.insert("normal_x".to_string(), Value::Float(nx));
                out.insert("normal_y".to_string(), Value::Float(ny));
                out.insert("depth".to_string(), Value::Float(depth));
            } else {
                out.insert("collided".to_string(), Value::Bool(false));
                out.insert("normal_x".to_string(), Value::Float(0.0));
                out.insert("normal_y".to_string(), Value::Float(0.0));
                out.insert("depth".to_string(), Value::Float(0.0));
            }
            Ok(Value::Map(Arc::new(RwLock::new(out))))
        }
        _ => Err(
            RuntimeError::new("physics.resolve_collision: expected two body maps".into()).at(span),
        ),
    }
}

fn physics_apply_force(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.apply_force", 3, &args, span)?;
    let fx = to_f64_val(&args[1]).unwrap_or(0.0);
    let fy = to_f64_val(&args[2]).unwrap_or(0.0);

    if let Value::Map(m) = &args[0] {
        let mut map = m.write().unwrap();
        let mass = get_f64_prop(&map, "mass", 1.0);
        if mass > 0.0 {
            let ax = get_f64_prop(&map, "acc_x", 0.0) + fx / mass;
            let ay = get_f64_prop(&map, "acc_y", 0.0) + fy / mass;
            map.insert("acc_x".to_string(), Value::Float(ax));
            map.insert("acc_y".to_string(), Value::Float(ay));
        }
        Ok(args[0].clone())
    } else {
        Err(RuntimeError::new("physics.apply_force: expected body map".into()).at(span))
    }
}

fn physics_apply_impulse(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.apply_impulse", 3, &args, span)?;
    let jx = to_f64_val(&args[1]).unwrap_or(0.0);
    let jy = to_f64_val(&args[2]).unwrap_or(0.0);

    if let Value::Map(m) = &args[0] {
        let mut map = m.write().unwrap();
        let mass = get_f64_prop(&map, "mass", 1.0);
        if mass > 0.0 {
            let vx = get_f64_prop(&map, "vel_x", 0.0) + jx / mass;
            let vy = get_f64_prop(&map, "vel_y", 0.0) + jy / mass;
            map.insert("vel_x".to_string(), Value::Float(vx));
            map.insert("vel_y".to_string(), Value::Float(vy));
        }
        Ok(args[0].clone())
    } else {
        Err(RuntimeError::new("physics.apply_impulse: expected body map".into()).at(span))
    }
}

// ---------------------------------------------------------------------------
// Physics World Simulation Builtins
// ---------------------------------------------------------------------------

fn physics_world_new(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let gravity_y = if !args.is_empty() {
        to_f64_val(&args[0]).unwrap_or(980.0)
    } else {
        980.0
    };

    let id = NEXT_WORLD_ID.fetch_add(1, Ordering::Relaxed);
    let world = PhysicsWorld {
        gravity_y,
        bodies: Vec::new(),
    };
    get_worlds()
        .lock()
        .unwrap()
        .insert(id, Arc::new(RwLock::new(world)));

    let mut map = IndexMap::new();
    map.insert("id".to_string(), Value::Int(id as i64));
    map.insert("gravity_y".to_string(), Value::Float(gravity_y));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn extract_world_id(v: &Value) -> Option<u64> {
    match v {
        Value::Int(i) => Some(*i as u64),
        Value::Map(m) => {
            let map = m.read().unwrap();
            map.get("id").and_then(|id_val| match id_val {
                Value::Int(i) => Some(*i as u64),
                _ => None,
            })
        }
        _ => None,
    }
}

fn physics_world_add_body(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("physics.world_add_body", 2, &args, span)?;
    let wid = extract_world_id(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.world_add_body: invalid world handle".into()).at(span)
    })?;

    let body_arc = if let Value::Map(m) = &args[1] {
        Arc::clone(m)
    } else {
        return Err(RuntimeError::new("physics.world_add_body: expected body map".into()).at(span));
    };

    let world_arc = get_worlds()
        .lock()
        .unwrap()
        .get(&wid)
        .cloned()
        .ok_or_else(|| {
            RuntimeError::new(format!("physics.world_add_body: world {} not found", wid)).at(span)
        })?;

    world_arc.write().unwrap().bodies.push(body_arc);
    Ok(args[1].clone())
}

fn physics_world_step(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("physics.world_step: expected world".into()).at(span));
    }
    let wid = extract_world_id(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.world_step: invalid world handle".into()).at(span)
    })?;
    let dt = if args.len() > 1 {
        to_f64_val(&args[1]).unwrap_or(1.0 / 60.0)
    } else {
        1.0 / 60.0
    };

    let world_arc = get_worlds()
        .lock()
        .unwrap()
        .get(&wid)
        .cloned()
        .ok_or_else(|| {
            RuntimeError::new(format!("physics.world_step: world {} not found", wid)).at(span)
        })?;

    let (gravity_y, bodies) = {
        let w = world_arc.read().unwrap();
        (w.gravity_y, w.bodies.clone())
    };

    // 1. Kinematics integration step
    for body_arc in &bodies {
        let mut map = body_arc.write().unwrap();
        integrate_map_body(&mut map, dt, gravity_y);
    }

    // 2. Collision detection & resolution (pairwise)
    let len = bodies.len();
    for i in 0..len {
        for j in (i + 1)..len {
            let mut map_a = bodies[i].write().unwrap();
            let mut map_b = bodies[j].write().unwrap();
            resolve_map_collision(&mut map_a, &mut map_b);
        }
    }

    Ok(Value::Null)
}

fn physics_world_get_bodies(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("physics.world_get_bodies: expected world".into()).at(span));
    }
    let wid = extract_world_id(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.world_get_bodies: invalid world handle".into()).at(span)
    })?;

    let world_arc = get_worlds()
        .lock()
        .unwrap()
        .get(&wid)
        .cloned()
        .ok_or_else(|| {
            RuntimeError::new(format!("physics.world_get_bodies: world {} not found", wid)).at(span)
        })?;

    let bodies = world_arc.read().unwrap().bodies.clone();
    let body_values: Vec<Value> = bodies.into_iter().map(Value::Map).collect();
    Ok(Value::Array(Arc::new(RwLock::new(body_values))))
}

fn physics_world_clear(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("physics.world_clear: expected world".into()).at(span));
    }
    let wid = extract_world_id(&args[0]).ok_or_else(|| {
        RuntimeError::new("physics.world_clear: invalid world handle".into()).at(span)
    })?;

    let world_arc = get_worlds()
        .lock()
        .unwrap()
        .get(&wid)
        .cloned()
        .ok_or_else(|| {
            RuntimeError::new(format!("physics.world_clear: world {} not found", wid)).at(span)
        })?;

    world_arc.write().unwrap().bodies.clear();
    Ok(Value::Null)
}

// ---------------------------------------------------------------------------
// Module Export Function
// ---------------------------------------------------------------------------

pub fn build_physics_module() -> Value {
    let mut map = IndexMap::new();
    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        // Constructors
        ("aabb", physics_aabb),
        ("circle", physics_circle),
        ("point", physics_point),
        ("ray", physics_ray),
        ("body", physics_body),
        // Intersections
        ("aabb_intersects", physics_aabb_intersects),
        ("aabb_contains_point", physics_aabb_contains_point),
        ("circle_intersects", physics_circle_intersects),
        ("circle_contains_point", physics_circle_contains_point),
        ("circle_aabb_intersects", physics_circle_aabb_intersects),
        ("ray_intersects_aabb", physics_ray_intersects_aabb),
        ("ray_intersects_circle", physics_ray_intersects_circle),
        // Dynamics
        ("integrate_body", physics_integrate_body),
        ("resolve_collision", physics_resolve_collision),
        ("apply_force", physics_apply_force),
        ("apply_impulse", physics_apply_impulse),
        // World
        ("world_new", physics_world_new),
        ("world_add_body", physics_world_add_body),
        ("world_step", physics_world_step),
        ("world_get_bodies", physics_world_get_bodies),
        ("world_clear", physics_world_clear),
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
