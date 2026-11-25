// Shae 3D Spatial Positional Audio Engine
// Part of Option 3: Hardware-Accelerated Graphics & Compute

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::math3d::Vec3;
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

pub const SPEED_OF_SOUND: f64 = 343.0; // meters per second

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttenuationModel {
    Inverse,
    Linear,
    Exponential,
}

#[derive(Debug, Clone)]
pub struct AudioListener3D {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub velocity: Vec3,
}

impl Default for AudioListener3D {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            forward: Vec3::FORWARD,
            up: Vec3::UP,
            velocity: Vec3::ZERO,
        }
    }
}

impl AudioListener3D {
    pub fn new(position: Vec3, forward: Vec3, up: Vec3, velocity: Vec3) -> Self {
        Self {
            position,
            forward: forward.normalize(),
            up: up.normalize(),
            velocity,
        }
    }

    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("position".to_string(), self.position.to_shae_value());
        map.insert("forward".to_string(), self.forward.to_shae_value());
        map.insert("up".to_string(), self.up.to_shae_value());
        map.insert("velocity".to_string(), self.velocity.to_shae_value());
        Value::Map(Arc::new(RwLock::new(map)))
    }

    pub fn from_shae_value(v: &Value, fn_name: &str, span: Span) -> Result<Self, RuntimeError> {
        match v {
            Value::Map(m) => {
                let map = m.read().unwrap();
                let position = match map.get("position") {
                    Some(pv) => Vec3::from_shae_value(pv, fn_name, span)?,
                    None => Vec3::ZERO,
                };
                let forward = match map.get("forward") {
                    Some(fv) => Vec3::from_shae_value(fv, fn_name, span)?,
                    None => Vec3::FORWARD,
                };
                let up = match map.get("up") {
                    Some(uv) => Vec3::from_shae_value(uv, fn_name, span)?,
                    None => Vec3::UP,
                };
                let velocity = match map.get("velocity") {
                    Some(vv) => Vec3::from_shae_value(vv, fn_name, span)?,
                    None => Vec3::ZERO,
                };
                Ok(Self::new(position, forward, up, velocity))
            }
            _ => Err(RuntimeError::new(format!("{}: expected AudioListener3D map", fn_name)).at(span)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioSource3D {
    pub position: Vec3,
    pub velocity: Vec3,
    pub ref_distance: f64,
    pub max_distance: f64,
    pub rolloff_factor: f64,
    pub model: AttenuationModel,
}

impl Default for AudioSource3D {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            ref_distance: 1.0,
            max_distance: 100.0,
            rolloff_factor: 1.0,
            model: AttenuationModel::Inverse,
        }
    }
}

impl AudioSource3D {
    pub fn new(
        position: Vec3,
        velocity: Vec3,
        ref_distance: f64,
        max_distance: f64,
        rolloff_factor: f64,
        model: AttenuationModel,
    ) -> Self {
        Self {
            position,
            velocity,
            ref_distance: ref_distance.max(0.001),
            max_distance: max_distance.max(ref_distance),
            rolloff_factor: rolloff_factor.max(0.0),
            model,
        }
    }

    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("position".to_string(), self.position.to_shae_value());
        map.insert("velocity".to_string(), self.velocity.to_shae_value());
        map.insert("ref_distance".to_string(), Value::Float(self.ref_distance));
        map.insert("max_distance".to_string(), Value::Float(self.max_distance));
        map.insert("rolloff_factor".to_string(), Value::Float(self.rolloff_factor));
        let model_str = match self.model {
            AttenuationModel::Inverse => "inverse",
            AttenuationModel::Linear => "linear",
            AttenuationModel::Exponential => "exponential",
        };
        map.insert("model".to_string(), Value::String(model_str.to_string()));
        Value::Map(Arc::new(RwLock::new(map)))
    }

    pub fn from_shae_value(v: &Value, fn_name: &str, span: Span) -> Result<Self, RuntimeError> {
        match v {
            Value::Map(m) => {
                let map = m.read().unwrap();
                let position = match map.get("position") {
                    Some(pv) => Vec3::from_shae_value(pv, fn_name, span)?,
                    None => Vec3::ZERO,
                };
                let velocity = match map.get("velocity") {
                    Some(vv) => Vec3::from_shae_value(vv, fn_name, span)?,
                    None => Vec3::ZERO,
                };
                let ref_distance = map.get("ref_distance").and_then(val_to_f64).unwrap_or(1.0);
                let max_distance = map.get("max_distance").and_then(val_to_f64).unwrap_or(100.0);
                let rolloff_factor = map.get("rolloff_factor").and_then(val_to_f64).unwrap_or(1.0);
                let model = match map.get("model") {
                    Some(Value::String(s)) if s == "linear" => AttenuationModel::Linear,
                    Some(Value::String(s)) if s == "exponential" => AttenuationModel::Exponential,
                    _ => AttenuationModel::Inverse,
                };
                Ok(Self::new(
                    position,
                    velocity,
                    ref_distance,
                    max_distance,
                    rolloff_factor,
                    model,
                ))
            }
            _ => Err(RuntimeError::new(format!("{}: expected AudioSource3D map", fn_name)).at(span)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialParams {
    pub distance: f64,
    pub attenuation: f64,
    pub azimuth: f64,
    pub left_gain: f64,
    pub right_gain: f64,
    pub doppler_factor: f64,
}

impl SpatialParams {
    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        map.insert("distance".to_string(), Value::Float(self.distance));
        map.insert("attenuation".to_string(), Value::Float(self.attenuation));
        map.insert("azimuth".to_string(), Value::Float(self.azimuth));
        map.insert("left_gain".to_string(), Value::Float(self.left_gain));
        map.insert("right_gain".to_string(), Value::Float(self.right_gain));
        map.insert("doppler_factor".to_string(), Value::Float(self.doppler_factor));
        Value::Map(Arc::new(RwLock::new(map)))
    }
}

pub fn compute_spatial_params(listener: &AudioListener3D, source: &AudioSource3D) -> SpatialParams {
    let rel_pos = source.position.sub(&listener.position);
    let distance = rel_pos.length();

    // 1. Distance Attenuation
    let attenuation = if distance <= source.ref_distance {
        1.0
    } else if distance >= source.max_distance {
        0.0
    } else {
        match source.model {
            AttenuationModel::Inverse => {
                source.ref_distance
                    / (source.ref_distance
                        + source.rolloff_factor * (distance - source.ref_distance))
            }
            AttenuationModel::Linear => {
                (1.0 - source.rolloff_factor * (distance - source.ref_distance)
                    / (source.max_distance - source.ref_distance))
                    .max(0.0)
            }
            AttenuationModel::Exponential => {
                (distance / source.ref_distance).powf(-source.rolloff_factor)
            }
        }
        .clamp(0.0, 1.0)
    };

    if distance < 1e-6 {
        return SpatialParams {
            distance: 0.0,
            attenuation: 1.0,
            azimuth: 0.0,
            left_gain: std::f64::consts::FRAC_1_SQRT_2,
            right_gain: std::f64::consts::FRAC_1_SQRT_2,
            doppler_factor: 1.0,
        };
    }

    // 2. Coordinate System Transformation into Listener Frame
    let f = listener.forward.normalize();
    let u_raw = listener.up.normalize();
    let r = f.cross(&u_raw).normalize();

    let x_local = rel_pos.dot(&r); // Positive = right, Negative = left
    let z_local = rel_pos.dot(&f); // Positive = front, Negative = back

    // 3. Azimuth Angle: -PI to +PI (0 = front, PI/2 = right, -PI/2 = left)
    let azimuth = x_local.atan2(z_local);

    // 4. Equal-Power Panning Law (constant total acoustic power: left^2 + right^2 == 1.0)
    let pan = (x_local / distance).clamp(-1.0, 1.0);
    let pan_angle = (pan + 1.0) * 0.25 * PI;
    let left_gain = pan_angle.cos() * attenuation;
    let right_gain = pan_angle.sin() * attenuation;

    // 5. Doppler Pitch Shift Factor
    // Acoustics: f = f0 * (c + v_l) / (c - v_s)
    // where v_l is listener velocity towards source, and v_s is source velocity towards listener.
    let to_source = rel_pos.normalize();
    let to_listener = to_source.mul_scalar(-1.0);

    let v_l = listener.velocity.dot(&to_source);
    let v_s = source.velocity.dot(&to_listener);
    let doppler_factor = ((SPEED_OF_SOUND + v_l) / (SPEED_OF_SOUND - v_s).max(0.1)).clamp(0.1, 10.0);

    SpatialParams {
        distance,
        attenuation,
        azimuth,
        left_gain,
        right_gain,
        doppler_factor,
    }
}

/// Generates interleaved stereo PCM samples with 3D spatial panning and attenuation
pub fn spatialize_mono_samples(
    mono_samples: &[f32],
    listener: &AudioListener3D,
    source: &AudioSource3D,
) -> Vec<f32> {
    let params = compute_spatial_params(listener, source);
    let mut stereo = Vec::with_capacity(mono_samples.len() * 2);

    for &s in mono_samples {
        let left = s * params.left_gain as f32;
        let right = s * params.right_gain as f32;
        stereo.push(left);
        stereo.push(right);
    }

    stereo
}

// -----------------------------------------------------------------------------
// Global Spatial Audio State
// -----------------------------------------------------------------------------

pub struct SpatialAudioEngine {
    pub listener: AudioListener3D,
    pub sources: HashMap<usize, AudioSource3D>,
}

impl SpatialAudioEngine {
    pub fn new() -> Self {
        Self {
            listener: AudioListener3D::default(),
            sources: HashMap::new(),
        }
    }
}

static NEXT_SOURCE_ID: AtomicUsize = AtomicUsize::new(1);
static SPATIAL_ENGINE: OnceLock<RwLock<SpatialAudioEngine>> = OnceLock::new();

pub fn get_spatial_engine() -> &'static RwLock<SpatialAudioEngine> {
    SPATIAL_ENGINE.get_or_init(|| RwLock::new(SpatialAudioEngine::new()))
}

// -----------------------------------------------------------------------------
// Standard Library Module: std:audio:spatial / std:spatial_audio
// -----------------------------------------------------------------------------

pub fn build_audio_spatial_module() -> Value {
    let mut map = IndexMap::new();

    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("listener", audio_spatial_listener),
        ("source", audio_spatial_source),
        ("compute_params", audio_spatial_compute_params),
        ("set_listener_pos", audio_spatial_set_listener_pos),
        ("set_listener_orientation", audio_spatial_set_listener_orientation),
        ("set_listener_velocity", audio_spatial_set_listener_velocity),
        ("create_source", audio_spatial_create_source),
        ("set_source_pos", audio_spatial_set_source_pos),
        ("set_source_velocity", audio_spatial_set_source_velocity),
        ("spatialize_tone", audio_spatial_spatialize_tone),
        ("speed_of_sound", audio_spatial_speed_of_sound),
    ];

    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: format!("spatial_audio.{}", name),
                func,
            },
        );
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn audio_spatial_listener(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        let eng = get_spatial_engine().read().unwrap();
        return Ok(eng.listener.to_shae_value());
    }
    let pos = Vec3::from_shae_value(&args[0], "spatial_audio.listener pos", span)?;
    let forward = if args.len() > 1 {
        Vec3::from_shae_value(&args[1], "spatial_audio.listener forward", span)?
    } else {
        Vec3::FORWARD
    };
    let up = if args.len() > 2 {
        Vec3::from_shae_value(&args[2], "spatial_audio.listener up", span)?
    } else {
        Vec3::UP
    };
    let vel = if args.len() > 3 {
        Vec3::from_shae_value(&args[3], "spatial_audio.listener velocity", span)?
    } else {
        Vec3::ZERO
    };

    let l = AudioListener3D::new(pos, forward, up, vel);
    get_spatial_engine().write().unwrap().listener = l.clone();
    Ok(l.to_shae_value())
}

