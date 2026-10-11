use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::opcode::OpCode;
use crate::parser::Parser;
use crate::vm::{InterpretResult, VM};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct FunctionMetric {
    pub name: String,
    pub calls: usize,
    pub total_time: Duration,
    pub self_time: Duration,
}

#[derive(Debug, Clone)]
struct CallFrameTiming {
    name: String,
    start_time: Instant,
    children_time: Duration,
}

#[derive(Debug, Clone)]
pub struct Profiler {
    start_time: Instant,
    instruction_count: usize,
    opcode_counts: [usize; 256],
    function_stats: HashMap<String, FunctionMetric>,
    call_stack: Vec<CallFrameTiming>,
    folded_samples: HashMap<String, usize>,
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Profiler {
    pub fn new() -> Self {
        Profiler {
            start_time: Instant::now(),
            instruction_count: 0,
            opcode_counts: [0; 256],
            function_stats: HashMap::new(),
            call_stack: Vec::with_capacity(64),
            folded_samples: HashMap::new(),
        }
    }

    #[inline]
    pub fn on_opcode(&mut self, op: OpCode) {
        self.instruction_count += 1;
        let idx = op as usize;
        if idx < 256 {
            self.opcode_counts[idx] += 1;
        }

        // Periodically record folded stack sample for flamegraphs
        if self.instruction_count % 100 == 0 && !self.call_stack.is_empty() {
            let stack_repr = self
                .call_stack
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(";");
            *self.folded_samples.entry(stack_repr).or_insert(0) += 1;
        }
    }

    pub fn enter_function(&mut self, name: &str) {
        self.call_stack.push(CallFrameTiming {
            name: name.to_string(),
            start_time: Instant::now(),
            children_time: Duration::ZERO,
        });

        let entry = self
            .function_stats
            .entry(name.to_string())
            .or_insert_with(|| FunctionMetric {
                name: name.to_string(),
                calls: 0,
                total_time: Duration::ZERO,
                self_time: Duration::ZERO,
            });
        entry.calls += 1;
    }

    pub fn exit_function(&mut self, name: &str) {
        if let Some(frame) = self.call_stack.pop() {
            let elapsed = frame.start_time.elapsed();
            let self_time = elapsed.saturating_sub(frame.children_time);

            if let Some(parent) = self.call_stack.last_mut() {
                parent.children_time += elapsed;
            }

            if let Some(entry) = self.function_stats.get_mut(name) {
                entry.total_time += elapsed;
                entry.self_time += self_time;
            }
        }
    }

    pub fn finish(mut self) -> ProfileReport {
        // Drain any remaining un-exited frames
        while let Some(frame) = self.call_stack.pop() {
            let elapsed = frame.start_time.elapsed();
            let self_time = elapsed.saturating_sub(frame.children_time);
            if let Some(entry) = self.function_stats.get_mut(&frame.name) {
                entry.total_time += elapsed;
                entry.self_time += self_time;
            }
        }

        let total_time = self.start_time.elapsed();

        let mut functions: Vec<FunctionMetric> = self.function_stats.into_values().collect();
        functions.sort_by(|a, b| b.total_time.cmp(&a.total_time));

        let mut top_opcodes: Vec<(OpCode, usize)> = Vec::new();
        for (idx, &count) in self.opcode_counts.iter().enumerate() {
            if count > 0 && idx <= OpCode::TailCall as usize {
                let op: OpCode = (idx as u8).into();
                top_opcodes.push((op, count));
            }
        }
        top_opcodes.sort_by(|a, b| b.1.cmp(&a.1));

        ProfileReport {
            total_time,
            instruction_count: self.instruction_count,
            functions,
            top_opcodes,
            folded_samples: self.folded_samples,
        }
    }
}

pub struct ProfileReport {
    pub total_time: Duration,
    pub instruction_count: usize,
    pub functions: Vec<FunctionMetric>,
    pub top_opcodes: Vec<(OpCode, usize)>,
    pub folded_samples: HashMap<String, usize>,
}

impl ProfileReport {
    pub fn render_table(&self) -> String {
        let mut out = String::new();
        out.push_str("========================================================================================\n");
        out.push_str("                             SHAE CPU EXECUTION PROFILE                                \n");
        out.push_str("========================================================================================\n");
        out.push_str(&format!(
            "Total Elapsed Time : {:.3} ms\n",
            self.total_time.as_secs_f64() * 1000.0
        ));
        out.push_str(&format!(
            "Total Instructions : {} opcodes executed\n\n",
            self.instruction_count
        ));

        if !self.functions.is_empty() {
            out.push_str("Top Functions by Total Execution Time:\n");
            out.push_str(&format!(
                "{:<28} {:>10} {:>16} {:>16} {:>14}\n",
                "FUNCTION", "CALLS", "TOTAL TIME", "SELF TIME", "AVG / CALL"
            ));
            out.push_str(&"-".repeat(88));
            out.push('\n');

            for f in &self.functions {
                let total_ms = f.total_time.as_secs_f64() * 1000.0;
                let self_ms = f.self_time.as_secs_f64() * 1000.0;
                let avg_ms = if f.calls > 0 {
                    total_ms / f.calls as f64
                } else {
                    0.0
                };

                let fn_display = if f.name.len() > 27 {
                    format!("{}...", &f.name[..24])
                } else {
                    f.name.clone()
                };

                out.push_str(&format!(
                    "{:<28} {:>10} {:>13.3} ms {:>13.3} ms {:>11.3} ms\n",
                    fn_display, f.calls, total_ms, self_ms, avg_ms
                ));
            }
            out.push('\n');
        }

        if !self.top_opcodes.is_empty() {
            out.push_str("Top Dispatched OpCodes:\n");
            out.push_str(&format!(
                "{:<28} {:>12} {:>12}\n",
                "OPCODE", "EXECUTIONS", "PERCENTAGE"
            ));
            out.push_str(&"-".repeat(56));
            out.push('\n');

            for (op, count) in self.top_opcodes.iter().take(12) {
                let pct = if self.instruction_count > 0 {
                    (*count as f64 / self.instruction_count as f64) * 100.0
                } else {
                    0.0
                };
                out.push_str(&format!(
                    "{:<28} {:>12} {:>11.1}%\n",
                    format!("{:?}", op),
                    count,
                    pct
                ));
            }
        }

        out.push_str("========================================================================================\n");
        out
    }

