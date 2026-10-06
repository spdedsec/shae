use crate::ast::{Expr, Literal, MatchArm, Pattern, Program, Span, Stmt, StmtKind};
use std::collections::HashSet;

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

pub struct Linter {
    scopes: Vec<HashSet<String>>,
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
            scopes: vec![HashSet::new()],
            builtins,
            diagnostics: Vec::new(),
        }
    }

    pub fn lint_program(&mut self, program: &Program) -> &[Diagnostic] {
        // First pass: collect top-level functions, structs, and enums so forward references work
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::FnDef { name, .. } => {
                    self.declare(name);
                }
                StmtKind::StructDef { name, .. } => {
                    self.declare(name);
                }
                StmtKind::EnumDef { name, .. } => {
                    self.declare(name);
                }
                _ => {}
            }
        }

        // Second pass: full lint
        for stmt in &program.statements {
            self.lint_stmt(stmt);
        }

        &self.diagnostics
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashSet::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    fn is_declared(&self, name: &str) -> bool {
        if self.builtins.contains(name) {
            return true;
        }
        for scope in self.scopes.iter().rev() {
            if scope.contains(name) {
                return true;
            }
        }
        false
    }

    fn all_accessible_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.builtins.iter().cloned().collect();
        for scope in &self.scopes {
            names.extend(scope.iter().cloned());
        }
        names
    }

    fn lint_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let { name, init } => {
                self.lint_expr(init);
                if let Some(scope) = self.scopes.last() {
                    if scope.contains(name) {
                        self.diagnostics.push(Diagnostic {
                            severity: DiagnosticSeverity::Warning,
                            message: format!("Variable '{}' is already declared in this scope", name),
                            hint: Some("Consider reusing the existing variable or using a different name.".into()),
                            span: stmt.span,
                        });
                    }
                }
                self.declare(name);
            }
            StmtKind::Assign { target, value } => {
                self.lint_expr(target);
                self.lint_expr(value);
            }
            StmtKind::FnDef { params, body, .. } => {
                self.push_scope();
                for param in params.iter() {
                    self.declare(param);
                }
                for s in body.iter() {
                    self.lint_stmt(s);
                }
                self.pop_scope();
            }
            StmtKind::If { condition, then_branch, else_branch } => {
                self.lint_expr(condition);
                self.push_scope();
                for s in then_branch {
                    self.lint_stmt(s);
                }
                self.pop_scope();
                if let Some(else_stmts) = else_branch {
                    self.push_scope();
                    for s in else_stmts {
                        self.lint_stmt(s);
                    }
                    self.pop_scope();
                }
            }
            StmtKind::While { condition, body } => {
                self.lint_expr(condition);
                self.push_scope();
                for s in body {
                    self.lint_stmt(s);
                }
                self.pop_scope();
            }
            StmtKind::For { item, iterable, body } => {
                self.lint_expr(iterable);
                self.push_scope();
                self.declare(item);
                for s in body {
                    self.lint_stmt(s);
                }
                self.pop_scope();
            }
            StmtKind::TryCatch { try_body, catch_ident, catch_body } => {
                self.push_scope();
                for s in try_body {
                    self.lint_stmt(s);
                }
                self.pop_scope();

                self.push_scope();
                self.declare(catch_ident);
                for s in catch_body {
                    self.lint_stmt(s);
                }
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
            StmtKind::StructDef { name, .. } => {
                self.declare(name);
            }
            StmtKind::EnumDef { name, .. } => {
                self.declare(name);
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
                for MatchArm { pattern, body } in arms {
                    self.push_scope();
                    self.bind_pattern(pattern);
                    self.lint_expr(body);
                    self.pop_scope();
                }
            }
            Expr::Lambda { params, body, .. } => {
                self.push_scope();
                for p in params.iter() {
                    self.declare(p);
                }
                for s in body.iter() {
                    self.lint_stmt(s);
                }
                self.pop_scope();
            }
        }
    }

    fn bind_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Wildcard | Pattern::Literal(Literal::Null) | Pattern::Literal(_) => {}
            Pattern::Variable(v) => {
                self.declare(v);
            }
            Pattern::Enum { fields, .. } => {
                for f in fields {
                    self.declare(f);
                }
            }
        }
    }
}
