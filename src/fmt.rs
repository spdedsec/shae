use crate::ast::*;
use std::fmt::Write;

pub struct Formatter {
    indent_level: usize,
    indent_str: &'static str,
    output: String,
}

impl Formatter {
    pub fn new() -> Self {
        Self {
            indent_level: 0,
            indent_str: "    ",
            output: String::new(),
        }
    }

    pub fn format_program(mut self, program: &Program) -> String {
        for (i, stmt) in program.statements.iter().enumerate() {
            if i > 0 && needs_extra_newline(&program.statements[i - 1], stmt) {
                self.output.push('\n');
            }
            self.format_stmt(stmt);
            self.output.push('\n');
        }
        self.output
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent_level {
            self.output.push_str(self.indent_str);
        }
    }

    fn format_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let { pattern, init } => {
                self.write_indent();
                self.output.push_str("let ");
                self.format_binding_pattern(pattern);
                self.output.push_str(" = ");
                self.format_expr(init, 0);
            }
            StmtKind::Assign { target, value } => {
                self.write_indent();
                self.format_expr(target, 0);
                self.output.push_str(" = ");
                self.format_expr(value, 0);
            }
            StmtKind::FnDef { name, params, body } => {
                self.write_indent();
                let _ = write!(self.output, "fn {}(", name);
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.output.push_str(p);
                }
                self.output.push_str(") {\n");
                self.indent_level += 1;
                for s in body.iter() {
                    self.format_stmt(s);
                    self.output.push('\n');
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.write_indent();
                self.output.push_str("if ");
                self.format_expr(condition, 0);
                self.output.push_str(" {\n");
                self.indent_level += 1;
                for s in then_branch {
                    self.format_stmt(s);
                    self.output.push('\n');
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');

                if let Some(else_stmts) = else_branch {
                    if else_stmts.len() == 1 && matches!(else_stmts[0].kind, StmtKind::If { .. }) {
                        self.output.push_str(" else ");
                        self.format_else_if(&else_stmts[0]);
                    } else {
                        self.output.push_str(" else {\n");
                        self.indent_level += 1;
                        for s in else_stmts {
                            self.format_stmt(s);
                            self.output.push('\n');
                        }
                        self.indent_level -= 1;
                        self.write_indent();
                        self.output.push('}');
                    }
                }
            }
            StmtKind::TryCatch {
                try_body,
                catch_ident,
                catch_body,
            } => {
                self.write_indent();
                self.output.push_str("try {\n");
                self.indent_level += 1;
                for s in try_body {
                    self.format_stmt(s);
                    self.output.push('\n');
                }
                self.indent_level -= 1;
                self.write_indent();
                let _ = write!(self.output, "}} catch ({}) {{\n", catch_ident);
                self.indent_level += 1;
                for s in catch_body {
                    self.format_stmt(s);
                    self.output.push('\n');
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');
            }
            StmtKind::While { condition, body } => {
                self.write_indent();
                self.output.push_str("while ");
                self.format_expr(condition, 0);
                self.output.push_str(" {\n");
                self.indent_level += 1;
                for s in body {
                    self.format_stmt(s);
                    self.output.push('\n');
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');
            }
            StmtKind::For {
                item,
                iterable,
                body,
            } => {
                self.write_indent();
                let _ = write!(self.output, "for {} in ", item);
                self.format_expr(iterable, 0);
                self.output.push_str(" {\n");
                self.indent_level += 1;
                for s in body {
                    self.format_stmt(s);
                    self.output.push('\n');
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');
            }
            StmtKind::Return(opt_expr) => {
                self.write_indent();
                self.output.push_str("return");
                if let Some(expr) = opt_expr {
                    self.output.push(' ');
                    self.format_expr(expr, 0);
                }
            }
            StmtKind::Break => {
                self.write_indent();
                self.output.push_str("break");
            }
            StmtKind::Continue => {
                self.write_indent();
                self.output.push_str("continue");
            }
            StmtKind::Expr(expr) => {
                self.write_indent();
                self.format_expr(expr, 0);
            }
            StmtKind::StructDef { name, fields } => {
                self.write_indent();
                let _ = write!(self.output, "struct {} {{ ", name);
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.output.push_str(f);
                }
                self.output.push_str(" }");
            }
            StmtKind::EnumDef { name, variants } => {
                self.write_indent();
                let _ = write!(self.output, "enum {} {{\n", name);
                self.indent_level += 1;
                for (i, v) in variants.iter().enumerate() {
                    self.write_indent();
                    self.output.push_str(&v.name);
                    if !v.fields.is_empty() {
                        self.output.push('(');
                        for (j, f) in v.fields.iter().enumerate() {
                            if j > 0 {
                                self.output.push_str(", ");
                            }
                            self.output.push_str(f);
                        }
                        self.output.push(')');
                    }
                    if i + 1 < variants.len() {
                        self.output.push(',');
                    }
                    self.output.push('\n');
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');
            }
            StmtKind::Use { imports, path } => {
                self.write_indent();
                if imports.len() == 1 && imports[0].name == "*" && imports[0].alias.is_none() {
                    let _ = write!(self.output, "use * from \"{}\"", path);
                } else {
                    self.output.push_str("use { ");
                    for (i, item) in imports.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.output.push_str(&item.name);
                        if let Some(alias) = &item.alias {
                            let _ = write!(self.output, " as {}", alias);
                        }
                    }
                    let _ = write!(self.output, " }} from \"{}\"", path);
                }
            }
        }
    }

    fn format_else_if(&mut self, stmt: &Stmt) {
        if let StmtKind::If {
            condition,
            then_branch,
            else_branch,
        } = &stmt.kind
        {
            self.output.push_str("if ");
            self.format_expr(condition, 0);
            self.output.push_str(" {\n");
            self.indent_level += 1;
            for s in then_branch {
                self.format_stmt(s);
                self.output.push('\n');
            }
            self.indent_level -= 1;
            self.write_indent();
            self.output.push('}');

            if let Some(else_stmts) = else_branch {
                if else_stmts.len() == 1 && matches!(else_stmts[0].kind, StmtKind::If { .. }) {
                    self.output.push_str(" else ");
                    self.format_else_if(&else_stmts[0]);
                } else {
                    self.output.push_str(" else {\n");
                    self.indent_level += 1;
                    for s in else_stmts {
                        self.format_stmt(s);
                        self.output.push('\n');
                    }
                    self.indent_level -= 1;
                    self.write_indent();
                    self.output.push('}');
                }
            }
        }
    }

    fn format_binding_pattern(&mut self, pattern: &BindingPattern) {
        match pattern {
            BindingPattern::Ident(name) => {
                self.output.push_str(name);
            }
            BindingPattern::Array { elements, rest } => {
                self.output.push('[');
                for (i, el) in elements.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.format_binding_pattern(el);
                }
                if let Some(r) = rest {
                    if !elements.is_empty() {
                        self.output.push_str(", ");
                    }
                    self.output.push_str("..");
                    self.output.push_str(r);
                }
                self.output.push(']');
            }
            BindingPattern::Object { fields, rest } => {
                self.output.push_str("{ ");
                for (i, (field, opt_sub)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.output.push_str(field);
                    if let Some(sub) = opt_sub {
                        self.output.push_str(": ");
                        self.format_binding_pattern(sub);
                    }
                }
                if let Some(r) = rest {
                    if !fields.is_empty() {
                        self.output.push_str(", ");
                    }
                    self.output.push_str("..");
                    self.output.push_str(r);
                }
                self.output.push_str(" }");
            }
        }
    }

