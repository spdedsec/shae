// Shae LLVM IR (SSA) Code Generation Engine
// Targets LLVM IR version 18 (Ubuntu clang 18.1.3 on x86_64-pc-linux-gnu).

use crate::ast::{
    BinaryOp, BindingPattern, Expr, Literal, Param, Program, Stmt, StmtKind, TypeAnnotation,
    UnaryOp,
};
use crate::lexer;
use crate::parser;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlvmType {
    I64,
    Double,
    I1,
    I8Ptr,
    I32,
    Void,
}

impl LlvmType {
    pub fn to_ir(&self) -> &'static str {
        match self {
            LlvmType::I64 => "i64",
            LlvmType::Double => "double",
            LlvmType::I1 => "i1",
            LlvmType::I8Ptr => "i8*",
            LlvmType::I32 => "i32",
            LlvmType::Void => "void",
        }
    }

    pub fn align(&self) -> usize {
        match self {
            LlvmType::I64 | LlvmType::Double | LlvmType::I8Ptr => 8,
            LlvmType::I32 => 4,
            LlvmType::I1 => 1,
            LlvmType::Void => 1,
        }
    }

    pub fn default_val(&self) -> &'static str {
        match self {
            LlvmType::I64 => "0",
            LlvmType::Double => "0.0",
            LlvmType::I1 => "false",
            LlvmType::I8Ptr => "null",
            LlvmType::I32 => "0",
            LlvmType::Void => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlvmValue {
    pub ty: LlvmType,
    pub repr: String,
}

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub label: String,
    pub instructions: Vec<String>,
    pub terminated: bool,
}

#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub name: String,
    pub params: Vec<(String, LlvmType)>,
    pub return_type: LlvmType,
}

pub struct LlvmCompiler {
    pub target_triple: String,
    pub data_layout: String,
}

impl Default for LlvmCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl LlvmCompiler {
    pub fn new() -> Self {
        Self {
            target_triple: "x86_64-pc-linux-gnu".to_string(),
            data_layout:
                "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
                    .to_string(),
        }
    }

    pub fn compile_source(&mut self, source: &str) -> Result<String, String> {
        let tokens = lexer::tokenize(source).map_err(|e| format!("Lexer error: {:?}", e))?;
        let program = parser::parse(tokens).map_err(|e| format!("Parser error: {:?}", e))?;
        self.compile_program(&program)
    }

    pub fn compile_and_link(
        &mut self,
        source: &str,
        ll_path: &str,
        bin_path: &str,
        opt_level: u32,
    ) -> Result<(), String> {
        let ir = self.compile_source(source)?;
        std::fs::write(ll_path, &ir)
            .map_err(|e| format!("Failed to write LLVM IR to {}: {}", ll_path, e))?;

        let clang_bin = if std::path::Path::new("/usr/bin/clang").exists() {
            "/usr/bin/clang"
        } else {
            "clang"
        };

        let opt_flag = format!("-O{}", opt_level);
        let status = std::process::Command::new(clang_bin)
            .args([&opt_flag, ll_path, "-o", bin_path])
            .status()
            .map_err(|e| format!("Failed to run clang: {}", e))?;

        if !status.success() {
            return Err(format!("Clang failed with exit status {:?}", status.code()));
        }
        Ok(())
    }

