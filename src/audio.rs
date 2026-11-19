use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::f64::consts::PI;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

// -------------------------------------------------------------
// Audio Engine & Event Representation
// -------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AudioEvent {
    pub sound: String,
    pub time_secs: f64,
    pub duration_ms: u64,
    pub volume: f64,
    pub freq: Option<f64>,
}

pub struct AudioEngine {
    pub volume: f64,
    pub muted: bool,
    pub start_time: Instant,
    pub log: Vec<AudioEvent>,
    pub sample_rate: u32,
}

impl AudioEngine {
    pub fn new() -> Self {
        AudioEngine {
            volume: 0.8,
            muted: false,
            start_time: Instant::now(),
            log: Vec::new(),
            sample_rate: 44100,
        }
    }
}

pub fn get_audio_engine() -> &'static RwLock<AudioEngine> {
    static ENGINE: OnceLock<RwLock<AudioEngine>> = OnceLock::new();
    ENGINE.get_or_init(|| RwLock::new(AudioEngine::new()))
}

// -------------------------------------------------------------
// Software Waveform Synthesis
// -------------------------------------------------------------

pub fn synthesize_waveform(freq: f64, ms: u64, wave: &str, volume: f64) -> Vec<f32> {
    let sample_rate = 44100.0;
    let num_samples = ((ms as f64 / 1000.0) * sample_rate) as usize;
    let mut buffer = Vec::with_capacity(num_samples);
    let vol = volume.clamp(0.0, 1.0) as f32;

    match wave.to_lowercase().as_str() {
        "square" => {
            for i in 0..num_samples {
                let t = i as f64 / sample_rate;
                let sample = if (2.0 * PI * freq * t).sin() >= 0.0 {
                    1.0f32
                } else {
                    -1.0f32
                };
                buffer.push(sample * vol);
            }
        }
        "triangle" => {
            for i in 0..num_samples {
                let t = i as f64 / sample_rate;
                let sample = (2.0 * ((2.0 * PI * freq * t).sin().asin() / PI)) as f32;
                buffer.push(sample * vol);
            }
        }
        "saw" | "sawtooth" => {
            for i in 0..num_samples {
                let t = i as f64 / sample_rate;
                let sample = (2.0 * (freq * t - (freq * t + 0.5).floor())) as f32;
                buffer.push(sample * vol);
            }
        }
        "noise" => {
            let mut seed = 0x12345678u64;
            for _ in 0..num_samples {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let normalized = ((seed >> 32) as i32 as f32) / (i32::MAX as f32);
                buffer.push(normalized * vol);
            }
        }
        _ => {
            // Default: sine wave
            for i in 0..num_samples {
                let t = i as f64 / sample_rate;
                let sample = (2.0 * PI * freq * t).sin() as f32;
                buffer.push(sample * vol);
            }
        }
    }

    buffer
}

pub fn synthesize_laser_buffer(volume: f64) -> Vec<f32> {
    let sample_rate = 44100.0;
    let ms = 90.0;
    let num_samples = ((ms / 1000.0) * sample_rate) as usize;
    let mut buffer = Vec::with_capacity(num_samples);
    let vol = volume.clamp(0.0, 1.0) as f32;
    let mut phase = 0.0;

    for i in 0..num_samples {
        let progress = i as f64 / num_samples as f64;
        let freq = 880.0 - (880.0 - 220.0) * progress;
        phase += 2.0 * PI * freq / sample_rate;
        let amp = (1.0 - progress as f32).max(0.0);
        let sample = phase.sin() as f32 * amp * vol;
        buffer.push(sample);
    }
    buffer
}

pub fn synthesize_explosion_buffer(volume: f64) -> Vec<f32> {
    let sample_rate = 44100.0;
    let ms = 300.0;
    let num_samples = ((ms / 1000.0) * sample_rate) as usize;
    let mut buffer = Vec::with_capacity(num_samples);
    let vol = volume.clamp(0.0, 1.0) as f32;
    let mut seed = 0x87654321u64;

    for i in 0..num_samples {
        let t = i as f64 / sample_rate;
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let noise = ((seed >> 32) as i32 as f32) / (i32::MAX as f32);
        let decay = (-4.0 * t / (ms / 1000.0)).exp() as f32;
        buffer.push(noise * decay * vol);
    }
    buffer
}

