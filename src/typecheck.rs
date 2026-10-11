use crate::ast::{
    BinaryOp, BindingPattern, Expr, Literal, Pattern, Program, Stmt, StmtKind, TypeAnnotation,
    UnaryOp,
};
use crate::linter::{Diagnostic, DiagnosticSeverity};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub params: Vec<TypeAnnotation>,
    pub return_type: TypeAnnotation,
}

pub struct TypeChecker {
    scopes: Vec<HashMap<String, TypeAnnotation>>,
    struct_defs: HashMap<String, HashMap<String, TypeAnnotation>>,
    fn_signatures: HashMap<String, FunctionSig>,
    current_return_type: Option<TypeAnnotation>,
    pub diagnostics: Vec<Diagnostic>,
}

impl TypeChecker {
    pub fn new() -> Self {
        let mut tc = Self {
            scopes: vec![HashMap::new()],
            struct_defs: HashMap::new(),
            fn_signatures: HashMap::new(),
            current_return_type: None,
            diagnostics: Vec::new(),
        };
        tc.register_builtins();
        tc
    }

    fn register_builtins(&mut self) {
        let any = TypeAnnotation::Any;
        let int = TypeAnnotation::Int;
        let float = TypeAnnotation::Float;
        let string = TypeAnnotation::String;
        let bool_t = TypeAnnotation::Bool;
        let void = TypeAnnotation::Void;

        let builtins: Vec<(&str, Vec<TypeAnnotation>, TypeAnnotation)> = vec![
            ("print", vec![any.clone()], void.clone()),
            ("println", vec![any.clone()], void.clone()),
            ("dbg", vec![any.clone()], any.clone()),
            ("len", vec![any.clone()], int.clone()),
            ("type", vec![any.clone()], string.clone()),
            ("str", vec![any.clone()], string.clone()),
            ("num", vec![any.clone()], float.clone()),
            (
                "range",
                vec![int.clone(), int.clone()],
                TypeAnnotation::Array(Box::new(int.clone())),
            ),
            (
                "push",
                vec![TypeAnnotation::Array(Box::new(any.clone())), any.clone()],
                TypeAnnotation::Array(Box::new(any.clone())),
            ),
            (
                "pop",
                vec![TypeAnnotation::Array(Box::new(any.clone()))],
                any.clone(),
            ),
            (
                "map",
                vec![
                    TypeAnnotation::Array(Box::new(any.clone())),
                    TypeAnnotation::Function {
                        params: vec![any.clone()],
                        return_type: Box::new(any.clone()),
                    },
                ],
                TypeAnnotation::Array(Box::new(any.clone())),
            ),
            (
                "filter",
                vec![
                    TypeAnnotation::Array(Box::new(any.clone())),
                    TypeAnnotation::Function {
                        params: vec![any.clone()],
                        return_type: Box::new(bool_t.clone()),
                    },
                ],
                TypeAnnotation::Array(Box::new(any.clone())),
            ),
            (
                "sum",
                vec![TypeAnnotation::Array(Box::new(float.clone()))],
                float.clone(),
            ),
            (
                "sort",
                vec![TypeAnnotation::Array(Box::new(any.clone()))],
                TypeAnnotation::Array(Box::new(any.clone())),
            ),
            (
                "keys",
                vec![any.clone()],
                TypeAnnotation::Array(Box::new(string.clone())),
            ),
            (
                "values",
                vec![any.clone()],
                TypeAnnotation::Array(Box::new(any.clone())),
            ),
            ("get", vec![any.clone(), any.clone()], any.clone()),
            ("json_parse", vec![string.clone()], any.clone()),
            ("json_stringify", vec![any.clone()], string.clone()),
            (
                "serve",
                vec![
                    int.clone(),
                    TypeAnnotation::Function {
                        params: vec![any.clone()],
                        return_type: Box::new(any.clone()),
                    },
                ],
                void.clone(),
            ),
            ("read", vec![string.clone()], string.clone()),
            (
                "write",
                vec![string.clone(), string.clone()],
                bool_t.clone(),
            ),
            ("fetch", vec![string.clone()], any.clone()),
            ("time", vec![], int.clone()),
            (
                "env",
                vec![string.clone()],
                TypeAnnotation::Nullable(Box::new(string.clone())),
            ),
            ("assert", vec![bool_t.clone()], void.clone()),
            ("assert_eq", vec![any.clone(), any.clone()], void.clone()),
        ];

        for (name, params, ret) in builtins {
            self.fn_signatures.insert(
                name.to_string(),
                FunctionSig {
                    params,
                    return_type: ret,
                },
            );
        }
    }

