// Shae GPU Compute Shader & High-Performance Tensor Engine
// Part of Option 3: Hardware-Accelerated Graphics & Compute

use crate::ast::Span;
use crate::builtins::expect_args;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

// -----------------------------------------------------------------------------
// Multi-Dimensional Tensor Representation
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Tensor {
    pub shape: Vec<usize>,
    pub strides: Vec<usize>,
    pub data: Vec<f64>,
}

impl Tensor {
    pub fn new(shape: Vec<usize>, data: Vec<f64>) -> Self {
        let strides = compute_strides(&shape);
        let expected_len: usize = shape.iter().product();
        let mut actual_data = data;
        if actual_data.len() < expected_len {
            actual_data.resize(expected_len, 0.0);
        }
        Self {
            shape,
            strides,
            data: actual_data,
        }
    }

    pub fn zeros(shape: Vec<usize>) -> Self {
        let size: usize = shape.iter().product();
        let strides = compute_strides(&shape);
        Self {
            shape,
            strides,
            data: vec![0.0; size],
        }
    }

    pub fn ones(shape: Vec<usize>) -> Self {
        let size: usize = shape.iter().product();
        let strides = compute_strides(&shape);
        Self {
            shape,
            strides,
            data: vec![1.0; size],
        }
    }

    pub fn arange(start: f64, stop: f64, step: f64) -> Self {
        let mut data = Vec::new();
        let mut curr = start;
        if step > 0.0 {
            while curr < stop {
                data.push(curr);
                curr += step;
            }
        } else if step < 0.0 {
            while curr > stop {
                data.push(curr);
                curr += step;
            }
        }
        let len = data.len();
        Self::new(vec![len], data)
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    pub fn num_dims(&self) -> usize {
        self.shape.len()
    }

    pub fn reshape(&self, new_shape: Vec<usize>) -> Result<Self, String> {
        let target_size: usize = new_shape.iter().product();
        if target_size != self.data.len() {
            return Err(format!(
                "Cannot reshape tensor of size {} into shape {:?}",
                self.data.len(),
                new_shape
            ));
        }
        let strides = compute_strides(&new_shape);
        Ok(Self {
            shape: new_shape,
            strides,
            data: self.data.clone(),
        })
    }

    pub fn transpose_2d(&self) -> Result<Self, String> {
        if self.shape.len() != 2 {
            return Err("transpose_2d requires a 2D tensor".to_string());
        }
        let rows = self.shape[0];
        let cols = self.shape[1];
        let mut out_data = vec![0.0; rows * cols];

        for r in 0..rows {
            for c in 0..cols {
                out_data[c * rows + r] = self.data[r * cols + c];
            }
        }

        Ok(Self::new(vec![cols, rows], out_data))
    }

    pub fn matmul(&self, other: &Self) -> Result<Self, String> {
        if self.shape.len() != 2 || other.shape.len() != 2 {
            return Err(format!(
                "matmul requires 2D tensors, got {:?} and {:?}",
                self.shape, other.shape
            ));
        }
        let m = self.shape[0];
        let k1 = self.shape[1];
        let k2 = other.shape[0];
        let n = other.shape[1];

        if k1 != k2 {
            return Err(format!(
                "Shape mismatch for matmul: [{}, {}] x [{}, {}]",
                m, k1, k2, n
            ));
        }

        let mut out = vec![0.0; m * n];
        let block_size = 64;

        // Tiled, cache-blocked matrix multiplication
        for i0 in (0..m).step_by(block_size) {
            let i_max = (i0 + block_size).min(m);
            for k0 in (0..k1).step_by(block_size) {
                let k_max = (k0 + block_size).min(k1);
                for j0 in (0..n).step_by(block_size) {
                    let j_max = (j0 + block_size).min(n);

                    for i in i0..i_max {
                        let i_offset = i * k1;
                        let out_offset = i * n;
                        for k in k0..k_max {
                            let a_val = self.data[i_offset + k];
                            if a_val != 0.0 {
                                let b_offset = k * n;
                                for j in j0..j_max {
                                    out[out_offset + j] += a_val * other.data[b_offset + j];
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Self::new(vec![m, n], out))
    }

    pub fn add(&self, other: &Self) -> Result<Self, String> {
        // Direct element-wise match
        if self.shape == other.shape {
            let res_data: Vec<f64> = self
                .data
                .iter()
                .zip(other.data.iter())
                .map(|(a, b)| a + b)
                .collect();
            return Ok(Self::new(self.shape.clone(), res_data));
        }

        // Bias broadcasting: [M, N] + [N] or [M, N] + [1, N]
        if self.shape.len() == 2 {
            let m = self.shape[0];
            let n = self.shape[1];

            if other.shape == vec![n] || other.shape == vec![1, n] {
                let mut out = self.data.clone();
                for i in 0..m {
                    for j in 0..n {
                        out[i * n + j] += other.data[j];
                    }
                }
                return Ok(Self::new(vec![m, n], out));
            }
        }

        // Scalar broadcasting: any shape + [1]
        if other.data.len() == 1 {
            let val = other.data[0];
            let out: Vec<f64> = self.data.iter().map(|x| x + val).collect();
            return Ok(Self::new(self.shape.clone(), out));
        }

        Err(format!(
            "Cannot add tensors with shapes {:?} and {:?}",
            self.shape, other.shape
        ))
    }

    pub fn sub(&self, other: &Self) -> Result<Self, String> {
        if self.shape == other.shape {
            let res_data: Vec<f64> = self
                .data
                .iter()
                .zip(other.data.iter())
                .map(|(a, b)| a - b)
                .collect();
            return Ok(Self::new(self.shape.clone(), res_data));
        }
        if other.data.len() == 1 {
            let val = other.data[0];
            let out: Vec<f64> = self.data.iter().map(|x| x - val).collect();
            return Ok(Self::new(self.shape.clone(), out));
        }
        Err(format!(
            "Cannot sub tensors with shapes {:?} and {:?}",
            self.shape, other.shape
        ))
    }

    pub fn mul(&self, other: &Self) -> Result<Self, String> {
        if self.shape == other.shape {
            let res_data: Vec<f64> = self
                .data
                .iter()
                .zip(other.data.iter())
                .map(|(a, b)| a * b)
                .collect();
            return Ok(Self::new(self.shape.clone(), res_data));
        }
        if other.data.len() == 1 {
            let val = other.data[0];
            let out: Vec<f64> = self.data.iter().map(|x| x * val).collect();
            return Ok(Self::new(self.shape.clone(), out));
        }
        Err(format!(
            "Cannot element-wise multiply tensors with shapes {:?} and {:?}",
            self.shape, other.shape
        ))
    }

    pub fn div(&self, other: &Self) -> Result<Self, String> {
        if self.shape == other.shape {
            let res_data: Vec<f64> = self
                .data
                .iter()
                .zip(other.data.iter())
                .map(|(a, b)| a / b)
                .collect();
            return Ok(Self::new(self.shape.clone(), res_data));
        }
        if other.data.len() == 1 {
            let val = other.data[0];
            let out: Vec<f64> = self.data.iter().map(|x| x / val).collect();
            return Ok(Self::new(self.shape.clone(), out));
        }
        Err(format!(
            "Cannot divide tensors with shapes {:?} and {:?}",
            self.shape, other.shape
        ))
    }

    pub fn relu(&self) -> Self {
        let out: Vec<f64> = self.data.iter().map(|&x| x.max(0.0)).collect();
        Self::new(self.shape.clone(), out)
    }

    pub fn sigmoid(&self) -> Self {
        let out: Vec<f64> = self.data.iter().map(|&x| 1.0 / (1.0 + (-x).exp())).collect();
        Self::new(self.shape.clone(), out)
    }

    pub fn softmax(&self, axis: usize) -> Result<Self, String> {
        if self.shape.len() == 1 || (self.shape.len() == 2 && axis == 1) {
            let (rows, cols) = if self.shape.len() == 1 {
                (1, self.shape[0])
            } else {
                (self.shape[0], self.shape[1])
            };

            let mut out = vec![0.0; rows * cols];
            for r in 0..rows {
                let row_start = r * cols;
                let row_slice = &self.data[row_start..row_start + cols];

                let max_val = row_slice.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let mut sum_exp = 0.0;
                let mut exps = Vec::with_capacity(cols);

                for &v in row_slice {
                    let exp_v = (v - max_val).exp();
                    exps.push(exp_v);
                    sum_exp += exp_v;
                }

                for c in 0..cols {
                    out[row_start + c] = exps[c] / sum_exp;
                }
            }

            return Ok(Self::new(self.shape.clone(), out));
        }

        Err("Softmax currently supported along axis 0 for 1D or axis 1 for 2D".to_string())
    }

    pub fn sum(&self) -> f64 {
        self.data.iter().sum()
    }

    pub fn mean(&self) -> f64 {
        if self.data.is_empty() {
            0.0
        } else {
            self.sum() / self.data.len() as f64
        }
    }

    pub fn max(&self) -> f64 {
        self.data.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
    }

    pub fn min(&self) -> f64 {
        self.data.iter().cloned().fold(f64::INFINITY, f64::min)
    }

    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();

        let shape_val: Vec<Value> = self.shape.iter().map(|&s| Value::Int(s as i64)).collect();
        let data_val: Vec<Value> = self.data.iter().map(|&d| Value::Float(d)).collect();

        map.insert(
            "shape".to_string(),
            Value::Array(Arc::new(RwLock::new(shape_val))),
        );
        map.insert(
            "data".to_string(),
            Value::Array(Arc::new(RwLock::new(data_val))),
        );
        map.insert("size".to_string(), Value::Int(self.size() as i64));
        map.insert("dims".to_string(), Value::Int(self.num_dims() as i64));

        Value::Map(Arc::new(RwLock::new(map)))
    }

    pub fn from_shae_value(v: &Value, fn_name: &str, span: Span) -> Result<Self, RuntimeError> {
        match v {
            Value::Map(m) => {
                let map = m.read().unwrap();
                let shape = match map.get("shape") {
                    Some(Value::Array(a)) => {
                        let arr = a.read().unwrap();
                        let mut s = Vec::with_capacity(arr.len());
                        for elem in arr.iter() {
                            match elem {
                                Value::Int(i) if *i >= 0 => s.push(*i as usize),
                                Value::Float(f) if *f >= 0.0 => s.push(*f as usize),
                                Value::Number(f) if *f >= 0.0 => s.push(*f as usize),
                                _ => {}
                            }
                        }
                        s
                    }
                    _ => Vec::new(),
                };

                let data = match map.get("data") {
                    Some(Value::Array(a)) => {
                        let arr = a.read().unwrap();
                        let mut d = Vec::with_capacity(arr.len());
                        for elem in arr.iter() {
                            match elem {
                                Value::Float(f) => d.push(*f),
                                Value::Number(f) => d.push(*f),
                                Value::Int(i) => d.push(*i as f64),
                                _ => d.push(0.0),
                            }
                        }
                        d
                    }
                    _ => Vec::new(),
                };

                if shape.is_empty() {
                    let len = data.len();
                    Ok(Self::new(vec![len], data))
                } else {
                    Ok(Self::new(shape, data))
                }
            }
            Value::Array(a) => {
                let arr = a.read().unwrap();
                let mut d = Vec::with_capacity(arr.len());
                for elem in arr.iter() {
                    match elem {
                        Value::Float(f) => d.push(*f),
                        Value::Number(f) => d.push(*f),
                        Value::Int(i) => d.push(*i as f64),
                        _ => d.push(0.0),
                    }
                }
                let len = d.len();
                Ok(Self::new(vec![len], d))
            }
            _ => Err(RuntimeError::new(format!("{}: expected Tensor map or array, got {}", fn_name, v.type_name())).at(span)),
        }
    }
}

fn compute_strides(shape: &[usize]) -> Vec<usize> {
    let mut strides = vec![1; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        strides[i] = strides[i + 1] * shape[i + 1];
    }
    strides
}

// -----------------------------------------------------------------------------
// Compute Shader & GPU Buffer Engine
// -----------------------------------------------------------------------------

pub struct ComputeBuffer {
    pub id: usize,
    pub size: usize,
    pub data: Vec<f64>,
}

pub struct GpuComputeDevice {
    pub buffers: HashMap<usize, ComputeBuffer>,
}

impl GpuComputeDevice {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
        }
    }

    pub fn create_buffer(&mut self, size: usize, initial: Option<Vec<f64>>) -> usize {
        let id = NEXT_GPU_BUF_ID.fetch_add(1, Ordering::SeqCst);
        let mut data = initial.unwrap_or_else(|| vec![0.0; size]);
        if data.len() < size {
            data.resize(size, 0.0);
        }
        self.buffers.insert(id, ComputeBuffer { id, size, data });
        id
    }

    pub fn write_buffer(&mut self, id: usize, offset: usize, data: &[f64]) -> Result<(), String> {
        let buf = self.buffers.get_mut(&id).ok_or_else(|| format!("Buffer {} not found", id))?;
        if offset + data.len() > buf.size {
            return Err("Buffer write overflow".to_string());
        }
        buf.data[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    pub fn read_buffer(&self, id: usize, offset: usize, count: usize) -> Result<Vec<f64>, String> {
        let buf = self.buffers.get(&id).ok_or_else(|| format!("Buffer {} not found", id))?;
        if offset + count > buf.size {
            return Err("Buffer read overflow".to_string());
        }
        Ok(buf.data[offset..offset + count].to_vec())
    }

    pub fn dispatch(&mut self, kernel: &str, workgroups: [usize; 3], buf_ids: &[usize]) -> Result<(), String> {
        let total_invocations = workgroups[0] * workgroups[1] * workgroups[2];
        match kernel {
            "saxpy" => {
                // y = a * x + y. buf_ids: [x_id, y_id], a = 2.0 (standard benchmark)
                if buf_ids.len() < 2 {
                    return Err("saxpy requires at least 2 buffers (x, y)".to_string());
                }
                let x_id = buf_ids[0];
                let y_id = buf_ids[1];
                let x_data = self.buffers.get(&x_id).map(|b| b.data.clone()).ok_or("Buffer x not found")?;
                let y_buf = self.buffers.get_mut(&y_id).ok_or("Buffer y not found")?;

                let count = total_invocations.min(x_data.len()).min(y_buf.data.len());
                for i in 0..count {
                    y_buf.data[i] = 2.0 * x_data[i] + y_buf.data[i];
                }
                Ok(())
            }
            "relu" => {
                if buf_ids.is_empty() {
                    return Err("relu kernel requires at least 1 buffer".to_string());
                }
                let buf = self.buffers.get_mut(&buf_ids[0]).ok_or("Buffer not found")?;
                let count = total_invocations.min(buf.data.len());
                for i in 0..count {
                    if buf.data[i] < 0.0 {
                        buf.data[i] = 0.0;
                    }
                }
                Ok(())
            }
            _ => Err(format!("Unknown or custom compute kernel '{}'", kernel)),
        }
    }
}

static NEXT_GPU_BUF_ID: AtomicUsize = AtomicUsize::new(1);
static GPU_DEVICE: OnceLock<Mutex<GpuComputeDevice>> = OnceLock::new();

fn get_gpu_device() -> &'static Mutex<GpuComputeDevice> {
    GPU_DEVICE.get_or_init(|| Mutex::new(GpuComputeDevice::new()))
}

// -----------------------------------------------------------------------------
// Standard Library Module: std:gpu & std:tensor
// -----------------------------------------------------------------------------

pub fn build_gpu_module() -> Value {
    let mut map = IndexMap::new();

    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("tensor", gpu_tensor),
        ("zeros", gpu_zeros),
        ("ones", gpu_ones),
        ("arange", gpu_arange),
        ("matmul", gpu_matmul),
        ("add", gpu_add),
        ("sub", gpu_sub),
        ("mul", gpu_mul),
        ("div", gpu_div),
        ("relu", gpu_relu),
        ("sigmoid", gpu_sigmoid),
        ("softmax", gpu_softmax),
        ("transpose", gpu_transpose),
        ("reshape", gpu_reshape),
        ("sum", gpu_sum),
        ("mean", gpu_mean),
        ("max", gpu_max),
        ("min", gpu_min),
        ("create_buffer", gpu_create_buffer),
        ("write_buffer", gpu_write_buffer),
        ("read_buffer", gpu_read_buffer),
        ("dispatch", gpu_dispatch),
    ];

    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: format!("gpu.{}", name),
                func,
            },
        );
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn gpu_tensor(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.tensor", 2, &args, span)?;
    let shape = extract_shape(&args[0], "gpu.tensor", span)?;
    let data = extract_data(&args[1], "gpu.tensor", span)?;
    Ok(Tensor::new(shape, data).to_shae_value())
}

fn gpu_zeros(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.zeros", 1, &args, span)?;
    let shape = extract_shape(&args[0], "gpu.zeros", span)?;
    Ok(Tensor::zeros(shape).to_shae_value())
}

fn gpu_ones(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.ones", 1, &args, span)?;
    let shape = extract_shape(&args[0], "gpu.ones", span)?;
    Ok(Tensor::ones(shape).to_shae_value())
}

fn gpu_arange(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new("gpu.arange requires (start, stop, [step])".to_string()).at(span));
    }
    let start = to_f64(&args[0], "gpu.arange", span)?;
    let stop = to_f64(&args[1], "gpu.arange", span)?;
    let step = if args.len() > 2 {
        to_f64(&args[2], "gpu.arange", span)?
    } else {
        1.0
    };
    Ok(Tensor::arange(start, stop, step).to_shae_value())
}

fn gpu_matmul(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.matmul", 2, &args, span)?;
    let a = Tensor::from_shae_value(&args[0], "gpu.matmul a", span)?;
    let b = Tensor::from_shae_value(&args[1], "gpu.matmul b", span)?;
    a.matmul(&b)
        .map(|t| t.to_shae_value())
        .map_err(|e| RuntimeError::new(format!("gpu.matmul: {}", e)).at(span))
}

fn gpu_add(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.add", 2, &args, span)?;
    let a = Tensor::from_shae_value(&args[0], "gpu.add a", span)?;
    let b = Tensor::from_shae_value(&args[1], "gpu.add b", span)?;
    a.add(&b)
        .map(|t| t.to_shae_value())
        .map_err(|e| RuntimeError::new(format!("gpu.add: {}", e)).at(span))
}

fn gpu_sub(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.sub", 2, &args, span)?;
    let a = Tensor::from_shae_value(&args[0], "gpu.sub a", span)?;
    let b = Tensor::from_shae_value(&args[1], "gpu.sub b", span)?;
    a.sub(&b)
        .map(|t| t.to_shae_value())
        .map_err(|e| RuntimeError::new(format!("gpu.sub: {}", e)).at(span))
}

fn gpu_mul(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.mul", 2, &args, span)?;
    let a = Tensor::from_shae_value(&args[0], "gpu.mul a", span)?;
    let b = Tensor::from_shae_value(&args[1], "gpu.mul b", span)?;
    a.mul(&b)
        .map(|t| t.to_shae_value())
        .map_err(|e| RuntimeError::new(format!("gpu.mul: {}", e)).at(span))
}

fn gpu_div(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.div", 2, &args, span)?;
    let a = Tensor::from_shae_value(&args[0], "gpu.div a", span)?;
    let b = Tensor::from_shae_value(&args[1], "gpu.div b", span)?;
    a.div(&b)
        .map(|t| t.to_shae_value())
        .map_err(|e| RuntimeError::new(format!("gpu.div: {}", e)).at(span))
}

fn gpu_relu(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.relu", 1, &args, span)?;
    let t = Tensor::from_shae_value(&args[0], "gpu.relu", span)?;
    Ok(t.relu().to_shae_value())
}

fn gpu_sigmoid(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.sigmoid", 1, &args, span)?;
    let t = Tensor::from_shae_value(&args[0], "gpu.sigmoid", span)?;
    Ok(t.sigmoid().to_shae_value())
}

fn gpu_softmax(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("gpu.softmax requires (tensor, [axis])".to_string()).at(span));
    }
    let t = Tensor::from_shae_value(&args[0], "gpu.softmax", span)?;
    let axis = if args.len() > 1 {
        match &args[1] {
            Value::Int(i) => *i as usize,
            _ => 1,
        }
    } else if t.shape.len() == 1 {
        0
    } else {
        1
    };
    t.softmax(axis)
        .map(|r| r.to_shae_value())
        .map_err(|e| RuntimeError::new(format!("gpu.softmax: {}", e)).at(span))
}