    fn format_expr(&mut self, expr: &Expr, parent_prec: u8) {
        match expr {
            Expr::Literal(lit) => self.format_literal(lit),
            Expr::Variable { name, .. } => self.output.push_str(name),
            Expr::Array(elements) => {
                self.output.push('[');
                for (i, el) in elements.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.format_expr(el, 0);
                }
                self.output.push(']');
            }
            Expr::Map(pairs) => {
                if pairs.is_empty() {
                    self.output.push_str("{}");
                } else {
                    self.output.push_str("{ ");
                    for (i, (k, v)) in pairs.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        if is_valid_ident(k) {
                            self.output.push_str(k);
                        } else {
                            self.output.push('"');
                            self.output
                                .push_str(&k.replace('\\', "\\\\").replace('"', "\\\""));
                            self.output.push('"');
                        }
                        self.output.push_str(": ");
                        self.format_expr(v, 0);
                    }
                    self.output.push_str(" }");
                }
            }
            Expr::Interpolated(parts) => {
                self.output.push('"');
                for p in parts {
                    match p {
                        InterpPart::Text(t) => {
                            for ch in t.chars() {
                                match ch {
                                    '"' => self.output.push_str("\\\""),
                                    '\\' => self.output.push_str("\\\\"),
                                    '{' => self.output.push_str("\\{"),
                                    '\n' => self.output.push_str("\\n"),
                                    '\t' => self.output.push_str("\\t"),
                                    '\r' => self.output.push_str("\\r"),
                                    _ => self.output.push(ch),
                                }
                            }
                        }
                        InterpPart::Expr(e) => {
                            self.output.push('{');
                            self.format_expr(e, 0);
                            self.output.push('}');
                        }
                    }
                }
                self.output.push('"');
            }
            Expr::Use { path, .. } => {
                self.output.push_str("use ");
                self.format_expr(path, 0);
            }
            Expr::Binary {
                left, op, right, ..
            } => {
                let prec = op_precedence(*op);
                let need_paren = prec < parent_prec;
                if need_paren {
                    self.output.push('(');
                }
                self.format_expr(left, prec);
                let _ = write!(self.output, " {} ", op_symbol(*op));
                // Right side needs prec + 1 for left-associative operators to parenthesize (a - (b - c))
                let right_prec = if is_left_associative(*op) {
                    prec + 1
                } else {
                    prec
                };
                self.format_expr(right, right_prec);
                if need_paren {
                    self.output.push(')');
                }
            }
            Expr::Unary { op, expr, .. } => {
                let symbol = match op {
                    UnaryOp::Not => "not ",
                    UnaryOp::Neg => "-",
                    UnaryOp::BitNot => "~",
                };
                self.output.push_str(symbol);
                let need_paren = matches!(expr.as_ref(), Expr::Binary { .. });
                if need_paren {
                    self.output.push('(');
                }
                self.format_expr(expr, 13);
                if need_paren {
                    self.output.push(')');
                }
            }
            Expr::Call { callee, args, .. } => {
                self.format_expr(callee, 14);
                self.output.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.format_expr(a, 0);
                }
                self.output.push(')');
            }
            Expr::Get {
                target,
                property,
                safe,
                ..
            } => {
                self.format_expr(target, 14);
                if *safe {
                    self.output.push_str("?.");
                } else {
                    self.output.push('.');
                }
                self.output.push_str(property);
            }
            Expr::Index { target, index, .. } => {
                self.format_expr(target, 14);
                self.output.push('[');
                self.format_expr(index, 0);
                self.output.push(']');
            }
            Expr::StructInit { name, fields, .. } => {
                let _ = write!(self.output, "{} {{ ", name);
                for (i, (f, val)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    let _ = write!(self.output, "{}: ", f);
                    self.format_expr(val, 0);
                }
                self.output.push_str(" }");
            }
            Expr::Match { target, arms, .. } => {
                self.output.push_str("match ");
                self.format_expr(target, 0);
                self.output.push_str(" {\n");
                self.indent_level += 1;
                for (i, arm) in arms.iter().enumerate() {
                    self.write_indent();
                    self.format_pattern(&arm.pattern);
                    if let Some(guard) = &arm.guard {
                        self.output.push_str(" if ");
                        self.format_expr(guard, 0);
                    }
                    self.output.push_str(" => ");
                    self.format_expr(&arm.body, 0);
                    if i + 1 < arms.len() {
                        self.output.push(',');
                    }
                    self.output.push('\n');
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');
            }
            Expr::Lambda { params, body, .. } => {
                self.output.push_str("fn(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.output.push_str(p);
                }
                self.output.push_str(") {\n");
                self.indent_level += 1;
                for s in body.iter() {
                    self.format_stmt(s);
                    self.output.push('\n');
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');
            }
        }
    }

    fn format_literal(&mut self, lit: &Literal) {
        match lit {
            Literal::Int(i) => {
                let _ = write!(self.output, "{}", i);
            }
            Literal::Float(f) => {
                if f.fract() == 0.0 {
                    let _ = write!(self.output, "{:.1}", f);
                } else {
                    let _ = write!(self.output, "{}", f);
                }
            }
            Literal::Number(n) => {
                if n.fract() == 0.0 {
                    let _ = write!(self.output, "{}", *n as i64);
                } else {
                    let _ = write!(self.output, "{}", n);
                }
            }
            Literal::String(s) => {
                self.output.push('"');
                for ch in s.chars() {
                    match ch {
                        '"' => self.output.push_str("\\\""),
                        '\\' => self.output.push_str("\\\\"),
                        '\n' => self.output.push_str("\\n"),
                        '\t' => self.output.push_str("\\t"),
                        '\r' => self.output.push_str("\\r"),
                        _ => self.output.push(ch),
                    }
                }
                self.output.push('"');
            }
            Literal::Bool(b) => {
                self.output.push_str(if *b { "true" } else { "false" });
            }
            Literal::Null => {
                self.output.push_str("null");
            }
        }
    }

    fn format_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Wildcard => self.output.push('_'),
            Pattern::Variable(v) => self.output.push_str(v),
            Pattern::Literal(lit) => self.format_literal(lit),
            Pattern::Range {
                start,
                end,
                inclusive,
            } => {
                self.format_literal(start);
                if *inclusive {
                    self.output.push_str("..=");
                } else {
                    self.output.push_str("..");
                }
                self.format_literal(end);
            }
            Pattern::Enum {
                enum_name,
                variant_name,
                fields,
            } => {
                if let Some(e) = enum_name {
                    let _ = write!(self.output, "{}.{}", e, variant_name);
                } else {
                    self.output.push_str(variant_name);
                }
                if !fields.is_empty() {
                    self.output.push('(');
                    for (i, f) in fields.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.format_pattern(f);
                    }
                    self.output.push(')');
                }
            }
        }
    }
}