pub fn synthesize_hit_buffer(volume: f64) -> Vec<f32> {
    let sample_rate = 44100.0;
    let ms = 120.0;
    let num_samples = ((ms / 1000.0) * sample_rate) as usize;
    let mut buffer = Vec::with_capacity(num_samples);
    let vol = volume.clamp(0.0, 1.0) as f32;
    let mut phase = 0.0;

    for i in 0..num_samples {
        let progress = i as f64 / num_samples as f64;
        let freq = 300.0 - (300.0 - 80.0) * progress;
        phase += 2.0 * PI * freq / sample_rate;
        let clean = phase.sin() as f32;
        // Crunchy overdrive distortion
        let distorted = (clean * 2.5).clamp(-1.0, 1.0);
        let amp = (1.0 - progress as f32).max(0.0);
        buffer.push(distorted * amp * vol);
    }
    buffer
}

// -------------------------------------------------------------
// Internal Audio Triggers
// -------------------------------------------------------------

fn record_event(sound: &str, duration_ms: u64, freq: Option<f64>) {
    let mut engine = get_audio_engine().write().unwrap();
    if engine.muted {
        return;
    }
    let elapsed = engine.start_time.elapsed().as_secs_f64();
    let vol = engine.volume;
    engine.log.push(AudioEvent {
        sound: sound.to_string(),
        time_secs: elapsed,
        duration_ms,
        volume: vol,
        freq,
    });
}

pub fn play_laser_internal() {
    let vol = {
        let engine = get_audio_engine().read().unwrap();
        if engine.muted {
            return;
        }
        engine.volume
    };
    let _samples = synthesize_laser_buffer(vol);
    record_event("laser", 90, Some(880.0));
}

pub fn play_explosion_internal() {
    let vol = {
        let engine = get_audio_engine().read().unwrap();
        if engine.muted {
            return;
        }
        engine.volume
    };
    let _samples = synthesize_explosion_buffer(vol);
    record_event("explosion", 300, None);
}

pub fn play_hit_internal() {
    let vol = {
        let engine = get_audio_engine().read().unwrap();
        if engine.muted {
            return;
        }
        engine.volume
    };
    let _samples = synthesize_hit_buffer(vol);
    record_event("hit", 120, Some(300.0));
}

pub fn play_jingle_internal(name: &str) {
    let vol = {
        let engine = get_audio_engine().read().unwrap();
        if engine.muted {
            return;
        }
        engine.volume
    };

    match name.to_lowercase().as_str() {
        "victory" => {
            let notes = [261.63, 329.63, 392.00, 523.25];
            let dur = 80;
            for &f in &notes {
                let _samples = synthesize_waveform(f, dur, "square", vol);
            }
            record_event("victory", 320, Some(523.25));
        }
        _ => {
            // Default: game_over
            let notes = [440.0, 370.0, 311.0, 220.0];
            let dur = 100;
            for &f in &notes {
                let _samples = synthesize_waveform(f, dur, "triangle", vol);
            }
            record_event("game_over", 400, Some(220.0));
        }
    }
}

pub fn play_tone_internal(freq: f64, ms: u64, wave: &str) {
    let vol = {
        let engine = get_audio_engine().read().unwrap();
        if engine.muted {
            return;
        }
        engine.volume
    };
    let _samples = synthesize_waveform(freq, ms, wave, vol);
    record_event("tone", ms, Some(freq));
}

// -------------------------------------------------------------
// Shae Standard Library std:audio Bindings
// -------------------------------------------------------------