fn gpu_transpose(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.transpose", 1, &args, span)?;
    let t = Tensor::from_shae_value(&args[0], "gpu.transpose", span)?;
    t.transpose_2d()
        .map(|r| r.to_shae_value())
        .map_err(|e| RuntimeError::new(format!("gpu.transpose: {}", e)).at(span))
}

fn gpu_reshape(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.reshape", 2, &args, span)?;
    let t = Tensor::from_shae_value(&args[0], "gpu.reshape", span)?;
    let new_shape = extract_shape(&args[1], "gpu.reshape", span)?;
    t.reshape(new_shape)
        .map(|r| r.to_shae_value())
        .map_err(|e| RuntimeError::new(format!("gpu.reshape: {}", e)).at(span))
}

fn gpu_sum(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.sum", 1, &args, span)?;
    let t = Tensor::from_shae_value(&args[0], "gpu.sum", span)?;
    Ok(Value::Float(t.sum()))
}

fn gpu_mean(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.mean", 1, &args, span)?;
    let t = Tensor::from_shae_value(&args[0], "gpu.mean", span)?;
    Ok(Value::Float(t.mean()))
}

fn gpu_max(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.max", 1, &args, span)?;
    let t = Tensor::from_shae_value(&args[0], "gpu.max", span)?;
    Ok(Value::Float(t.max()))
}