fn audio_spatial_source(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Ok(AudioSource3D::default().to_shae_value());
    }
    let pos = Vec3::from_shae_value(&args[0], "spatial_audio.source pos", span)?;
    let ref_dist = if args.len() > 1 {
        to_f64(&args[1], "spatial_audio.source ref_dist", span)?
    } else {
        1.0
    };
    let max_dist = if args.len() > 2 {
        to_f64(&args[2], "spatial_audio.source max_dist", span)?
    } else {
        100.0
    };
    let rolloff = if args.len() > 3 {
        to_f64(&args[3], "spatial_audio.source rolloff", span)?
    } else {
        1.0
    };
    let s = AudioSource3D::new(
        pos,
        Vec3::ZERO,
        ref_dist,
        max_dist,
        rolloff,
        AttenuationModel::Inverse,
    );
    Ok(s.to_shae_value())
}

fn audio_spatial_compute_params(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("spatial_audio.compute_params", 2, &args, span)?;
    let listener = AudioListener3D::from_shae_value(&args[0], "spatial_audio.compute_params listener", span)?;
    let source = AudioSource3D::from_shae_value(&args[1], "spatial_audio.compute_params source", span)?;
    let p = compute_spatial_params(&listener, &source);
    Ok(p.to_shae_value())
}