    pub fn compile_program(&mut self, program: &Program) -> Result<String, String> {
        let mut string_pool: Vec<(String, String, usize)> = Vec::new();
        let mut string_map: HashMap<String, String> = HashMap::new();
        let mut globals: HashMap<String, LlvmType> = HashMap::new();
        let mut functions: HashMap<String, FunctionSig> = HashMap::new();
        let mut has_user_main = false;

        // 1. First pass: Register function signatures and top-level let variables
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::FnDef {
                    name,
                    params,
                    return_type,
                    ..
                } => {
                    let mut sig_params = Vec::new();
                    for p in params.iter() {
                        let ty = map_type_ann(&p.type_ann);
                        sig_params.push((p.name.clone(), ty));
                    }
                    let ret_ty = map_type_ann(return_type);
                    if name == "main" {
                        has_user_main = true;
                    }
                    functions.insert(
                        name.clone(),
                        FunctionSig {
                            name: name.clone(),
                            params: sig_params,
                            return_type: ret_ty,
                        },
                    );
                }
                StmtKind::Let {
                    pattern: BindingPattern::Ident(name),
                    type_ann,
                    init,
                } => {
                    let ty = match type_ann {
                        Some(ann) => map_type_ann(&Some(ann.clone())),
                        None => infer_expr_type(init, &globals, &[]),
                    };
                    globals.insert(name.clone(), ty);
                }
                _ => {}
            }
        }

        // 2. Compile user-defined functions
        let mut compiled_functions: Vec<String> = Vec::new();
        for stmt in &program.statements {
            if let StmtKind::FnDef {
                name,
                params,
                return_type,
                body,
                ..
            } = &stmt.kind
            {
                let llvm_func_name = if name == "main" {
                    "__shae_user_main".to_string()
                } else {
                    name.clone()
                };
                let ret_ty = map_type_ann(return_type);
                let mut fc = FunctionCompiler::new(
                    llvm_func_name,
                    ret_ty,
                    &globals,
                    &functions,
                    &mut string_pool,
                    &mut string_map,
                );

                // Add parameters as local allocas
                for (i, p) in params.iter().enumerate() {
                    let p_ty = map_type_ann(&p.type_ann);
                    let p_reg = format!("%p{}", i);
                    let slot = format!("%{}.addr", sanitize_ident(&p.name));
                    fc.allocas.push(format!(
                        "  {} = alloca {}, align {}",
                        slot,
                        p_ty.to_ir(),
                        p_ty.align()
                    ));
                    fc.emit(format!(
                        "  store {} {}, {}* {}, align {}",
                        p_ty.to_ir(),
                        p_reg,
                        p_ty.to_ir(),
                        slot,
                        p_ty.align()
                    ));
                    fc.locals
                        .last_mut()
                        .unwrap()
                        .insert(p.name.clone(), (p_ty, slot));
                }

                fc.compile_stmts(body)?;
                compiled_functions.push(fc.finish(params)?);
            }
        }

        // 3. Compile top-level statements into @main(i32 %argc, i8** %argv) -> i32
        let mut main_fc = FunctionCompiler::new(
            "main".to_string(),
            LlvmType::I32,
            &globals,
            &functions,
            &mut string_pool,
            &mut string_map,
        );

        let mut has_top_level_stmts = false;
        for stmt in &program.statements {
            if !matches!(stmt.kind, StmtKind::FnDef { .. }) {
                has_top_level_stmts = true;
                main_fc.compile_stmt(stmt)?;
            }
        }

        if !has_top_level_stmts && has_user_main {
            // Call user main if defined
            let reg = main_fc.next_reg();
            main_fc.emit(format!("  {} = call i64 @__shae_user_main()", reg));
            let ret_reg = main_fc.next_reg();
            main_fc.emit(format!("  {} = trunc i64 {} to i32", ret_reg, reg));
            main_fc.emit(format!("  ret i32 {}", ret_reg));
            main_fc.current_block_mut().terminated = true;
        }

        let main_func_code = main_fc.finish_main()?;

        // 4. Assemble the complete LLVM IR text
        let mut out = String::new();
        out.push_str("; ModuleID = 'shae_module'\n");
        out.push_str("source_filename = \"shae_module\"\n");
        out.push_str(&format!("target datalayout = \"{}\"\n", self.data_layout));
        out.push_str(&format!("target triple = \"{}\"\n\n", self.target_triple));

        // Format strings
        out.push_str("; --- Standard Format Strings ---\n");
        out.push_str(
            "@.fmt_int_nl = private unnamed_addr constant [5 x i8] c\"%ld\\0A\\00\", align 1\n",
        );
        out.push_str("@.fmt_int = private unnamed_addr constant [4 x i8] c\"%ld\\00\", align 1\n");
        out.push_str(
            "@.fmt_flt_nl = private unnamed_addr constant [4 x i8] c\"%f\\0A\\00\", align 1\n",
        );
        out.push_str("@.fmt_flt = private unnamed_addr constant [3 x i8] c\"%f\\00\", align 1\n");
        out.push_str(
            "@.fmt_str_nl = private unnamed_addr constant [4 x i8] c\"%s\\0A\\00\", align 1\n",
        );
        out.push_str("@.fmt_str = private unnamed_addr constant [3 x i8] c\"%s\\00\", align 1\n");
        out.push_str(
            "@.fmt_bool_t_nl = private unnamed_addr constant [6 x i8] c\"true\\0A\\00\", align 1\n",
        );
        out.push_str("@.fmt_bool_f_nl = private unnamed_addr constant [7 x i8] c\"false\\0A\\00\", align 1\n");
        out.push_str(
            "@.fmt_bool_t = private unnamed_addr constant [5 x i8] c\"true\\00\", align 1\n",
        );
        out.push_str(
            "@.fmt_bool_f = private unnamed_addr constant [6 x i8] c\"false\\00\", align 1\n",
        );
        out.push_str(
            "@.fmt_nl = private unnamed_addr constant [2 x i8] c\"\\0A\\00\", align 1\n\n",
        );

        // User string constants
        if !string_pool.is_empty() {
            out.push_str("; --- String Constants ---\n");
            for (label, escaped, len) in &string_pool {
                out.push_str(&format!(
                    "@{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1\n",
                    label, len, escaped
                ));
            }
            out.push('\n');
        }

        // Global variables
        if !globals.is_empty() {
            out.push_str("; --- Global Variables ---\n");
            for (name, ty) in &globals {
                out.push_str(&format!(
                    "@{} = internal global {} {}, align {}\n",
                    name,
                    ty.to_ir(),
                    ty.default_val(),
                    ty.align()
                ));
            }
            out.push('\n');
        }

        // External declarations
        out.push_str("; --- External Declarations ---\n");
        out.push_str("declare i32 @printf(i8*, ...)\n");
        out.push_str("declare i32 @puts(i8*)\n");
        out.push_str("declare i8* @malloc(i64)\n");
        out.push_str("declare void @free(i8*)\n");
        out.push_str("declare void @exit(i32)\n");
        out.push_str("declare i64 @strlen(i8*)\n");
        out.push_str("declare i8* @strcpy(i8*, i8*)\n");
        out.push_str("declare i8* @strcat(i8*, i8*)\n");
        out.push_str("declare i32 @strcmp(i8*, i8*)\n\n");

        // User functions
        if !compiled_functions.is_empty() {
            out.push_str("; --- User Functions ---\n");
            for func_code in compiled_functions {
                out.push_str(&func_code);
                out.push('\n');
            }
        }

        // Entrypoint main
        out.push_str("; --- Main Entrypoint ---\n");
        out.push_str(&main_func_code);

        Ok(out)
    }
}

pub struct FunctionCompiler<'a> {
    pub func_name: String,
    pub return_type: LlvmType,
    pub globals: &'a HashMap<String, LlvmType>,
    pub functions: &'a HashMap<String, FunctionSig>,
    pub string_pool: &'a mut Vec<(String, String, usize)>,
    pub string_map: &'a mut HashMap<String, String>,
    pub allocas: Vec<String>,
    pub blocks: Vec<BasicBlock>,
    pub current_block_idx: usize,
    pub locals: Vec<HashMap<String, (LlvmType, String)>>,
    pub loop_stack: Vec<(String, String)>,
    pub reg_counter: usize,
    pub label_counter: usize,
}