fn gpu_min(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.min", 1, &args, span)?;
    let t = Tensor::from_shae_value(&args[0], "gpu.min", span)?;
    Ok(Value::Float(t.min()))
}

fn gpu_create_buffer(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("gpu.create_buffer requires (size, [initial_data])".to_string()).at(span));
    }
    let size = match &args[0] {
        Value::Int(i) if *i >= 0 => *i as usize,
        Value::Float(f) if *f >= 0.0 => *f as usize,
        _ => return Err(RuntimeError::new("gpu.create_buffer: invalid size".to_string()).at(span)),
    };
    let initial = if args.len() > 1 {
        Some(extract_data(&args[1], "gpu.create_buffer", span)?)
    } else {
        None
    };

    let id = get_gpu_device().lock().unwrap().create_buffer(size, initial);
    Ok(Value::Int(id as i64))
}

fn gpu_write_buffer(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.write_buffer", 3, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("gpu.write_buffer: invalid buffer id".to_string()).at(span)),
    };
    let offset = match &args[1] {
        Value::Int(i) if *i >= 0 => *i as usize,
        _ => 0,
    };
    let data = extract_data(&args[2], "gpu.write_buffer", span)?;

    get_gpu_device()
        .lock()
        .unwrap()
        .write_buffer(id, offset, &data)
        .map(|_| Value::Bool(true))
        .map_err(|e| RuntimeError::new(format!("gpu.write_buffer: {}", e)).at(span))
}