fn audio_spatial_set_listener_pos(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("spatial_audio.set_listener_pos", 3, &args, span)?;
    let x = to_f64(&args[0], "spatial_audio.set_listener_pos x", span)?;
    let y = to_f64(&args[1], "spatial_audio.set_listener_pos y", span)?;
    let z = to_f64(&args[2], "spatial_audio.set_listener_pos z", span)?;

    let mut eng = get_spatial_engine().write().unwrap();
    eng.listener.position = Vec3::new(x, y, z);
    Ok(Value::Bool(true))
}

fn audio_spatial_set_listener_orientation(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("spatial_audio.set_listener_orientation", 6, &args, span)?;
    let fx = to_f64(&args[0], "fx", span)?;
    let fy = to_f64(&args[1], "fy", span)?;
    let fz = to_f64(&args[2], "fz", span)?;
    let ux = to_f64(&args[3], "ux", span)?;
    let uy = to_f64(&args[4], "uy", span)?;
    let uz = to_f64(&args[5], "uz", span)?;

    let mut eng = get_spatial_engine().write().unwrap();
    eng.listener.forward = Vec3::new(fx, fy, fz).normalize();
    eng.listener.up = Vec3::new(ux, uy, uz).normalize();
    Ok(Value::Bool(true))
}

fn audio_spatial_set_listener_velocity(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("spatial_audio.set_listener_velocity", 3, &args, span)?;
    let vx = to_f64(&args[0], "vx", span)?;
    let vy = to_f64(&args[1], "vy", span)?;
    let vz = to_f64(&args[2], "vz", span)?;

    let mut eng = get_spatial_engine().write().unwrap();
    eng.listener.velocity = Vec3::new(vx, vy, vz);
    Ok(Value::Bool(true))
}

