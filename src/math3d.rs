// Shae 3D Math Engine: High-Performance Vector, Matrix, Quaternion & Spatial Math
// Part of Option 3: Hardware-Accelerated Graphics & 3D Engine

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::sync::{Arc, RwLock};

fn val_to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Float(n) | Value::Number(n) => Some(*n),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn to_f64(v: &Value, fn_name: &str, span: Span) -> Result<f64, RuntimeError> {
    val_to_f64(v).ok_or_else(|| {
        RuntimeError::new(format!("{}: expected number, got {}", fn_name, v.type_name())).at(span)
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    pub const ONE: Self = Self { x: 1.0, y: 1.0, z: 1.0 };
    pub const UP: Self = Self { x: 0.0, y: 1.0, z: 0.0 };
    pub const DOWN: Self = Self { x: 0.0, y: -1.0, z: 0.0 };
    pub const FORWARD: Self = Self { x: 0.0, y: 0.0, z: -1.0 };
    pub const BACK: Self = Self { x: 0.0, y: 0.0, z: 1.0 };
    pub const LEFT: Self = Self { x: -1.0, y: 0.0, z: 0.0 };
    pub const RIGHT: Self = Self { x: 1.0, y: 0.0, z: 0.0 };

    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn add(&self, other: &Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    pub fn sub(&self, other: &Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    pub fn mul_scalar(&self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }

    pub fn div_scalar(&self, s: f64) -> Self {
        if s == 0.0 {
            Self::ZERO
        } else {
            Self::new(self.x / s, self.y / s, self.z / s)
        }
    }

    pub fn dot(&self, other: &Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(&self, other: &Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn length_squared(&self) -> f64 {
        self.dot(self)
    }

    pub fn length(&self) -> f64 {
        self.length_squared().sqrt()
    }

    pub fn normalize(&self) -> Self {
        let len = self.length();
        if len > 0.0 {
            self.div_scalar(len)
        } else {
            Self::ZERO
        }
    }

    pub fn distance(&self, other: &Self) -> f64 {
        self.sub(other).length()
    }

    pub fn lerp(&self, other: &Self, t: f64) -> Self {
        Self::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
            self.z + (other.z - self.z) * t,
        )
    }

    pub fn reflect(&self, normal: &Self) -> Self {
        self.sub(&normal.mul_scalar(2.0 * self.dot(normal)))
    }

    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("x".to_string(), Value::Float(self.x));
        map.insert("y".to_string(), Value::Float(self.y));
        map.insert("z".to_string(), Value::Float(self.z));
        Value::Map(Arc::new(RwLock::new(map)))
    }

    pub fn from_shae_value(v: &Value, fn_name: &str, span: Span) -> Result<Self, RuntimeError> {
        match v {
            Value::Map(map_lock) => {
                let m = map_lock.read().unwrap();
                let x = m.get("x").and_then(val_to_f64).unwrap_or(0.0);
                let y = m.get("y").and_then(val_to_f64).unwrap_or(0.0);
                let z = m.get("z").and_then(val_to_f64).unwrap_or(0.0);
                Ok(Self::new(x, y, z))
            }
            Value::Array(arr_lock) => {
                let l = arr_lock.read().unwrap();
                if l.len() >= 3 {
                    let x = val_to_f64(&l[0]).unwrap_or(0.0);
                    let y = val_to_f64(&l[1]).unwrap_or(0.0);
                    let z = val_to_f64(&l[2]).unwrap_or(0.0);
                    Ok(Self::new(x, y, z))
                } else {
                    Err(RuntimeError::new(format!("{}: vec3 array requires at least 3 elements", fn_name)).at(span))
                }
            }
            other => Err(RuntimeError::new(format!("{}: expected Vec3 map or list, got {}", fn_name, other.type_name())).at(span)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec4 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Vec4 {
    pub fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    pub fn dot(&self, other: &Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("x".to_string(), Value::Float(self.x));
        map.insert("y".to_string(), Value::Float(self.y));
        map.insert("z".to_string(), Value::Float(self.z));
        map.insert("w".to_string(), Value::Float(self.w));
        Value::Map(Arc::new(RwLock::new(map)))
    }
}

/// 4x4 Transformation Matrix in Row-Major order
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    pub m: [f64; 16],
}

impl Mat4 {
    pub const IDENTITY: Self = Self {
        m: [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    pub fn identity() -> Self {
        Self::IDENTITY
    }

    pub fn translation(x: f64, y: f64, z: f64) -> Self {
        Self {
            m: [
                1.0, 0.0, 0.0, x,
                0.0, 1.0, 0.0, y,
                0.0, 0.0, 1.0, z,
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    pub fn scaling(x: f64, y: f64, z: f64) -> Self {
        Self {
            m: [
                x,   0.0, 0.0, 0.0,
                0.0, y,   0.0, 0.0,
                0.0, 0.0, z,   0.0,
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    pub fn rotation_x(rad: f64) -> Self {
        let c = rad.cos();
        let s = rad.sin();
        Self {
            m: [
                1.0, 0.0,  0.0, 0.0,
                0.0, c,   -s,   0.0,
                0.0, s,    c,   0.0,
                0.0, 0.0,  0.0, 1.0,
            ],
        }
    }

    pub fn rotation_y(rad: f64) -> Self {
        let c = rad.cos();
        let s = rad.sin();
        Self {
            m: [
                c,   0.0, s,   0.0,
                0.0, 1.0, 0.0, 0.0,
                -s,  0.0, c,   0.0,
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    pub fn rotation_z(rad: f64) -> Self {
        let c = rad.cos();
        let s = rad.sin();
        Self {
            m: [
                c,   -s,  0.0, 0.0,
                s,    c,  0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    pub fn rotation_axis(axis: &Vec3, rad: f64) -> Self {
        let a = axis.normalize();
        let c = rad.cos();
        let s = rad.sin();
        let omc = 1.0 - c;

        Self {
            m: [
                c + a.x * a.x * omc,       a.x * a.y * omc - a.z * s, a.x * a.z * omc + a.y * s, 0.0,
                a.y * a.x * omc + a.z * s, c + a.y * a.y * omc,       a.y * a.z * omc - a.x * s, 0.0,
                a.z * a.x * omc - a.y * s, a.z * a.y * omc + a.x * s, c + a.z * a.z * omc,       0.0,
                0.0,                       0.0,                       0.0,                       1.0,
            ],
        }
    }

    pub fn perspective(fov_rad: f64, aspect: f64, near: f64, far: f64) -> Self {
        let f = 1.0 / (fov_rad * 0.5).tan();
        let nf = 1.0 / (near - far);
        Self {
            m: [
                f / aspect, 0.0, 0.0,               0.0,
                0.0,        f,   0.0,               0.0,
                0.0,        0.0, (far + near) * nf, 2.0 * far * near * nf,
                0.0,        0.0, -1.0,              0.0,
            ],
        }
    }

    pub fn orthographic(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) -> Self {
        let rl = 1.0 / (right - left);
        let tb = 1.0 / (top - bottom);
        let fn_inv = 1.0 / (far - near);
        Self {
            m: [
                2.0 * rl, 0.0,      0.0,        -(right + left) * rl,
                0.0,      2.0 * tb, 0.0,        -(top + bottom) * tb,
                0.0,      0.0,      -2.0 * fn_inv, -(far + near) * fn_inv,
                0.0,      0.0,      0.0,        1.0,
            ],
        }
    }

    pub fn look_at(eye: &Vec3, target: &Vec3, up: &Vec3) -> Self {
        let z_axis = eye.sub(target).normalize(); // forward
        let x_axis = up.cross(&z_axis).normalize(); // right
        let y_axis = z_axis.cross(&x_axis).normalize(); // up

        Self {
            m: [
                x_axis.x, x_axis.y, x_axis.z, -x_axis.dot(eye),
                y_axis.x, y_axis.y, y_axis.z, -y_axis.dot(eye),
                z_axis.x, z_axis.y, z_axis.z, -z_axis.dot(eye),
                0.0,      0.0,      0.0,       1.0,
            ],
        }
    }

    pub fn mul(&self, other: &Self) -> Self {
        let mut res = [0.0; 16];
        for row in 0..4 {
            for col in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += self.m[row * 4 + k] * other.m[k * 4 + col];
                }
                res[row * 4 + col] = sum;
            }
        }
        Self { m: res }
    }

    pub fn transform_point(&self, p: &Vec3) -> Vec3 {
        let x = self.m[0] * p.x + self.m[1] * p.y + self.m[2] * p.z + self.m[3];
        let y = self.m[4] * p.x + self.m[5] * p.y + self.m[6] * p.z + self.m[7];
        let z = self.m[8] * p.x + self.m[9] * p.y + self.m[10] * p.z + self.m[11];
        let w = self.m[12] * p.x + self.m[13] * p.y + self.m[14] * p.z + self.m[15];
        if w != 0.0 && w != 1.0 {
            Vec3::new(x / w, y / w, z / w)
        } else {
            Vec3::new(x, y, z)
        }
    }

    pub fn transform_vector(&self, v: &Vec3) -> Vec3 {
        let x = self.m[0] * v.x + self.m[1] * v.y + self.m[2] * v.z;
        let y = self.m[4] * v.x + self.m[5] * v.y + self.m[6] * v.z;
        let z = self.m[8] * v.x + self.m[9] * v.y + self.m[10] * v.z;
        Vec3::new(x, y, z)
    }

    pub fn transpose(&self) -> Self {
        let mut res = [0.0; 16];
        for r in 0..4 {
            for c in 0..4 {
                res[c * 4 + r] = self.m[r * 4 + c];
            }
        }
        Self { m: res }
    }

    pub fn inverse(&self) -> Option<Self> {
        let m = &self.m;
        let mut inv = [0.0; 16];

        inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
            + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
        inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
            - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
        inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
            + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
        inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
            - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];

        let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
        if det.abs() < 1e-12 {
            return None;
        }

        inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
            - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
        inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
            + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
        inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
            - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
        inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
            + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];

        inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
            + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
        inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
            - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
        inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
            + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
        inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
            - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];

        inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
            - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
        inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
            + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
        inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
            - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
        inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
            + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];

        let inv_det = 1.0 / det;
        for i in 0..16 {
            inv[i] *= inv_det;
        }

        Some(Self { m: inv })
    }

    pub fn to_shae_value(&self) -> Value {
        let list: Vec<Value> = self.m.iter().map(|&f| Value::Float(f)).collect();
        Value::Array(Arc::new(RwLock::new(list)))
    }

    pub fn from_shae_value(v: &Value, fn_name: &str, span: Span) -> Result<Self, RuntimeError> {
        match v {
            Value::Array(arr_lock) => {
                let l = arr_lock.read().unwrap();
                if l.len() >= 16 {
                    let mut m = [0.0; 16];
                    for i in 0..16 {
                        m[i] = val_to_f64(&l[i]).unwrap_or(0.0);
                    }
                    Ok(Self { m })
                } else {
                    Err(RuntimeError::new(format!("{}: mat4 array requires 16 elements, got {}", fn_name, l.len())).at(span))
                }
            }
            other => Err(RuntimeError::new(format!("{}: expected mat4 list, got {}", fn_name, other.type_name())).at(span)),
        }
    }
}

/// Unit Quaternion for 3D Rotations
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Quat {
    pub const IDENTITY: Self = Self { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    pub fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    pub fn from_axis_angle(axis: &Vec3, rad: f64) -> Self {
        let half = rad * 0.5;
        let s = half.sin();
        let a = axis.normalize();
        Self::new(a.x * s, a.y * s, a.z * s, half.cos())
    }

    pub fn from_euler(pitch: f64, yaw: f64, roll: f64) -> Self {
        let cy = (yaw * 0.5).cos();
        let sy = (yaw * 0.5).sin();
        let cp = (pitch * 0.5).cos();
        let sp = (pitch * 0.5).sin();
        let cr = (roll * 0.5).cos();
        let sr = (roll * 0.5).sin();

        Self::new(
            sr * cp * cy - cr * sp * sy,
            cr * sp * cy + sr * cp * sy,
            cr * cp * sy - sr * sp * cy,
            cr * cp * cy + sr * sp * sy,
        )
    }

    pub fn mul(&self, other: &Self) -> Self {
        Self::new(
            self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y,
            self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x,
            self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w,
            self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z,
        )
    }

    pub fn rotate_vec3(&self, v: &Vec3) -> Vec3 {
        let q_vec = Vec3::new(self.x, self.y, self.z);
        let t = q_vec.cross(v).mul_scalar(2.0);
        v.add(&t.mul_scalar(self.w)).add(&q_vec.cross(&t))
    }

    pub fn normalize(&self) -> Self {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if len > 0.0 {
            Self::new(self.x / len, self.y / len, self.z / len, self.w / len)
        } else {
            Self::IDENTITY
        }
    }

    pub fn slerp(&self, other: &Self, t: f64) -> Self {
        let mut cos_theta = self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w;
        let mut target = *other;

        if cos_theta < 0.0 {
            cos_theta = -cos_theta;
            target = Self::new(-target.x, -target.y, -target.z, -target.w);
        }

        if cos_theta > 0.9995 {
            // Linear interpolation for near angles
            return Self::new(
                self.x + t * (target.x - self.x),
                self.y + t * (target.y - self.y),
                self.z + t * (target.z - self.z),
                self.w + t * (target.w - self.w),
            ).normalize();
        }

        let theta = cos_theta.acos();
        let sin_theta = theta.sin();
        let scale0 = ((1.0 - t) * theta).sin() / sin_theta;
        let scale1 = (t * theta).sin() / sin_theta;

        Self::new(
            scale0 * self.x + scale1 * target.x,
            scale0 * self.y + scale1 * target.y,
            scale0 * self.z + scale1 * target.z,
            scale0 * self.w + scale1 * target.w,
        )
    }

    pub fn to_mat4(&self) -> Mat4 {
        let x2 = self.x + self.x;
        let y2 = self.y + self.y;
        let z2 = self.z + self.z;
        let xx = self.x * x2;
        let xy = self.x * y2;
        let xz = self.x * z2;
        let yy = self.y * y2;
        let yz = self.y * z2;
        let zz = self.z * z2;
        let wx = self.w * x2;
        let wy = self.w * y2;
        let wz = self.w * z2;

        Mat4 {
            m: [
                1.0 - (yy + zz), xy - wz,         xz + wy,         0.0,
                xy + wz,         1.0 - (xx + zz), yz - wx,         0.0,
                xz - wy,         yz + wx,         1.0 - (xx + yy), 0.0,
                0.0,             0.0,             0.0,             1.0,
            ],
        }
    }

    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("x".to_string(), Value::Float(self.x));
        map.insert("y".to_string(), Value::Float(self.y));
        map.insert("z".to_string(), Value::Float(self.z));
        map.insert("w".to_string(), Value::Float(self.w));
        Value::Map(Arc::new(RwLock::new(map)))
    }
}

/// 3D Spatial Ray
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    pub fn new(origin: Vec3, direction: Vec3) -> Self {
        Self {
            origin,
            direction: direction.normalize(),
        }
    }

    /// Möller–Trumbore ray-triangle intersection algorithm
    /// Returns Some((distance_t, barycentric_u, barycentric_v))
    pub fn intersect_triangle(&self, v0: &Vec3, v1: &Vec3, v2: &Vec3) -> Option<(f64, f64, f64)> {
        const EPSILON: f64 = 1e-8;
        let edge1 = v1.sub(v0);
        let edge2 = v2.sub(v0);
        let h = self.direction.cross(&edge2);
        let a = edge1.dot(&h);

        if a > -EPSILON && a < EPSILON {
            return None; // Parallel to triangle
        }

        let f = 1.0 / a;
        let s = self.origin.sub(v0);
        let u = f * s.dot(&h);

        if u < 0.0 || u > 1.0 {
            return None;
        }

        let q = s.cross(&edge1);
        let v = f * self.direction.dot(&q);

        if v < 0.0 || u + v > 1.0 {
            return None;
        }

        let t = f * edge2.dot(&q);
        if t > EPSILON {
            Some((t, u, v))
        } else {
            None
        }
    }

    pub fn intersect_aabb(&self, min: &Vec3, max: &Vec3) -> Option<f64> {
        let mut tmin = (min.x - self.origin.x) / self.direction.x;
        let mut tmax = (max.x - self.origin.x) / self.direction.x;

        if tmin > tmax {
            std::mem::swap(&mut tmin, &mut tmax);
        }

        let mut tymin = (min.y - self.origin.y) / self.direction.y;
        let mut tymax = (max.y - self.origin.y) / self.direction.y;

        if tymin > tymax {
            std::mem::swap(&mut tymin, &mut tymax);
        }

        if (tmin > tymax) || (tymin > tmax) {
            return None;
        }

        if tymin > tmin {
            tmin = tymin;
        }

        if tymax < tmax {
            tmax = tymax;
        }

        let mut tzmin = (min.z - self.origin.z) / self.direction.z;
        let mut tzmax = (max.z - self.origin.z) / self.direction.z;

        if tzmin > tzmax {
            std::mem::swap(&mut tzmin, &mut tzmax);
        }

        if (tmin > tzmax) || (tzmin > tmax) {
            return None;
        }

        if tzmin > tmin {
            tmin = tzmin;
        }

        if tmin < 0.0 {
            if tmax < 0.0 { return None; }
            Some(tmax)
        } else {
            Some(tmin)
        }
    }
}

// --- Standard Library Module Construction ---

pub fn build_math3d_module() -> Value {
    let mut map = IndexMap::new();

    map.insert("vec3".to_string(), Value::Builtin {
        name: "math3d.vec3".to_string(),
        func: shae_vec3,
    });
    map.insert("vec3_add".to_string(), Value::Builtin {
        name: "math3d.vec3_add".to_string(),
        func: shae_vec3_add,
    });
    map.insert("vec3_sub".to_string(), Value::Builtin {
        name: "math3d.vec3_sub".to_string(),
        func: shae_vec3_sub,
    });
    map.insert("vec3_dot".to_string(), Value::Builtin {
        name: "math3d.vec3_dot".to_string(),
        func: shae_vec3_dot,
    });
    map.insert("vec3_cross".to_string(), Value::Builtin {
        name: "math3d.vec3_cross".to_string(),
        func: shae_vec3_cross,
    });
    map.insert("vec3_length".to_string(), Value::Builtin {
        name: "math3d.vec3_length".to_string(),
        func: shae_vec3_length,
    });
    map.insert("vec3_normalize".to_string(), Value::Builtin {
        name: "math3d.vec3_normalize".to_string(),
        func: shae_vec3_normalize,
    });
    map.insert("vec3_distance".to_string(), Value::Builtin {
        name: "math3d.vec3_distance".to_string(),
        func: shae_vec3_distance,
    });
    map.insert("vec3_lerp".to_string(), Value::Builtin {
        name: "math3d.vec3_lerp".to_string(),
        func: shae_vec3_lerp,
    });

    // Matrix Functions
    map.insert("mat4_identity".to_string(), Value::Builtin {
        name: "math3d.mat4_identity".to_string(),
        func: shae_mat4_identity,
    });
    map.insert("mat4_translation".to_string(), Value::Builtin {
        name: "math3d.mat4_translation".to_string(),
        func: shae_mat4_translation,
    });
    map.insert("mat4_scaling".to_string(), Value::Builtin {
        name: "math3d.mat4_scaling".to_string(),
        func: shae_mat4_scaling,
    });
    map.insert("mat4_rotation_x".to_string(), Value::Builtin {
        name: "math3d.mat4_rotation_x".to_string(),
        func: shae_mat4_rotation_x,
    });
    map.insert("mat4_rotation_y".to_string(), Value::Builtin {
        name: "math3d.mat4_rotation_y".to_string(),
        func: shae_mat4_rotation_y,
    });
    map.insert("mat4_rotation_z".to_string(), Value::Builtin {
        name: "math3d.mat4_rotation_z".to_string(),
        func: shae_mat4_rotation_z,
    });
    map.insert("mat4_perspective".to_string(), Value::Builtin {
        name: "math3d.mat4_perspective".to_string(),
        func: shae_mat4_perspective,
    });
    
    // Ergonomic Matrix & Math Aliases
    map.insert("look_at".to_string(), Value::Builtin {
        name: "math3d.look_at".to_string(),
        func: shae_mat4_look_at,
    });
    map.insert("perspective".to_string(), Value::Builtin {
        name: "math3d.perspective".to_string(),
        func: shae_mat4_perspective,
    });
    map.insert("translation".to_string(), Value::Builtin {
        name: "math3d.translation".to_string(),
        func: shae_mat4_translation,
    });
    map.insert("scaling".to_string(), Value::Builtin {
        name: "math3d.scaling".to_string(),
        func: shae_mat4_scaling,
    });
    map.insert("identity".to_string(), Value::Builtin {
        name: "math3d.identity".to_string(),
        func: shae_mat4_identity,
    });
    map.insert("mat4_look_at".to_string(), Value::Builtin {
        name: "math3d.mat4_look_at".to_string(),
        func: shae_mat4_look_at,
    });
    map.insert("mat4_mul".to_string(), Value::Builtin {
        name: "math3d.mat4_mul".to_string(),
        func: shae_mat4_mul,
    });
    map.insert("mat4_transform_point".to_string(), Value::Builtin {
        name: "math3d.mat4_transform_point".to_string(),
        func: shae_mat4_transform_point,
    });
    map.insert("mat4_inverse".to_string(), Value::Builtin {
        name: "math3d.mat4_inverse".to_string(),
        func: shae_mat4_inverse,
    });

    // Quaternions
    map.insert("quat_from_axis_angle".to_string(), Value::Builtin {
        name: "math3d.quat_from_axis_angle".to_string(),
        func: shae_quat_from_axis_angle,
    });
    map.insert("quat_slerp".to_string(), Value::Builtin {
        name: "math3d.quat_slerp".to_string(),
        func: shae_quat_slerp,
    });
    map.insert("quat_to_mat4".to_string(), Value::Builtin {
        name: "math3d.quat_to_mat4".to_string(),
        func: shae_quat_to_mat4,
    });

    // Ray & Intersection
    map.insert("ray".to_string(), Value::Builtin {
        name: "math3d.ray".to_string(),
        func: shae_ray,
    });
    map.insert("ray_intersect_triangle".to_string(), Value::Builtin {
        name: "math3d.ray_intersect_triangle".to_string(),
        func: shae_ray_intersect_triangle,
    });
    map.insert("ray_intersect_aabb".to_string(), Value::Builtin {
        name: "math3d.ray_intersect_aabb".to_string(),
        func: shae_ray_intersect_aabb,
    });

    Value::Map(Arc::new(RwLock::new(map)))
}

fn shae_vec3(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.vec3", 3, &args, span)?;
    let x = to_f64(&args[0], "math3d.vec3", span)?;
    let y = to_f64(&args[1], "math3d.vec3", span)?;
    let z = to_f64(&args[2], "math3d.vec3", span)?;
    Ok(Vec3::new(x, y, z).to_shae_value())
}

fn shae_vec3_add(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.vec3_add", 2, &args, span)?;
    let v1 = Vec3::from_shae_value(&args[0], "math3d.vec3_add", span)?;
    let v2 = Vec3::from_shae_value(&args[1], "math3d.vec3_add", span)?;
    Ok(v1.add(&v2).to_shae_value())
}

fn shae_vec3_sub(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.vec3_sub", 2, &args, span)?;
    let v1 = Vec3::from_shae_value(&args[0], "math3d.vec3_sub", span)?;
    let v2 = Vec3::from_shae_value(&args[1], "math3d.vec3_sub", span)?;
    Ok(v1.sub(&v2).to_shae_value())
}

fn shae_vec3_dot(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.vec3_dot", 2, &args, span)?;
    let v1 = Vec3::from_shae_value(&args[0], "math3d.vec3_dot", span)?;
    let v2 = Vec3::from_shae_value(&args[1], "math3d.vec3_dot", span)?;
    Ok(Value::Float(v1.dot(&v2)))
}

fn shae_vec3_cross(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.vec3_cross", 2, &args, span)?;
    let v1 = Vec3::from_shae_value(&args[0], "math3d.vec3_cross", span)?;
    let v2 = Vec3::from_shae_value(&args[1], "math3d.vec3_cross", span)?;
    Ok(v1.cross(&v2).to_shae_value())
}

fn shae_vec3_length(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.vec3_length", 1, &args, span)?;
    let v = Vec3::from_shae_value(&args[0], "math3d.vec3_length", span)?;
    Ok(Value::Float(v.length()))
}

fn shae_vec3_normalize(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.vec3_normalize", 1, &args, span)?;
    let v = Vec3::from_shae_value(&args[0], "math3d.vec3_normalize", span)?;
    Ok(v.normalize().to_shae_value())
}

fn shae_vec3_distance(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.vec3_distance", 2, &args, span)?;
    let v1 = Vec3::from_shae_value(&args[0], "math3d.vec3_distance", span)?;
    let v2 = Vec3::from_shae_value(&args[1], "math3d.vec3_distance", span)?;
    Ok(Value::Float(v1.distance(&v2)))
}

fn shae_vec3_lerp(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.vec3_lerp", 3, &args, span)?;
    let v1 = Vec3::from_shae_value(&args[0], "math3d.vec3_lerp", span)?;
    let v2 = Vec3::from_shae_value(&args[1], "math3d.vec3_lerp", span)?;
    let t = to_f64(&args[2], "math3d.vec3_lerp", span)?;
    Ok(v1.lerp(&v2, t).to_shae_value())
}

fn shae_mat4_identity(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    Ok(Mat4::identity().to_shae_value())
}

fn shae_mat4_translation(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.mat4_translation", 3, &args, span)?;
    let x = to_f64(&args[0], "math3d.mat4_translation", span)?;
    let y = to_f64(&args[1], "math3d.mat4_translation", span)?;
    let z = to_f64(&args[2], "math3d.mat4_translation", span)?;
    Ok(Mat4::translation(x, y, z).to_shae_value())
}

fn shae_mat4_scaling(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.mat4_scaling", 3, &args, span)?;
    let x = to_f64(&args[0], "math3d.mat4_scaling", span)?;
    let y = to_f64(&args[1], "math3d.mat4_scaling", span)?;
    let z = to_f64(&args[2], "math3d.mat4_scaling", span)?;
    Ok(Mat4::scaling(x, y, z).to_shae_value())
}

fn shae_mat4_rotation_x(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.mat4_rotation_x", 1, &args, span)?;
    let rad = to_f64(&args[0], "math3d.mat4_rotation_x", span)?;
    Ok(Mat4::rotation_x(rad).to_shae_value())
}

fn shae_mat4_rotation_y(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.mat4_rotation_y", 1, &args, span)?;
    let rad = to_f64(&args[0], "math3d.mat4_rotation_y", span)?;
    Ok(Mat4::rotation_y(rad).to_shae_value())
}

fn shae_mat4_rotation_z(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.mat4_rotation_z", 1, &args, span)?;
    let rad = to_f64(&args[0], "math3d.mat4_rotation_z", span)?;
    Ok(Mat4::rotation_z(rad).to_shae_value())
}

fn shae_mat4_perspective(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.mat4_perspective", 4, &args, span)?;
    let fov = to_f64(&args[0], "math3d.mat4_perspective", span)?;
    let aspect = to_f64(&args[1], "math3d.mat4_perspective", span)?;
    let near = to_f64(&args[2], "math3d.mat4_perspective", span)?;
    let far = to_f64(&args[3], "math3d.mat4_perspective", span)?;
    Ok(Mat4::perspective(fov, aspect, near, far).to_shae_value())
}

fn shae_mat4_look_at(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.mat4_look_at", 3, &args, span)?;
    let eye = Vec3::from_shae_value(&args[0], "math3d.mat4_look_at", span)?;
    let target = Vec3::from_shae_value(&args[1], "math3d.mat4_look_at", span)?;
    let up = Vec3::from_shae_value(&args[2], "math3d.mat4_look_at", span)?;
    Ok(Mat4::look_at(&eye, &target, &up).to_shae_value())
}

fn shae_mat4_mul(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.mat4_mul", 2, &args, span)?;
    let m1 = Mat4::from_shae_value(&args[0], "math3d.mat4_mul", span)?;
    let m2 = Mat4::from_shae_value(&args[1], "math3d.mat4_mul", span)?;
    Ok(m1.mul(&m2).to_shae_value())
}

fn shae_mat4_transform_point(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.mat4_transform_point", 2, &args, span)?;
    let m = Mat4::from_shae_value(&args[0], "math3d.mat4_transform_point", span)?;
    let p = Vec3::from_shae_value(&args[1], "math3d.mat4_transform_point", span)?;
    Ok(m.transform_point(&p).to_shae_value())
}

fn shae_mat4_inverse(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.mat4_inverse", 1, &args, span)?;
    let m = Mat4::from_shae_value(&args[0], "math3d.mat4_inverse", span)?;
    match m.inverse() {
        Some(inv) => Ok(inv.to_shae_value()),
        None => Ok(Value::Null),
    }
}

fn shae_quat_from_axis_angle(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.quat_from_axis_angle", 2, &args, span)?;
    let axis = Vec3::from_shae_value(&args[0], "math3d.quat_from_axis_angle", span)?;
    let rad = to_f64(&args[1], "math3d.quat_from_axis_angle", span)?;
    Ok(Quat::from_axis_angle(&axis, rad).to_shae_value())
}

fn shae_quat_slerp(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.quat_slerp", 3, &args, span)?;
    let q1 = match &args[0] {
        Value::Map(m) => {
            let g = m.read().unwrap();
            Quat::new(
                g.get("x").and_then(val_to_f64).unwrap_or(0.0),
                g.get("y").and_then(val_to_f64).unwrap_or(0.0),
                g.get("z").and_then(val_to_f64).unwrap_or(0.0),
                g.get("w").and_then(val_to_f64).unwrap_or(1.0),
            )
        }
        _ => return Err(RuntimeError::new("expected quat map".to_string()).at(span)),
    };
    let q2 = match &args[1] {
        Value::Map(m) => {
            let g = m.read().unwrap();
            Quat::new(
                g.get("x").and_then(val_to_f64).unwrap_or(0.0),
                g.get("y").and_then(val_to_f64).unwrap_or(0.0),
                g.get("z").and_then(val_to_f64).unwrap_or(0.0),
                g.get("w").and_then(val_to_f64).unwrap_or(1.0),
            )
        }
        _ => return Err(RuntimeError::new("expected quat map".to_string()).at(span)),
    };
    let t = to_f64(&args[2], "math3d.quat_slerp", span)?;
    Ok(q1.slerp(&q2, t).to_shae_value())
}

fn shae_quat_to_mat4(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.quat_to_mat4", 1, &args, span)?;
    let q = match &args[0] {
        Value::Map(m) => {
            let g = m.read().unwrap();
            Quat::new(
                g.get("x").and_then(val_to_f64).unwrap_or(0.0),
                g.get("y").and_then(val_to_f64).unwrap_or(0.0),
                g.get("z").and_then(val_to_f64).unwrap_or(0.0),
                g.get("w").and_then(val_to_f64).unwrap_or(1.0),
            )
        }
        _ => return Err(RuntimeError::new("expected quat map".to_string()).at(span)),
    };
    Ok(q.to_mat4().to_shae_value())
}

fn shae_ray(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.ray", 2, &args, span)?;
    let origin = Vec3::from_shae_value(&args[0], "math3d.ray", span)?;
    let dir = Vec3::from_shae_value(&args[1], "math3d.ray", span)?;
    let ray = Ray::new(origin, dir);

    let mut map = IndexMap::new();
    map.insert("origin".to_string(), ray.origin.to_shae_value());
    map.insert("direction".to_string(), ray.direction.to_shae_value());
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn shae_ray_intersect_triangle(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.ray_intersect_triangle", 4, &args, span)?;
    let (origin, dir) = match &args[0] {
        Value::Map(m) => {
            let g = m.read().unwrap();
            let o = g.get("origin").ok_or_else(|| RuntimeError::new("ray must have origin".to_string()).at(span))?;
            let d = g.get("direction").ok_or_else(|| RuntimeError::new("ray must have direction".to_string()).at(span))?;
            (Vec3::from_shae_value(o, "ray.origin", span)?, Vec3::from_shae_value(d, "ray.direction", span)?)
        }
        _ => return Err(RuntimeError::new("expected ray map".to_string()).at(span)),
    };
    let ray = Ray::new(origin, dir);
    let v0 = Vec3::from_shae_value(&args[1], "math3d.ray_intersect_triangle", span)?;
    let v1 = Vec3::from_shae_value(&args[2], "math3d.ray_intersect_triangle", span)?;
    let v2 = Vec3::from_shae_value(&args[3], "math3d.ray_intersect_triangle", span)?;

    match ray.intersect_triangle(&v0, &v1, &v2) {
        Some((t, u, v)) => {
            let mut res = IndexMap::new();
            res.insert("hit".to_string(), Value::Bool(true));
            res.insert("distance".to_string(), Value::Float(t));
            res.insert("u".to_string(), Value::Float(u));
            res.insert("v".to_string(), Value::Float(v));
            let hit_point = ray.origin.add(&ray.direction.mul_scalar(t));
            res.insert("point".to_string(), hit_point.to_shae_value());
            Ok(Value::Map(Arc::new(RwLock::new(res))))
        }
        None => {
            let mut res = IndexMap::new();
            res.insert("hit".to_string(), Value::Bool(false));
            Ok(Value::Map(Arc::new(RwLock::new(res))))
        }
    }
}

fn shae_ray_intersect_aabb(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("math3d.ray_intersect_aabb", 3, &args, span)?;
    let (origin, dir) = match &args[0] {
        Value::Map(m) => {
            let g = m.read().unwrap();
            let o = g.get("origin").ok_or_else(|| RuntimeError::new("ray must have origin".to_string()).at(span))?;
            let d = g.get("direction").ok_or_else(|| RuntimeError::new("ray must have direction".to_string()).at(span))?;
            (Vec3::from_shae_value(o, "ray.origin", span)?, Vec3::from_shae_value(d, "ray.direction", span)?)
        }
        _ => return Err(RuntimeError::new("expected ray map".to_string()).at(span)),
    };
    let ray = Ray::new(origin, dir);
    let min = Vec3::from_shae_value(&args[1], "math3d.ray_intersect_aabb", span)?;
    let max = Vec3::from_shae_value(&args[2], "math3d.ray_intersect_aabb", span)?;

    match ray.intersect_aabb(&min, &max) {
        Some(t) => {
            let mut res = IndexMap::new();
            res.insert("hit".to_string(), Value::Bool(true));
            res.insert("distance".to_string(), Value::Float(t));
            let hit_point = ray.origin.add(&ray.direction.mul_scalar(t));
            res.insert("point".to_string(), hit_point.to_shae_value());
            Ok(Value::Map(Arc::new(RwLock::new(res))))
        }
        None => {
            let mut res = IndexMap::new();
            res.insert("hit".to_string(), Value::Bool(false));
            Ok(Value::Map(Arc::new(RwLock::new(res))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec3_basic_ops() {
        let v1 = Vec3::new(1.0, 2.0, 3.0);
        let v2 = Vec3::new(4.0, 5.0, 6.0);

        assert_eq!(v1.add(&v2), Vec3::new(5.0, 7.0, 9.0));
        assert_eq!(v2.sub(&v1), Vec3::new(3.0, 3.0, 3.0));
        assert_eq!(v1.dot(&v2), 32.0);

        let cross = v1.cross(&v2);
        assert_eq!(cross, Vec3::new(-3.0, 6.0, -3.0));
        assert_eq!(v1.dot(&cross), 0.0);
        assert_eq!(v2.dot(&cross), 0.0);

        let v3 = Vec3::new(3.0, 0.0, 4.0);
        assert_eq!(v3.length(), 5.0);
        assert_eq!(v3.normalize(), Vec3::new(0.6, 0.0, 0.8));
    }

    #[test]
    fn test_mat4_transform_and_inverse() {
        let t = Mat4::translation(10.0, -5.0, 2.0);
        let p = Vec3::new(1.0, 2.0, 3.0);
        let transformed = t.transform_point(&p);
        assert_eq!(transformed, Vec3::new(11.0, -3.0, 5.0));

        let inv = t.inverse().expect("translation matrix should be invertible");
        let restored = inv.transform_point(&transformed);
        assert!((restored.x - p.x).abs() < 1e-10);
        assert!((restored.y - p.y).abs() < 1e-10);
        assert!((restored.z - p.z).abs() < 1e-10);
    }

    #[test]
    fn test_quat_rotation() {
        let axis = Vec3::UP;
        let q = Quat::from_axis_angle(&axis, std::f64::consts::FRAC_PI_2); // 90 degrees around Y
        let p = Vec3::FORWARD; // (0, 0, -1)
        let rotated = q.rotate_vec3(&p);

        // Rotating (0, 0, -1) by 90 deg around Y gives (-1, 0, 0)
        assert!((rotated.x - (-1.0)).abs() < 1e-10);
        assert!(rotated.y.abs() < 1e-10);
        assert!(rotated.z.abs() < 1e-10);
    }

    #[test]
    fn test_ray_triangle_intersection() {
        let ray = Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0));
        let v0 = Vec3::new(-1.0, -1.0, 0.0);
        let v1 = Vec3::new(1.0, -1.0, 0.0);
        let v2 = Vec3::new(0.0, 1.0, 0.0);

        let hit = ray.intersect_triangle(&v0, &v1, &v2);
        assert!(hit.is_some());
        let (dist, u, v) = hit.unwrap();
        assert!((dist - 5.0).abs() < 1e-6);
        assert!(u >= 0.0 && v >= 0.0 && u + v <= 1.0);

        // Ray missing triangle
        let miss_ray = Ray::new(Vec3::new(10.0, 10.0, 5.0), Vec3::new(0.0, 0.0, -1.0));
        assert!(miss_ray.intersect_triangle(&v0, &v1, &v2).is_none());
    }

    #[test]
    fn test_ray_aabb_intersection() {
        let ray = Ray::new(Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 1.0));
        let min = Vec3::new(-1.0, -1.0, -1.0);
        let max = Vec3::new(1.0, 1.0, 1.0);

        let hit = ray.intersect_aabb(&min, &max);
        assert!(hit.is_some());
        assert!((hit.unwrap() - 4.0).abs() < 1e-6);
    }
}