fn gpu_read_buffer(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.read_buffer", 3, &args, span)?;
    let id = match &args[0] {
        Value::Int(i) => *i as usize,
        _ => return Err(RuntimeError::new("gpu.read_buffer: invalid buffer id".to_string()).at(span)),
    };
    let offset = match &args[1] {
        Value::Int(i) if *i >= 0 => *i as usize,
        _ => 0,
    };
    let count = match &args[2] {
        Value::Int(i) if *i >= 0 => *i as usize,
        _ => 0,
    };

    get_gpu_device()
        .lock()
        .unwrap()
        .read_buffer(id, offset, count)
        .map(|d| {
            let arr: Vec<Value> = d.into_iter().map(Value::Float).collect();
            Value::Array(Arc::new(RwLock::new(arr)))
        })
        .map_err(|e| RuntimeError::new(format!("gpu.read_buffer: {}", e)).at(span))
}

fn gpu_dispatch(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    expect_args("gpu.dispatch", 3, &args, span)?;
    let kernel = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(RuntimeError::new("gpu.dispatch: expected kernel string".to_string()).at(span)),
    };
    let workgroups = match &args[1] {
        Value::Array(a) => {
            let arr = a.read().unwrap();
            let x = arr.first().and_then(val_to_usize).unwrap_or(1);
            let y = arr.get(1).and_then(val_to_usize).unwrap_or(1);
            let z = arr.get(2).and_then(val_to_usize).unwrap_or(1);
            [x, y, z]
        }
        Value::Int(i) if *i > 0 => [*i as usize, 1, 1],
        _ => [1, 1, 1],
    };
    let buf_ids = match &args[2] {
        Value::Array(a) => {
            let arr = a.read().unwrap();
            let mut ids = Vec::new();
            for item in arr.iter() {
                if let Some(id) = val_to_usize(item) {
                    ids.push(id);
                }
            }
            ids
        }
        _ => Vec::new(),
    };

    get_gpu_device()
        .lock()
        .unwrap()
        .dispatch(&kernel, workgroups, &buf_ids)
        .map(|_| Value::Bool(true))
        .map_err(|e| RuntimeError::new(format!("gpu.dispatch: {}", e)).at(span))
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn extract_shape(v: &Value, fn_name: &str, span: Span) -> Result<Vec<usize>, RuntimeError> {
    match v {
        Value::Array(a) => {
            let arr = a.read().unwrap();
            let mut res = Vec::with_capacity(arr.len());
            for elem in arr.iter() {
                let s = val_to_usize(elem).ok_or_else(|| {
                    RuntimeError::new(format!("{}: shape dimension must be a non-negative integer", fn_name)).at(span)
                })?;
                res.push(s);
            }
            Ok(res)
        }
        _ => Err(RuntimeError::new(format!("{}: expected shape array", fn_name)).at(span)),
    }
}