    pub fn check_program(&mut self, program: &Program) -> &[Diagnostic] {
        // Pass 1: Collect struct definitions and function signatures
        for stmt in &program.statements {
            match &stmt.kind {
                StmtKind::StructDef { name, fields } => {
                    let mut field_map = HashMap::new();
                    for f in fields {
                        field_map.insert(
                            f.name.clone(),
                            f.type_ann.clone().unwrap_or(TypeAnnotation::Any),
                        );
                    }
                    self.struct_defs.insert(name.clone(), field_map);
                }
                StmtKind::FnDef {
                    name,
                    params,
                    return_type,
                    ..
                } => {
                    let sig_params = params
                        .iter()
                        .map(|p| p.type_ann.clone().unwrap_or(TypeAnnotation::Any))
                        .collect();
                    let sig_ret = return_type.clone().unwrap_or(TypeAnnotation::Any);
                    self.fn_signatures.insert(
                        name.clone(),
                        FunctionSig {
                            params: sig_params,
                            return_type: sig_ret,
                        },
                    );
                }
                _ => {}
            }
        }

        // Pass 2: Type check statements
        for stmt in &program.statements {
            self.check_stmt(stmt);
        }

        &self.diagnostics
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare_var(&mut self, name: &str, ty: TypeAnnotation) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), ty);
        }
    }

    fn lookup_var(&self, name: &str) -> Option<TypeAnnotation> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty.clone());
            }
        }
        if let Some(sig) = self.fn_signatures.get(name) {
            return Some(TypeAnnotation::Function {
                params: sig.params.clone(),
                return_type: Box::new(sig.return_type.clone()),
            });
        }
        None
    }

    fn is_assignable_to(&self, actual: &TypeAnnotation, expected: &TypeAnnotation) -> bool {
        if actual == expected || actual == &TypeAnnotation::Any || expected == &TypeAnnotation::Any
        {
            return true;
        }

        // Numeric widening: int -> float
        if expected == &TypeAnnotation::Float && actual == &TypeAnnotation::Int {
            return true;
        }

        // Nullable types: T?
        if let TypeAnnotation::Nullable(inner) = expected {
            if actual == &TypeAnnotation::Null {
                return true;
            }
            return self.is_assignable_to(actual, inner);
        }

        // Union types: T | U
        if let TypeAnnotation::Union(expected_variants) = expected {
            return expected_variants
                .iter()
                .any(|variant| self.is_assignable_to(actual, variant));
        }

        if let TypeAnnotation::Union(actual_variants) = actual {
            return actual_variants
                .iter()
                .all(|variant| self.is_assignable_to(variant, expected));
        }

        // Arrays: [T] -> [U]
        if let (TypeAnnotation::Array(act_elem), TypeAnnotation::Array(exp_elem)) =
            (actual, expected)
        {
            return self.is_assignable_to(act_elem, exp_elem);
        }

        // Maps: map<K1, V1> -> map<K2, V2>
        if let (
            TypeAnnotation::Map { key: k1, value: v1 },
            TypeAnnotation::Map { key: k2, value: v2 },
        ) = (actual, expected)
        {
            return self.is_assignable_to(k1, k2) && self.is_assignable_to(v1, v2);
        }

        // Functions
        if let (
            TypeAnnotation::Function {
                params: p1,
                return_type: r1,
            },
            TypeAnnotation::Function {
                params: p2,
                return_type: r2,
            },
        ) = (actual, expected)
        {
            if p1.len() != p2.len() {
                return false;
            }
            // Parameters are contravariant, return type is covariant
            let params_ok = p1
                .iter()
                .zip(p2.iter())
                .all(|(actual_p, expected_p)| self.is_assignable_to(expected_p, actual_p));
            let ret_ok = self.is_assignable_to(r1, r2);
            return params_ok && ret_ok;
        }

        false
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let {
                pattern,
                type_ann,
                init,
            } => {
                let init_ty = self.infer_expr(init);
                if let Some(declared) = type_ann {
                    if !self.is_assignable_to(&init_ty, declared) {
                        self.diagnostics.push(Diagnostic {
                            severity: DiagnosticSeverity::Error,
                            message: format!(
                                "Type mismatch: expected '{}', found '{}'",
                                declared, init_ty
                            ),
                            hint: Some(format!(
                                "The variable is annotated with type '{}' but initialized with '{}'.",
                                declared, init_ty
                            )),
                            span: stmt.span,
                        });
                    }
                    self.bind_pattern_type(pattern, declared);
                } else {
                    self.bind_pattern_type(pattern, &init_ty);
                }
            }
            StmtKind::Assign { target, value } => {
                let target_ty = self.infer_expr(target);
                let value_ty = self.infer_expr(value);
                if !self.is_assignable_to(&value_ty, &target_ty) {
                    self.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!(
                            "Type mismatch in assignment: cannot assign '{}' to '{}'",
                            value_ty, target_ty
                        ),
                        hint: Some(format!(
                            "Expected type compatible with '{}', but found '{}'.",
                            target_ty, value_ty
                        )),
                        span: stmt.span,
                    });
                }
            }
            StmtKind::FnDef {
                name: _,
                params,
                return_type,
                body,
            } => {
                self.push_scope();
                let declared_ret = return_type.clone().unwrap_or(TypeAnnotation::Any);
                let old_ret = self.current_return_type.take();
                self.current_return_type = Some(declared_ret.clone());

                for p in params.iter() {
                    let p_ty = p.type_ann.clone().unwrap_or(TypeAnnotation::Any);
                    self.declare_var(&p.name, p_ty);
                }

                for s in body.iter() {
                    self.check_stmt(s);
                }

                self.current_return_type = old_ret;
                self.pop_scope();
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.infer_expr(condition);
                self.push_scope();
                for s in then_branch {
                    self.check_stmt(s);
                }
                self.pop_scope();

                if let Some(else_b) = else_branch {
                    self.push_scope();
                    for s in else_b {
                        self.check_stmt(s);
                    }
                    self.pop_scope();
                }
            }
            StmtKind::While { condition, body } => {
                self.infer_expr(condition);
                self.push_scope();
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope();
            }
            StmtKind::For {
                item,
                iterable,
                body,
            } => {
                let iter_ty = self.infer_expr(iterable);
                let item_ty = match iter_ty {
                    TypeAnnotation::Array(elem) => *elem,
                    TypeAnnotation::Map { key, .. } => *key,
                    TypeAnnotation::String => TypeAnnotation::String,
                    _ => TypeAnnotation::Any,
                };
                self.push_scope();
                self.declare_var(item, item_ty);
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope();
            }
            StmtKind::TryCatch {
                try_body,
                catch_ident,
                catch_body,
            } => {
                self.push_scope();
                for s in try_body {
                    self.check_stmt(s);
                }
                self.pop_scope();

                self.push_scope();
                self.declare_var(catch_ident, TypeAnnotation::Any);
                for s in catch_body {
                    self.check_stmt(s);
                }
                self.pop_scope();
            }
            StmtKind::Return(opt_expr) => {
                let ret_val_ty = if let Some(expr) = opt_expr {
                    self.infer_expr(expr)
                } else {
                    TypeAnnotation::Void
                };

                if let Some(expected_ret) = &self.current_return_type {
                    if expected_ret != &TypeAnnotation::Any
                        && !self.is_assignable_to(&ret_val_ty, expected_ret)
                    {
                        self.diagnostics.push(Diagnostic {
                            severity: DiagnosticSeverity::Error,
                            message: format!(
                                "Return type mismatch: expected '{}', found '{}'",
                                expected_ret, ret_val_ty
                            ),
                            hint: Some(format!(
                                "Function is declared to return '{}', but returning '{}'.",
                                expected_ret, ret_val_ty
                            )),
                            span: stmt.span,
                        });
                    }
                }
            }
            StmtKind::Expr(expr) => {
                self.infer_expr(expr);
            }
            StmtKind::StructDef { .. }
            | StmtKind::EnumDef { .. }
            | StmtKind::Use { .. }
            | StmtKind::Break
            | StmtKind::Continue => {}
        }
    }

    fn bind_pattern_type(&mut self, pattern: &BindingPattern, ty: &TypeAnnotation) {
        match pattern {
            BindingPattern::Ident(name) => {
                self.declare_var(name, ty.clone());
            }
            BindingPattern::Array { elements, rest } => {
                let elem_ty = match ty {
                    TypeAnnotation::Array(inner) => (**inner).clone(),
                    _ => TypeAnnotation::Any,
                };
                for elem_pat in elements {
                    self.bind_pattern_type(elem_pat, &elem_ty);
                }
                if let Some(rest_name) = rest {
                    self.declare_var(rest_name, TypeAnnotation::Array(Box::new(elem_ty)));
                }
            }
            BindingPattern::Object { fields, rest } => {
                for (field_name, sub_pat) in fields {
                    let field_ty = match ty {
                        TypeAnnotation::Custom(struct_name) => {
                            if let Some(f_map) = self.struct_defs.get(struct_name) {
                                f_map
                                    .get(field_name)
                                    .cloned()
                                    .unwrap_or(TypeAnnotation::Any)
                            } else {
                                TypeAnnotation::Any
                            }
                        }
                        _ => TypeAnnotation::Any,
                    };
                    if let Some(pat) = sub_pat {
                        self.bind_pattern_type(pat, &field_ty);
                    } else {
                        self.declare_var(field_name, field_ty);
                    }
                }
                if let Some(rest_name) = rest {
                    self.declare_var(
                        rest_name,
                        TypeAnnotation::Map {
                            key: Box::new(TypeAnnotation::String),
                            value: Box::new(TypeAnnotation::Any),
                        },
                    );
                }
            }
        }
    }

    pub fn infer_expr(&mut self, expr: &Expr) -> TypeAnnotation {
        match expr {
            Expr::Literal(lit) => match lit {
                Literal::Int(_) => TypeAnnotation::Int,
                Literal::Float(_) | Literal::Number(_) => TypeAnnotation::Float,
                Literal::String(_) => TypeAnnotation::String,
                Literal::Bool(_) => TypeAnnotation::Bool,
                Literal::Null => TypeAnnotation::Null,
            },
            Expr::Variable { name, span: _ } => {
                self.lookup_var(name).unwrap_or(TypeAnnotation::Any)
            }
            Expr::Array(elements) => {
                if elements.is_empty() {
                    return TypeAnnotation::Array(Box::new(TypeAnnotation::Any));
                }
                let first_ty = self.infer_expr(&elements[0]);
                for elem in elements.iter().skip(1) {
                    let t = self.infer_expr(elem);
                    if !self.is_assignable_to(&t, &first_ty) {
                        return TypeAnnotation::Array(Box::new(TypeAnnotation::Any));
                    }
                }
                TypeAnnotation::Array(Box::new(first_ty))
            }
            Expr::Map(pairs) => {
                if pairs.is_empty() {
                    return TypeAnnotation::Map {
                        key: Box::new(TypeAnnotation::Any),
                        value: Box::new(TypeAnnotation::Any),
                    };
                }
                let val_ty = self.infer_expr(&pairs[0].1);
                TypeAnnotation::Map {
                    key: Box::new(TypeAnnotation::String),
                    value: Box::new(val_ty),
                }
            }
            Expr::Interpolated(parts) => {
                for p in parts {
                    if let crate::ast::InterpPart::Expr(e) = p {
                        self.infer_expr(e);
                    }
                }
                TypeAnnotation::String
            }
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                let l_ty = self.infer_expr(left);
                let r_ty = self.infer_expr(right);

                match op {
                    BinaryOp::Add => {
                        if l_ty == TypeAnnotation::String || r_ty == TypeAnnotation::String {
                            TypeAnnotation::String
                        } else if l_ty == TypeAnnotation::Float || r_ty == TypeAnnotation::Float {
                            TypeAnnotation::Float
                        } else if l_ty == TypeAnnotation::Int && r_ty == TypeAnnotation::Int {
                            TypeAnnotation::Int
                        } else if l_ty == TypeAnnotation::Any || r_ty == TypeAnnotation::Any {
                            TypeAnnotation::Any
                        } else {
                            self.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!(
                                    "Operator '+' cannot be applied to types '{}' and '{}'",
                                    l_ty, r_ty
                                ),
                                hint: Some("Only numbers or strings can be added.".into()),
                                span: *span,
                            });
                            TypeAnnotation::Any
                        }
                    }
                    BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                        let l_is_num = l_ty == TypeAnnotation::Int
                            || l_ty == TypeAnnotation::Float
                            || l_ty == TypeAnnotation::Any;
                        let r_is_num = r_ty == TypeAnnotation::Int
                            || r_ty == TypeAnnotation::Float
                            || r_ty == TypeAnnotation::Any;

                        if !l_is_num || !r_is_num {
                            self.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!(
                                    "Arithmetic operator cannot be applied to types '{}' and '{}'",
                                    l_ty, r_ty
                                ),
                                hint: Some(
                                    "Both operands must be numeric ('int' or 'float').".into(),
                                ),
                                span: *span,
                            });
                            return TypeAnnotation::Any;
                        }

                        if l_ty == TypeAnnotation::Float || r_ty == TypeAnnotation::Float {
                            TypeAnnotation::Float
                        } else if l_ty == TypeAnnotation::Int && r_ty == TypeAnnotation::Int {
                            if *op == BinaryOp::Div {
                                TypeAnnotation::Float
                            } else {
                                TypeAnnotation::Int
                            }
                        } else {
                            TypeAnnotation::Any
                        }
                    }
                    BinaryOp::Eq | BinaryOp::NotEq => TypeAnnotation::Bool,
                    BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                        TypeAnnotation::Bool
                    }
                    BinaryOp::And | BinaryOp::Or => TypeAnnotation::Bool,
                    BinaryOp::Coalesce => {
                        if let TypeAnnotation::Nullable(inner) = l_ty {
                            *inner
                        } else {
                            l_ty
                        }
                    }
                    BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr => {
                        let l_is_int = l_ty == TypeAnnotation::Int || l_ty == TypeAnnotation::Any;
                        let r_is_int = r_ty == TypeAnnotation::Int || r_ty == TypeAnnotation::Any;
                        if !l_is_int || !r_is_int {
                            self.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!(
                                    "Bitwise operator cannot be applied to types '{}' and '{}'",
                                    l_ty, r_ty
                                ),
                                hint: Some(
                                    "Both operands of bitwise operations must be 'int'.".into(),
                                ),
                                span: *span,
                            });
                        }
                        TypeAnnotation::Int
                    }
                    BinaryOp::Pipe => r_ty,
                }
            }
            Expr::Unary { op, expr, span } => {
                let ty = self.infer_expr(expr);
                match op {
                    UnaryOp::Not => TypeAnnotation::Bool,
                    UnaryOp::Neg => {
                        if ty != TypeAnnotation::Int
                            && ty != TypeAnnotation::Float
                            && ty != TypeAnnotation::Any
                        {
                            self.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!("Cannot negate non-numeric type '{}'", ty),
                                hint: Some("Unary '-' requires 'int' or 'float'.".into()),
                                span: *span,
                            });
                        }
                        ty
                    }
                    UnaryOp::BitNot => {
                        if ty != TypeAnnotation::Int && ty != TypeAnnotation::Any {
                            self.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!("Cannot bitwise NOT non-integer type '{}'", ty),
                                hint: Some("Unary '~' requires 'int'.".into()),
                                span: *span,
                            });
                        }
                        TypeAnnotation::Int
                    }
                }
            }
            Expr::Call { callee, args, span } => {
                let callee_ty = self.infer_expr(callee);
                let arg_types: Vec<TypeAnnotation> =
                    args.iter().map(|a| self.infer_expr(a)).collect();

                match callee_ty {
                    TypeAnnotation::Function {
                        params,
                        return_type,
                    } => {
                        if params.len() != arg_types.len() {
                            self.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!(
                                    "Function expects {} argument(s), but got {}",
                                    params.len(),
                                    arg_types.len()
                                ),
                                hint: None,
                                span: *span,
                            });
                        } else {
                            for (i, (arg_t, param_t)) in
                                arg_types.iter().zip(params.iter()).enumerate()
                            {
                                if !self.is_assignable_to(arg_t, param_t) {
                                    self.diagnostics.push(Diagnostic {
                                        severity: DiagnosticSeverity::Error,
                                        message: format!(
                                            "Argument {} type mismatch: expected '{}', found '{}'",
                                            i + 1,
                                            param_t,
                                            arg_t
                                        ),
                                        hint: Some(format!(
                                            "Parameter {} is annotated with type '{}'.",
                                            i + 1,
                                            param_t
                                        )),
                                        span: *span,
                                    });
                                }
                            }
                        }
                        *return_type
                    }
                    TypeAnnotation::Any => TypeAnnotation::Any,
                    other => {
                        self.diagnostics.push(Diagnostic {
                            severity: DiagnosticSeverity::Error,
                            message: format!("Cannot call non-function type '{}'", other),
                            hint: Some("Only functions can be invoked with '()'.".into()),
                            span: *span,
                        });
                        TypeAnnotation::Any
                    }
                }
            }
            Expr::Get {
                target,
                property,
                safe: _,
                span,
            } => {
                let target_ty = self.infer_expr(target);
                match &target_ty {
                    TypeAnnotation::Custom(struct_name) => {
                        if let Some(field_map) = self.struct_defs.get(struct_name) {
                            if let Some(prop_ty) = field_map.get(property) {
                                prop_ty.clone()
                            } else {
                                self.diagnostics.push(Diagnostic {
                                    severity: DiagnosticSeverity::Error,
                                    message: format!(
                                        "Property '{}' does not exist on struct '{}'",
                                        property, struct_name
                                    ),
                                    hint: Some(format!(
                                        "Available fields on '{}' are: {}",
                                        struct_name,
                                        field_map.keys().cloned().collect::<Vec<_>>().join(", ")
                                    )),
                                    span: *span,
                                });
                                TypeAnnotation::Any
                            }
                        } else {
                            TypeAnnotation::Any
                        }
                    }
                    TypeAnnotation::String => {
                        if property == "len" {
                            TypeAnnotation::Int
                        } else {
                            TypeAnnotation::Any
                        }
                    }
                    TypeAnnotation::Array(_) => {
                        if property == "len" {
                            TypeAnnotation::Int
                        } else {
                            TypeAnnotation::Any
                        }
                    }
                    TypeAnnotation::Any => TypeAnnotation::Any,
                    _ => TypeAnnotation::Any,
                }
            }
            Expr::Index {
                target,
                index,
                span: _,
            } => {
                let target_ty = self.infer_expr(target);
                let _idx_ty = self.infer_expr(index);

                match target_ty {
                    TypeAnnotation::Array(elem) => *elem,
                    TypeAnnotation::Map { value, .. } => *value,
                    TypeAnnotation::String => TypeAnnotation::String,
                    _ => TypeAnnotation::Any,
                }
            }
            Expr::StructInit { name, fields, span } => {
                if let Some(struct_fields) = self.struct_defs.get(name).cloned() {
                    for (f_name, f_val) in fields {
                        let val_ty = self.infer_expr(f_val);
                        if let Some(expected_f_ty) = struct_fields.get(f_name) {
                            if !self.is_assignable_to(&val_ty, expected_f_ty) {
                                self.diagnostics.push(Diagnostic {
                                    severity: DiagnosticSeverity::Error,
                                    message: format!(
                                        "Field '{}' on struct '{}' expects '{}', but got '{}'",
                                        f_name, name, expected_f_ty, val_ty
                                    ),
                                    hint: Some(format!(
                                        "Field '{}' was declared as '{}'.",
                                        f_name, expected_f_ty
                                    )),
                                    span: *span,
                                });
                            }
                        } else {
                            self.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!(
                                    "Unknown field '{}' for struct '{}'",
                                    f_name, name
                                ),
                                hint: None,
                                span: *span,
                            });
                        }
                    }
                }
                TypeAnnotation::Custom(name.clone())
            }
            Expr::Match {
                target,
                arms,
                span: _,
            } => {
                self.infer_expr(target);
                let mut unified = TypeAnnotation::Any;
                for (i, arm) in arms.iter().enumerate() {
                    self.bind_match_pattern(&arm.pattern);
                    if let Some(g) = &arm.guard {
                        self.infer_expr(g);
                    }
                    let arm_ty = self.infer_expr(&arm.body);
                    if i == 0 {
                        unified = arm_ty;
                    } else if !self.is_assignable_to(&arm_ty, &unified) {
                        unified = TypeAnnotation::Any;
                    }
                }
                unified
            }
            Expr::Lambda {
                params,
                return_type,
                body,
                span: _,
            } => {
                self.push_scope();
                let declared_ret = return_type.clone().unwrap_or(TypeAnnotation::Any);
                let old_ret = self.current_return_type.take();
                self.current_return_type = Some(declared_ret.clone());

                let mut sig_params = Vec::new();
                for p in params.iter() {
                    let p_ty = p.type_ann.clone().unwrap_or(TypeAnnotation::Any);
                    self.declare_var(&p.name, p_ty.clone());
                    sig_params.push(p_ty);
                }

                for s in body.iter() {
                    self.check_stmt(s);
                }

                self.current_return_type = old_ret;
                self.pop_scope();

                TypeAnnotation::Function {
                    params: sig_params,
                    return_type: Box::new(declared_ret),
                }
            }
            Expr::Use { .. } => TypeAnnotation::Any,
        }
    }

    fn bind_match_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Variable(v) => {
                self.declare_var(v, TypeAnnotation::Any);
            }
            Pattern::Enum { fields, .. } => {
                for f in fields {
                    self.bind_match_pattern(f);
                }
            }
            _ => {}
        }
    }
}