pub fn build_audio_module() -> Value {
    crate::stdlib::make_map(vec![
        ("play_laser", audio_play_laser),
        ("play_explosion", audio_play_explosion),
        ("play_hit", audio_play_hit),
        ("play_jingle", audio_play_jingle),
        ("play_tone", audio_play_tone),
        ("set_volume", audio_set_volume),
        ("get_volume", audio_get_volume),
        ("mute", audio_mute),
        ("unmute", audio_unmute),
        ("is_muted", audio_is_muted),
        ("get_audio_log", audio_get_audio_log),
        ("last_sound", audio_last_sound),
        ("clear_log", audio_clear_log),
    ])
}

fn audio_play_laser(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    play_laser_internal();
    Ok(Value::Null)
}

fn audio_play_explosion(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    play_explosion_internal();
    Ok(Value::Null)
}

fn audio_play_hit(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    play_hit_internal();
    Ok(Value::Null)
}

fn audio_play_jingle(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let name = if !args.is_empty() {
        args[0].to_display()
    } else {
        "game_over".to_string()
    };
    play_jingle_internal(&name);
    Ok(Value::Null)
}

fn audio_play_tone(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("audio.play_tone", 3, &args, span)?;
    let freq = match &args[0] {
        Value::Int(i) => *i as f64,
        Value::Float(f) | Value::Number(f) => *f,
        _ => 440.0,
    };
    let ms = match &args[1] {
        Value::Int(i) => (*i).max(0) as u64,
        Value::Float(f) | Value::Number(f) => f.max(0.0) as u64,
        _ => 100,
    };
    let wave = match &args[2] {
        Value::String(s) => s.as_str(),
        _ => "sine",
    };
    play_tone_internal(freq, ms, wave);
    Ok(Value::Null)
}

fn audio_set_volume(
    _ev: &mut Evaluator,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    expect_args("audio.set_volume", 1, &args, span)?;
    let vol = match &args[0] {
        Value::Float(f) | Value::Number(f) => *f,
        Value::Int(i) => *i as f64,
        _ => 0.8,
    };
    let mut engine = get_audio_engine().write().unwrap();
    engine.volume = vol.clamp(0.0, 1.0);
    Ok(Value::Null)
}

fn audio_get_volume(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let vol = get_audio_engine().read().unwrap().volume;
    Ok(Value::Float(vol))
}

fn audio_mute(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let mut engine = get_audio_engine().write().unwrap();
    engine.muted = true;
    Ok(Value::Null)
}

fn audio_unmute(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let mut engine = get_audio_engine().write().unwrap();
    engine.muted = false;
    Ok(Value::Null)
}

fn audio_is_muted(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let muted = get_audio_engine().read().unwrap().muted;
    Ok(Value::Bool(muted))
}

fn audio_get_audio_log(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let engine = get_audio_engine().read().unwrap();
    let mut list = Vec::with_capacity(engine.log.len());
    for item in &engine.log {
        let mut map = IndexMap::new();
        map.insert("sound".to_string(), Value::String(item.sound.clone()));
        map.insert("time".to_string(), Value::Float(item.time_secs));
        map.insert("duration_ms".to_string(), Value::Int(item.duration_ms as i64));
        map.insert("volume".to_string(), Value::Float(item.volume));
        if let Some(f) = item.freq {
            map.insert("freq".to_string(), Value::Float(f));
        }
        list.push(Value::Map(Arc::new(RwLock::new(map))));
    }
    Ok(Value::Array(Arc::new(RwLock::new(list))))
}

fn audio_last_sound(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    let engine = get_audio_engine().read().unwrap();
    if let Some(last) = engine.log.last() {
        Ok(Value::String(last.sound.clone()))
    } else {
        Ok(Value::Null)
    }
}

pub fn clear_log_internal() {
    let mut engine = get_audio_engine().write().unwrap();
    engine.log.clear();
}

fn audio_clear_log(
    _ev: &mut Evaluator,
    _args: Vec<Value>,
    _span: Span,
) -> Result<Value, RuntimeError> {
    clear_log_internal();
    Ok(Value::Null)
}