    pub fn render_folded(&self) -> String {
        let mut out = String::new();
        let mut sorted: Vec<(&String, &usize)> = self.folded_samples.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (stack, count) in sorted {
            out.push_str(&format!("{} {}\n", stack, count));
        }
        out
    }
}

pub fn profile_file(path_str: &str, format_folded: bool) -> Result<(), String> {
    let source = fs::read_to_string(path_str)
        .map_err(|e| format!("Failed to read file '{}': {}", path_str, e))?;

    let mut lexer = Lexer::new(&source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| format!("Lexing error: {:?}", e))?;

    let mut parser = Parser::new(tokens);
    let program = parser
        .parse()
        .map_err(|e| format!("Parsing error: {:?}", e))?;

    let chunk = Compiler::new()
        .compile_program(&program)
        .map_err(|e| format!("Compilation error: {}", e))?;

    let mut vm = VM::new();
    vm.current_file = Some(Path::new(path_str).to_path_buf());
    vm.profiler = Some(Profiler::new());

    match vm.interpret(chunk) {
        InterpretResult::Ok(_) => {
            if let Some(prof) = vm.profiler.take() {
                let report = prof.finish();
                if format_folded {
                    print!("{}", report.render_folded());
                } else {
                    println!("{}", report.render_table());
                }
            }
            Ok(())
        }
        InterpretResult::RuntimeError(e) => Err(format!("Runtime error during profile: {}", e)),
        InterpretResult::CompileError => Err("VM compile error during profile".to_string()),
    }
}
