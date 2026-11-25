// Shae 3D Software Rasterizer Engine & Pipeline
// Part of Option 3: Hardware-Accelerated Graphics & Compute

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::math3d::{Mat4, Vec3};
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

#[inline]
pub fn pack_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

#[inline]
pub fn unpack_rgba(c: u32) -> (u8, u8, u8, u8) {
    (
        ((c >> 16) & 0xFF) as u8,
        ((c >> 8) & 0xFF) as u8,
        (c & 0xFF) as u8,
        ((c >> 24) & 0xFF) as u8,
    )
}

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

fn to_usize(v: &Value, fn_name: &str, span: Span) -> Result<usize, RuntimeError> {
    match v {
        Value::Int(i) if *i >= 0 => Ok(*i as usize),
        Value::Float(f) if *f >= 0.0 => Ok(*f as usize),
        Value::Number(f) if *f >= 0.0 => Ok(*f as usize),
        _ => Err(RuntimeError::new(format!("{}: expected non-negative integer, got {}", fn_name, v.type_name())).at(span)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex3D {
    pub pos: Vec3,
    pub normal: Vec3,
    pub uv: (f64, f64),
    pub color: u32,
}

impl Vertex3D {
    pub fn new(pos: Vec3, normal: Vec3, uv: (f64, f64), color: u32) -> Self {
        Self {
            pos,
            normal,
            uv,
            color,
        }
    }

    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("pos".to_string(), self.pos.to_shae_value());
        map.insert("normal".to_string(), self.normal.to_shae_value());
        map.insert(
            "uv".to_string(),
            Value::Array(Arc::new(RwLock::new(vec![
                Value::Float(self.uv.0),
                Value::Float(self.uv.1),
            ]))),
        );
        map.insert("color".to_string(), Value::Int(self.color as i64));
        Value::Map(Arc::new(RwLock::new(map)))
    }

    pub fn from_shae_value(v: &Value, fn_name: &str, span: Span) -> Result<Self, RuntimeError> {
        match v {
            Value::Map(s) => {
                let map = s.read().unwrap();
                let pos = match map.get("pos") {
                    Some(pv) => Vec3::from_shae_value(pv, fn_name, span)?,
                    None => Vec3::ZERO,
                };
                let normal = match map.get("normal") {
                    Some(nv) => Vec3::from_shae_value(nv, fn_name, span)?,
                    None => Vec3::UP,
                };
                let uv = match map.get("uv") {
                    Some(Value::Array(a)) => {
                        let arr = a.read().unwrap();
                        let u = arr.first().and_then(val_to_f64).unwrap_or(0.0);
                        let v = arr.get(1).and_then(val_to_f64).unwrap_or(0.0);
                        (u, v)
                    }
                    _ => (0.0, 0.0),
                };
                let color = match map.get("color") {
                    Some(cv) => match cv {
                        Value::Int(i) => *i as u32,
                        Value::Float(f) => *f as u32,
                        Value::Number(f) => *f as u32,
                        _ => 0xFFFFFFFF,
                    },
                    None => 0xFFFFFFFF,
                };
                Ok(Self::new(pos, normal, uv, color))
            }
            _ => Err(RuntimeError::new(format!("{}: expected Vertex3D map, got {}", fn_name, v.type_name())).at(span)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mesh3D {
    pub vertices: Vec<Vertex3D>,
    pub indices: Vec<usize>,
}

impl Mesh3D {
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn compute_normals(&mut self) {
        let mut normals = vec![Vec3::ZERO; self.vertices.len()];
        let num_triangles = self.indices.len() / 3;

        for i in 0..num_triangles {
            let i0 = self.indices[i * 3];
            let i1 = self.indices[i * 3 + 1];
            let i2 = self.indices[i * 3 + 2];

            if i0 < self.vertices.len() && i1 < self.vertices.len() && i2 < self.vertices.len() {
                let p0 = self.vertices[i0].pos;
                let p1 = self.vertices[i1].pos;
                let p2 = self.vertices[i2].pos;

                let edge1 = p1.sub(&p0);
                let edge2 = p2.sub(&p0);
                let face_normal = edge1.cross(&edge2);

                normals[i0] = normals[i0].add(&face_normal);
                normals[i1] = normals[i1].add(&face_normal);
                normals[i2] = normals[i2].add(&face_normal);
            }
        }

        for (v, n) in self.vertices.iter_mut().zip(normals.into_iter()) {
            let len = n.length();
            if len > 1e-9 {
                v.normal = n.div_scalar(len);
            }
        }
    }

    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        let verts_val: Vec<Value> = self.vertices.iter().map(|v| v.to_shae_value()).collect();
        let idx_val: Vec<Value> = self.indices.iter().map(|i| Value::Int(*i as i64)).collect();

        map.insert(
            "vertices".to_string(),
            Value::Array(Arc::new(RwLock::new(verts_val))),
        );
        map.insert(
            "indices".to_string(),
            Value::Array(Arc::new(RwLock::new(idx_val))),
        );
        map.insert("vertex_count".to_string(), Value::Int(self.vertex_count() as i64));
        map.insert("triangle_count".to_string(), Value::Int(self.triangle_count() as i64));
        Value::Map(Arc::new(RwLock::new(map)))
    }

    pub fn from_shae_value(v: &Value, fn_name: &str, span: Span) -> Result<Self, RuntimeError> {
        match v {
            Value::Map(s) => {
                let map = s.read().unwrap();
                let vertices = match map.get("vertices") {
                    Some(Value::Array(a)) => {
                        let arr = a.read().unwrap();
                        let mut res = Vec::with_capacity(arr.len());
                        for elem in arr.iter() {
                            res.push(Vertex3D::from_shae_value(elem, fn_name, span)?);
                        }
                        res
                    }
                    _ => Vec::new(),
                };
                let indices = match map.get("indices") {
                    Some(Value::Array(a)) => {
                        let arr = a.read().unwrap();
                        let mut res = Vec::with_capacity(arr.len());
                        for elem in arr.iter() {
                            match elem {
                                Value::Int(i) => res.push(*i as usize),
                                Value::Float(f) => res.push(*f as usize),
                                Value::Number(f) => res.push(*f as usize),
                                _ => {}
                            }
                        }
                        res
                    }
                    _ => Vec::new(),
                };
                Ok(Self { vertices, indices })
            }
            _ => Err(RuntimeError::new(format!("{}: expected Mesh3D map, got {}", fn_name, v.type_name())).at(span)),
        }
    }
}

// -----------------------------------------------------------------------------
// Procedural Primitive Mesh Generators
// -----------------------------------------------------------------------------

pub fn create_cube(size: f64) -> Mesh3D {
    let h = size * 0.5;
    let color = 0xFFFFFFFF;

    let raw_faces: [([Vec3; 4], Vec3, [(f64, f64); 4]); 6] = [
        // Front (Z+)
        (
            [Vec3::new(-h, -h, h), Vec3::new(h, -h, h), Vec3::new(h, h, h), Vec3::new(-h, h, h)],
            Vec3::BACK,
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        ),
        // Back (Z-)
        (
            [Vec3::new(h, -h, -h), Vec3::new(-h, -h, -h), Vec3::new(-h, h, -h), Vec3::new(h, h, -h)],
            Vec3::FORWARD,
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        ),
        // Left (X-)
        (
            [Vec3::new(-h, -h, -h), Vec3::new(-h, -h, h), Vec3::new(-h, h, h), Vec3::new(-h, h, -h)],
            Vec3::LEFT,
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        ),
        // Right (X+)
        (
            [Vec3::new(h, -h, h), Vec3::new(h, -h, -h), Vec3::new(h, h, -h), Vec3::new(h, h, h)],
            Vec3::RIGHT,
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        ),
        // Top (Y+)
        (
            [Vec3::new(-h, h, h), Vec3::new(h, h, h), Vec3::new(h, h, -h), Vec3::new(-h, h, -h)],
            Vec3::UP,
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        ),
        // Bottom (Y-)
        (
            [Vec3::new(-h, -h, -h), Vec3::new(h, -h, -h), Vec3::new(h, -h, h), Vec3::new(-h, -h, h)],
            Vec3::DOWN,
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        ),
    ];

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    for (quad, normal, uvs) in raw_faces.iter() {
        let base_idx = vertices.len();
        for i in 0..4 {
            vertices.push(Vertex3D::new(quad[i], *normal, uvs[i], color));
        }
        indices.push(base_idx);
        indices.push(base_idx + 1);
        indices.push(base_idx + 2);

        indices.push(base_idx);
        indices.push(base_idx + 2);
        indices.push(base_idx + 3);
    }

    Mesh3D { vertices, indices }
}

pub fn create_sphere(radius: f64, rings: usize, sectors: usize) -> Mesh3D {
    let rings = rings.max(3);
    let sectors = sectors.max(3);

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let color = 0xFFFFFFFF;

    for r in 0..=rings {
        let v = r as f64 / rings as f64;
        let phi = v * std::f64::consts::PI;

        for s in 0..=sectors {
            let u = s as f64 / sectors as f64;
            let theta = u * std::f64::consts::PI * 2.0;

            let x = theta.cos() * phi.sin();
            let y = phi.cos();
            let z = theta.sin() * phi.sin();

            let normal = Vec3::new(x, y, z);
            let pos = normal.mul_scalar(radius);

            vertices.push(Vertex3D::new(pos, normal, (u, v), color));
        }
    }

    for r in 0..rings {
        for s in 0..sectors {
            let first = r * (sectors + 1) + s;
            let second = first + sectors + 1;

            indices.push(first);
            indices.push(second);
            indices.push(first + 1);

            indices.push(second);
            indices.push(second + 1);
            indices.push(first + 1);
        }
    }

    Mesh3D { vertices, indices }
}

pub fn create_plane(width: f64, depth: f64) -> Mesh3D {
    let hw = width * 0.5;
    let hd = depth * 0.5;
    let color = 0xFFFFFFFF;

    let vertices = vec![
        Vertex3D::new(Vec3::new(-hw, 0.0, -hd), Vec3::UP, (0.0, 0.0), color),
        Vertex3D::new(Vec3::new(hw, 0.0, -hd), Vec3::UP, (1.0, 0.0), color),
        Vertex3D::new(Vec3::new(hw, 0.0, hd), Vec3::UP, (1.0, 1.0), color),
        Vertex3D::new(Vec3::new(-hw, 0.0, hd), Vec3::UP, (0.0, 1.0), color),
    ];

    let indices = vec![0, 1, 2, 0, 2, 3];
    Mesh3D { vertices, indices }
}

pub fn create_cylinder(radius: f64, height: f64, segments: usize) -> Mesh3D {
    let segments = segments.max(3);
    let hh = height * 0.5;
    let color = 0xFFFFFFFF;

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // Side vertices
    for i in 0..=segments {
        let u = i as f64 / segments as f64;
        let theta = u * std::f64::consts::PI * 2.0;
        let x = theta.cos();
        let z = theta.sin();
        let normal = Vec3::new(x, 0.0, z);

        vertices.push(Vertex3D::new(Vec3::new(x * radius, -hh, z * radius), normal, (u, 0.0), color));
        vertices.push(Vertex3D::new(Vec3::new(x * radius, hh, z * radius), normal, (u, 1.0), color));
    }

    for i in 0..segments {
        let base = i * 2;
        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 2);

        indices.push(base + 1);
        indices.push(base + 3);
        indices.push(base + 2);
    }

    Mesh3D { vertices, indices }
}

// -----------------------------------------------------------------------------
// Wavefront OBJ Parser
// -----------------------------------------------------------------------------

pub fn parse_obj(content: &str) -> Result<Mesh3D, String> {
    let mut raw_positions = Vec::new();
    let mut raw_normals = Vec::new();
    let mut raw_uvs = Vec::new();

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let tag = match parts.next() {
            Some(t) => t,
            None => continue,
        };

        match tag {
            "v" => {
                let x: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let y: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let z: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                raw_positions.push(Vec3::new(x, y, z));
            }
            "vn" => {
                let x: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let y: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let z: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                raw_normals.push(Vec3::new(x, y, z).normalize());
            }
            "vt" => {
                let u: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let v: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                raw_uvs.push((u, v));
            }
            "f" => {
                let mut face_verts = Vec::new();
                for token in parts {
                    let comps: Vec<&str> = token.split('/').collect();
                    let v_idx: Option<usize> = comps.first().and_then(|s| s.parse::<isize>().ok()).map(|idx| {
                        if idx < 0 {
                            (raw_positions.len() as isize + idx) as usize
                        } else {
                            (idx - 1) as usize
                        }
                    });

                    let vt_idx: Option<usize> = comps.get(1).and_then(|s| {
                        if s.is_empty() { None } else { s.parse::<isize>().ok() }
                    }).map(|idx| {
                        if idx < 0 {
                            (raw_uvs.len() as isize + idx) as usize
                        } else {
                            (idx - 1) as usize
                        }
                    });

                    let vn_idx: Option<usize> = comps.get(2).and_then(|s| {
                        if s.is_empty() { None } else { s.parse::<isize>().ok() }
                    }).map(|idx| {
                        if idx < 0 {
                            (raw_normals.len() as isize + idx) as usize
                        } else {
                            (idx - 1) as usize
                        }
                    });

                    if let Some(vi) = v_idx {
                        if vi < raw_positions.len() {
                            let pos = raw_positions[vi];
                            let normal = vn_idx.and_then(|i| raw_normals.get(i).copied()).unwrap_or(Vec3::UP);
                            let uv = vt_idx.and_then(|i| raw_uvs.get(i).copied()).unwrap_or((0.0, 0.0));
                            face_verts.push(Vertex3D::new(pos, normal, uv, 0xFFFFFFFF));
                        }
                    }
                }

                // Fan triangulation
                if face_verts.len() >= 3 {
                    let base_v = face_verts[0];
                    for i in 1..(face_verts.len() - 1) {
                        let i0 = vertices.len();
                        vertices.push(base_v);
                        vertices.push(face_verts[i]);
                        vertices.push(face_verts[i + 1]);

                        indices.push(i0);
                        indices.push(i0 + 1);
                        indices.push(i0 + 2);
                    }
                }
            }
            _ => {}
        }
    }

    let mut mesh = Mesh3D { vertices, indices };
    if raw_normals.is_empty() {
        mesh.compute_normals();
    }
    Ok(mesh)
}

// -----------------------------------------------------------------------------
// High-Performance 3D Software Rasterizer
// -----------------------------------------------------------------------------

pub struct Rasterizer3D {
    pub width: usize,
    pub height: usize,
    pub color_buffer: Vec<u32>,
    pub depth_buffer: Vec<f64>,
    pub cull_backface: bool,
    pub wireframe: bool,
}

impl Rasterizer3D {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width,
            height,
            color_buffer: vec![0xFF000000; size],
            depth_buffer: vec![f64::INFINITY; size],
            cull_backface: true,
            wireframe: false,
        }
    }

    pub fn clear(&mut self, color: u32, depth: f64) {
        self.color_buffer.fill(color);
        self.depth_buffer.fill(depth);
    }

    pub fn clear_color(&mut self, color: u32) {
        self.color_buffer.fill(color);
    }

    pub fn clear_depth(&mut self, depth: f64) {
        self.depth_buffer.fill(depth);
    }

    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            self.color_buffer[y * self.width + x]
        } else {
            0
        }
    }

    pub fn get_depth(&self, x: usize, y: usize) -> f64 {
        if x < self.width && y < self.height {
            self.depth_buffer[y * self.width + x]
        } else {
            f64::INFINITY
        }
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, color: u32, depth: f64) {
        if x < self.width && y < self.height {
            let idx = y * self.width + x;
            if depth <= self.depth_buffer[idx] {
                self.depth_buffer[idx] = depth;
                self.color_buffer[idx] = color;
            }
        }
    }

    pub fn draw_line(&mut self, x0: i64, y0: i64, z0: f64, x1: i64, y1: i64, z1: f64, color: u32) {
        let mut x = x0;
        let mut y = y0;
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        let total_dist = ((x1 - x0).pow(2) as f64 + (y1 - y0).pow(2) as f64).sqrt().max(1.0);

        loop {
            if x >= 0 && x < self.width as i64 && y >= 0 && y < self.height as i64 {
                let cur_dist = ((x - x0).pow(2) as f64 + (y - y0).pow(2) as f64).sqrt();
                let t = (cur_dist / total_dist).clamp(0.0, 1.0);
                let z = z0 + (z1 - z0) * t;
                self.set_pixel(x as usize, y as usize, color, z);
            }

            if x == x1 && y == y1 {
                break;
            }

            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Rasterizes a single 3D triangle transformed through MVP matrix with lighting.
    pub fn draw_triangle(
        &mut self,
        v0: &Vertex3D,
        v1: &Vertex3D,
        v2: &Vertex3D,
        mvp: &Mat4,
        model: &Mat4,
        light_dir: Option<Vec3>,
        ambient: f64,
        base_color: Option<u32>,
    ) {
        let (p0_clip, w0) = mvp_transform_homogeneous(mvp, &v0.pos);
        let (p1_clip, w1) = mvp_transform_homogeneous(mvp, &v1.pos);
        let (p2_clip, w2) = mvp_transform_homogeneous(mvp, &v2.pos);

        if w0 <= 0.001 || w1 <= 0.001 || w2 <= 0.001 {
            return;
        }

        let ndc0 = Vec3::new(p0_clip.x / w0, p0_clip.y / w0, p0_clip.z / w0);
        let ndc1 = Vec3::new(p1_clip.x / w1, p1_clip.y / w1, p1_clip.z / w1);
        let ndc2 = Vec3::new(p2_clip.x / w2, p2_clip.y / w2, p2_clip.z / w2);

        let sw = self.width as f64;
        let sh = self.height as f64;

        let s0_x = (ndc0.x + 1.0) * 0.5 * sw;
        let s0_y = (1.0 - (ndc0.y + 1.0) * 0.5) * sh;

        let s1_x = (ndc1.x + 1.0) * 0.5 * sw;
        let s1_y = (1.0 - (ndc1.y + 1.0) * 0.5) * sh;

        let s2_x = (ndc2.x + 1.0) * 0.5 * sw;
        let s2_y = (1.0 - (ndc2.y + 1.0) * 0.5) * sh;

        let area = (s1_x - s0_x) * (s2_y - s0_y) - (s1_y - s0_y) * (s2_x - s0_x);

        if area.abs() < 1e-5 {
            return;
        }

        if self.cull_backface && area < 0.0 {
            return;
        }

        if self.wireframe {
            let col = base_color.unwrap_or(v0.color);
            self.draw_line(s0_x as i64, s0_y as i64, ndc0.z, s1_x as i64, s1_y as i64, ndc1.z, col);
            self.draw_line(s1_x as i64, s1_y as i64, ndc1.z, s2_x as i64, s2_y as i64, ndc2.z, col);
            self.draw_line(s2_x as i64, s2_y as i64, ndc2.z, s0_x as i64, s0_y as i64, ndc0.z, col);
            return;
        }

        let n0 = model.transform_vector(&v0.normal).normalize();
        let n1 = model.transform_vector(&v1.normal).normalize();
        let n2 = model.transform_vector(&v2.normal).normalize();

        let min_x = (s0_x.min(s1_x).min(s2_x).floor() as i64).max(0).min(self.width as i64 - 1) as usize;
        let max_x = (s0_x.max(s1_x).max(s2_x).ceil() as i64).max(0).min(self.width as i64 - 1) as usize;
        let min_y = (s0_y.min(s1_y).min(s2_y).floor() as i64).max(0).min(self.height as i64 - 1) as usize;
        let max_y = (s0_y.max(s1_y).max(s2_y).ceil() as i64).max(0).min(self.height as i64 - 1) as usize;

        let inv_area = 1.0 / area;
        let inv_w0 = 1.0 / w0;
        let inv_w1 = 1.0 / w1;
        let inv_w2 = 1.0 / w2;

        let l_dir = light_dir.map(|l| l.normalize());

        for y in min_y..=max_y {
            let py = y as f64 + 0.5;
            for x in min_x..=max_x {
                let px = x as f64 + 0.5;

                let w_a = ((s1_x - px) * (s2_y - py) - (s1_y - py) * (s2_x - px)) * inv_area;
                let w_b = ((s2_x - px) * (s0_y - py) - (s2_y - py) * (s0_x - px)) * inv_area;
                let w_c = ((s0_x - px) * (s1_y - py) - (s0_y - py) * (s1_x - px)) * inv_area;

                if w_a >= 0.0 && w_b >= 0.0 && w_c >= 0.0 {
                    let inv_w = w_a * inv_w0 + w_b * inv_w1 + w_c * inv_w2;
                    let z = 1.0 / inv_w;

                    let pixel_idx = y * self.width + x;
                    if z < self.depth_buffer[pixel_idx] {
                        self.depth_buffer[pixel_idx] = z;

                        let interp_normal = Vec3::new(
                            (w_a * n0.x * inv_w0 + w_b * n1.x * inv_w1 + w_c * n2.x * inv_w2) * z,
                            (w_a * n0.y * inv_w0 + w_b * n1.y * inv_w1 + w_c * n2.y * inv_w2) * z,
                            (w_a * n0.z * inv_w0 + w_b * n1.z * inv_w1 + w_c * n2.z * inv_w2) * z,
                        ).normalize();

                        let raw_color = base_color.unwrap_or(v0.color);
                        let (r, g, b, a) = unpack_rgba(raw_color);

                        let intensity = if let Some(ref l) = l_dir {
                            let diffuse = interp_normal.dot(l).max(0.0);
                            (ambient + (1.0 - ambient) * diffuse).clamp(0.0, 1.0)
                        } else {
                            1.0
                        };

                        let shaded_r = ((r as f64 * intensity) as u32).min(255) as u8;
                        let shaded_g = ((g as f64 * intensity) as u32).min(255) as u8;
                        let shaded_b = ((b as f64 * intensity) as u32).min(255) as u8;

                        self.color_buffer[pixel_idx] = pack_rgba(shaded_r, shaded_g, shaded_b, a);
                    }
                }
            }
        }
    }

    pub fn draw_mesh(
        &mut self,
        mesh: &Mesh3D,
        mvp: &Mat4,
        model: &Mat4,
        light_dir: Option<Vec3>,
        ambient: f64,
        base_color: Option<u32>,
    ) {
        let num_triangles = mesh.indices.len() / 3;
        for i in 0..num_triangles {
            let i0 = mesh.indices[i * 3];
            let i1 = mesh.indices[i * 3 + 1];
            let i2 = mesh.indices[i * 3 + 2];

            if i0 < mesh.vertices.len() && i1 < mesh.vertices.len() && i2 < mesh.vertices.len() {
                self.draw_triangle(
                    &mesh.vertices[i0],
                    &mesh.vertices[i1],
                    &mesh.vertices[i2],
                    mvp,
                    model,
                    light_dir,
                    ambient,
                    base_color,
                );
            }
        }
    }

    pub fn render_to_window(&self, window_id: usize) -> Result<(), String> {
        let registry = crate::gui::get_window_registry();
        let mut map = registry.lock().unwrap();
        if let Some(win) = map.get_mut(&window_id) {
            let copy_w = self.width.min(win.width);
            let copy_h = self.height.min(win.height);

            for y in 0..copy_h {
                let src_offset = y * self.width;
                let dst_offset = y * win.width;
                for x in 0..copy_w {
                    win.buffer[dst_offset + x] = self.color_buffer[src_offset + x];
                }
            }
            Ok(())
        } else {
            Err(format!("Window {} not found", window_id))
        }
    }

    pub fn to_image(&self) -> Result<usize, String> {
        let img = crate::gui::ShaeImage {
            width: self.width,
            height: self.height,
            pixels: self.color_buffer.clone(),
        };
        let registry = crate::gui::get_image_registry();
        let mut map = registry.lock().unwrap();
        let id = NEXT_RASTER_IMAGE_ID.fetch_add(1, Ordering::SeqCst);
        map.insert(id, Arc::new(img));
        Ok(id)
    }
}

static NEXT_RASTER_IMAGE_ID: AtomicUsize = AtomicUsize::new(1000000);

#[inline]
fn mvp_transform_homogeneous(m: &Mat4, v: &Vec3) -> (Vec3, f64) {
    let d = &m.m;
    let x = d[0] * v.x + d[1] * v.y + d[2] * v.z + d[3];
    let y = d[4] * v.x + d[5] * v.y + d[6] * v.z + d[7];
    let z = d[8] * v.x + d[9] * v.y + d[10] * v.z + d[11];
    let w = d[12] * v.x + d[13] * v.y + d[14] * v.z + d[15];
    (Vec3::new(x, y, z), w)
}

// -----------------------------------------------------------------------------
// Global Registry for Active 3D Rasterizers
// -----------------------------------------------------------------------------

static RASTER_REGISTRY: OnceLock<Mutex<HashMap<usize, Rasterizer3D>>> = OnceLock::new();
static NEXT_RASTER_ID: AtomicUsize = AtomicUsize::new(1);

fn get_raster_registry() -> &'static Mutex<HashMap<usize, Rasterizer3D>> {
    RASTER_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

// -----------------------------------------------------------------------------
// Standard Library Module: std:gui3d / std:render3d
// -----------------------------------------------------------------------------

pub fn build_gui3d_module() -> Value {
    let mut map = IndexMap::new();

    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("rasterizer", gui3d_rasterizer),
        ("clear", gui3d_clear),
        ("clear_color", gui3d_clear_color),
        ("clear_depth", gui3d_clear_depth),
        ("get_pixel", gui3d_get_pixel),
        ("get_depth", gui3d_get_depth),
        ("set_pixel", gui3d_set_pixel),
        ("set_cull", gui3d_set_cull),
        ("set_wireframe", gui3d_set_wireframe),
        ("draw_triangle", gui3d_draw_triangle),
        ("draw_mesh", gui3d_draw_mesh),
        ("render_to_window", gui3d_render_to_window),
        ("to_image", gui3d_to_image),
        ("cube", gui3d_cube),
        ("sphere", gui3d_sphere),
        ("plane", gui3d_plane),
        ("cylinder", gui3d_cylinder),
        ("parse_obj", gui3d_parse_obj),
        ("load_obj", gui3d_load_obj),
        ("vertex", gui3d_vertex),
        ("pack_color", gui3d_pack_color),
        ("unpack_color", gui3d_unpack_color),
    ];

    for (name, func) in entries {
        map.insert(name.to_string(), Value::Builtin {
            name: format!("gui3d.{}", name),
            func,
        });
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn gui3d_rasterizer(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.rasterizer", 2, &args, span)?;
    let width = to_usize(&args[0], "gui3d.rasterizer width", span)?;
    let height = to_usize(&args[1], "gui3d.rasterizer height", span)?;

    let id = NEXT_RASTER_ID.fetch_add(1, Ordering::SeqCst);
    let raster = Rasterizer3D::new(width, height);
    get_raster_registry().lock().unwrap().insert(id, raster);

    let mut map = IndexMap::new();
    map.insert("id".to_string(), Value::Int(id as i64));
    map.insert("width".to_string(), Value::Int(width as i64));
    map.insert("height".to_string(), Value::Int(height as i64));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

fn gui3d_clear(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("gui3d.clear requires at least (id, color)".to_string()).at(span));
    }
    let id = to_usize(&args[0], "gui3d.clear id", span)?;
    let color = if args.len() > 1 {
        match &args[1] {
            Value::Int(i) => *i as u32,
            _ => 0xFF000000,
        }
    } else {
        0xFF000000
    };
    let depth = if args.len() > 2 {
        to_f64(&args[2], "gui3d.clear depth", span)?
    } else {
        f64::INFINITY
    };

    let mut reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get_mut(&id) {
        r.clear(color, depth);
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new(format!("gui3d.clear: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_clear_color(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.clear_color", 2, &args, span)?;
    let id = to_usize(&args[0], "gui3d.clear_color id", span)?;
    let color = match &args[1] {
        Value::Int(i) => *i as u32,
        _ => 0xFF000000,
    };

    let mut reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get_mut(&id) {
        r.clear_color(color);
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new(format!("gui3d.clear_color: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_clear_depth(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.clear_depth", 2, &args, span)?;
    let id = to_usize(&args[0], "gui3d.clear_depth id", span)?;
    let depth = to_f64(&args[1], "gui3d.clear_depth depth", span)?;

    let mut reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get_mut(&id) {
        r.clear_depth(depth);
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new(format!("gui3d.clear_depth: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_get_pixel(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.get_pixel", 3, &args, span)?;
    let id = to_usize(&args[0], "gui3d.get_pixel id", span)?;
    let x = to_usize(&args[1], "gui3d.get_pixel x", span)?;
    let y = to_usize(&args[2], "gui3d.get_pixel y", span)?;

    let reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get(&id) {
        Ok(Value::Int(r.get_pixel(x, y) as i64))
    } else {
        Err(RuntimeError::new(format!("gui3d.get_pixel: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_get_depth(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.get_depth", 3, &args, span)?;
    let id = to_usize(&args[0], "gui3d.get_depth id", span)?;
    let x = to_usize(&args[1], "gui3d.get_depth x", span)?;
    let y = to_usize(&args[2], "gui3d.get_depth y", span)?;

    let reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get(&id) {
        Ok(Value::Float(r.get_depth(x, y)))
    } else {
        Err(RuntimeError::new(format!("gui3d.get_depth: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_set_pixel(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.set_pixel", 5, &args, span)?;
    let id = to_usize(&args[0], "gui3d.set_pixel id", span)?;
    let x = to_usize(&args[1], "gui3d.set_pixel x", span)?;
    let y = to_usize(&args[2], "gui3d.set_pixel y", span)?;
    let color = match &args[3] {
        Value::Int(i) => *i as u32,
        _ => 0xFFFFFFFF,
    };
    let depth = to_f64(&args[4], "gui3d.set_pixel depth", span)?;

    let mut reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get_mut(&id) {
        r.set_pixel(x, y, color, depth);
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new(format!("gui3d.set_pixel: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_set_cull(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.set_cull", 2, &args, span)?;
    let id = to_usize(&args[0], "gui3d.set_cull id", span)?;
    let cull = match &args[1] {
        Value::Bool(b) => *b,
        _ => true,
    };

    let mut reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get_mut(&id) {
        r.cull_backface = cull;
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new(format!("gui3d.set_cull: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_set_wireframe(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.set_wireframe", 2, &args, span)?;
    let id = to_usize(&args[0], "gui3d.set_wireframe id", span)?;
    let wire = match &args[1] {
        Value::Bool(b) => *b,
        _ => true,
    };

    let mut reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get_mut(&id) {
        r.wireframe = wire;
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new(format!("gui3d.set_wireframe: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_draw_triangle(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 6 {
        return Err(RuntimeError::new("gui3d.draw_triangle: requires (id, v0, v1, v2, mvp, model, [light_dir, ambient, color])".to_string()).at(span));
    }
    let id = to_usize(&args[0], "gui3d.draw_triangle id", span)?;
    let v0 = Vertex3D::from_shae_value(&args[1], "gui3d.draw_triangle v0", span)?;
    let v1 = Vertex3D::from_shae_value(&args[2], "gui3d.draw_triangle v1", span)?;
    let v2 = Vertex3D::from_shae_value(&args[3], "gui3d.draw_triangle v2", span)?;
    let mvp = Mat4::from_shae_value(&args[4], "gui3d.draw_triangle mvp", span)?;
    let model = Mat4::from_shae_value(&args[5], "gui3d.draw_triangle model", span)?;

    let light_dir = if args.len() > 6 && !matches!(&args[6], Value::Null) {
        Some(Vec3::from_shae_value(&args[6], "gui3d.draw_triangle light_dir", span)?)
    } else {
        None
    };

    let ambient = if args.len() > 7 {
        to_f64(&args[7], "gui3d.draw_triangle ambient", span).unwrap_or(0.2)
    } else {
        0.2
    };

    let base_color = if args.len() > 8 {
        match &args[8] {
            Value::Int(i) => Some(*i as u32),
            _ => None,
        }
    } else {
        None
    };

    let mut reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get_mut(&id) {
        r.draw_triangle(&v0, &v1, &v2, &mvp, &model, light_dir, ambient, base_color);
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new(format!("gui3d.draw_triangle: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_draw_mesh(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 4 {
        return Err(RuntimeError::new("gui3d.draw_mesh requires (id, mesh, mvp, model, [light_dir, ambient, base_color])".to_string()).at(span));
    }
    let id = to_usize(&args[0], "gui3d.draw_mesh id", span)?;
    let mesh = Mesh3D::from_shae_value(&args[1], "gui3d.draw_mesh mesh", span)?;
    let mvp = Mat4::from_shae_value(&args[2], "gui3d.draw_mesh mvp", span)?;
    let model = Mat4::from_shae_value(&args[3], "gui3d.draw_mesh model", span)?;

    let light_dir = if args.len() > 4 && !matches!(&args[4], Value::Null) {
        Some(Vec3::from_shae_value(&args[4], "gui3d.draw_mesh light_dir", span)?)
    } else {
        None
    };

    let ambient = if args.len() > 5 {
        to_f64(&args[5], "gui3d.draw_mesh ambient", span).unwrap_or(0.2)
    } else {
        0.2
    };

    let base_color = if args.len() > 6 {
        match &args[6] {
            Value::Int(i) => Some(*i as u32),
            _ => None,
        }
    } else {
        None
    };

    let mut reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get_mut(&id) {
        r.draw_mesh(&mesh, &mvp, &model, light_dir, ambient, base_color);
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new(format!("gui3d.draw_mesh: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_render_to_window(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.render_to_window", 2, &args, span)?;
    let id = to_usize(&args[0], "gui3d.render_to_window id", span)?;
    let win_id = to_usize(&args[1], "gui3d.render_to_window win_id", span)?;

    let reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get(&id) {
        r.render_to_window(win_id)
            .map(|_| Value::Bool(true))
            .map_err(|e| RuntimeError::new(format!("gui3d.render_to_window: {}", e)).at(span))
    } else {
        Err(RuntimeError::new(format!("gui3d.render_to_window: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_to_image(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.to_image", 1, &args, span)?;
    let id = to_usize(&args[0], "gui3d.to_image id", span)?;

    let reg = get_raster_registry().lock().unwrap();
    if let Some(r) = reg.get(&id) {
        r.to_image()
            .map(|img_id| Value::Int(img_id as i64))
            .map_err(|e| RuntimeError::new(format!("gui3d.to_image: {}", e)).at(span))
    } else {
        Err(RuntimeError::new(format!("gui3d.to_image: rasterizer {} not found", id)).at(span))
    }
}

fn gui3d_cube(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let size = if !args.is_empty() {
        to_f64(&args[0], "gui3d.cube size", span)?
    } else {
        1.0
    };
    Ok(create_cube(size).to_shae_value())
}

fn gui3d_sphere(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let radius = if !args.is_empty() {
        to_f64(&args[0], "gui3d.sphere radius", span)?
    } else {
        1.0
    };
    let rings = if args.len() > 1 {
        to_usize(&args[1], "gui3d.sphere rings", span)?
    } else {
        16
    };
    let sectors = if args.len() > 2 {
        to_usize(&args[2], "gui3d.sphere sectors", span)?
    } else {
        32
    };
    Ok(create_sphere(radius, rings, sectors).to_shae_value())
}

fn gui3d_plane(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let width = if !args.is_empty() {
        to_f64(&args[0], "gui3d.plane width", span)?
    } else {
        1.0
    };
    let depth = if args.len() > 1 {
        to_f64(&args[1], "gui3d.plane depth", span)?
    } else {
        1.0
    };
    Ok(create_plane(width, depth).to_shae_value())
}

fn gui3d_cylinder(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let radius = if !args.is_empty() {
        to_f64(&args[0], "gui3d.cylinder radius", span)?
    } else {
        1.0
    };
    let height = if args.len() > 1 {
        to_f64(&args[1], "gui3d.cylinder height", span)?
    } else {
        2.0
    };
    let segments = if args.len() > 2 {
        to_usize(&args[2], "gui3d.cylinder segments", span)?
    } else {
        16
    };
    Ok(create_cylinder(radius, height, segments).to_shae_value())
}

fn gui3d_parse_obj(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.parse_obj", 1, &args, span)?;
    match &args[0] {
        Value::String(s) => parse_obj(s)
            .map(|m| m.to_shae_value())
            .map_err(|e| RuntimeError::new(format!("gui3d.parse_obj: {}", e)).at(span)),
        _ => Err(RuntimeError::new("gui3d.parse_obj: expected string content".to_string()).at(span)),
    }
}

fn gui3d_load_obj(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.load_obj", 1, &args, span)?;
    match &args[0] {
        Value::String(path) => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| RuntimeError::new(format!("gui3d.load_obj: failed reading '{}': {}", path, e)).at(span))?;
            parse_obj(&content)
                .map(|m| m.to_shae_value())
                .map_err(|e| RuntimeError::new(format!("gui3d.load_obj: {}", e)).at(span))
        }
        _ => Err(RuntimeError::new("gui3d.load_obj: expected file path string".to_string()).at(span)),
    }
}

fn gui3d_vertex(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.vertex", 4, &args, span)?;
    let pos = Vec3::from_shae_value(&args[0], "gui3d.vertex pos", span)?;
    let normal = Vec3::from_shae_value(&args[1], "gui3d.vertex normal", span)?;
    let uv = match &args[2] {
        Value::Array(a) => {
            let arr = a.read().unwrap();
            let u = arr.first().and_then(val_to_f64).unwrap_or(0.0);
            let v = arr.get(1).and_then(val_to_f64).unwrap_or(0.0);
            (u, v)
        }
        _ => (0.0, 0.0),
    };
    let color = match &args[3] {
        Value::Int(i) => *i as u32,
        _ => 0xFFFFFFFF,
    };
    Ok(Vertex3D::new(pos, normal, uv, color).to_shae_value())
}

fn gui3d_pack_color(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.pack_color", 4, &args, span)?;
    let r = to_usize(&args[0], "gui3d.pack_color r", span)? as u8;
    let g = to_usize(&args[1], "gui3d.pack_color g", span)? as u8;
    let b = to_usize(&args[2], "gui3d.pack_color b", span)? as u8;
    let a = to_usize(&args[3], "gui3d.pack_color a", span)? as u8;
    Ok(Value::Int(pack_rgba(r, g, b, a) as i64))
}

fn gui3d_unpack_color(_eval: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gui3d.unpack_color", 1, &args, span)?;
    let c = match &args[0] {
        Value::Int(i) => *i as u32,
        _ => 0,
    };
    let (r, g, b, a) = unpack_rgba(c);
    let mut map = IndexMap::new();
    map.insert("r".to_string(), Value::Int(r as i64));
    map.insert("g".to_string(), Value::Int(g as i64));
    map.insert("b".to_string(), Value::Int(b as i64));
    map.insert("a".to_string(), Value::Int(a as i64));
    Ok(Value::Map(Arc::new(RwLock::new(map))))
}

// -----------------------------------------------------------------------------
// Built-in Unit Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cube_generation() {
        let cube = create_cube(2.0);
        assert_eq!(cube.vertices.len(), 24);
        assert_eq!(cube.indices.len(), 36);
        assert_eq!(cube.triangle_count(), 12);
    }

    #[test]
    fn test_sphere_generation() {
        let sphere = create_sphere(1.0, 8, 16);
        assert!(sphere.vertices.len() > 0);
        assert_eq!(sphere.triangle_count(), 8 * 16 * 2);
    }

    #[test]
    fn test_obj_parser() {
        let obj = r#"
# Test Triangle
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
vn 0.0 0.0 1.0
f 1//1 2//1 3//1
"#;
        let mesh = parse_obj(obj).expect("OBJ should parse");
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.indices.len(), 3);
        assert_eq!(mesh.triangle_count(), 1);
    }

    #[test]
    fn test_rasterizer_depth_buffering() {
        let mut raster = Rasterizer3D::new(64, 64);
        raster.clear(0xFF000000, 100.0);

        // Near pixel at (32, 32)
        raster.set_pixel(32, 32, 0xFFFF0000, 5.0);
        assert_eq!(raster.get_pixel(32, 32), 0xFFFF0000);
        assert_eq!(raster.get_depth(32, 32), 5.0);

        // Occluded far pixel at (32, 32)
        raster.set_pixel(32, 32, 0xFF00FF00, 10.0);
        assert_eq!(raster.get_pixel(32, 32), 0xFFFF0000); // Should remain red
        assert_eq!(raster.get_depth(32, 32), 5.0);

        // Closer pixel at (32, 32)
        raster.set_pixel(32, 32, 0xFF0000FF, 2.0);
        assert_eq!(raster.get_pixel(32, 32), 0xFF0000FF); // Should update to blue
        assert_eq!(raster.get_depth(32, 32), 2.0);
    }
}