fn extract_data(v: &Value, fn_name: &str, span: Span) -> Result<Vec<f64>, RuntimeError> {
    match v {
        Value::Array(a) => {
            let arr = a.read().unwrap();
            let mut res = Vec::with_capacity(arr.len());
            for elem in arr.iter() {
                let d = val_to_f64(elem).ok_or_else(|| {
                    RuntimeError::new(format!("{}: tensor data element must be a number", fn_name)).at(span)
                })?;
                res.push(d);
            }
            Ok(res)
        }
        _ => Err(RuntimeError::new(format!("{}: expected tensor data array", fn_name)).at(span)),
    }
}

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

fn val_to_usize(v: &Value) -> Option<usize> {
    match v {
        Value::Int(i) if *i >= 0 => Some(*i as usize),
        Value::Float(f) if *f >= 0.0 => Some(*f as usize),
        Value::Number(f) if *f >= 0.0 => Some(*f as usize),
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// Built-in Unit Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tensor_matmul() {
        // A: 2x3
        // [[1, 2, 3],
        //  [4, 5, 6]]
        let a = Tensor::new(vec![2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);

        // B: 3x2
        // [[7, 8],
        //  [9, 1],
        //  [2, 3]]
        let b = Tensor::new(vec![3, 2], vec![7.0, 8.0, 9.0, 1.0, 2.0, 3.0]);

        // C = A x B: 2x2
        // [[1*7 + 2*9 + 3*2, 1*8 + 2*1 + 3*3],
        //  [4*7 + 5*9 + 6*2, 4*8 + 5*1 + 6*3]]
        // = [[7 + 18 + 6, 8 + 2 + 9],
        //    [28 + 45 + 12, 32 + 5 + 18]]
        // = [[31, 19],
        //    [85, 55]]
        let c = a.matmul(&b).expect("matmul should succeed");
        assert_eq!(c.shape, vec![2, 2]);
        assert_eq!(c.data, vec![31.0, 19.0, 85.0, 55.0]);
    }

    #[test]
    fn test_tensor_activations() {
        let t = Tensor::new(vec![4], vec![-2.0, 0.0, 1.5, 3.0]);
        let relu = t.relu();
        assert_eq!(relu.data, vec![0.0, 0.0, 1.5, 3.0]);

        let sm = t.softmax(0).expect("softmax");
        let sum: f64 = sm.data.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_compute_buffer_dispatch() {
        let mut dev = GpuComputeDevice::new();
        let x = dev.create_buffer(4, Some(vec![1.0, 2.0, 3.0, 4.0]));
        let y = dev.create_buffer(4, Some(vec![10.0, 20.0, 30.0, 40.0]));

        // y = 2 * x + y
        dev.dispatch("saxpy", [4, 1, 1], &[x, y]).expect("saxpy dispatch");
        let out = dev.read_buffer(y, 0, 4).expect("read y");
        assert_eq!(out, vec![12.0, 24.0, 36.0, 48.0]);
    }
}
