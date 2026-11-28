//! Pure-Rust Server-Side Rendering (SSR) Compiler, Streaming Chunks,
//! and XSS-Safe Client Hydration Emitter for Shae.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use indexmap::IndexMap;
use serde_json::Value as JsonValue;

use crate::ast::Span;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;
use crate::web_vdom::{
    escape_html_attr, escape_html_text, is_void_tag, sanitize_attr_name, sanitize_tag_name,
    shae_value_to_vnode, VNode,
};

const MAX_TEMPLATE_DEPTH: usize = 64;

// ============================================================================
// 1. Safe JSON Script Serialization & Shae Value Conversion
// ============================================================================

fn shae_to_json(val: &Value) -> JsonValue {
    match val {
        Value::Null => JsonValue::Null,
        Value::Bool(b) => JsonValue::Bool(*b),
        Value::Int(i) => JsonValue::Number((*i).into()),
        Value::Float(f) | Value::Number(f) => serde_json::Number::from_f64(*f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::String(s) => JsonValue::String(s.clone()),
        Value::Array(arr) => {
            let items = arr.read().unwrap();
            JsonValue::Array(items.iter().map(shae_to_json).collect())
        }
        Value::Map(map) => {
            let g = map.read().unwrap();
            let mut obj = serde_json::Map::new();
            for (k, v) in g.iter() {
                obj.insert(k.clone(), shae_to_json(v));
            }
            JsonValue::Object(obj)
        }
        other => JsonValue::String(other.to_display()),
    }
}

/// Serializes any Shae Value to a JSON string safe for inline `<script>` tags.
/// Escapes `<`, `>`, `&`, U+2028, and U+2029 to prevent `</script>` breakout XSS.
pub fn serialize_safe_script_json(val: &Value) -> String {
    let json = shae_to_json(val);
    let raw = serde_json::to_string(&json).unwrap_or_else(|_| "null".to_string());
    escape_json_for_html_script(&raw)
}

pub fn escape_json_for_html_script(raw_json: &str) -> String {
    let mut out = String::with_capacity(raw_json.len());
    for ch in raw_json.chars() {
        match ch {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out
}

// ============================================================================
// 2. Compiled SSR Template Engine
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum TemplateNode {
    Literal(String),
    EscapedExpr(String),
    RawExpr(String),
    If {
        cond: String,
        then_branch: Vec<TemplateNode>,
        else_branch: Vec<TemplateNode>,
    },
    Each {
        list_expr: String,
        item_var: String,
        body: Vec<TemplateNode>,
    },
}

#[derive(Debug, Clone)]
pub struct CompiledTemplate {
    pub nodes: Vec<TemplateNode>,
}

pub fn compile_template(src: &str) -> Result<CompiledTemplate, String> {
    let mut pos = 0usize;
    let (nodes, stop_tag) = parse_template_nodes(src, &mut pos, &[], 0)?;
    if let Some(tag) = stop_tag {
        return Err(format!("Unexpected closing template tag '{{{{{}}}}}'", tag));
    }
    Ok(CompiledTemplate { nodes })
}

fn parse_template_nodes(
    src: &str,
    pos: &mut usize,
    stop_tags: &[&str],
    depth: usize,
) -> Result<(Vec<TemplateNode>, Option<String>), String> {
    if depth > MAX_TEMPLATE_DEPTH {
        return Err("Template nesting exceeds maximum depth".to_string());
    }
    let mut nodes = Vec::new();
    let bytes = src.as_bytes();

    while *pos < bytes.len() {
        if let Some(rel_open) = src[*pos..].find("{{") {
            if rel_open > 0 {
                nodes.push(TemplateNode::Literal(src[*pos..*pos + rel_open].to_string()));
                *pos += rel_open;
            }
            // Check triple braces {{{ raw }}}
            if src[*pos..].starts_with("{{{") {
                let expr_start = *pos + 3;
                let rel_close = src[expr_start..]
                    .find("}}}")
                    .ok_or_else(|| "Unclosed '{{{' template expression".to_string())?;
                let expr = src[expr_start..expr_start + rel_close].trim().to_string();
                nodes.push(TemplateNode::RawExpr(expr));
                *pos = expr_start + rel_close + 3;
                continue;
            }

            let tag_start = *pos + 2;
            let rel_close = src[tag_start..]
                .find("}}")
                .ok_or_else(|| "Unclosed '{{' template tag".to_string())?;
            let inner = src[tag_start..tag_start + rel_close].trim();
            *pos = tag_start + rel_close + 2;

            for &st in stop_tags {
                if inner == st {
                    return Ok((nodes, Some(st.to_string())));
                }
            }

            if let Some(cond_raw) = inner.strip_prefix("#if ") {
                let cond = cond_raw.trim().to_string();
                let (then_branch, term) =
                    parse_template_nodes(src, pos, &["else", "/if"], depth + 1)?;
                let term = term.ok_or_else(|| "Unclosed '{{#if}}' block".to_string())?;
                let else_branch = if term == "else" {
                    let (eb, end_tag) = parse_template_nodes(src, pos, &["/if"], depth + 1)?;
                    if end_tag.as_deref() != Some("/if") {
                        return Err("Unclosed '{{else}}' branch inside '{{#if}}'".to_string());
                    }
                    eb
                } else {
                    Vec::new()
                };
                nodes.push(TemplateNode::If {
                    cond,
                    then_branch,
                    else_branch,
                });
            } else if let Some(each_raw) = inner.strip_prefix("#each ") {
                let (list_expr, item_var) = parse_each_header(each_raw)?;
                let (body, term) = parse_template_nodes(src, pos, &["/each"], depth + 1)?;
                if term.as_deref() != Some("/each") {
                    return Err("Unclosed '{{#each}}' block".to_string());
                }
                nodes.push(TemplateNode::Each {
                    list_expr,
                    item_var,
                    body,
                });
            } else if let Some(raw_expr) = inner.strip_prefix('!') {
                nodes.push(TemplateNode::RawExpr(raw_expr.trim().to_string()));
            } else if inner.starts_with('/') {
                return Err(format!("Unexpected closing tag '{{{{{}}}}}'", inner));
            } else {
                nodes.push(TemplateNode::EscapedExpr(inner.to_string()));
            }
        } else {
            nodes.push(TemplateNode::Literal(src[*pos..].to_string()));
            *pos = bytes.len();
        }
    }

    Ok((nodes, None))
}

fn parse_each_header(raw: &str) -> Result<(String, String), String> {
    let trimmed = raw.trim();
    if let Some((list_part, item_part)) = trimmed.split_once(" as ") {
        let list_expr = list_part.trim().to_string();
        let item_var = item_part.trim().to_string();
        if list_expr.is_empty() || item_var.is_empty() {
            return Err("Invalid {{#each list as item}} syntax".to_string());
        }
        Ok((list_expr, item_var))
    } else if !trimmed.is_empty() {
        Ok((trimmed.to_string(), "this".to_string()))
    } else {
        Err("Empty {{#each}} header".to_string())
    }
}

impl CompiledTemplate {
    pub fn render(&self, ctx: &Value) -> Result<String, String> {
        let mut out = String::new();
        let mut scopes: Vec<IndexMap<String, Value>> = Vec::new();
        if let Value::Map(m) = ctx {
            scopes.push(m.read().unwrap().clone());
        } else {
            let mut root = IndexMap::new();
            root.insert("this".to_string(), ctx.clone());
            scopes.push(root);
        }
        eval_template_nodes(&self.nodes, &mut scopes, &mut out)?;
        Ok(out)
    }
}

fn eval_template_nodes(
    nodes: &[TemplateNode],
    scopes: &mut Vec<IndexMap<String, Value>>,
    out: &mut String,
) -> Result<(), String> {
    for node in nodes {
        match node {
            TemplateNode::Literal(lit) => out.push_str(lit),
            TemplateNode::EscapedExpr(expr) => {
                if let Some(val) = resolve_expr(expr, scopes) {
                    if !matches!(val, Value::Null) {
                        out.push_str(&escape_html_text(&val.to_display()));
                    }
                }
            }
            TemplateNode::RawExpr(expr) => {
                if let Some(val) = resolve_expr(expr, scopes) {
                    if !matches!(val, Value::Null) {
                        out.push_str(&val.to_display());
                    }
                }
            }
            TemplateNode::If {
                cond,
                then_branch,
                else_branch,
            } => {
                if eval_condition(cond, scopes) {
                    eval_template_nodes(then_branch, scopes, out)?;
                } else {
                    eval_template_nodes(else_branch, scopes, out)?;
                }
            }
            TemplateNode::Each {
                list_expr,
                item_var,
                body,
            } => {
                let list_val = resolve_expr(list_expr, scopes).unwrap_or(Value::Null);
                if let Value::Array(arr) = list_val {
                    let items = arr.read().unwrap().clone();
                    for (idx, item) in items.into_iter().enumerate() {
                        let mut frame = IndexMap::new();
                        frame.insert(item_var.clone(), item.clone());
                        frame.insert("this".to_string(), item);
                        frame.insert("@index".to_string(), Value::Int(idx as i64));
                        scopes.push(frame);
                        let res = eval_template_nodes(body, scopes, out);
                        scopes.pop();
                        res?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn eval_condition(cond: &str, scopes: &[IndexMap<String, Value>]) -> bool {
    let trimmed = cond.trim();
    if let Some(neg) = trimmed.strip_prefix('!') {
        return !eval_condition(neg, scopes);
    }
    match resolve_expr(trimmed, scopes) {
        Some(v) => is_truthy_ssr(&v),
        None => false,
    }
}

fn is_truthy_ssr(val: &Value) -> bool {
    match val {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Float(f) | Value::Number(f) => *f != 0.0 && !f.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.read().unwrap().is_empty(),
        Value::Map(m) => !m.read().unwrap().is_empty(),
        _ => true,
    }
}

fn resolve_expr(expr: &str, scopes: &[IndexMap<String, Value>]) -> Option<Value> {
    let parts: Vec<&str> = expr.trim().split('.').collect();
    if parts.is_empty() {
        return None;
    }
    let first = parts[0];
    let mut cur: Option<Value> = None;
    for scope in scopes.iter().rev() {
        if let Some(v) = scope.get(first) {
            cur = Some(v.clone());
            break;
        }
    }
    let mut val = cur?;
    for seg in &parts[1..] {
        let next = match &val {
            Value::Map(m) => m.read().unwrap().get(*seg)?.clone(),
            Value::Array(a) => {
                if *seg == "len" || *seg == "length" {
                    Value::Int(a.read().unwrap().len() as i64)
                } else if let Ok(idx) = seg.parse::<usize>() {
                    a.read().unwrap().get(idx)?.clone()
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        val = next;
    }
    Some(val)
}

// ============================================================================
// 3. Streaming VDOM HTML Renderer & Hydration Metadata Emitter
// ============================================================================

#[derive(Debug, Clone)]
pub struct HydrationRecord {
    pub hid: usize,
    pub tag: String,
    pub key: Option<String>,
    pub events: Vec<String>,
}

pub fn render_vnode_with_hydration(
    node: &VNode,
    hydrate: bool,
) -> (String, Vec<HydrationRecord>) {
    let mut out = String::new();
    let mut records = Vec::new();
    let mut next_hid = 0usize;
    render_hydrated_buf(node, hydrate, &mut next_hid, &mut records, &mut out);
    (out, records)
}

fn render_hydrated_buf(
    node: &VNode,
    hydrate: bool,
    next_hid: &mut usize,
    records: &mut Vec<HydrationRecord>,
    out: &mut String,
) {
    match node {
        VNode::Text(s) => out.push_str(&escape_html_text(s)),
        VNode::Comment(s) => {
            out.push_str("<!--");
            out.push_str(&s.replace("-->", "--&gt;"));
            out.push_str("-->");
        }
        VNode::Fragment(children) => {
            if hydrate {
                out.push_str("<!--shae:frag-->");
            }
            for child in children {
                render_hydrated_buf(child, hydrate, next_hid, records, out);
            }
            if hydrate {
                out.push_str("<!--/shae:frag-->");
            }
        }
        VNode::Element(el) => {
            let safe_tag = sanitize_tag_name(&el.tag);
            out.push('<');
            out.push_str(&safe_tag);

            for (k, v) in &el.props {
                if v == "__false__" {
                    continue;
                }
                let raw_name = if k == "className" { "class" } else { k.as_str() };
                let attr_name = sanitize_attr_name(raw_name);
                if attr_name.is_empty() {
                    continue;
                }
                if v == "__true__" {
                    out.push(' ');
                    out.push_str(&attr_name);
                } else {
                    out.push(' ');
                    out.push_str(&attr_name);
                    out.push_str("=\"");
                    out.push_str(&escape_html_attr(v));
                    out.push('"');
                }
            }

            if hydrate && (!el.events.is_empty() || el.key.is_some()) {
                let hid = *next_hid;
                *next_hid += 1;
                let events: Vec<String> = el.events.keys().cloned().collect();
                out.push_str(" data-shae-hid=\"");
                out.push_str(&hid.to_string());
                out.push('"');
                if !events.is_empty() {
                    out.push_str(" data-shae-events=\"");
                    out.push_str(&escape_html_attr(&events.join(",")));
                    out.push('"');
                }
                if let Some(ref k) = el.key {
                    out.push_str(" data-shae-key=\"");
                    out.push_str(&escape_html_attr(k));
                    out.push('"');
                }
                records.push(HydrationRecord {
                    hid,
                    tag: safe_tag.clone(),
                    key: el.key.clone(),
                    events,
                });
            }

            out.push('>');
            if !is_void_tag(&safe_tag) {
                if safe_tag.eq_ignore_ascii_case("script") || safe_tag.eq_ignore_ascii_case("style") {
                    for child in &el.children {
                        if let VNode::Text(s) = child {
                            out.push_str(s);
                        } else {
                            render_hydrated_buf(child, false, next_hid, records, out);
                        }
                    }
                } else {
                    for child in &el.children {
                        render_hydrated_buf(child, hydrate, next_hid, records, out);
                    }
                }
                out.push_str("</");
                out.push_str(&safe_tag);
                out.push('>');
            }
        }
    }
}

/// Splits a rendered HTML string on UTF-8 character boundaries into chunks of up to `chunk_size` bytes.
pub fn chunk_html_utf8(html: &str, chunk_size: usize) -> Vec<String> {
    let size = chunk_size.max(16);
    if html.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < html.len() {
        let mut end = (start + size).min(html.len());
        while end > start && !html.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            // Advance at least one char if size < char UTF-8 width
            end = start + html[start..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
        chunks.push(html[start..end].to_string());
        start = end;
    }
    chunks
}

pub fn default_client_hydration_script(root_id: &str) -> String {
    format!(
        "(function(){{var d=document.getElementById('__SHAE_DATA__');var root=document.getElementById('{}');if(!root)return;var state=d?JSON.parse(d.textContent||'{{}}'):{{}};window.__SHAE_STATE__=state;root.querySelectorAll('[data-shae-hid]').forEach(function(el){{var evs=(el.getAttribute('data-shae-events')||'').split(',').filter(Boolean);evs.forEach(function(ev){{el.addEventListener(ev,function(e){{root.dispatchEvent(new CustomEvent('shae:event',{{detail:{{hid:el.getAttribute('data-shae-hid'),event:ev,original:e}}}}));}});}});}});}})();",
        escape_html_attr(root_id)
    )
}

pub fn render_suspense_fallback(slot_id: &str, fallback_html: &str) -> String {
    let safe_id = sanitize_attr_name(slot_id);
    format!(
        "<template id=\"B:{id}\"></template><!--$?-->{fb}<!--/$-->",
        id = safe_id,
        fb = fallback_html
    )
}

pub fn render_suspense_resolve(slot_id: &str, content_html: &str) -> String {
    let safe_id = sanitize_attr_name(slot_id);
    format!(
        "<div hidden id=\"S:{id}\">{content}</div><script>(function(){{var b=document.getElementById('B:{id}');var s=document.getElementById('S:{id}');if(!b||!s)return;var p=b.parentNode;while(b.nextSibling&&!(b.nextSibling.nodeType===8&&b.nextSibling.data==='/$')){{p.removeChild(b.nextSibling);}}while(s.firstChild){{p.insertBefore(s.firstChild,b);}}p.removeChild(b);s.remove();}})();</script>",
        id = safe_id,
        content = content_html
    )
}

// ============================================================================
// 4. Compiled Template Handle Registry & Shae Stdlib Module
// ============================================================================

static NEXT_TPL_ID: AtomicU64 = AtomicU64::new(1);
static TPL_REGISTRY: OnceLock<Mutex<HashMap<u64, CompiledTemplate>>> = OnceLock::new();

fn templates() -> &'static Mutex<HashMap<u64, CompiledTemplate>> {
    TPL_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn eval_ssr_template_method(
    _ev: &mut Evaluator,
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let id = {
        let g = map.read().unwrap();
        match g.get("_ssr_template_id") {
            Some(Value::Int(i)) if *i > 0 => *i as u64,
            _ => return Err(RuntimeError::new("Invalid SSR template handle".to_string()).at(span)),
        }
    };
    match method {
        "render" => {
            let ctx = args.first().cloned().unwrap_or(Value::Null);
            let tpl = {
                let guard = templates().lock().unwrap();
                guard.get(&id).cloned().ok_or_else(|| {
                    RuntimeError::new(format!("Unknown SSR template id {}", id)).at(span)
                })?
            };
            let html = tpl.render(&ctx).map_err(|e| RuntimeError::new(e).at(span))?;
            Ok(Value::String(html))
        }
        other => Err(RuntimeError::new(format!("Unknown SSR template method '{}'", other)).at(span)),
    }
}

pub fn build_ssr_module() -> Value {
    let mut map = IndexMap::new();
    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("compile", builtin_compile),
        ("render_template", builtin_render_template),
        ("renderTemplate", builtin_render_template),
        ("render_vnode", builtin_render_vnode),
        ("renderVnode", builtin_render_vnode),
        ("render_stream", builtin_render_stream),
        ("renderStream", builtin_render_stream),
        ("render_document", builtin_render_document),
        ("renderDocument", builtin_render_document),
        ("serialize_state", builtin_serialize_state),
        ("serializeState", builtin_serialize_state),
        ("hydration_script", builtin_hydration_script),
        ("hydrationScript", builtin_hydration_script),
        ("suspense_fallback", builtin_suspense_fallback),
        ("suspenseFallback", builtin_suspense_fallback),
        ("suspense_resolve", builtin_suspense_resolve),
        ("suspenseResolve", builtin_suspense_resolve),
        ("escape_html", builtin_escape_html),
        ("escapeHtml", builtin_escape_html),
    ];

    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: format!("ssr.{}", name),
                func,
            },
        );
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn builtin_compile(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let src = args
        .first()
        .ok_or_else(|| RuntimeError::new("ssr.compile requires a template string".to_string()).at(span))?
        .to_display();
    let tpl = compile_template(&src).map_err(|e| RuntimeError::new(e).at(span))?;
    let id = NEXT_TPL_ID.fetch_add(1, Ordering::SeqCst);
    templates().lock().unwrap().insert(id, tpl);

    let mut m = IndexMap::new();
    m.insert("_ssr_template_id".to_string(), Value::Int(id as i64));
    m.insert("id".to_string(), Value::Int(id as i64));
    Ok(Value::Map(Arc::new(RwLock::new(m))))
}

fn builtin_render_template(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("ssr.render_template requires (template, [data])".to_string()).at(span));
    }
    let ctx = args.get(1).cloned().unwrap_or(Value::Null);
    if let Value::Map(m) = &args[0] {
        if let Some(Value::Int(id)) = m.read().unwrap().get("_ssr_template_id").cloned() {
            let tpl = templates()
                .lock()
                .unwrap()
                .get(&(id as u64))
                .cloned()
                .ok_or_else(|| RuntimeError::new(format!("Unknown SSR template id {}", id)).at(span))?;
            let html = tpl.render(&ctx).map_err(|e| RuntimeError::new(e).at(span))?;
            return Ok(Value::String(html));
        }
    }
    let src = args[0].to_display();
    let tpl = compile_template(&src).map_err(|e| RuntimeError::new(e).at(span))?;
    let html = tpl.render(&ctx).map_err(|e| RuntimeError::new(e).at(span))?;
    Ok(Value::String(html))
}

fn builtin_render_vnode(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let vnode_val = args
        .first()
        .ok_or_else(|| RuntimeError::new("ssr.render_vnode requires a VNode".to_string()).at(span))?;
    let hydrate = match args.get(1) {
        Some(Value::Map(m)) => matches!(m.read().unwrap().get("hydrate"), Some(Value::Bool(true))),
        Some(Value::Bool(b)) => *b,
        _ => false,
    };
    let node = shae_value_to_vnode(vnode_val).map_err(|e| RuntimeError::new(e).at(span))?;
    let (html, records) = render_vnode_with_hydration(&node, hydrate);

    if matches!(args.get(1), Some(Value::Map(m)) if matches!(m.read().unwrap().get("with_meta"), Some(Value::Bool(true)))) {
        let mut res = IndexMap::new();
        res.insert("html".to_string(), Value::String(html));
        let rec_vals: Vec<Value> = records
            .into_iter()
            .map(|r| {
                let mut rm = IndexMap::new();
                rm.insert("hid".to_string(), Value::Int(r.hid as i64));
                rm.insert("tag".to_string(), Value::String(r.tag));
                rm.insert(
                    "key".to_string(),
                    r.key.map(Value::String).unwrap_or(Value::Null),
                );
                let evs: Vec<Value> = r.events.into_iter().map(Value::String).collect();
                rm.insert("events".to_string(), Value::Array(Arc::new(RwLock::new(evs))));
                Value::Map(Arc::new(RwLock::new(rm)))
            })
            .collect();
        res.insert("markers".to_string(), Value::Array(Arc::new(RwLock::new(rec_vals))));
        Ok(Value::Map(Arc::new(RwLock::new(res))))
    } else {
        Ok(Value::String(html))
    }
}

fn builtin_render_stream(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let first = args
        .first()
        .ok_or_else(|| RuntimeError::new("ssr.render_stream requires a VNode or HTML string".to_string()).at(span))?;
    let mut chunk_size = 1024usize;
    let mut hydrate = false;

    if let Some(arg1) = args.get(1) {
        match arg1 {
            Value::Int(n) if *n > 0 => chunk_size = *n as usize,
            Value::Map(m) => {
                let g = m.read().unwrap();
                if let Some(Value::Int(n)) = g.get("chunk_size") {
                    if *n > 0 {
                        chunk_size = *n as usize;
                    }
                }
                if let Some(Value::Bool(b)) = g.get("hydrate") {
                    hydrate = *b;
                }
            }
            _ => {}
        }
    }

    let html = match first {
        Value::String(s) => s.clone(),
        other => {
            let node = shae_value_to_vnode(other).map_err(|e| RuntimeError::new(e).at(span))?;
            render_vnode_with_hydration(&node, hydrate).0
        }
    };

    let chunks: Vec<Value> = chunk_html_utf8(&html, chunk_size)
        .into_iter()
        .map(Value::String)
        .collect();
    Ok(Value::Array(Arc::new(RwLock::new(chunks))))
}

fn builtin_render_document(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let Value::Map(m) = args.first().ok_or_else(|| {
        RuntimeError::new("ssr.render_document requires an options map".to_string()).at(span)
    })? else {
        return Err(RuntimeError::new("ssr.render_document expects an options map".to_string()).at(span));
    };

    let opts = m.read().unwrap().clone();
    let title = opts
        .get("title")
        .map(|v| escape_html_text(&v.to_display()))
        .unwrap_or_else(|| "Shae App".to_string());
    let lang = opts
        .get("lang")
        .map(|v| escape_html_attr(&v.to_display()))
        .unwrap_or_else(|| "en".to_string());
    let root_id = opts
        .get("root_id")
        .map(|v| sanitize_attr_name(&v.to_display()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "app".to_string());
    let head_extra = opts
        .get("head")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => shae_value_to_vnode(other)
                .map(|n| render_vnode_with_hydration(&n, false).0)
                .unwrap_or_default(),
        })
        .unwrap_or_default();

    let hydrate = matches!(opts.get("hydrate"), Some(Value::Bool(true))) || opts.contains_key("state");

    let body_html = match opts.get("body") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => {
            let node = shae_value_to_vnode(other).map_err(|e| RuntimeError::new(e).at(span))?;
            render_vnode_with_hydration(&node, hydrate).0
        }
        None => String::new(),
    };

    let mut state_script = String::new();
    if let Some(state_val) = opts.get("state") {
        let safe_json = serialize_safe_script_json(state_val);
        state_script = format!(
            "<script id=\"__SHAE_DATA__\" type=\"application/json\">{}</script>",
            safe_json
        );
    }

    let bootstrap_script = if hydrate {
        format!("<script>{}</script>", default_client_hydration_script(&root_id))
    } else {
        String::new()
    };

    let doc = format!(
        "<!DOCTYPE html><html lang=\"{lang}\"><head><meta charset=\"utf-8\"><title>{title}</title>{head}</head><body><div id=\"{root_id}\">{body}</div>{state}{boot}</body></html>",
        lang = lang,
        title = title,
        head = head_extra,
        root_id = root_id,
        body = body_html,
        state = state_script,
        boot = bootstrap_script
    );
    Ok(Value::String(doc))
}

fn builtin_serialize_state(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let val = args
        .first()
        .ok_or_else(|| RuntimeError::new("ssr.serialize_state requires 1 argument".to_string()).at(span))?;
    Ok(Value::String(serialize_safe_script_json(val)))
}

fn builtin_hydration_script(_ev: &mut Evaluator, args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let root_id = args
        .first()
        .map(|v| sanitize_attr_name(&v.to_display()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "app".to_string());
    Ok(Value::String(default_client_hydration_script(&root_id)))
}

fn builtin_suspense_fallback(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new("ssr.suspense_fallback requires (slot_id, fallback)".to_string()).at(span));
    }
    let slot_id = args[0].to_display();
    let fb_html = match &args[1] {
        Value::String(s) => s.clone(),
        other => {
            let node = shae_value_to_vnode(other).map_err(|e| RuntimeError::new(e).at(span))?;
            render_vnode_with_hydration(&node, false).0
        }
    };
    Ok(Value::String(render_suspense_fallback(&slot_id, &fb_html)))
}

fn builtin_suspense_resolve(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new("ssr.suspense_resolve requires (slot_id, content)".to_string()).at(span));
    }
    let slot_id = args[0].to_display();
    let content_html = match &args[1] {
        Value::String(s) => s.clone(),
        other => {
            let node = shae_value_to_vnode(other).map_err(|e| RuntimeError::new(e).at(span))?;
            render_vnode_with_hydration(&node, true).0
        }
    };
    Ok(Value::String(render_suspense_resolve(&slot_id, &content_html)))
}

fn builtin_escape_html(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let s = args
        .first()
        .ok_or_else(|| RuntimeError::new("ssr.escape_html requires 1 argument".to_string()).at(span))?
        .to_display();
    Ok(Value::String(escape_html_text(&s)))
}

// ============================================================================
// 5. Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_vdom::VElement;

    #[test]
    fn test_ssr_template_compile_conditionals_and_loops() {
        let src = "<section><h1>{{title}}</h1>{{#if show}}<ul>{{#each items as item}}<li>{{item}}</li>{{/each}}</ul>{{else}}<p>Empty</p>{{/if}}<div>{{{raw}}}</div></section>";
        let tpl = compile_template(src).expect("compile succeeds");

        let mut ctx = IndexMap::new();
        ctx.insert("title".to_string(), Value::String("Hello <World>".to_string()));
        ctx.insert("show".to_string(), Value::Bool(true));
        ctx.insert(
            "items".to_string(),
            Value::Array(Arc::new(RwLock::new(vec![
                Value::String("A&B".to_string()),
                Value::String("C".to_string()),
            ]))),
        );
        ctx.insert("raw".to_string(), Value::String("<b>bold</b>".to_string()));

        let rendered = tpl.render(&Value::Map(Arc::new(RwLock::new(ctx)))).unwrap();
        assert_eq!(
            rendered,
            "<section><h1>Hello &lt;World&gt;</h1><ul><li>A&amp;B</li><li>C</li></ul><div><b>bold</b></div></section>"
        );
    }

    #[test]
    fn test_ssr_json_script_xss_neutralization() {
        let mut m = IndexMap::new();
        m.insert("bio".to_string(), Value::String("</script><script>alert(1)</script>".to_string()));
        let safe = serialize_safe_script_json(&Value::Map(Arc::new(RwLock::new(m))));
        assert!(!safe.contains("</script>"), "Must escape closing script tag: {}", safe);
        assert!(safe.contains("\\u003c/script\\u003e"), "Expected escaped script sequence: {}", safe);
    }

    #[test]
    fn test_ssr_streaming_utf8_chunk_safety() {
        let html = "<div>Hello \u{1F680} Shae Engine!</div>";
        let chunks = chunk_html_utf8(html, 7);
        assert!(chunks.len() >= 3);
        let reassembled: String = chunks.concat();
        assert_eq!(reassembled, html);
    }

    #[test]
    fn test_ssr_vnode_hydration_markers() {
        let mut events = IndexMap::new();
        events.insert("click".to_string(), Value::Null);
        let node = VNode::Element(VElement {
            tag: "button".to_string(),
            key: Some("btn-1".to_string()),
            props: IndexMap::new(),
            events,
            children: vec![VNode::Text("Save".to_string())],
        });
        let (html, records) = render_vnode_with_hydration(&node, true);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].hid, 0);
        assert_eq!(records[0].events, vec!["click".to_string()]);
        assert_eq!(
            html,
            "<button data-shae-hid=\"0\" data-shae-events=\"click\" data-shae-key=\"btn-1\">Save</button>"
        );
    }
}
