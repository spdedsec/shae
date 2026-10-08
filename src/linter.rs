use crate::ast::{BindingPattern, Expr, MatchArm, Pattern, Program, Span, Stmt, StmtKind};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub hint: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
struct VarInfo {
    span: Span,
    used: bool,
    is_param: bool,
    is_top_level_def: bool,
}

pub struct Linter {
    scopes: Vec<HashMap<String, VarInfo>>,
    builtins: HashSet<String>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Linter {
    pub fn new() -> Self {
        let mut builtins = HashSet::new();
        let list = [
            "print", "dbg", "len", "type", "str", "num", "range",
            "push", "pop", "map", "filter", "reduce", "sum", "sort",
            "keys", "values", "get", "json_parse", "json_stringify",
            "serve", "read", "write", "fetch", "time", "env", "exec",
            "spawn", "join", "assert", "assert_eq",
        ];
        for b in list {
            builtins.insert(b.to_string());
        }

        Self {
            scopes: vec![HashMap::new()],
            builtins,
            diagnostics: Vec::new(),
        }
    }

    pub fn lint_program(&mut self, program: &Program) -> &[Diagnostic] {
        // First pass: collect top-level functions, structs, and enums so forward references work
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::FnDef { name, .. }
                | StmtKind::StructDef { name, .. }
                | StmtKind::EnumDef { name, .. } => {
                    self.declare_top_level_def(name, stmt.span);
                }
                _ => {}
            }
        }

        // Second pass: full lint of statements in block
        self.lint_stmt_block(&program.statements);