fn needs_extra_newline(prev: &Stmt, next: &Stmt) -> bool {
    matches!(
        prev.kind,
        StmtKind::FnDef { .. } | StmtKind::StructDef { .. } | StmtKind::EnumDef { .. }
    ) || matches!(
        next.kind,
        StmtKind::FnDef { .. } | StmtKind::StructDef { .. } | StmtKind::EnumDef { .. }
    )
}

fn op_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Eq => "==",
        BinaryOp::NotEq => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::LtEq => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::GtEq => ">=",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
        BinaryOp::Pipe => "|>",
        BinaryOp::Coalesce => "??",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
    }
}

fn op_precedence(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Pipe => 1,
        BinaryOp::Or => 2,
        BinaryOp::And => 3,
        BinaryOp::BitOr => 4,
        BinaryOp::BitXor => 5,
        BinaryOp::BitAnd => 6,
        BinaryOp::Coalesce => 7,
        BinaryOp::Eq | BinaryOp::NotEq => 8,
        BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => 9,
        BinaryOp::Shl | BinaryOp::Shr => 10,
        BinaryOp::Add | BinaryOp::Sub => 11,
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => 12,
    }
}

fn is_left_associative(op: BinaryOp) -> bool {
    // Almost all binary ops in Shae are left-associative except coalesce if chained
    !matches!(op, BinaryOp::Coalesce)
}

fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    }
}

pub fn format_program(program: &Program) -> String {
    let fmt = Formatter::new();
    fmt.format_program(program)
}

pub fn format_source(source: &str) -> Result<String, crate::ShaeError> {
    let tokens = crate::lexer::tokenize(source)?;
    let program = crate::parser::parse(tokens)?;
    Ok(format_program(&program))
}