fn audio_spatial_create_source(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let pos = if !args.is_empty() {
        Vec3::from_shae_value(&args[0], "spatial_audio.create_source pos", span)?
    } else {
        Vec3::ZERO
    };
    let ref_dist = if args.len() > 1 {
        to_f64(&args[1], "ref_dist", span)?
    } else {
        1.0
    };
    let max_dist = if args.len() > 2 {
        to_f64(&args[2], "max_dist", span)?
    } else {
        100.0
    };
    let rolloff = if args.len() > 3 {
        to_f64(&args[3], "rolloff", span)?
    } else {
        1.0
    };

    let id = NEXT_SOURCE_ID.fetch_add(1, Ordering::SeqCst);
    let s = AudioSource3D::new(
        pos,
        Vec3::ZERO,
        ref_dist,
        max_dist,
        rolloff,
        AttenuationModel::Inverse,
    );
    get_spatial_engine().write().unwrap().sources.insert(id, s);
    Ok(Value::Int(id as i64))
}

fn audio_spatial_set_source_pos(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("spatial_audio.set_source_pos", 4, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("expected integer source id".to_string()).at(span)),
    };
    let x = to_f64(&args[1], "x", span)?;
    let y = to_f64(&args[2], "y", span)?;
    let z = to_f64(&args[3], "z", span)?;

    let mut eng = get_spatial_engine().write().unwrap();
    if let Some(src) = eng.sources.get_mut(&id) {
        src.position = Vec3::new(x, y, z);
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new(format!("audio source {} not found", id)).at(span))
    }
}

