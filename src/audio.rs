use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::collections::HashMap;
use std::f32::consts::PI;
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

pub struct SendOutputStream(pub OutputStream);
unsafe impl Send for SendOutputStream {}
unsafe impl Sync for SendOutputStream {}

pub struct AudioState {
    _stream: Option<SendOutputStream>,
    stream_handle: Option<OutputStreamHandle>,
    active_sinks: HashMap<usize, Arc<Sink>>,
    master_volume: f32,
    is_headless: bool,
}

static AUDIO_STATE: OnceLock<Mutex<AudioState>> = OnceLock::new();
static NEXT_SOUND_ID: AtomicUsize = AtomicUsize::new(1);
static HEADLESS_MODE: AtomicBool = AtomicBool::new(false);

pub fn set_audio_headless(headless: bool) {
    HEADLESS_MODE.store(headless, Ordering::SeqCst);
}

fn get_audio_state() -> &'static Mutex<AudioState> {
    AUDIO_STATE.get_or_init(|| {
        let is_force_headless = HEADLESS_MODE.load(Ordering::SeqCst)
            || std::env::var("SHAE_HEADLESS").is_ok()
            || std::env::var("CI").is_ok();

        if is_force_headless {
            Mutex::new(AudioState {
                _stream: None,
                stream_handle: None,
                active_sinks: HashMap::new(),
                master_volume: 1.0,
                is_headless: true,
            })
        } else {
            match OutputStream::try_default() {
                Ok((stream, handle)) => Mutex::new(AudioState {
                    _stream: Some(SendOutputStream(stream)),
                    stream_handle: Some(handle),
                    active_sinks: HashMap::new(),
                    master_volume: 1.0,
                    is_headless: false,
                }),
                Err(_) => {
                    // Graceful fallback for environments without audio devices
                    Mutex::new(AudioState {
                        _stream: None,
                        stream_handle: None,
                        active_sinks: HashMap::new(),
                        master_volume: 1.0,
                        is_headless: true,
                    })
                }
            }
        }
    })
}

pub fn build_audio_module() -> Value {
    let mut map = IndexMap::new();
    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("beep", audio_beep),
        ("tone", audio_tone),
        ("play", audio_play),
        ("play_wav", audio_play),
        ("playWav", audio_play),
        ("loop", audio_loop),
        ("loop_wav", audio_loop),
        ("loopWav", audio_loop),
        ("stop", audio_stop),
        ("stop_sound", audio_stop_sound),
        ("stopSound", audio_stop_sound),
        ("set_volume", audio_set_volume),
        ("setVolume", audio_set_volume),
        ("get_volume", audio_get_volume),
        ("getVolume", audio_get_volume),
        ("is_playing", audio_is_playing),
        ("isPlaying", audio_is_playing),
        ("is_available", audio_is_available),
        ("isAvailable", audio_is_available),
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

fn to_f32(val: &Value) -> Option<f32> {
    match val {
        Value::Float(f) | Value::Number(f) => Some(*f as f32),
        Value::Int(i) => Some(*i as f32),
        _ => None,
    }
}

fn to_u64(val: &Value) -> Option<u64> {
    match val {
        Value::Int(i) if *i >= 0 => Some(*i as u64),
        Value::Float(f) | Value::Number(f) if *f >= 0.0 => Some(*f as u64),
        _ => None,
    }
}

fn generate_waveform_samples(
    waveform: &str,
    freq: f32,
    duration_ms: u64,
    sample_rate: u32,
    volume: f32,
) -> Vec<f32> {
    let num_samples = ((sample_rate as u64 * duration_ms) / 1000) as usize;
    let mut samples = Vec::with_capacity(num_samples);
    let freq = freq.max(20.0).min(20000.0);

    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let phase = (t * freq).fract(); // 0.0 to 1.0

        // Simple ADSR envelope to prevent clicks
        let envelope = if i < 100 {
            i as f32 / 100.0 // 100 samples attack
        } else if i > num_samples - 200 {
            (num_samples - i) as f32 / 200.0 // 200 samples decay
        } else {
            1.0
        };

        let sample = match waveform {
            "square" => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            "triangle" => {
                if phase < 0.5 {
                    4.0 * phase - 1.0
                } else {
                    3.0 - 4.0 * phase
                }
            }
            "saw" | "sawtooth" => 2.0 * phase - 1.0,
            _ => (2.0 * PI * freq * t).sin(), // sine default
        };

        samples.push(sample * volume * envelope);
    }

    samples
}

fn play_samples(samples: Vec<f32>, sample_rate: u32) -> Result<usize, String> {
    let sound_id = NEXT_SOUND_ID.fetch_add(1, Ordering::SeqCst);
    let mut state = get_audio_state().lock().unwrap();

    if state.is_headless || state.stream_handle.is_none() {
        return Ok(sound_id);
    }

    let handle = state.stream_handle.as_ref().unwrap();
    let sink = match Sink::try_new(handle) {
        Ok(s) => Arc::new(s),
        Err(e) => return Err(format!("Failed to create audio sink: {}", e)),
    };

    sink.set_volume(state.master_volume);
    let source = SamplesBuffer::new(1, sample_rate, samples);
    sink.append(source);

    // Clean up finished sinks
    state.active_sinks.retain(|_, s| !s.empty());
    state.active_sinks.insert(sound_id, sink);

    Ok(sound_id)
}

fn audio_beep(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let freq = if !args.is_empty() {
        to_f32(&args[0]).unwrap_or(440.0)
    } else {
        440.0
    };
    let duration_ms = if args.len() > 1 {
        to_u64(&args[1]).unwrap_or(200)
    } else {
        200
    };

    let samples = generate_waveform_samples("sine", freq, duration_ms, 44100, 0.5);
    match play_samples(samples, 44100) {
        Ok(id) => Ok(Value::Int(id as i64)),
        Err(e) => Err(RuntimeError::new(format!("audio.beep: {}", e)).at(span)),
    }
}

