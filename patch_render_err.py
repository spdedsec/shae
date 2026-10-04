with open("src/lib.rs", "r") as f:
    content = f.read()

target = """        ShaeError::Runtime(e) => {
            if let Some(span) = e.span {
                (span.line, span.col, e.to_string())
            } else {
                return e.to_string();
            }
        }
    };

    let mut lines = source.lines();
    if let Some(source_line) = lines.nth(line.saturating_sub(1)) {
        let caret = " ".repeat(col.saturating_sub(1)) + "^";
        format!(
            "{}\\n\\n  {} | {}\\n  {} | {}",
            msg,
            line,
            source_line,
            " ".repeat(line.to_string().len()),
            caret
        )
    } else {
        msg
    }
}"""

replacement = """        ShaeError::Runtime(e) => {
            if let Some(span) = e.span {
                let mut out = e.to_string();
                if !e.stack.is_empty() {
                    out.push_str("\\n\\nStack Trace:");
                    for (func, sp) in e.stack.iter().rev() {
                        out.push_str(&format!("\\n  at {} (line {}:{})", func, sp.line, sp.col));
                    }
                }
                (span.line, span.col, out)
            } else {
                return e.to_string();
            }
        }
    };

    let mut lines = source.lines();
    if let Some(source_line) = lines.nth(line.saturating_sub(1)) {
        let caret = " ".repeat(col.saturating_sub(1)) + "^";
        format!(
            "{}\\n\\n  {} | {}\\n  {} | {}",
            msg,
            line,
            source_line,
            " ".repeat(line.to_string().len()),
            caret
        )
    } else {
        msg
    }
}"""

content = content.replace(target, replacement)
with open("src/lib.rs", "w") as f:
    f.write(content)