impl<'a> FunctionCompiler<'a> {
    pub fn new(
        func_name: String,
        return_type: LlvmType,
        globals: &'a HashMap<String, LlvmType>,
        functions: &'a HashMap<String, FunctionSig>,
        string_pool: &'a mut Vec<(String, String, usize)>,
        string_map: &'a mut HashMap<String, String>,
    ) -> Self {
        let entry_block = BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminated: false,
        };
        Self {
            func_name,
            return_type,
            globals,
            functions,
            string_pool,
            string_map,
            allocas: Vec::new(),
            blocks: vec![entry_block],
            current_block_idx: 0,
            locals: vec![HashMap::new()],
            loop_stack: Vec::new(),
            reg_counter: 1,
            label_counter: 1,
        }
    }

    pub fn next_reg(&mut self) -> String {
        let r = format!("%t.{}", self.reg_counter);
        self.reg_counter += 1;
        r
    }

    pub fn next_id(&mut self) -> usize {
        let id = self.label_counter;
        self.label_counter += 1;
        id
    }

    pub fn emit(&mut self, instr: String) {
        if !self.blocks[self.current_block_idx].terminated {
            self.blocks[self.current_block_idx].instructions.push(instr);
        }
    }

    pub fn current_block(&self) -> &BasicBlock {
        &self.blocks[self.current_block_idx]
    }

    pub fn current_block_mut(&mut self) -> &mut BasicBlock {
        &mut self.blocks[self.current_block_idx]
    }

    pub fn get_or_create_string(&mut self, s: &str) -> String {
        if let Some(label) = self.string_map.get(s) {
            return format!("@{}", label);
        }
        let (escaped, len) = escape_llvm_string(s.as_bytes());
        let label = format!(".str.{}", self.string_pool.len());
        self.string_pool.push((label.clone(), escaped, len));
        self.string_map.insert(s.to_string(), label.clone());
        format!("@{}", label)
    }

    pub fn cast_to(&mut self, val: LlvmValue, target_ty: &LlvmType) -> LlvmValue {
        if &val.ty == target_ty {
            return val;
        }
        match (&val.ty, target_ty) {
            (LlvmType::I1, LlvmType::I64) => {
                let reg = self.next_reg();
                self.emit(format!("  {} = zext i1 {} to i64", reg, val.repr));
                LlvmValue {
                    ty: LlvmType::I64,
                    repr: reg,
                }
            }
            (LlvmType::I64, LlvmType::I1) => {
                let reg = self.next_reg();
                self.emit(format!("  {} = icmp ne i64 {}, 0", reg, val.repr));
                LlvmValue {
                    ty: LlvmType::I1,
                    repr: reg,
                }
            }
            (LlvmType::I64, LlvmType::Double) => {
                let reg = self.next_reg();
                self.emit(format!("  {} = sitofp i64 {} to double", reg, val.repr));
                LlvmValue {
                    ty: LlvmType::Double,
                    repr: reg,
                }
            }
            (LlvmType::Double, LlvmType::I64) => {
                let reg = self.next_reg();
                self.emit(format!("  {} = fptosi double {} to i64", reg, val.repr));
                LlvmValue {
                    ty: LlvmType::I64,
                    repr: reg,
                }
            }
            (LlvmType::Double, LlvmType::I1) => {
                let reg = self.next_reg();
                self.emit(format!("  {} = fcmp one double {}, 0.0", reg, val.repr));
                LlvmValue {
                    ty: LlvmType::I1,
                    repr: reg,
                }
            }
            (LlvmType::I8Ptr, LlvmType::I1) => {
                let reg = self.next_reg();
                self.emit(format!("  {} = icmp ne i8* {}, null", reg, val.repr));
                LlvmValue {
                    ty: LlvmType::I1,
                    repr: reg,
                }
            }
            (LlvmType::I64, LlvmType::I32) => {
                let reg = self.next_reg();
                self.emit(format!("  {} = trunc i64 {} to i32", reg, val.repr));
                LlvmValue {
                    ty: LlvmType::I32,
                    repr: reg,
                }
            }
            (LlvmType::I32, LlvmType::I64) => {
                let reg = self.next_reg();
                self.emit(format!("  {} = sext i32 {} to i64", reg, val.repr));
                LlvmValue {
                    ty: LlvmType::I64,
                    repr: reg,
                }
            }
            _ => val,
        }
    }

    pub fn compile_stmts(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        for s in stmts {
            self.compile_stmt(s)?;
        }
        Ok(())
    }

    pub fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        if self.current_block().terminated {
            // Dead code after return/break
            return Ok(());
        }

        match &stmt.kind {
            StmtKind::Let {
                pattern,
                type_ann,
                init,
            } => {
                let name = match pattern {
                    BindingPattern::Ident(name) => name.clone(),
                    _ => {
                        return Err(
                            "Only identifier patterns are supported in LLVM code generation"
                                .to_string(),
                        );
                    }
                };

                let val = self.compile_expr(init)?;

                if self.func_name == "main" && self.globals.contains_key(&name) {
                    let g_ty = self.globals.get(&name).unwrap().clone();
                    let casted = self.cast_to(val, &g_ty);
                    self.emit(format!(
                        "  store {} {}, {}* @{}, align {}",
                        g_ty.to_ir(),
                        casted.repr,
                        g_ty.to_ir(),
                        name,
                        g_ty.align()
                    ));
                } else {
                    let ty = match type_ann {
                        Some(ann) => map_type_ann(&Some(ann.clone())),
                        None => val.ty.clone(),
                    };
                    let casted = self.cast_to(val, &ty);
                    let id = self.next_id();
                    let slot = format!("%{}.addr.{}", sanitize_ident(&name), id);
                    self.allocas.push(format!(
                        "  {} = alloca {}, align {}",
                        slot,
                        ty.to_ir(),
                        ty.align()
                    ));
                    self.emit(format!(
                        "  store {} {}, {}* {}, align {}",
                        ty.to_ir(),
                        casted.repr,
                        ty.to_ir(),
                        slot,
                        ty.align()
                    ));
                    self.locals.last_mut().unwrap().insert(name, (ty, slot));
                }
            }
            StmtKind::Assign { target, value } => match target {
                Expr::Variable { name, .. } => {
                    let val = self.compile_expr(value)?;
                    let mut found = None;
                    for scope in self.locals.iter().rev() {
                        if let Some((ty, slot)) = scope.get(name) {
                            found = Some((ty.clone(), slot.clone(), false));
                            break;
                        }
                    }
                    if found.is_none() {
                        if let Some(ty) = self.globals.get(name) {
                            found = Some((ty.clone(), format!("@{}", name), true));
                        }
                    }

                    if let Some((ty, slot, _is_global)) = found {
                        let casted = self.cast_to(val, &ty);
                        self.emit(format!(
                            "  store {} {}, {}* {}, align {}",
                            ty.to_ir(),
                            casted.repr,
                            ty.to_ir(),
                            slot,
                            ty.align()
                        ));
                    } else {
                        return Err(format!("Assignment to undefined variable '{}'", name));
                    }
                }
                Expr::Index {
                    target: base,
                    index,
                    ..
                } => {
                    let base_val = self.compile_expr(base)?;
                    let idx_val = self.compile_expr(index)?;
                    let val = self.compile_expr(value)?;

                    let ptr_reg = self.next_reg();
                    self.emit(format!(
                        "  {} = inttoptr i64 {} to i64*",
                        ptr_reg, base_val.repr
                    ));
                    let offset_reg = self.next_reg();
                    self.emit(format!("  {} = add i64 {}, 1", offset_reg, idx_val.repr));
                    let elem_ptr = self.next_reg();
                    self.emit(format!(
                        "  {} = getelementptr i64, i64* {}, i64 {}",
                        elem_ptr, ptr_reg, offset_reg
                    ));
                    let val_i64 = self.cast_to(val, &LlvmType::I64);
                    self.emit(format!(
                        "  store i64 {}, i64* {}, align 8",
                        val_i64.repr, elem_ptr
                    ));
                }
                _ => {
                    return Err("Unsupported assignment target in LLVM codegen".to_string());
                }
            },
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.compile_expr(condition)?;
                let cond_bool = self.cast_to(cond_val, &LlvmType::I1);

                let id = self.next_id();
                let then_lbl = format!("then.{}", id);
                let else_lbl = format!("else.{}", id);
                let merge_lbl = format!("merge.{}", id);

                if let Some(else_stmts) = else_branch {
                    self.emit(format!(
                        "  br i1 {}, label %{}, label %{}",
                        cond_bool.repr, then_lbl, else_lbl
                    ));
                    self.current_block_mut().terminated = true;

                    // Then branch
                    self.blocks.push(BasicBlock {
                        label: then_lbl,
                        instructions: Vec::new(),
                        terminated: false,
                    });
                    self.current_block_idx = self.blocks.len() - 1;
                    self.locals.push(HashMap::new());
                    self.compile_stmts(then_branch)?;
                    self.locals.pop();
                    let mut branches_to_merge = 0;
                    if !self.current_block().terminated {
                        self.emit(format!("  br label %{}", merge_lbl));
                        self.current_block_mut().terminated = true;
                        branches_to_merge += 1;
                    }

                    // Else branch
                    self.blocks.push(BasicBlock {
                        label: else_lbl,
                        instructions: Vec::new(),
                        terminated: false,
                    });
                    self.current_block_idx = self.blocks.len() - 1;
                    self.locals.push(HashMap::new());
                    self.compile_stmts(else_stmts)?;
                    self.locals.pop();
                    if !self.current_block().terminated {
                        self.emit(format!("  br label %{}", merge_lbl));
                        self.current_block_mut().terminated = true;
                        branches_to_merge += 1;
                    }

                    // Merge branch
                    if branches_to_merge > 0 {
                        self.blocks.push(BasicBlock {
                            label: merge_lbl,
                            instructions: Vec::new(),
                            terminated: false,
                        });
                        self.current_block_idx = self.blocks.len() - 1;
                    }
                } else {
                    self.emit(format!(
                        "  br i1 {}, label %{}, label %{}",
                        cond_bool.repr, then_lbl, merge_lbl
                    ));
                    self.current_block_mut().terminated = true;

                    // Then branch
                    self.blocks.push(BasicBlock {
                        label: then_lbl,
                        instructions: Vec::new(),
                        terminated: false,
                    });
                    self.current_block_idx = self.blocks.len() - 1;
                    self.locals.push(HashMap::new());
                    self.compile_stmts(then_branch)?;
                    self.locals.pop();
                    if !self.current_block().terminated {
                        self.emit(format!("  br label %{}", merge_lbl));
                        self.current_block_mut().terminated = true;
                    }

                    // Merge branch is always reachable from entry when cond is false
                    self.blocks.push(BasicBlock {
                        label: merge_lbl,
                        instructions: Vec::new(),
                        terminated: false,
                    });
                    self.current_block_idx = self.blocks.len() - 1;
                }
            }
            StmtKind::While { condition, body } => {
                let id = self.next_id();
                let cond_lbl = format!("loop.cond.{}", id);
                let body_lbl = format!("loop.body.{}", id);
                let exit_lbl = format!("loop.exit.{}", id);

                self.emit(format!("  br label %{}", cond_lbl));
                self.current_block_mut().terminated = true;

                // Condition block
                self.blocks.push(BasicBlock {
                    label: cond_lbl.clone(),
                    instructions: Vec::new(),
                    terminated: false,
                });
                self.current_block_idx = self.blocks.len() - 1;
                let cond_val = self.compile_expr(condition)?;
                let cond_bool = self.cast_to(cond_val, &LlvmType::I1);
                self.emit(format!(
                    "  br i1 {}, label %{}, label %{}",
                    cond_bool.repr, body_lbl, exit_lbl
                ));
                self.current_block_mut().terminated = true;

                // Body block
                self.blocks.push(BasicBlock {
                    label: body_lbl,
                    instructions: Vec::new(),
                    terminated: false,
                });
                self.current_block_idx = self.blocks.len() - 1;
                self.loop_stack.push((cond_lbl.clone(), exit_lbl.clone()));
                self.locals.push(HashMap::new());
                self.compile_stmts(body)?;
                self.locals.pop();
                self.loop_stack.pop();
                if !self.current_block().terminated {
                    self.emit(format!("  br label %{}", cond_lbl));
                    self.current_block_mut().terminated = true;
                }

                // Exit block
                self.blocks.push(BasicBlock {
                    label: exit_lbl,
                    instructions: Vec::new(),
                    terminated: false,
                });
                self.current_block_idx = self.blocks.len() - 1;
            }
            StmtKind::Break => {
                if let Some((_, exit_lbl)) = self.loop_stack.last() {
                    let exit = exit_lbl.clone();
                    self.emit(format!("  br label %{}", exit));
                    self.current_block_mut().terminated = true;
                } else {
                    return Err("Cannot 'break' outside of a loop".to_string());
                }
            }
            StmtKind::Continue => {
                if let Some((cond_lbl, _)) = self.loop_stack.last() {
                    let cond = cond_lbl.clone();
                    self.emit(format!("  br label %{}", cond));
                    self.current_block_mut().terminated = true;
                } else {
                    return Err("Cannot 'continue' outside of a loop".to_string());
                }
            }
            StmtKind::Return(opt_expr) => {
                if let Some(expr) = opt_expr {
                    let val = self.compile_expr(expr)?;
                    let casted = self.cast_to(val, &self.return_type.clone());
                    self.emit(format!(
                        "  ret {} {}",
                        self.return_type.to_ir(),
                        casted.repr
                    ));
                } else if self.return_type == LlvmType::Void {
                    self.emit("  ret void".to_string());
                } else {
                    self.emit(format!(
                        "  ret {} {}",
                        self.return_type.to_ir(),
                        self.return_type.default_val()
                    ));
                }
                self.current_block_mut().terminated = true;
            }
            StmtKind::Expr(expr) => {
                self.compile_expr(expr)?;
            }
            _ => {}
        }

        Ok(())
    }

    pub fn compile_expr(&mut self, expr: &Expr) -> Result<LlvmValue, String> {
        match expr {
            Expr::Literal(lit) => match lit {
                Literal::Int(n) => Ok(LlvmValue {
                    ty: LlvmType::I64,
                    repr: n.to_string(),
                }),
                Literal::Float(f) => Ok(LlvmValue {
                    ty: LlvmType::Double,
                    repr: format_float(*f),
                }),
                Literal::Number(f) => Ok(LlvmValue {
                    ty: LlvmType::Double,
                    repr: format_float(*f),
                }),
                Literal::Bool(b) => Ok(LlvmValue {
                    ty: LlvmType::I1,
                    repr: if *b { "true" } else { "false" }.to_string(),
                }),
                Literal::String(s) => {
                    let ptr_repr = self.get_or_create_string(s);
                    Ok(LlvmValue {
                        ty: LlvmType::I8Ptr,
                        repr: ptr_repr,
                    })
                }
                Literal::Null => Ok(LlvmValue {
                    ty: LlvmType::I64,
                    repr: "0".to_string(),
                }),
            },
            Expr::Variable { name, .. } => {
                // Check locals
                let local_found = self.locals.iter().rev().find_map(|s| s.get(name).cloned());
                if let Some((ty, slot)) = local_found {
                    let reg = self.next_reg();
                    self.emit(format!(
                        "  {} = load {}, {}* {}, align {}",
                        reg,
                        ty.to_ir(),
                        ty.to_ir(),
                        slot,
                        ty.align()
                    ));
                    return Ok(LlvmValue { ty, repr: reg });
                }
                // Check globals
                let global_ty = self.globals.get(name).cloned();
                if let Some(ty) = global_ty {
                    let reg = self.next_reg();
                    self.emit(format!(
                        "  {} = load {}, {}* @{}, align {}",
                        reg,
                        ty.to_ir(),
                        ty.to_ir(),
                        name,
                        ty.align()
                    ));
                    return Ok(LlvmValue { ty, repr: reg });
                }
                Err(format!("Undefined variable '{}'", name))
            }
            Expr::Unary { op, expr, .. } => {
                let val = self.compile_expr(expr)?;
                match op {
                    UnaryOp::Neg => {
                        if val.ty == LlvmType::Double {
                            let reg = self.next_reg();
                            self.emit(format!("  {} = fneg double {}", reg, val.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::Double,
                                repr: reg,
                            })
                        } else {
                            let v = self.cast_to(val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = sub i64 0, {}", reg, v.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I64,
                                repr: reg,
                            })
                        }
                    }
                    UnaryOp::Not => {
                        let v = self.cast_to(val, &LlvmType::I1);
                        let reg = self.next_reg();
                        self.emit(format!("  {} = xor i1 {}, true", reg, v.repr));
                        Ok(LlvmValue {
                            ty: LlvmType::I1,
                            repr: reg,
                        })
                    }
                    UnaryOp::BitNot => {
                        let v = self.cast_to(val, &LlvmType::I64);
                        let reg = self.next_reg();
                        self.emit(format!("  {} = xor i64 {}, -1", reg, v.repr));
                        Ok(LlvmValue {
                            ty: LlvmType::I64,
                            repr: reg,
                        })
                    }
                }
            }
            Expr::Binary {
                left, op, right, ..
            } => {
                if *op == BinaryOp::Pipe {
                    // Lower e.g. x |> f to f(x)
                    let call = Expr::Call {
                        callee: right.clone(),
                        args: vec![(**left).clone()],
                        span: crate::ast::Span::new(0, 0),
                    };
                    return self.compile_expr(&call);
                }

                let l_val = self.compile_expr(left)?;
                let r_val = self.compile_expr(right)?;

                match op {
                    BinaryOp::Add => {
                        if l_val.ty == LlvmType::I8Ptr || r_val.ty == LlvmType::I8Ptr {
                            // String concatenation
                            let ls = self.cast_to(l_val, &LlvmType::I8Ptr);
                            let rs = self.cast_to(r_val, &LlvmType::I8Ptr);
                            let l1 = self.next_reg();
                            self.emit(format!("  {} = call i64 @strlen(i8* {})", l1, ls.repr));
                            let l2 = self.next_reg();
                            self.emit(format!("  {} = call i64 @strlen(i8* {})", l2, rs.repr));
                            let tot = self.next_reg();
                            self.emit(format!("  {} = add i64 {}, {}", tot, l1, l2));
                            let tot1 = self.next_reg();
                            self.emit(format!("  {} = add i64 {}, 1", tot1, tot));
                            let mem = self.next_reg();
                            self.emit(format!("  {} = call i8* @malloc(i64 {})", mem, tot1));
                            let dummy1 = self.next_reg();
                            self.emit(format!(
                                "  {} = call i8* @strcpy(i8* {}, i8* {})",
                                dummy1, mem, ls.repr
                            ));
                            let dummy2 = self.next_reg();
                            self.emit(format!(
                                "  {} = call i8* @strcat(i8* {}, i8* {})",
                                dummy2, mem, rs.repr
                            ));
                            Ok(LlvmValue {
                                ty: LlvmType::I8Ptr,
                                repr: mem,
                            })
                        } else if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = fadd double {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::Double,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = add i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I64,
                                repr: reg,
                            })
                        }
                    }
                    BinaryOp::Sub => {
                        if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = fsub double {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::Double,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = sub i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I64,
                                repr: reg,
                            })
                        }
                    }
                    BinaryOp::Mul => {
                        if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = fmul double {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::Double,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = mul i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I64,
                                repr: reg,
                            })
                        }
                    }
                    BinaryOp::Div => {
                        if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = fdiv double {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::Double,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = sdiv i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I64,
                                repr: reg,
                            })
                        }
                    }
                    BinaryOp::Mod => {
                        if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = frem double {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::Double,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = srem i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I64,
                                repr: reg,
                            })
                        }
                    }
                    BinaryOp::BitAnd => {
                        let l = self.cast_to(l_val, &LlvmType::I64);
                        let r = self.cast_to(r_val, &LlvmType::I64);
                        let reg = self.next_reg();
                        self.emit(format!("  {} = and i64 {}, {}", reg, l.repr, r.repr));
                        Ok(LlvmValue {
                            ty: LlvmType::I64,
                            repr: reg,
                        })
                    }
                    BinaryOp::BitOr => {
                        let l = self.cast_to(l_val, &LlvmType::I64);
                        let r = self.cast_to(r_val, &LlvmType::I64);
                        let reg = self.next_reg();
                        self.emit(format!("  {} = or i64 {}, {}", reg, l.repr, r.repr));
                        Ok(LlvmValue {
                            ty: LlvmType::I64,
                            repr: reg,
                        })
                    }
                    BinaryOp::BitXor => {
                        let l = self.cast_to(l_val, &LlvmType::I64);
                        let r = self.cast_to(r_val, &LlvmType::I64);
                        let reg = self.next_reg();
                        self.emit(format!("  {} = xor i64 {}, {}", reg, l.repr, r.repr));
                        Ok(LlvmValue {
                            ty: LlvmType::I64,
                            repr: reg,
                        })
                    }
                    BinaryOp::Shl => {
                        let l = self.cast_to(l_val, &LlvmType::I64);
                        let r = self.cast_to(r_val, &LlvmType::I64);
                        let reg = self.next_reg();
                        self.emit(format!("  {} = shl i64 {}, {}", reg, l.repr, r.repr));
                        Ok(LlvmValue {
                            ty: LlvmType::I64,
                            repr: reg,
                        })
                    }
                    BinaryOp::Shr => {
                        let l = self.cast_to(l_val, &LlvmType::I64);
                        let r = self.cast_to(r_val, &LlvmType::I64);
                        let reg = self.next_reg();
                        self.emit(format!("  {} = ashr i64 {}, {}", reg, l.repr, r.repr));
                        Ok(LlvmValue {
                            ty: LlvmType::I64,
                            repr: reg,
                        })
                    }
                    BinaryOp::And => {
                        let l = self.cast_to(l_val, &LlvmType::I1);
                        let r = self.cast_to(r_val, &LlvmType::I1);
                        let reg = self.next_reg();
                        self.emit(format!("  {} = and i1 {}, {}", reg, l.repr, r.repr));
                        Ok(LlvmValue {
                            ty: LlvmType::I1,
                            repr: reg,
                        })
                    }
                    BinaryOp::Or => {
                        let l = self.cast_to(l_val, &LlvmType::I1);
                        let r = self.cast_to(r_val, &LlvmType::I1);
                        let reg = self.next_reg();
                        self.emit(format!("  {} = or i1 {}, {}", reg, l.repr, r.repr));
                        Ok(LlvmValue {
                            ty: LlvmType::I1,
                            repr: reg,
                        })
                    }
                    BinaryOp::Eq => {
                        if l_val.ty == LlvmType::I8Ptr && r_val.ty == LlvmType::I8Ptr {
                            let cmp_reg = self.next_reg();
                            self.emit(format!(
                                "  {} = call i32 @strcmp(i8* {}, i8* {})",
                                cmp_reg, l_val.repr, r_val.repr
                            ));
                            let reg = self.next_reg();
                            self.emit(format!("  {} = icmp eq i32 {}, 0", reg, cmp_reg));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        } else if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!(
                                "  {} = fcmp oeq double {}, {}",
                                reg, l.repr, r.repr
                            ));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        } else if l_val.ty == LlvmType::I1 && r_val.ty == LlvmType::I1 {
                            let reg = self.next_reg();
                            self.emit(format!(
                                "  {} = icmp eq i1 {}, {}",
                                reg, l_val.repr, r_val.repr
                            ));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = icmp eq i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        }
                    }
                    BinaryOp::NotEq => {
                        if l_val.ty == LlvmType::I8Ptr && r_val.ty == LlvmType::I8Ptr {
                            let cmp_reg = self.next_reg();
                            self.emit(format!(
                                "  {} = call i32 @strcmp(i8* {}, i8* {})",
                                cmp_reg, l_val.repr, r_val.repr
                            ));
                            let reg = self.next_reg();
                            self.emit(format!("  {} = icmp ne i32 {}, 0", reg, cmp_reg));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        } else if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!(
                                "  {} = fcmp one double {}, {}",
                                reg, l.repr, r.repr
                            ));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        } else if l_val.ty == LlvmType::I1 && r_val.ty == LlvmType::I1 {
                            let reg = self.next_reg();
                            self.emit(format!(
                                "  {} = icmp ne i1 {}, {}",
                                reg, l_val.repr, r_val.repr
                            ));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = icmp ne i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        }
                    }
                    BinaryOp::Lt => {
                        if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!(
                                "  {} = fcmp olt double {}, {}",
                                reg, l.repr, r.repr
                            ));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = icmp slt i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        }
                    }
                    BinaryOp::LtEq => {
                        if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!(
                                "  {} = fcmp ole double {}, {}",
                                reg, l.repr, r.repr
                            ));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = icmp sle i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        }
                    }
                    BinaryOp::Gt => {
                        if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!(
                                "  {} = fcmp ogt double {}, {}",
                                reg, l.repr, r.repr
                            ));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = icmp sgt i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        }
                    }
                    BinaryOp::GtEq => {
                        if l_val.ty == LlvmType::Double || r_val.ty == LlvmType::Double {
                            let l = self.cast_to(l_val, &LlvmType::Double);
                            let r = self.cast_to(r_val, &LlvmType::Double);
                            let reg = self.next_reg();
                            self.emit(format!(
                                "  {} = fcmp oge double {}, {}",
                                reg, l.repr, r.repr
                            ));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        } else {
                            let l = self.cast_to(l_val, &LlvmType::I64);
                            let r = self.cast_to(r_val, &LlvmType::I64);
                            let reg = self.next_reg();
                            self.emit(format!("  {} = icmp sge i64 {}, {}", reg, l.repr, r.repr));
                            Ok(LlvmValue {
                                ty: LlvmType::I1,
                                repr: reg,
                            })
                        }
                    }
                    _ => Err(format!(
                        "Unsupported binary operator in LLVM codegen: {:?}",
                        op
                    )),
                }
            }
            Expr::Call { callee, args, .. } => {
                if let Expr::Variable { name, .. } = &**callee {
                    if name == "println" || name == "print" {
                        let is_println = name == "println";
                        if args.is_empty() {
                            if is_println {
                                let reg = self.next_reg();
                                self.emit(format!(
                                    "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_nl)",
                                    reg
                                ));
                            }
                            return Ok(LlvmValue {
                                ty: LlvmType::I64,
                                repr: "0".to_string(),
                            });
                        }

                        // Single arg optimization using %ld\n, %f\n, %s\n
                        if is_println && args.len() == 1 {
                            let val = self.compile_expr(&args[0])?;
                            let reg = self.next_reg();
                            match val.ty {
                                LlvmType::I64 => {
                                    self.emit(format!(
                                        "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_int_nl, i64 {})",
                                        reg, val.repr
                                    ));
                                }
                                LlvmType::Double => {
                                    self.emit(format!(
                                        "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_flt_nl, double {})",
                                        reg, val.repr
                                    ));
                                }
                                LlvmType::I8Ptr => {
                                    self.emit(format!(
                                        "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_str_nl, i8* {})",
                                        reg, val.repr
                                    ));
                                }
                                LlvmType::I1 => {
                                    let sel = self.next_reg();
                                    self.emit(format!(
                                        "  {} = select i1 {}, i8* @.fmt_bool_t_nl, i8* @.fmt_bool_f_nl",
                                        sel, val.repr
                                    ));
                                    self.emit(format!(
                                        "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_str, i8* {})",
                                        reg, sel
                                    ));
                                }
                                _ => {
                                    let v = self.cast_to(val, &LlvmType::I64);
                                    self.emit(format!(
                                        "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_int_nl, i64 {})",
                                        reg, v.repr
                                    ));
                                }
                            }
                            return Ok(LlvmValue {
                                ty: LlvmType::I64,
                                repr: "0".to_string(),
                            });
                        }

                        // Multiple args
                        for arg in args {
                            let val = self.compile_expr(arg)?;
                            let reg = self.next_reg();
                            match val.ty {
                                LlvmType::I64 => {
                                    self.emit(format!(
                                        "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_int, i64 {})",
                                        reg, val.repr
                                    ));
                                }
                                LlvmType::Double => {
                                    self.emit(format!(
                                        "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_flt, double {})",
                                        reg, val.repr
                                    ));
                                }
                                LlvmType::I8Ptr => {
                                    self.emit(format!(
                                        "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_str, i8* {})",
                                        reg, val.repr
                                    ));
                                }
                                LlvmType::I1 => {
                                    let sel = self.next_reg();
                                    self.emit(format!(
                                        "  {} = select i1 {}, i8* @.fmt_bool_t, i8* @.fmt_bool_f",
                                        sel, val.repr
                                    ));
                                    self.emit(format!(
                                        "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_str, i8* {})",
                                        reg, sel
                                    ));
                                }
                                _ => {
                                    let v = self.cast_to(val, &LlvmType::I64);
                                    self.emit(format!(
                                        "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_int, i64 {})",
                                        reg, v.repr
                                    ));
                                }
                            }
                        }
                        if is_println {
                            let reg = self.next_reg();
                            self.emit(format!(
                                "  {} = call i32 (i8*, ...) @printf(i8* @.fmt_nl)",
                                reg
                            ));
                        }
                        return Ok(LlvmValue {
                            ty: LlvmType::I64,
                            repr: "0".to_string(),
                        });
                    }

                    if name == "puts" {
                        let str_val = if !args.is_empty() {
                            let val = self.compile_expr(&args[0])?;
                            self.cast_to(val, &LlvmType::I8Ptr)
                        } else {
                            LlvmValue {
                                ty: LlvmType::I8Ptr,
                                repr: "null".to_string(),
                            }
                        };
                        let reg = self.next_reg();
                        self.emit(format!("  {} = call i32 @puts(i8* {})", reg, str_val.repr));
                        return Ok(LlvmValue {
                            ty: LlvmType::I32,
                            repr: reg,
                        });
                    }

                    if name == "exit" {
                        let code_val = if !args.is_empty() {
                            let val = self.compile_expr(&args[0])?;
                            self.cast_to(val, &LlvmType::I32)
                        } else {
                            LlvmValue {
                                ty: LlvmType::I32,
                                repr: "0".to_string(),
                            }
                        };
                        self.emit(format!("  call void @exit(i32 {})", code_val.repr));
                        self.emit("  unreachable".to_string());
                        self.current_block_mut().terminated = true;
                        return Ok(LlvmValue {
                            ty: LlvmType::Void,
                            repr: "".to_string(),
                        });
                    }

                    if name == "malloc" {
                        let size_val = if !args.is_empty() {
                            let val = self.compile_expr(&args[0])?;
                            self.cast_to(val, &LlvmType::I64)
                        } else {
                            LlvmValue {
                                ty: LlvmType::I64,
                                repr: "0".to_string(),
                            }
                        };
                        let reg = self.next_reg();
                        self.emit(format!(
                            "  {} = call i8* @malloc(i64 {})",
                            reg, size_val.repr
                        ));
                        return Ok(LlvmValue {
                            ty: LlvmType::I8Ptr,
                            repr: reg,
                        });
                    }

                    if name == "free" {
                        let ptr_val = if !args.is_empty() {
                            let val = self.compile_expr(&args[0])?;
                            self.cast_to(val, &LlvmType::I8Ptr)
                        } else {
                            LlvmValue {
                                ty: LlvmType::I8Ptr,
                                repr: "null".to_string(),
                            }
                        };
                        self.emit(format!("  call void @free(i8* {})", ptr_val.repr));
                        return Ok(LlvmValue {
                            ty: LlvmType::Void,
                            repr: "".to_string(),
                        });
                    }

                    // User function call
                    let llvm_name = if name == "main" {
                        "__shae_user_main".to_string()
                    } else {
                        name.clone()
                    };

                    let sig = self.functions.get(name).cloned();
                    let mut arg_vals = Vec::new();

                    for (i, arg) in args.iter().enumerate() {
                        let val = self.compile_expr(arg)?;
                        let expected_ty = if let Some(ref s) = sig {
                            if i < s.params.len() {
                                s.params[i].1.clone()
                            } else {
                                val.ty.clone()
                            }
                        } else {
                            val.ty.clone()
                        };
                        let casted = self.cast_to(val, &expected_ty);
                        arg_vals.push(format!("{} {}", casted.ty.to_ir(), casted.repr));
                    }

                    let ret_ty = sig.map(|s| s.return_type).unwrap_or(LlvmType::I64);

                    if ret_ty == LlvmType::Void {
                        self.emit(format!(
                            "  call void @{}({})",
                            llvm_name,
                            arg_vals.join(", ")
                        ));
                        Ok(LlvmValue {
                            ty: LlvmType::Void,
                            repr: "".to_string(),
                        })
                    } else {
                        let reg = self.next_reg();
                        self.emit(format!(
                            "  {} = call {} @{}({})",
                            reg,
                            ret_ty.to_ir(),
                            llvm_name,
                            arg_vals.join(", ")
                        ));
                        Ok(LlvmValue {
                            ty: ret_ty,
                            repr: reg,
                        })
                    }
                } else {
                    Err("Dynamic indirect calls are not supported in LLVM IR backend".to_string())
                }
            }
            Expr::Array(elements) => {
                let len = elements.len();
                let bytes = (len + 1) * 8;
                let mem_reg = self.next_reg();
                self.emit(format!("  {} = call i8* @malloc(i64 {})", mem_reg, bytes));
                let arr_reg = self.next_reg();
                self.emit(format!("  {} = bitcast i8* {} to i64*", arr_reg, mem_reg));
                // store length at slot 0
                self.emit(format!("  store i64 {}, i64* {}, align 8", len, arr_reg));
                for (i, elem) in elements.iter().enumerate() {
                    let val = self.compile_expr(elem)?;
                    let val_i64 = self.cast_to(val, &LlvmType::I64);
                    let elem_ptr = self.next_reg();
                    self.emit(format!(
                        "  {} = getelementptr i64, i64* {}, i64 {}",
                        elem_ptr,
                        arr_reg,
                        i + 1
                    ));
                    self.emit(format!(
                        "  store i64 {}, i64* {}, align 8",
                        val_i64.repr, elem_ptr
                    ));
                }
                let int_arr = self.next_reg();
                self.emit(format!("  {} = ptrtoint i64* {} to i64", int_arr, arr_reg));
                Ok(LlvmValue {
                    ty: LlvmType::I64,
                    repr: int_arr,
                })
            }
            Expr::Index { target, index, .. } => {
                let base_val = self.compile_expr(target)?;
                let idx_val = self.compile_expr(index)?;
                let base_i64 = self.cast_to(base_val, &LlvmType::I64);
                let idx_i64 = self.cast_to(idx_val, &LlvmType::I64);

                let ptr_reg = self.next_reg();
                self.emit(format!(
                    "  {} = inttoptr i64 {} to i64*",
                    ptr_reg, base_i64.repr
                ));
                let offset_reg = self.next_reg();
                self.emit(format!("  {} = add i64 {}, 1", offset_reg, idx_i64.repr));
                let elem_ptr = self.next_reg();
                self.emit(format!(
                    "  {} = getelementptr i64, i64* {}, i64 {}",
                    elem_ptr, ptr_reg, offset_reg
                ));
                let reg = self.next_reg();
                self.emit(format!("  {} = load i64, i64* {}, align 8", reg, elem_ptr));
                Ok(LlvmValue {
                    ty: LlvmType::I64,
                    repr: reg,
                })
            }
            Expr::Get {
                target, property, ..
            } => {
                if property == "len" {
                    let base_val = self.compile_expr(target)?;
                    let base_i64 = self.cast_to(base_val, &LlvmType::I64);
                    let ptr_reg = self.next_reg();
                    self.emit(format!(
                        "  {} = inttoptr i64 {} to i64*",
                        ptr_reg, base_i64.repr
                    ));
                    let reg = self.next_reg();
                    self.emit(format!("  {} = load i64, i64* {}, align 8", reg, ptr_reg));
                    Ok(LlvmValue {
                        ty: LlvmType::I64,
                        repr: reg,
                    })
                } else {
                    Err(format!(
                        "Unsupported property get '{}' in LLVM backend",
                        property
                    ))
                }
            }
            _ => Err("Expression not supported in LLVM IR code generator".to_string()),
        }
    }

    pub fn finish(self, params: &[Param]) -> Result<String, String> {
        let mut param_strs = Vec::new();
        for (i, p) in params.iter().enumerate() {
            let ty = map_type_ann(&p.type_ann);
            param_strs.push(format!("{} %p{}", ty.to_ir(), i));
        }

        let mut out = String::new();
        out.push_str(&format!(
            "define {} @{}({}) {{\n",
            self.return_type.to_ir(),
            self.func_name,
            param_strs.join(", ")
        ));

        // Format basic blocks
        for (i, block) in self.blocks.iter().enumerate() {
            if i > 0 {
                out.push_str(&format!("\n{}:\n", block.label));
            } else {
                out.push_str("entry:\n");
                for alloca in &self.allocas {
                    out.push_str(alloca);
                    out.push('\n');
                }
            }
            for instr in &block.instructions {
                out.push_str(instr);
                out.push('\n');
            }
        }

        // If last block is not terminated, emit default return
        if let Some(last_block) = self.blocks.last() {
            if !last_block.terminated {
                match self.return_type {
                    LlvmType::I64 => out.push_str("  ret i64 0\n"),
                    LlvmType::Double => out.push_str("  ret double 0.0\n"),
                    LlvmType::I1 => out.push_str("  ret i1 false\n"),
                    LlvmType::I8Ptr => out.push_str("  ret i8* null\n"),
                    LlvmType::I32 => out.push_str("  ret i32 0\n"),
                    LlvmType::Void => out.push_str("  ret void\n"),
                }
            }
        }

        out.push_str("}\n");
        Ok(out)
    }

    pub fn finish_main(self) -> Result<String, String> {
        let mut out = String::new();
        out.push_str("define i32 @main(i32 %argc, i8** %argv) {\n");

        for (i, block) in self.blocks.iter().enumerate() {
            if i > 0 {
                out.push_str(&format!("\n{}:\n", block.label));
            } else {
                out.push_str("entry:\n");
                for alloca in &self.allocas {
                    out.push_str(alloca);
                    out.push('\n');
                }
            }
            for instr in &block.instructions {
                out.push_str(instr);
                out.push('\n');
            }
        }

        if let Some(last_block) = self.blocks.last() {
            if !last_block.terminated {
                out.push_str("  ret i32 0\n");
            }
        }

        out.push_str("}\n");
        Ok(out)
    }
}