fn audio_spatial_set_source_velocity(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("spatial_audio.set_source_velocity", 4, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("expected integer source id".to_string()).at(span)),
    };
    let vx = to_f64(&args[1], "vx", span)?;
    let vy = to_f64(&args[2], "vy", span)?;
    let vz = to_f64(&args[3], "vz", span)?;

    let mut eng = get_spatial_engine().write().unwrap();
    if let Some(src) = eng.sources.get_mut(&id) {
        src.velocity = Vec3::new(vx, vy, vz);
        Ok(Value::Bool(true))
    } else {
        Err(RuntimeError::new(format!("audio source {} not found", id)).at(span))
    }
}

fn audio_spatial_spatialize_tone(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 5 {
        return Err(RuntimeError::new("spatial_audio.spatialize_tone requires (wave, freq, ms, listener, source)".to_string()).at(span));
    }
    let wave = match &args[0] {
        Value::String(s) => s.clone(),
        _ => "sine".to_string(),
    };
    let freq = to_f64(&args[1], "freq", span)?;
    let ms = match &args[2] {
        Value::Int(i) => *i as u64,
        Value::Float(f) => *f as u64,
        _ => 100,
    };
    let listener = AudioListener3D::from_shae_value(&args[3], "listener", span)?;
    let source = AudioSource3D::from_shae_value(&args[4], "source", span)?;

    let params = compute_spatial_params(&listener, &source);
    // Frequency shifted by Doppler
    let shifted_freq = (freq * params.doppler_factor).max(10.0);
    let mono = crate::audio::synthesize_waveform(shifted_freq, ms, &wave, 1.0);
    let stereo = spatialize_mono_samples(&mono, &listener, &source);

    let arr: Vec<Value> = stereo.into_iter().map(|f| Value::Float(f as f64)).collect();
    Ok(Value::Array(Arc::new(RwLock::new(arr))))
}

fn audio_spatial_speed_of_sound(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    Ok(Value::Float(SPEED_OF_SOUND))
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn val_to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        Value::Number(f) => Some(*f),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn to_f64(v: &Value, fn_name: &str, span: Span) -> Result<f64, RuntimeError> {
    val_to_f64(v).ok_or_else(|| {
        RuntimeError::new(format!("{}: expected number, got {}", fn_name, v.type_name())).at(span)
    })
}

// -----------------------------------------------------------------------------
// Built-in Unit Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_panning_left_and_right() {
        let listener = AudioListener3D::default();

        // Source to the right at (10, 0, 0)
        let source_right = AudioSource3D::new(
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::ZERO,
            1.0,
            100.0,
            1.0,
            AttenuationModel::Inverse,
        );
        let params_right = compute_spatial_params(&listener, &source_right);
        assert!(params_right.right_gain > params_right.left_gain);

        // Source to the left at (-10, 0, 0)
        let source_left = AudioSource3D::new(
            Vec3::new(-10.0, 0.0, 0.0),
            Vec3::ZERO,
            1.0,
            100.0,
            1.0,
            AttenuationModel::Inverse,
        );
        let params_left = compute_spatial_params(&listener, &source_left);
        assert!(params_left.left_gain > params_left.right_gain);

        // Symmetry: right gain on the right should equal left gain on the left
        assert!((params_right.right_gain - params_left.left_gain).abs() < 1e-6);
    }

    #[test]
    fn test_spatial_distance_attenuation() {
        let listener = AudioListener3D::default();

        let source_near = AudioSource3D::new(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::ZERO,
            1.0,
            100.0,
            1.0,
            AttenuationModel::Inverse,
        );
        let params_near = compute_spatial_params(&listener, &source_near);

        let source_far = AudioSource3D::new(
            Vec3::new(0.0, 0.0, -20.0),
            Vec3::ZERO,
            1.0,
            100.0,
            1.0,
            AttenuationModel::Inverse,
        );
        let params_far = compute_spatial_params(&listener, &source_far);

        assert!(params_near.attenuation > params_far.attenuation);
    }

    #[test]
    fn test_doppler_effect() {
        let listener = AudioListener3D::default();

        // Source at (0, 0, -100) approaching listener at 34.3 m/s (+Z direction)
        let source_approaching = AudioSource3D::new(
            Vec3::new(0.0, 0.0, -100.0),
            Vec3::new(0.0, 0.0, 34.3),
            1.0,
            1000.0,
            1.0,
            AttenuationModel::Inverse,
        );
        let params = compute_spatial_params(&listener, &source_approaching);
        // Higher frequency when approaching
        assert!(params.doppler_factor > 1.0);
    }
}