fn audio_tone(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(
            RuntimeError::new("audio.tone expects (freq, [waveform], [duration_ms], [volume])".into())
                .at(span),
        );
    }
    let freq = to_f32(&args[0]).unwrap_or(440.0);
    let waveform = if args.len() > 1 {
        match &args[1] {
            Value::String(s) => s.to_lowercase(),
            _ => "sine".to_string(),
        }
    } else {
        "sine".to_string()
    };
    let duration_ms = if args.len() > 2 {
        to_u64(&args[2]).unwrap_or(200)
    } else {
        200
    };
    let volume = if args.len() > 3 {
        to_f32(&args[3]).unwrap_or(0.5).max(0.0).min(1.0)
    } else {
        0.5
    };

    let samples = generate_waveform_samples(&waveform, freq, duration_ms, 44100, volume);
    match play_samples(samples, 44100) {
        Ok(id) => Ok(Value::Int(id as i64)),
        Err(e) => Err(RuntimeError::new(format!("audio.tone: {}", e)).at(span)),
    }
}

fn audio_play(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("audio.play", 1, &args, span)?;
    let path = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("audio.play: expected file path string".into()).at(span)),
    };

    let sound_id = NEXT_SOUND_ID.fetch_add(1, Ordering::SeqCst);
    let mut state = get_audio_state().lock().unwrap();

    if state.is_headless || state.stream_handle.is_none() {
        return Ok(Value::Int(sound_id as i64));
    }

    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            return Err(RuntimeError::new(format!(
                "audio.play: failed to open audio file '{}': {}",
                path, e
            ))
            .at(span));
        }
    };

    let source = match Decoder::new(BufReader::new(file)) {
        Ok(s) => s,
        Err(e) => {
            return Err(RuntimeError::new(format!(
                "audio.play: failed to decode audio file '{}': {}",
                path, e
            ))
            .at(span));
        }
    };

    let handle = state.stream_handle.as_ref().unwrap();
    let sink = match Sink::try_new(handle) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            return Err(RuntimeError::new(format!("audio.play: failed to create sink: {}", e)).at(span));
        }
    };

    sink.set_volume(state.master_volume);
    sink.append(source);

    state.active_sinks.retain(|_, s| !s.empty());
    state.active_sinks.insert(sound_id, sink);

    Ok(Value::Int(sound_id as i64))
}

fn audio_loop(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("audio.loop", 1, &args, span)?;
    let path = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("audio.loop: expected file path string".into()).at(span)),
    };

    let sound_id = NEXT_SOUND_ID.fetch_add(1, Ordering::SeqCst);
    let mut state = get_audio_state().lock().unwrap();

    if state.is_headless || state.stream_handle.is_none() {
        return Ok(Value::Int(sound_id as i64));
    }

    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            return Err(RuntimeError::new(format!(
                "audio.loop: failed to open audio file '{}': {}",
                path, e
            ))
            .at(span));
        }
    };

    let source = match Decoder::new_looped(BufReader::new(file)) {
        Ok(s) => s,
        Err(e) => {
            return Err(RuntimeError::new(format!(
                "audio.loop: failed to decode looped audio file '{}': {}",
                path, e
            ))
            .at(span));
        }
    };

    let handle = state.stream_handle.as_ref().unwrap();
    let sink = match Sink::try_new(handle) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            return Err(RuntimeError::new(format!("audio.loop: failed to create sink: {}", e)).at(span));
        }
    };

    sink.set_volume(state.master_volume);
    sink.append(source);

    state.active_sinks.retain(|_, s| !s.empty());
    state.active_sinks.insert(sound_id, sink);

    Ok(Value::Int(sound_id as i64))
}

fn audio_stop(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let mut state = get_audio_state().lock().unwrap();
    for (_, sink) in state.active_sinks.drain() {
        sink.stop();
    }
    Ok(Value::Null)
}

fn audio_stop_sound(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("audio.stop_sound", 1, &args, span)?;
    let sound_id = match to_u64(&args[0]) {
        Some(id) => id as usize,
        None => return Ok(Value::Bool(false)),
    };

    let mut state = get_audio_state().lock().unwrap();
    if let Some(sink) = state.active_sinks.remove(&sound_id) {
        sink.stop();
        Ok(Value::Bool(true))
    } else {
        Ok(Value::Bool(false))
    }
}

fn audio_set_volume(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("audio.set_volume", 1, &args, span)?;
    let vol = match to_f32(&args[0]) {
        Some(v) => v.max(0.0).min(1.0),
        None => {
            return Err(RuntimeError::new("audio.set_volume expects float (0.0 to 1.0)".into()).at(span))
        }
    };

    let mut state = get_audio_state().lock().unwrap();
    state.master_volume = vol;
    for sink in state.active_sinks.values() {
        sink.set_volume(vol);
    }
    Ok(Value::Null)
}

fn audio_get_volume(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let state = get_audio_state().lock().unwrap();
    Ok(Value::Float(state.master_volume as f64))
}

fn audio_is_playing(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let mut state = get_audio_state().lock().unwrap();
    state.active_sinks.retain(|_, s| !s.empty());
    Ok(Value::Bool(!state.active_sinks.is_empty()))
}

fn audio_is_available(_ev: &mut Evaluator, _args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let state = get_audio_state().lock().unwrap();
    Ok(Value::Bool(!state.is_headless && state.stream_handle.is_some()))
}