        // Check top-level scope for unused let variables
        if let Some(global_scope) = self.scopes.pop() {
            for (name, info) in global_scope {
                if !info.used && !info.is_top_level_def && !name.starts_with('_') {
                    self.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        message: format!("Variable '{}' is declared but never read", name),
                        hint: Some(format!(
                            "If this is intentional, prefix with an underscore: '_{}'",
                            name
                        )),
                        span: info.span,
                    });
                }
            }
        }

        &self.diagnostics
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for (name, info) in scope {
                if !info.used && !name.starts_with('_') {
                    let (message, hint) = if info.is_param {
                        (
                            format!("Parameter '{}' is never used", name),
                            format!(
                                "If this is intentional, prefix with an underscore: '_{}'",
                                name
                            ),
                        )
                    } else {
                        (
                            format!("Variable '{}' is declared but never read", name),
                            format!(
                                "If this is intentional, prefix with an underscore: '_{}'",
                                name
                            ),
                        )
                    };
                    self.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        message,
                        hint: Some(hint),
                        span: info.span,
                    });
                }
            }
        }
    }

    fn declare_top_level_def(&mut self, name: &str, span: Span) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name.to_string(),
                VarInfo {
                    span,
                    used: true,
                    is_param: false,
                    is_top_level_def: true,
                },
            );
        }
    }

    fn declare_var(&mut self, name: &str, span: Span, is_param: bool) {
        if let Some(scope) = self.scopes.last() {
            if scope.contains_key(name) {
                self.diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Warning,
                    message: format!("Variable '{}' is already declared in this scope", name),
                    hint: Some("Consider reusing the existing variable or using a different name.".into()),
                    span,
                });
            } else if !name.starts_with('_') {
                if self.builtins.contains(name) {
                    self.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        message: format!("Variable '{}' shadows a built-in function", name),
                        hint: Some("Consider using a different name to avoid masking the built-in function.".into()),
                        span,
                    });
                } else if self.scopes.len() > 1 {
                    for outer in self.scopes[..self.scopes.len() - 1].iter().rev() {
                        if outer.contains_key(name) {
                            self.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Warning,
                                message: format!("Variable '{}' shadows an outer variable of the same name", name),
                                hint: Some("Consider using a different name to prevent unintentional shadowing.".into()),
                                span,
                            });
                            break;
                        }
                    }
                }
            }
        }

        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name.to_string(),
                VarInfo {
                    span,
                    used: false,
                    is_param,
                    is_top_level_def: false,
                },
            );
        }
    }

    fn mark_used(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.used = true;
                return;
            }
        }
    }

    fn is_declared(&self, name: &str) -> bool {
        if self.builtins.contains(name) {
            return true;
        }
        for scope in self.scopes.iter().rev() {
            if scope.contains_key(name) {
                return true;
            }
        }
        false
    }

    fn all_accessible_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.builtins.iter().cloned().collect();
        for scope in &self.scopes {
            names.extend(scope.keys().cloned());
        }
        names
    }

    fn lint_stmt_block(&mut self, stmts: &[Stmt]) {
        let mut terminated = false;
        for stmt in stmts {
            if terminated {
                self.diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Warning,
                    message: "Unreachable code detected".into(),
                    hint: Some("This statement follows a 'return', 'break', or 'continue' and will never execute.".into()),
                    span: stmt.span,
                });
            } else {
                self.lint_stmt(stmt);
                if stmt_always_terminates(stmt) {
                    terminated = true;
                }
            }
        }
    }

    fn lint_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let { pattern, init } => {
                self.lint_expr(init);
                self.declare_binding_pattern(pattern, stmt.span);
            }
            StmtKind::Assign { target, value } => {
                match target {
                    Expr::Variable { name, span } => {
                        if !self.is_declared(name) {
                            let candidates = self.all_accessible_names();
                            let hint = crate::suggest::closest(name, &candidates)
                                .map(|c| format!("Did you mean '{}'?", c));

                            self.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!("Undefined variable or symbol '{}'", name),
                                hint,
                                span: *span,
                            });
                        }
                    }
                    other => {
                        self.lint_expr(other);
                    }
                }
                self.lint_expr(value);
            }
            StmtKind::FnDef {
                name,
                params,
                body,
            } => {
                if self.scopes.len() > 1 {
                    self.declare_var(name, stmt.span, false);
                }
                self.push_scope();
                for param in params.iter() {
                    self.declare_var(param, stmt.span, true);
                }
                self.lint_stmt_block(body);
                self.pop_scope();
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.lint_expr(condition);
                self.push_scope();
                self.lint_stmt_block(then_branch);
                self.pop_scope();

                if let Some(else_stmts) = else_branch {
                    self.push_scope();
                    self.lint_stmt_block(else_stmts);
                    self.pop_scope();
                }
            }
            StmtKind::While { condition, body } => {
                self.lint_expr(condition);
                self.push_scope();
                self.lint_stmt_block(body);
                self.pop_scope();
            }
            StmtKind::For {
                item,
                iterable,
                body,
            } => {
                self.lint_expr(iterable);
                self.push_scope();
                self.declare_var(item, stmt.span, false);
                self.lint_stmt_block(body);
                self.pop_scope();
            }
            StmtKind::TryCatch {
                try_body,
                catch_ident,
                catch_body,
            } => {
                self.push_scope();
                self.lint_stmt_block(try_body);
                self.pop_scope();

                self.push_scope();
                self.declare_var(catch_ident, stmt.span, false);
                self.lint_stmt_block(catch_body);
                self.pop_scope();
            }
            StmtKind::Return(opt_expr) => {
                if let Some(expr) = opt_expr {
                    self.lint_expr(expr);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Expr(expr) => {
                self.lint_expr(expr);
            }
            StmtKind::StructDef { name, .. } | StmtKind::EnumDef { name, .. } => {
                if self.scopes.len() > 1 {
                    self.declare_var(name, stmt.span, false);
                }
            }
        }
    }

    fn lint_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal(_) => {}
            Expr::Variable { name, span } => {
                if !self.is_declared(name) {
                    let candidates = self.all_accessible_names();
                    let hint = crate::suggest::closest(name, &candidates)
                        .map(|c| format!("Did you mean '{}'?", c));

                    self.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!("Undefined variable or symbol '{}'", name),
                        hint,
                        span: *span,
                    });
                } else {
                    self.mark_used(name);
                }
            }
            Expr::Array(elements) => {
                for e in elements {
                    self.lint_expr(e);
                }
            }
            Expr::Map(pairs) => {
                for (_, v) in pairs {
                    self.lint_expr(v);
                }
            }
            Expr::Interpolated(parts) => {
                for part in parts {
                    if let crate::ast::InterpPart::Expr(e) = part {
                        self.lint_expr(e);
                    }
                }
            }
            Expr::Use { path, .. } => {
                self.lint_expr(path);
            }
            Expr::Binary { left, right, .. } => {
                self.lint_expr(left);
                self.lint_expr(right);
            }
            Expr::Unary { expr, .. } => {
                self.lint_expr(expr);
            }
            Expr::Call { callee, args, span } => {
                self.lint_expr(callee);
                for arg in args {
                    self.lint_expr(arg);
                }

                // Check built-in arities statically where possible
                if let Expr::Variable { name, .. } = &**callee {
                    let expected_arity = match name.as_str() {
                        "len" | "type" | "str" | "num" | "read" | "fetch" | "env" | "exec" | "spawn" | "join" => Some(1),
                        "push" | "write" | "serve" => Some(2),
                        _ => None,
                    };
                    if let Some(expected) = expected_arity {
                        if args.len() != expected {
                            self.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!("Built-in '{}' expects {} argument(s), but got {}", name, expected, args.len()),
                                hint: None,
                                span: *span,
                            });
                        }
                    }
                }
            }
            Expr::Get { target, .. } => {
                self.lint_expr(target);
            }
            Expr::Index { target, index, .. } => {
                self.lint_expr(target);
                self.lint_expr(index);
            }
            Expr::StructInit { fields, .. } => {
                for (_, f_expr) in fields {
                    self.lint_expr(f_expr);
                }
            }
            Expr::Match { target, arms, .. } => {
                self.lint_expr(target);
                let fallback = target.span().unwrap_or(Span { line: 0, col: 0 });
                for MatchArm { pattern, guard, body } in arms {
                    self.push_scope();
                    self.bind_pattern(pattern, fallback);
                    if let Some(g) = guard {
                        self.lint_expr(g);
                    }
                    self.lint_expr(body);
                    self.pop_scope();
                }
            }
            Expr::Lambda { params, body, span } => {
                self.push_scope();
                for p in params.iter() {
                    self.declare_var(p, *span, true);
                }
                self.lint_stmt_block(body);
                self.pop_scope();
            }
        }
    }

    fn bind_pattern(&mut self, pattern: &Pattern, fallback_span: Span) {
        match pattern {
            Pattern::Wildcard | Pattern::Literal(_) | Pattern::Range { .. } => {}
            Pattern::Variable(v) => {
                self.declare_var(v, fallback_span, false);
            }
            Pattern::Enum { fields, .. } => {
                for f in fields {
                    self.bind_pattern(f, fallback_span);
                }
            }
        }
    }

    fn declare_binding_pattern(&mut self, pattern: &BindingPattern, span: Span) {
        match pattern {
            BindingPattern::Ident(name) => {
                self.declare_var(name, span, false);
            }
            BindingPattern::Array { elements, rest } => {
                for elem in elements {
                    self.declare_binding_pattern(elem, span);
                }
                if let Some(r) = rest {
                    self.declare_var(r, span, false);
                }
            }
            BindingPattern::Object { fields, rest } => {
                for (name, opt_sub) in fields {
                    if let Some(sub) = opt_sub {
                        self.declare_binding_pattern(sub, span);
                    } else {
                        self.declare_var(name, span, false);
                    }
                }
                if let Some(r) = rest {
                    self.declare_var(r, span, false);
                }
            }
        }
    }
}

fn stmt_always_terminates(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue => true,
        StmtKind::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => block_always_terminates(then_branch) && block_always_terminates(else_branch),
        _ => false,
    }
}

fn block_always_terminates(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_always_terminates)
}