fn map_type_ann(ann: &Option<TypeAnnotation>) -> LlvmType {
    match ann {
        Some(TypeAnnotation::Int) => LlvmType::I64,
        Some(TypeAnnotation::Float) => LlvmType::Double,
        Some(TypeAnnotation::Bool) => LlvmType::I1,
        Some(TypeAnnotation::String) => LlvmType::I8Ptr,
        Some(TypeAnnotation::Void) => LlvmType::Void,
        _ => LlvmType::I64,
    }
}

fn infer_expr_type(
    expr: &Expr,
    globals: &HashMap<String, LlvmType>,
    locals: &[HashMap<String, (LlvmType, String)>],
) -> LlvmType {
    match expr {
        Expr::Literal(Literal::Int(_)) => LlvmType::I64,
        Expr::Literal(Literal::Float(_)) | Expr::Literal(Literal::Number(_)) => LlvmType::Double,
        Expr::Literal(Literal::Bool(_)) => LlvmType::I1,
        Expr::Literal(Literal::String(_)) => LlvmType::I8Ptr,
        Expr::Variable { name, .. } => {
            for scope in locals.iter().rev() {
                if let Some((ty, _)) = scope.get(name) {
                    return ty.clone();
                }
            }
            if let Some(ty) = globals.get(name) {
                return ty.clone();
            }
            LlvmType::I64
        }
        Expr::Binary {
            op, left, right, ..
        } => match op {
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq
            | BinaryOp::And
            | BinaryOp::Or => LlvmType::I1,
            _ => {
                let lt = infer_expr_type(left, globals, locals);
                let rt = infer_expr_type(right, globals, locals);
                if lt == LlvmType::I8Ptr || rt == LlvmType::I8Ptr {
                    LlvmType::I8Ptr
                } else if lt == LlvmType::Double || rt == LlvmType::Double {
                    LlvmType::Double
                } else {
                    LlvmType::I64
                }
            }
        },
        Expr::Unary { op, expr, .. } => match op {
            UnaryOp::Not => LlvmType::I1,
            UnaryOp::Neg => infer_expr_type(expr, globals, locals),
            UnaryOp::BitNot => LlvmType::I64,
        },
        _ => LlvmType::I64,
    }
}

fn format_float(val: f64) -> String {
    let s = format!("{}", val);
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        format!("{}.0", s)
    } else {
        s
    }
}

fn sanitize_ident(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

fn escape_llvm_string(bytes: &[u8]) -> (String, usize) {
    let mut out = String::new();
    let mut len = 0;
    for &b in bytes {
        len += 1;
        if b == b'\\' {
            out.push_str("\\5C");
        } else if b == b'"' {
            out.push_str("\\22");
        } else if (32..127).contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\{:02X}", b));
        }
    }
    out.push_str("\\00");
    len += 1;
    (out, len)
}
