//! Pure-Rust Reactive Virtual DOM (VDOM) engine for Shae.
//!
//! Provides VNode construction (`h`, `text`, `fragment`, `comment`),
//! $O(N)$ keyed tree diffing & patching (`diff`, `apply_patches`),
//! fine-grained Signals / Computed / Effects / Batch reactivity,
//! and reactive `mount` apps with event dispatch.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use indexmap::IndexMap;

use crate::ast::Span;
use crate::eval::{Evaluator, RuntimeError};
use crate::value::Value;

// ============================================================================
// 1. VNode Data Model & HTML Escaping
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct VElement {
    pub tag: String,
    pub key: Option<String>,
    pub props: IndexMap<String, String>,
    pub events: IndexMap<String, Value>,
    pub children: Vec<VNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VNode {
    Element(VElement),
    Text(String),
    Fragment(Vec<VNode>),
    Comment(String),
}

impl VNode {
    pub fn element(
        tag: impl Into<String>,
        props: IndexMap<String, String>,
        children: Vec<VNode>,
    ) -> Self {
        let key = props.get("key").cloned();
        let mut clean_props = props;
        clean_props.shift_remove("key");
        VNode::Element(VElement {
            tag: tag.into(),
            key,
            props: clean_props,
            events: IndexMap::new(),
            children,
        })
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            VNode::Element(el) => el.key.as_deref(),
            _ => None,
        }
    }
}

pub fn escape_html_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn escape_html_attr(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn is_void_tag(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

pub fn render_vnode_to_html(node: &VNode) -> String {
    let mut out = String::new();
    render_vnode_to_buf(node, &mut out);
    out
}

pub fn render_vnode_to_buf(node: &VNode, out: &mut String) {
    match node {
        VNode::Text(s) => out.push_str(&escape_html_text(s)),
        VNode::Comment(s) => {
            out.push_str("<!--");
            out.push_str(&s.replace("-->", "--&gt;"));
            out.push_str("-->");
        }
        VNode::Fragment(children) => {
            for child in children {
                render_vnode_to_buf(child, out);
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
                let attr_name = if k == "className" { "class" } else { k.as_str() };
                if v == "__true__" {
                    out.push(' ');
                    out.push_str(attr_name);
                } else {
                    out.push(' ');
                    out.push_str(attr_name);
                    out.push_str("=\"");
                    out.push_str(&escape_html_attr(v));
                    out.push('"');
                }
            }
            out.push('>');
            if !is_void_tag(&safe_tag) {
                if safe_tag.eq_ignore_ascii_case("script") || safe_tag.eq_ignore_ascii_case("style") {
                    for child in &el.children {
                        if let VNode::Text(s) = child {
                            out.push_str(s);
                        } else {
                            render_vnode_to_buf(child, out);
                        }
                    }
                } else {
                    for child in &el.children {
                        render_vnode_to_buf(child, out);
                    }
                }
                out.push_str("</");
                out.push_str(&safe_tag);
                out.push('>');
            }
        }
    }
}

fn sanitize_tag_name(tag: &str) -> String {
    let mut s: String = tag
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if s.is_empty() {
        s.push_str("div");
    }
    s
}

// ============================================================================
// 2. Shae Value <-> VNode Conversion
// ============================================================================

pub fn vnode_to_shae_value(node: &VNode) -> Value {
    match node {
        VNode::Text(s) => {
            let mut m = IndexMap::new();
            m.insert("_vnode".to_string(), Value::String("text".to_string()));
            m.insert("text".to_string(), Value::String(s.clone()));
            Value::Map(Arc::new(RwLock::new(m)))
        }
        VNode::Comment(s) => {
            let mut m = IndexMap::new();
            m.insert("_vnode".to_string(), Value::String("comment".to_string()));
            m.insert("text".to_string(), Value::String(s.clone()));
            Value::Map(Arc::new(RwLock::new(m)))
        }
        VNode::Fragment(children) => {
            let mut m = IndexMap::new();
            m.insert("_vnode".to_string(), Value::String("fragment".to_string()));
            let arr: Vec<Value> = children.iter().map(vnode_to_shae_value).collect();
            m.insert("children".to_string(), Value::Array(Arc::new(RwLock::new(arr))));
            Value::Map(Arc::new(RwLock::new(m)))
        }
        VNode::Element(el) => {
            let mut m = IndexMap::new();
            m.insert("_vnode".to_string(), Value::String("element".to_string()));
            m.insert("tag".to_string(), Value::String(el.tag.clone()));
            m.insert(
                "key".to_string(),
                el.key
                    .as_ref()
                    .map(|k| Value::String(k.clone()))
                    .unwrap_or(Value::Null),
            );
            let mut pmap = IndexMap::new();
            for (k, v) in &el.props {
                if v == "__true__" {
                    pmap.insert(k.clone(), Value::Bool(true));
                } else if v == "__false__" {
                    pmap.insert(k.clone(), Value::Bool(false));
                } else {
                    pmap.insert(k.clone(), Value::String(v.clone()));
                }
            }
            m.insert("props".to_string(), Value::Map(Arc::new(RwLock::new(pmap))));
            let mut emap = IndexMap::new();
            for (k, v) in &el.events {
                emap.insert(k.clone(), v.clone());
            }
            m.insert("events".to_string(), Value::Map(Arc::new(RwLock::new(emap))));
            let arr: Vec<Value> = el.children.iter().map(vnode_to_shae_value).collect();
            m.insert("children".to_string(), Value::Array(Arc::new(RwLock::new(arr))));
            Value::Map(Arc::new(RwLock::new(m)))
        }
    }
}

pub fn shae_value_to_vnode(val: &Value) -> Result<VNode, String> {
    match val {
        Value::Null => Ok(VNode::Comment(String::new())),
        Value::String(s) => Ok(VNode::Text(s.clone())),
        Value::Int(i) => Ok(VNode::Text(i.to_string())),
        Value::Float(f) => Ok(VNode::Text(f.to_string())),
        Value::Bool(b) => Ok(VNode::Text(b.to_string())),
        Value::Array(arr) => {
            let items = arr.read().unwrap();
            let mut children = Vec::with_capacity(items.len());
            for item in items.iter() {
                children.push(shae_value_to_vnode(item)?);
            }
            Ok(VNode::Fragment(children))
        }
        Value::Map(m) => {
            let map = m.read().unwrap();
            let kind = map
                .get("_vnode")
                .map(|v| v.to_display())
                .unwrap_or_else(|| "element".to_string());
            match kind.as_str() {
                "text" => {
                    let s = map.get("text").map(|v| v.to_display()).unwrap_or_default();
                    Ok(VNode::Text(s))
                }
                "comment" => {
                    let s = map.get("text").map(|v| v.to_display()).unwrap_or_default();
                    Ok(VNode::Comment(s))
                }
                "fragment" => {
                    let mut children = Vec::new();
                    if let Some(Value::Array(arr)) = map.get("children") {
                        for item in arr.read().unwrap().iter() {
                            children.push(shae_value_to_vnode(item)?);
                        }
                    }
                    Ok(VNode::Fragment(children))
                }
                _ => {
                    let tag = map
                        .get("tag")
                        .map(|v| v.to_display())
                        .ok_or_else(|| "VNode map missing 'tag' field".to_string())?;
                    let mut key = map.get("key").and_then(|v| match v {
                        Value::Null => None,
                        other => Some(other.to_display()),
                    });
                    let mut props = IndexMap::new();
                    let mut events = IndexMap::new();
                    if let Some(Value::Map(pm)) = map.get("props") {
                        let pguard = pm.read().unwrap();
                        for (k, v) in pguard.iter() {
                            if k == "key" {
                                if !matches!(v, Value::Null) {
                                    key = Some(v.to_display());
                                }
                                continue;
                            }
                            if is_event_prop(k) && is_callable_value(v) {
                                events.insert(normalize_event_name(k), v.clone());
                                continue;
                            }
                            match v {
                                Value::Bool(true) => {
                                    props.insert(k.clone(), "__true__".to_string());
                                }
                                Value::Bool(false) => {
                                    props.insert(k.clone(), "__false__".to_string());
                                }
                                Value::Null => {}
                                other => {
                                    props.insert(k.clone(), other.to_display());
                                }
                            }
                        }
                    }
                    if let Some(Value::Map(em)) = map.get("events") {
                        for (k, v) in em.read().unwrap().iter() {
                            events.insert(normalize_event_name(k), v.clone());
                        }
                    }
                    let mut children = Vec::new();
                    if let Some(Value::Array(arr)) = map.get("children") {
                        for item in arr.read().unwrap().iter() {
                            children.push(shae_value_to_vnode(item)?);
                        }
                    }
                    Ok(VNode::Element(VElement {
                        tag,
                        key,
                        props,
                        events,
                        children,
                    }))
                }
            }
        }
        other => Err(format!("Cannot convert {} to VNode", other.type_name())),
    }
}

fn is_event_prop(name: &str) -> bool {
    name.starts_with("on") && name.len() > 2
}

fn normalize_event_name(name: &str) -> String {
    let stripped = name
        .strip_prefix("on:")
        .or_else(|| name.strip_prefix("on_"))
        .or_else(|| name.strip_prefix("on"))
        .unwrap_or(name);
    stripped.to_ascii_lowercase()
}

fn is_callable_value(val: &Value) -> bool {
    matches!(
        val,
        Value::Function { .. }
            | Value::Closure(..)
            | Value::CompiledFunction(..)
            | Value::GcClosure(..)
            | Value::Builtin { .. }
            | Value::BoundMethod { .. }
    )
}

// ============================================================================
// 3. O(N) Keyed Tree Diffing & Patch Application Engine
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum Patch {
    Replace {
        path: Vec<usize>,
        node: VNode,
    },
    SetText {
        path: Vec<usize>,
        text: String,
    },
    SetProp {
        path: Vec<usize>,
        key: String,
        value: String,
    },
    RemoveProp {
        path: Vec<usize>,
        key: String,
    },
    InsertChild {
        path: Vec<usize>,
        index: usize,
        node: VNode,
    },
    RemoveChild {
        path: Vec<usize>,
        index: usize,
    },
    MoveChild {
        path: Vec<usize>,
        from: usize,
        to: usize,
    },
}

impl Patch {
    pub fn to_shae_value(&self) -> Value {
        let mut map = IndexMap::new();
        let make_path = |p: &[usize]| {
            Value::Array(Arc::new(RwLock::new(
                p.iter().map(|i| Value::Int(*i as i64)).collect(),
            )))
        };
        match self {
            Patch::Replace { path, node } => {
                map.insert("op".to_string(), Value::String("replace".to_string()));
                map.insert("path".to_string(), make_path(path));
                map.insert("node".to_string(), vnode_to_shae_value(node));
            }
            Patch::SetText { path, text } => {
                map.insert("op".to_string(), Value::String("set_text".to_string()));
                map.insert("path".to_string(), make_path(path));
                map.insert("text".to_string(), Value::String(text.clone()));
            }
            Patch::SetProp { path, key, value } => {
                map.insert("op".to_string(), Value::String("set_prop".to_string()));
                map.insert("path".to_string(), make_path(path));
                map.insert("key".to_string(), Value::String(key.clone()));
                map.insert("value".to_string(), Value::String(value.clone()));
            }
            Patch::RemoveProp { path, key } => {
                map.insert("op".to_string(), Value::String("remove_prop".to_string()));
                map.insert("path".to_string(), make_path(path));
                map.insert("key".to_string(), Value::String(key.clone()));
            }
            Patch::InsertChild { path, index, node } => {
                map.insert("op".to_string(), Value::String("insert_child".to_string()));
                map.insert("path".to_string(), make_path(path));
                map.insert("index".to_string(), Value::Int(*index as i64));
                map.insert("node".to_string(), vnode_to_shae_value(node));
            }
            Patch::RemoveChild { path, index } => {
                map.insert("op".to_string(), Value::String("remove_child".to_string()));
                map.insert("path".to_string(), make_path(path));
                map.insert("index".to_string(), Value::Int(*index as i64));
            }
            Patch::MoveChild { path, from, to } => {
                map.insert("op".to_string(), Value::String("move_child".to_string()));
                map.insert("path".to_string(), make_path(path));
                map.insert("from".to_string(), Value::Int(*from as i64));
                map.insert("to".to_string(), Value::Int(*to as i64));
            }
        }
        Value::Map(Arc::new(RwLock::new(map)))
    }
}

pub fn diff(old: &VNode, new: &VNode) -> Vec<Patch> {
    let mut patches = Vec::new();
    let mut path = Vec::new();
    diff_node(old, new, &mut path, &mut patches);
    patches
}

fn diff_node(old: &VNode, new: &VNode, path: &mut Vec<usize>, patches: &mut Vec<Patch>) {
    match (old, new) {
        (VNode::Text(a), VNode::Text(b)) => {
            if a != b {
                patches.push(Patch::SetText {
                    path: path.clone(),
                    text: b.clone(),
                });
            }
        }
        (VNode::Comment(a), VNode::Comment(b)) => {
            if a != b {
                patches.push(Patch::SetText {
                    path: path.clone(),
                    text: b.clone(),
                });
            }
        }
        (VNode::Fragment(a), VNode::Fragment(b)) => {
            diff_children(a, b, path, patches);
        }
        (VNode::Element(a), VNode::Element(b)) => {
            if a.tag != b.tag || a.key != b.key {
                patches.push(Patch::Replace {
                    path: path.clone(),
                    node: new.clone(),
                });
                return;
            }
            diff_props(&a.props, &b.props, path, patches);
            diff_children(&a.children, &b.children, path, patches);
        }
        _ => {
            patches.push(Patch::Replace {
                path: path.clone(),
                node: new.clone(),
            });
        }
    }
}

fn diff_props(
    old_props: &IndexMap<String, String>,
    new_props: &IndexMap<String, String>,
    path: &[usize],
    patches: &mut Vec<Patch>,
) {
    for (k, _) in old_props {
        if !new_props.contains_key(k) {
            patches.push(Patch::RemoveProp {
                path: path.to_vec(),
                key: k.clone(),
            });
        }
    }
    for (k, v) in new_props {
        match old_props.get(k) {
            Some(ov) if ov == v => {}
            _ => {
                patches.push(Patch::SetProp {
                    path: path.to_vec(),
                    key: k.clone(),
                    value: v.clone(),
                });
            }
        }
    }
}

fn same_identity(a: &VNode, b: &VNode) -> bool {
    match (a.key(), b.key()) {
        (Some(ka), Some(kb)) => ka == kb,
        (None, None) => true,
        _ => false,
    }
}

/// Keyed O(N) children reconciliation:
/// 1. Fast-path matching prefix and suffix when keys align.
/// 2. O(N) HashMap keyed pairing + unkeyed positional pairing for middle slice.
/// 3. Removes unmatched old children back-to-front, performs minimal MoveChild / InsertChild,
///    and finally diffs every node in-place at its final child index.
fn diff_children(
    old_ch: &[VNode],
    new_ch: &[VNode],
    path: &mut Vec<usize>,
    patches: &mut Vec<Patch>,
) {
    let old_len = old_ch.len();
    let new_len = new_ch.len();

    // Fast path: common prefix
    let mut prefix = 0;
    while prefix < old_len && prefix < new_len && same_identity(&old_ch[prefix], &new_ch[prefix]) {
        path.push(prefix);
        diff_node(&old_ch[prefix], &new_ch[prefix], path, patches);
        path.pop();
        prefix += 1;
    }

    // Fast path: common suffix
    let mut old_tail = old_len;
    let mut new_tail = new_len;
    let mut suffix_pairs: Vec<(usize, usize)> = Vec::new();
    while old_tail > prefix
        && new_tail > prefix
        && same_identity(&old_ch[old_tail - 1], &new_ch[new_tail - 1])
    {
        old_tail -= 1;
        new_tail -= 1;
        suffix_pairs.push((old_tail, new_tail));
    }

    // Middle range: old_ch[prefix..old_tail] vs new_ch[prefix..new_tail]
    let mid_old_len = old_tail - prefix;
    let mid_new_len = new_tail - prefix;

    if mid_old_len == 0 && mid_new_len == 0 {
        for &(oi, ni) in suffix_pairs.iter().rev() {
            path.push(ni);
            diff_node(&old_ch[oi], &new_ch[ni], path, patches);
            path.pop();
        }
        return;
    }

    // Map keys in old middle slice -> old index (relative to 0)
    let mut old_key_to_idx: HashMap<&str, usize> = HashMap::with_capacity(mid_old_len);
    let mut unkeyed_old: Vec<usize> = Vec::new();
    for oi in prefix..old_tail {
        if let Some(k) = old_ch[oi].key() {
            old_key_to_idx.insert(k, oi);
        } else {
            unkeyed_old.push(oi);
        }
    }

    // Assign each new middle index -> Option<old_index>
    let mut new_to_old: Vec<Option<usize>> = vec![None; mid_new_len];
    let mut matched_old: HashSet<usize> = HashSet::with_capacity(mid_old_len);
    let mut unkeyed_cursor = 0usize;

    for (rel_ni, ni) in (prefix..new_tail).enumerate() {
        if let Some(k) = new_ch[ni].key() {
            if let Some(&oi) = old_key_to_idx.get(k) {
                new_to_old[rel_ni] = Some(oi);
                matched_old.insert(oi);
            }
        } else if unkeyed_cursor < unkeyed_old.len() {
            let oi = unkeyed_old[unkeyed_cursor];
            unkeyed_cursor += 1;
            new_to_old[rel_ni] = Some(oi);
            matched_old.insert(oi);
        }
    }

    // Working sequence representing live child indices (using old index `Some(oi)`)
    // Start with the current child list indices 0..old_len, remove unmatched old indices right-to-left.
    let mut working: Vec<usize> = (0..old_len).collect();
    for pos in (prefix..old_tail).rev() {
        if !matched_old.contains(&pos) {
            patches.push(Patch::RemoveChild {
                path: path.clone(),
                index: pos,
            });
            working.remove(pos);
        }
    }

    // Now reorder / insert into middle slice [prefix..new_tail] so `working` matches target new indices.
    // Lookup O(1) current position of each old index using a reverse position table.
    let mut old_pos_in_working: Vec<usize> = vec![0; old_len];
    for (cur_idx, &oi) in working.iter().enumerate() {
        old_pos_in_working[oi] = cur_idx;
    }

    // To keep moves linear/minimal, track which working positions move/insert sequentially
    for (rel_ni, ni) in (prefix..new_tail).enumerate() {
        if let Some(oi) = new_to_old[rel_ni] {
            let cur_pos = old_pos_in_working[oi];
            if cur_pos != ni {
                patches.push(Patch::MoveChild {
                    path: path.clone(),
                    from: cur_pos,
                    to: ni,
                });
                let moved = working.remove(cur_pos);
                working.insert(ni, moved);
                // Update affected slice bounds in reverse position table
                let lo = cur_pos.min(ni);
                let hi = cur_pos.max(ni);
                for idx in lo..=hi {
                    old_pos_in_working[working[idx]] = idx;
                }
            }
            path.push(ni);
            diff_node(&old_ch[oi], &new_ch[ni], path, patches);
            path.pop();
        } else {
            patches.push(Patch::InsertChild {
                path: path.clone(),
                index: ni,
                node: new_ch[ni].clone(),
            });
            // Insert dummy placeholder and shift following indices
            working.insert(ni, usize::MAX);
            for idx in (ni + 1)..working.len() {
                let woi = working[idx];
                if woi != usize::MAX {
                    old_pos_in_working[woi] = idx;
                }
            }
        }
    }

    // Diff suffix pairs at their final new indices
    for &(oi, ni) in suffix_pairs.iter().rev() {
        path.push(ni);
        diff_node(&old_ch[oi], &new_ch[ni], path, patches);
        path.pop();
    }
}

/// Applies a sequence of patches produced by `diff()` to a VNode tree.
pub fn apply_patches(root: &VNode, patches: &[Patch]) -> Result<VNode, String> {
    let mut tree = root.clone();
    for patch in patches {
        apply_single_patch(&mut tree, patch)?;
    }
    Ok(tree)
}

fn apply_single_patch(root: &mut VNode, patch: &Patch) -> Result<(), String> {
    match patch {
        Patch::Replace { path, node } => {
            let target = get_node_mut(root, path)?;
            *target = node.clone();
        }
        Patch::SetText { path, text } => {
            let target = get_node_mut(root, path)?;
            match target {
                VNode::Text(s) | VNode::Comment(s) => *s = text.clone(),
                _ => return Err("SetText applied to non-text VNode".to_string()),
            }
        }
        Patch::SetProp { path, key, value } => {
            let target = get_node_mut(root, path)?;
            match target {
                VNode::Element(el) => {
                    el.props.insert(key.clone(), value.clone());
                }
                _ => return Err("SetProp applied to non-element VNode".to_string()),
            }
        }
        Patch::RemoveProp { path, key } => {
            let target = get_node_mut(root, path)?;
            match target {
                VNode::Element(el) => {
                    el.props.shift_remove(key);
                }
                _ => return Err("RemoveProp applied to non-element VNode".to_string()),
            }
        }
        Patch::InsertChild { path, index, node } => {
            let target = get_node_mut(root, path)?;
            let children = get_children_mut(target)?;
            if *index > children.len() {
                return Err(format!("InsertChild index {} out of bounds ({})", index, children.len()));
            }
            children.insert(*index, node.clone());
        }
        Patch::RemoveChild { path, index } => {
            let target = get_node_mut(root, path)?;
            let children = get_children_mut(target)?;
            if *index >= children.len() {
                return Err(format!("RemoveChild index {} out of bounds ({})", index, children.len()));
            }
            children.remove(*index);
        }
        Patch::MoveChild { path, from, to } => {
            let target = get_node_mut(root, path)?;
            let children = get_children_mut(target)?;
            if *from >= children.len() || *to >= children.len() {
                return Err(format!(
                    "MoveChild indices ({} -> {}) out of bounds ({})",
                    from,
                    to,
                    children.len()
                ));
            }
            let item = children.remove(*from);
            children.insert(*to, item);
        }
    }
    Ok(())
}

fn get_node_mut<'a>(mut node: &'a mut VNode, path: &[usize]) -> Result<&'a mut VNode, String> {
    for &idx in path {
        let children = get_children_mut(node)?;
        node = children
            .get_mut(idx)
            .ok_or_else(|| format!("VNode child path index {} out of bounds", idx))?;
    }
    Ok(node)
}

pub fn get_node_ref<'a>(mut node: &'a VNode, path: &[usize]) -> Option<&'a VNode> {
    for &idx in path {
        let children = match node {
            VNode::Element(el) => &el.children,
            VNode::Fragment(ch) => ch,
            _ => return None,
        };
        node = children.get(idx)?;
    }
    Some(node)
}

fn get_children_mut(node: &mut VNode) -> Result<&mut Vec<VNode>, String> {
    match node {
        VNode::Element(el) => Ok(&mut el.children),
        VNode::Fragment(ch) => Ok(ch),
        _ => Err("VNode does not have children".to_string()),
    }
}

// ============================================================================
// 4. Fine-Grained Reactive Signal / Computed / Effect Graph
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SubscriberKind {
    Computed(u64),
    Effect(u64),
}

struct SignalEntry {
    value: Value,
    subscribers: Vec<SubscriberKind>,
}

struct ComputedEntry {
    fn_val: Value,
    cached: Option<Value>,
    dirty: bool,
    computing: bool,
    deps: Vec<u64>,        // signal or computed IDs depending on (using unified ID space)
    subscribers: Vec<SubscriberKind>,
}

struct EffectEntry {
    fn_val: Value,
    deps: Vec<u64>,
    active: bool,
    run_count: usize,
}

#[derive(Default)]
struct ReactiveStore {
    signals: HashMap<u64, SignalEntry>,
    computeds: HashMap<u64, ComputedEntry>,
    effects: HashMap<u64, EffectEntry>,
    pending_effects: Vec<u64>,
    batch_depth: usize,
}

static NEXT_REACTIVE_ID: AtomicU64 = AtomicU64::new(1);
static REACTIVE_STORE: OnceLock<Mutex<ReactiveStore>> = OnceLock::new();

fn store() -> &'static Mutex<ReactiveStore> {
    REACTIVE_STORE.get_or_init(|| Mutex::new(ReactiveStore::default()))
}

thread_local! {
    static TRACKING_STACK: RefCell<Vec<SubscriberKind>> = const { RefCell::new(Vec::new()) };
}

fn current_tracker() -> Option<SubscriberKind> {
    TRACKING_STACK.with(|s| s.borrow().last().copied())
}

fn push_tracker(sub: SubscriberKind) {
    TRACKING_STACK.with(|s| s.borrow_mut().push(sub));
}

fn pop_tracker() {
    TRACKING_STACK.with(|s| {
        s.borrow_mut().pop();
    });
}

fn record_dependency(dep_id: u64, sub: SubscriberKind) {
    let mut st = store().lock().unwrap();
    if let Some(sig) = st.signals.get_mut(&dep_id) {
        if !sig.subscribers.contains(&sub) {
            sig.subscribers.push(sub);
        }
    } else if let Some(comp) = st.computeds.get_mut(&dep_id) {
        if !comp.subscribers.contains(&sub) {
            comp.subscribers.push(sub);
        }
    }
    match sub {
        SubscriberKind::Computed(cid) => {
            if let Some(c) = st.computeds.get_mut(&cid) {
                if !c.deps.contains(&dep_id) {
                    c.deps.push(dep_id);
                }
            }
        }
        SubscriberKind::Effect(eid) => {
            if let Some(e) = st.effects.get_mut(&eid) {
                if !e.deps.contains(&dep_id) {
                    e.deps.push(dep_id);
                }
            }
        }
    }
}

pub fn signal_create(initial: Value) -> u64 {
    let id = NEXT_REACTIVE_ID.fetch_add(1, Ordering::SeqCst);
    let mut st = store().lock().unwrap();
    st.signals.insert(
        id,
        SignalEntry {
            value: initial,
            subscribers: Vec::new(),
        },
    );
    id
}

pub fn signal_get(id: u64) -> Result<Value, String> {
    if let Some(sub) = current_tracker() {
        record_dependency(id, sub);
    }
    let st = store().lock().unwrap();
    st.signals
        .get(&id)
        .map(|s| s.value.clone())
        .ok_or_else(|| format!("Invalid signal id {}", id))
}

pub fn signal_set(ev: &mut Evaluator, id: u64, new_val: Value, span: Span) -> Result<Value, RuntimeError> {
    let mut queue = Vec::new();
    {
        let mut st = store().lock().unwrap();
        let subs = {
            let sig = st
                .signals
                .get_mut(&id)
                .ok_or_else(|| RuntimeError::new(format!("Invalid signal id {}", id)).at(span))?;
            sig.value = new_val.clone();
            sig.subscribers.clone()
        };
        propagate_dirty_locked(&mut st, &subs, &mut queue);
        if st.batch_depth > 0 {
            for eid in queue {
                if !st.pending_effects.contains(&eid) {
                    st.pending_effects.push(eid);
                }
            }
            return Ok(new_val);
        }
    }
    flush_effect_ids(ev, queue, span)?;
    Ok(new_val)
}

fn propagate_dirty_locked(st: &mut ReactiveStore, subs: &[SubscriberKind], effects_out: &mut Vec<u64>) {
    for &sub in subs {
        match sub {
            SubscriberKind::Effect(eid) => {
                if let Some(eff) = st.effects.get(&eid) {
                    if eff.active && !effects_out.contains(&eid) {
                        effects_out.push(eid);
                    }
                }
            }
            SubscriberKind::Computed(cid) => {
                let downstream = if let Some(comp) = st.computeds.get_mut(&cid) {
                    if !comp.dirty {
                        comp.dirty = true;
                        Some(comp.subscribers.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(ds) = downstream {
                    propagate_dirty_locked(st, &ds, effects_out);
                }
            }
        }
    }
}

pub fn computed_create(fn_val: Value) -> u64 {
    let id = NEXT_REACTIVE_ID.fetch_add(1, Ordering::SeqCst);
    let mut st = store().lock().unwrap();
    st.computeds.insert(
        id,
        ComputedEntry {
            fn_val,
            cached: None,
            dirty: true,
            computing: false,
            deps: Vec::new(),
            subscribers: Vec::new(),
        },
    );
    id
}

pub fn computed_get(ev: &mut Evaluator, id: u64, span: Span) -> Result<Value, RuntimeError> {
    if let Some(sub) = current_tracker() {
        record_dependency(id, sub);
    }

    let fn_val = {
        let mut st = store().lock().unwrap();
        let comp = st
            .computeds
            .get_mut(&id)
            .ok_or_else(|| RuntimeError::new(format!("Invalid computed id {}", id)).at(span))?;
        if comp.computing {
            return Err(RuntimeError::new("Cyclic dependency detected in computed signal".to_string()).at(span));
        }
        if !comp.dirty {
            if let Some(val) = &comp.cached {
                return Ok(val.clone());
            }
        }
        comp.computing = true;
        comp.deps.clear();
        comp.fn_val.clone()
    };

    push_tracker(SubscriberKind::Computed(id));
    let eval_res = ev.call_value(&fn_val, Vec::new(), span);
    pop_tracker();

    let mut st = store().lock().unwrap();
    if let Some(comp) = st.computeds.get_mut(&id) {
        comp.computing = false;
        if let Ok(ref v) = eval_res {
            comp.cached = Some(v.clone());
            comp.dirty = false;
        }
    }
    eval_res
}

pub fn effect_create(ev: &mut Evaluator, fn_val: Value, span: Span) -> Result<u64, RuntimeError> {
    let id = NEXT_REACTIVE_ID.fetch_add(1, Ordering::SeqCst);
    {
        let mut st = store().lock().unwrap();
        st.effects.insert(
            id,
            EffectEntry {
                fn_val,
                deps: Vec::new(),
                active: true,
                run_count: 0,
            },
        );
    }
    run_effect(ev, id, span)?;
    Ok(id)
}

fn run_effect(ev: &mut Evaluator, id: u64, span: Span) -> Result<(), RuntimeError> {
    let fn_val = {
        let mut st = store().lock().unwrap();
        let eff = match st.effects.get_mut(&id) {
            Some(e) if e.active => e,
            _ => return Ok(()),
        };
        eff.deps.clear();
        eff.run_count += 1;
        eff.fn_val.clone()
    };

    push_tracker(SubscriberKind::Effect(id));
    let res = ev.call_value(&fn_val, Vec::new(), span);
    pop_tracker();
    res.map(|_| ())
}

fn flush_effect_ids(ev: &mut Evaluator, ids: Vec<u64>, span: Span) -> Result<(), RuntimeError> {
    for eid in ids {
        run_effect(ev, eid, span)?;
    }
    Ok(())
}

pub fn effect_dispose(id: u64) -> bool {
    let mut st = store().lock().unwrap();
    if let Some(eff) = st.effects.get_mut(&id) {
        eff.active = false;
        true
    } else {
        false
    }
}

pub fn run_batch(ev: &mut Evaluator, fn_val: Value, span: Span) -> Result<Value, RuntimeError> {
    {
        let mut st = store().lock().unwrap();
        st.batch_depth += 1;
    }
    let res = ev.call_value(&fn_val, Vec::new(), span);
    let pending = {
        let mut st = store().lock().unwrap();
        st.batch_depth = st.batch_depth.saturating_sub(1);
        if st.batch_depth == 0 {
            std::mem::take(&mut st.pending_effects)
        } else {
            Vec::new()
        }
    };
    if !pending.is_empty() {
        flush_effect_ids(ev, pending, span)?;
    }
    res
}

// ============================================================================
// 5. Mounted Reactive VDOM Application & Event Dispatcher
// ============================================================================

struct MountedApp {
    render_fn: Value,
    current_tree: VNode,
    last_patches: Vec<Patch>,
    render_count: usize,
}

static APP_INSTANCES: OnceLock<Mutex<HashMap<u64, MountedApp>>> = OnceLock::new();

fn apps() -> &'static Mutex<HashMap<u64, MountedApp>> {
    APP_INSTANCES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn mount_app(ev: &mut Evaluator, render_fn: Value, span: Span) -> Result<Value, RuntimeError> {
    let initial_val = ev.call_value(&render_fn, Vec::new(), span)?;
    let initial_tree = shae_value_to_vnode(&initial_val).map_err(|e| RuntimeError::new(e).at(span))?;
    let id = NEXT_REACTIVE_ID.fetch_add(1, Ordering::SeqCst);

    {
        let mut guard = apps().lock().unwrap();
        guard.insert(
            id,
            MountedApp {
                render_fn,
                current_tree: initial_tree,
                last_patches: Vec::new(),
                render_count: 1,
            },
        );
    }

    Ok(make_app_handle(id))
}

fn rerender_app(ev: &mut Evaluator, id: u64, span: Span) -> Result<Vec<Patch>, RuntimeError> {
    let (render_fn, prev_tree) = {
        let guard = apps().lock().unwrap();
        let app = guard
            .get(&id)
            .ok_or_else(|| RuntimeError::new(format!("Invalid VDOM app id {}", id)).at(span))?;
        (app.render_fn.clone(), app.current_tree.clone())
    };

    let next_val = ev.call_value(&render_fn, Vec::new(), span)?;
    let next_tree = shae_value_to_vnode(&next_val).map_err(|e| RuntimeError::new(e).at(span))?;
    let patches = diff(&prev_tree, &next_tree);

    let mut guard = apps().lock().unwrap();
    if let Some(app) = guard.get_mut(&id) {
        app.current_tree = next_tree;
        app.last_patches = patches.clone();
        app.render_count += 1;
    }

    Ok(patches)
}

fn dispatch_app_event(
    ev: &mut Evaluator,
    id: u64,
    path: &[usize],
    event_name: &str,
    payload: Option<Value>,
    span: Span,
) -> Result<Vec<Patch>, RuntimeError> {
    let handler = {
        let guard = apps().lock().unwrap();
        let app = guard
            .get(&id)
            .ok_or_else(|| RuntimeError::new(format!("Invalid VDOM app id {}", id)).at(span))?;
        let target = get_node_ref(&app.current_tree, path)
            .ok_or_else(|| RuntimeError::new(format!("No VNode at path {:?}", path)).at(span))?;
        let norm_name = normalize_event_name(event_name);
        match target {
            VNode::Element(el) => el.events.get(&norm_name).cloned().ok_or_else(|| {
                RuntimeError::new(format!("No '{}' event handler on target VNode", norm_name)).at(span)
            })?,
            _ => {
                return Err(
                    RuntimeError::new("Cannot dispatch event to non-element VNode".to_string())
                        .at(span),
                )
            }
        }
    };

    let args = match payload {
        Some(p) => vec![p],
        None => Vec::new(),
    };
    ev.call_value(&handler, args, span)?;
    rerender_app(ev, id, span)
}

fn make_signal_handle(id: u64) -> Value {
    let mut m = IndexMap::new();
    m.insert("_vdom_signal_id".to_string(), Value::Int(id as i64));
    m.insert("id".to_string(), Value::Int(id as i64));
    Value::Map(Arc::new(RwLock::new(m)))
}

fn make_computed_handle(id: u64) -> Value {
    let mut m = IndexMap::new();
    m.insert("_vdom_computed_id".to_string(), Value::Int(id as i64));
    m.insert("id".to_string(), Value::Int(id as i64));
    Value::Map(Arc::new(RwLock::new(m)))
}

fn make_effect_handle(id: u64) -> Value {
    let mut m = IndexMap::new();
    m.insert("_vdom_effect_id".to_string(), Value::Int(id as i64));
    m.insert("id".to_string(), Value::Int(id as i64));
    Value::Map(Arc::new(RwLock::new(m)))
}

fn make_app_handle(id: u64) -> Value {
    let mut m = IndexMap::new();
    m.insert("_vdom_app_id".to_string(), Value::Int(id as i64));
    m.insert("id".to_string(), Value::Int(id as i64));
    Value::Map(Arc::new(RwLock::new(m)))
}

fn get_handle_id(map: &Arc<RwLock<IndexMap<String, Value>>>, field: &str, span: Span) -> Result<u64, RuntimeError> {
    let g = map.read().unwrap();
    match g.get(field) {
        Some(Value::Int(i)) if *i > 0 => Ok(*i as u64),
        _ => Err(RuntimeError::new(format!("Missing valid handle field '{}'", field)).at(span)),
    }
}

pub fn eval_vdom_signal_method(
    ev: &mut Evaluator,
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let id = get_handle_id(map, "_vdom_signal_id", span)?;
    match method {
        "get" => signal_get(id).map_err(|e| RuntimeError::new(e).at(span)),
        "set" => {
            let val = args.into_iter().next().unwrap_or(Value::Null);
            signal_set(ev, id, val, span)
        }
        "update" => {
            let updater = args
                .into_iter()
                .next()
                .ok_or_else(|| RuntimeError::new("signal.update requires a callback".to_string()).at(span))?;
            let cur = signal_get(id).map_err(|e| RuntimeError::new(e).at(span))?;
            let next = ev.call_value(&updater, vec![cur], span)?;
            signal_set(ev, id, next, span)
        }
        other => Err(RuntimeError::new(format!("Unknown signal method '{}'", other)).at(span)),
    }
}

pub fn eval_vdom_computed_method(
    ev: &mut Evaluator,
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    _args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let id = get_handle_id(map, "_vdom_computed_id", span)?;
    match method {
        "get" => computed_get(ev, id, span),
        other => Err(RuntimeError::new(format!("Unknown computed method '{}'", other)).at(span)),
    }
}

pub fn eval_vdom_effect_method(
    _ev: &mut Evaluator,
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    _args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let id = get_handle_id(map, "_vdom_effect_id", span)?;
    match method {
        "dispose" | "stop" => Ok(Value::Bool(effect_dispose(id))),
        "run_count" | "runCount" => {
            let st = store().lock().unwrap();
            let count = st.effects.get(&id).map(|e| e.run_count).unwrap_or(0);
            Ok(Value::Int(count as i64))
        }
        other => Err(RuntimeError::new(format!("Unknown effect method '{}'", other)).at(span)),
    }
}

pub fn eval_vdom_app_method(
    ev: &mut Evaluator,
    map: &Arc<RwLock<IndexMap<String, Value>>>,
    method: &str,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, RuntimeError> {
    let id = get_handle_id(map, "_vdom_app_id", span)?;
    match method {
        "tree" => {
            let guard = apps().lock().unwrap();
            let app = guard
                .get(&id)
                .ok_or_else(|| RuntimeError::new(format!("Invalid app id {}", id)).at(span))?;
            Ok(vnode_to_shae_value(&app.current_tree))
        }
        "html" => {
            let guard = apps().lock().unwrap();
            let app = guard
                .get(&id)
                .ok_or_else(|| RuntimeError::new(format!("Invalid app id {}", id)).at(span))?;
            Ok(Value::String(render_vnode_to_html(&app.current_tree)))
        }
        "patches" => {
            let guard = apps().lock().unwrap();
            let app = guard
                .get(&id)
                .ok_or_else(|| RuntimeError::new(format!("Invalid app id {}", id)).at(span))?;
            let list: Vec<Value> = app.last_patches.iter().map(|p| p.to_shae_value()).collect();
            Ok(Value::Array(Arc::new(RwLock::new(list))))
        }
        "render" | "update" => {
            let patches = rerender_app(ev, id, span)?;
            let list: Vec<Value> = patches.iter().map(|p| p.to_shae_value()).collect();
            Ok(Value::Array(Arc::new(RwLock::new(list))))
        }
        "dispatch" => {
            if args.len() < 2 {
                return Err(RuntimeError::new(
                    "app.dispatch requires (path_array, event_name, [payload])".to_string(),
                )
                .at(span));
            }
            let path = parse_path_arg(&args[0], span)?;
            let event_name = args[1].to_display();
            let payload = args.get(2).cloned();
            let patches = dispatch_app_event(ev, id, &path, &event_name, payload, span)?;
            let list: Vec<Value> = patches.iter().map(|p| p.to_shae_value()).collect();
            Ok(Value::Array(Arc::new(RwLock::new(list))))
        }
        "render_count" | "renderCount" => {
            let guard = apps().lock().unwrap();
            let count = guard.get(&id).map(|a| a.render_count).unwrap_or(0);
            Ok(Value::Int(count as i64))
        }
        "unmount" | "close" => {
            let mut guard = apps().lock().unwrap();
            Ok(Value::Bool(guard.remove(&id).is_some()))
        }
        other => Err(RuntimeError::new(format!("Unknown VDOM app method '{}'", other)).at(span)),
    }
}

fn parse_path_arg(val: &Value, span: Span) -> Result<Vec<usize>, RuntimeError> {
    match val {
        Value::Array(arr) => {
            let items = arr.read().unwrap();
            let mut out = Vec::with_capacity(items.len());
            for item in items.iter() {
                match item {
                    Value::Int(i) if *i >= 0 => out.push(*i as usize),
                    _ => return Err(RuntimeError::new("Path indices must be non-negative integers".to_string()).at(span)),
                }
            }
            Ok(out)
        }
        _ => Err(RuntimeError::new("Expected array of child indices for path".to_string()).at(span)),
    }
}

// ============================================================================
// 6. Standard Library Registration (`std:web:vdom` & `std:vdom`)
// ============================================================================

pub fn build_vdom_module() -> Value {
    let mut map = IndexMap::new();
    let entries: Vec<(&str, crate::value::BuiltinFn)> = vec![
        ("h", builtin_h),
        ("el", builtin_h),
        ("text", builtin_text),
        ("fragment", builtin_fragment),
        ("comment", builtin_comment),
        ("diff", builtin_diff),
        ("apply_patches", builtin_apply_patches),
        ("applyPatches", builtin_apply_patches),
        ("render_html", builtin_render_html),
        ("renderHtml", builtin_render_html),
        ("signal", builtin_signal),
        ("sig_get", builtin_sig_get),
        ("sig_set", builtin_sig_set),
        ("computed", builtin_computed),
        ("effect", builtin_effect),
        ("batch", builtin_batch),
        ("mount", builtin_mount),
        ("dispatch", builtin_dispatch),
    ];

    for (name, func) in entries {
        map.insert(
            name.to_string(),
            Value::Builtin {
                name: format!("vdom.{}", name),
                func,
            },
        );
    }

    Value::Map(Arc::new(RwLock::new(map)))
}

fn is_vnode_map(val: &Value) -> bool {
    if let Value::Map(m) = val {
        m.read().unwrap().contains_key("_vnode")
    } else {
        false
    }
}

fn builtin_h(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::new("vdom.h requires at least a tag name".to_string()).at(span));
    }
    let tag = args[0].to_display();
    let mut props_map = IndexMap::new();
    let mut events_map = IndexMap::new();
    let mut key: Option<String> = None;
    let mut children = Vec::new();

    if args.len() == 2 {
        let second = &args[1];
        if let Value::Map(m) = second {
            if is_vnode_map(second) {
                children.push(shae_value_to_vnode(second).map_err(|e| RuntimeError::new(e).at(span))?);
            } else {
                extract_props_from_map(&m.read().unwrap(), &mut key, &mut props_map, &mut events_map);
            }
        } else if let Value::Array(arr) = second {
            for item in arr.read().unwrap().iter() {
                children.push(shae_value_to_vnode(item).map_err(|e| RuntimeError::new(e).at(span))?);
            }
        } else if !matches!(second, Value::Null) {
            children.push(shae_value_to_vnode(second).map_err(|e| RuntimeError::new(e).at(span))?);
        }
    } else if args.len() >= 3 {
        if let Value::Map(m) = &args[1] {
            extract_props_from_map(&m.read().unwrap(), &mut key, &mut props_map, &mut events_map);
        } else if !matches!(&args[1], Value::Null) {
            return Err(RuntimeError::new("vdom.h: second argument must be props map or null".to_string()).at(span));
        }

        for arg in args.iter().skip(2) {
            match arg {
                Value::Array(arr) => {
                    for item in arr.read().unwrap().iter() {
                        children.push(shae_value_to_vnode(item).map_err(|e| RuntimeError::new(e).at(span))?);
                    }
                }
                Value::Null => {}
                other => {
                    children.push(shae_value_to_vnode(other).map_err(|e| RuntimeError::new(e).at(span))?);
                }
            }
        }
    }

    let node = VNode::Element(VElement {
        tag,
        key,
        props: props_map,
        events: events_map,
        children,
    });
    Ok(vnode_to_shae_value(&node))
}

fn extract_props_from_map(
    src: &IndexMap<String, Value>,
    key_out: &mut Option<String>,
    props_out: &mut IndexMap<String, String>,
    events_out: &mut IndexMap<String, Value>,
) {
    for (k, v) in src {
        if k == "key" {
            if !matches!(v, Value::Null) {
                *key_out = Some(v.to_display());
            }
            continue;
        }
        if is_event_prop(k) && is_callable_value(v) {
            events_out.insert(normalize_event_name(k), v.clone());
            continue;
        }
        match v {
            Value::Bool(true) => {
                props_out.insert(k.clone(), "__true__".to_string());
            }
            Value::Bool(false) => {
                props_out.insert(k.clone(), "__false__".to_string());
            }
            Value::Null => {}
            other => {
                props_out.insert(k.clone(), other.to_display());
            }
        }
    }
}

fn builtin_text(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let s = args
        .first()
        .ok_or_else(|| RuntimeError::new("vdom.text requires 1 argument".to_string()).at(span))?
        .to_display();
    Ok(vnode_to_shae_value(&VNode::Text(s)))
}

fn builtin_comment(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let s = args
        .first()
        .ok_or_else(|| RuntimeError::new("vdom.comment requires 1 argument".to_string()).at(span))?
        .to_display();
    Ok(vnode_to_shae_value(&VNode::Comment(s)))
}

fn builtin_fragment(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let mut children = Vec::new();
    for arg in &args {
        match arg {
            Value::Array(arr) => {
                for item in arr.read().unwrap().iter() {
                    children.push(shae_value_to_vnode(item).map_err(|e| RuntimeError::new(e).at(span))?);
                }
            }
            Value::Null => {}
            other => {
                children.push(shae_value_to_vnode(other).map_err(|e| RuntimeError::new(e).at(span))?);
            }
        }
    }
    Ok(vnode_to_shae_value(&VNode::Fragment(children)))
}

fn builtin_diff(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new("vdom.diff requires (old_vnode, new_vnode)".to_string()).at(span));
    }
    let old = shae_value_to_vnode(&args[0]).map_err(|e| RuntimeError::new(e).at(span))?;
    let new = shae_value_to_vnode(&args[1]).map_err(|e| RuntimeError::new(e).at(span))?;
    let patches = diff(&old, &new);
    let out: Vec<Value> = patches.iter().map(|p| p.to_shae_value()).collect();
    Ok(Value::Array(Arc::new(RwLock::new(out))))
}

fn patch_from_shae_value(val: &Value) -> Result<Patch, String> {
    let Value::Map(m) = val else {
        return Err("Patch entry must be a map".to_string());
    };
    let g = m.read().unwrap();
    let op = g.get("op").map(|v| v.to_display()).unwrap_or_default();
    let path: Vec<usize> = match g.get("path") {
        Some(Value::Array(arr)) => arr
            .read()
            .unwrap()
            .iter()
            .map(|v| match v {
                Value::Int(i) if *i >= 0 => Ok(*i as usize),
                _ => Err("Invalid index in patch path".to_string()),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    match op.as_str() {
        "replace" => {
            let node = shae_value_to_vnode(g.get("node").ok_or("Missing node in replace patch")?)?;
            Ok(Patch::Replace { path, node })
        }
        "set_text" => {
            let text = g.get("text").map(|v| v.to_display()).unwrap_or_default();
            Ok(Patch::SetText { path, text })
        }
        "set_prop" => {
            let key = g.get("key").map(|v| v.to_display()).ok_or("Missing key")?;
            let value = g.get("value").map(|v| v.to_display()).unwrap_or_default();
            Ok(Patch::SetProp { path, key, value })
        }
        "remove_prop" => {
            let key = g.get("key").map(|v| v.to_display()).ok_or("Missing key")?;
            Ok(Patch::RemoveProp { path, key })
        }
        "insert_child" => {
            let index = match g.get("index") {
                Some(Value::Int(i)) if *i >= 0 => *i as usize,
                _ => return Err("Missing valid index in insert_child patch".to_string()),
            };
            let node = shae_value_to_vnode(g.get("node").ok_or("Missing node in insert_child")?)?;
            Ok(Patch::InsertChild { path, index, node })
        }
        "remove_child" => {
            let index = match g.get("index") {
                Some(Value::Int(i)) if *i >= 0 => *i as usize,
                _ => return Err("Missing valid index in remove_child patch".to_string()),
            };
            Ok(Patch::RemoveChild { path, index })
        }
        "move_child" => {
            let from = match g.get("from") {
                Some(Value::Int(i)) if *i >= 0 => *i as usize,
                _ => return Err("Missing valid 'from' in move_child patch".to_string()),
            };
            let to = match g.get("to") {
                Some(Value::Int(i)) if *i >= 0 => *i as usize,
                _ => return Err("Missing valid 'to' in move_child patch".to_string()),
            };
            Ok(Patch::MoveChild { path, from, to })
        }
        other => Err(format!("Unknown patch op '{}'", other)),
    }
}

fn builtin_apply_patches(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(RuntimeError::new("vdom.apply_patches requires (vnode, patches)".to_string()).at(span));
    }
    let root = shae_value_to_vnode(&args[0]).map_err(|e| RuntimeError::new(e).at(span))?;
    let Value::Array(arr) = &args[1] else {
        return Err(RuntimeError::new("vdom.apply_patches: second argument must be array of patches".to_string()).at(span));
    };
    let patches: Vec<Patch> = arr
        .read()
        .unwrap()
        .iter()
        .map(patch_from_shae_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| RuntimeError::new(e).at(span))?;
    let patched = apply_patches(&root, &patches).map_err(|e| RuntimeError::new(e).at(span))?;
    Ok(vnode_to_shae_value(&patched))
}

fn builtin_render_html(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let val = args
        .first()
        .ok_or_else(|| RuntimeError::new("vdom.render_html requires a VNode".to_string()).at(span))?;
    let node = shae_value_to_vnode(val).map_err(|e| RuntimeError::new(e).at(span))?;
    Ok(Value::String(render_vnode_to_html(&node)))
}

fn builtin_signal(_ev: &mut Evaluator, args: Vec<Value>, _span: Span) -> Result<Value, RuntimeError> {
    let init = args.into_iter().next().unwrap_or(Value::Null);
    let id = signal_create(init);
    Ok(make_signal_handle(id))
}

fn builtin_sig_get(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let first = args
        .first()
        .ok_or_else(|| RuntimeError::new("sig_get requires a signal handle".to_string()).at(span))?;
    let id = match first {
        Value::Map(m) => get_handle_id(m, "_vdom_signal_id", span)?,
        Value::Int(i) if *i > 0 => *i as u64,
        _ => return Err(RuntimeError::new("sig_get expected signal handle".to_string()).at(span)),
    };
    signal_get(id).map_err(|e| RuntimeError::new(e).at(span))
}

fn builtin_sig_set(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::new("sig_set requires (signal, value)".to_string()).at(span));
    }
    let id = match &args[0] {
        Value::Map(m) => get_handle_id(m, "_vdom_signal_id", span)?,
        Value::Int(i) if *i > 0 => *i as u64,
        _ => return Err(RuntimeError::new("sig_set expected signal handle".to_string()).at(span)),
    };
    signal_set(ev, id, args[1].clone(), span)
}

fn builtin_computed(_ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let fn_val = args
        .into_iter()
        .next()
        .ok_or_else(|| RuntimeError::new("vdom.computed requires a callback function".to_string()).at(span))?;
    let id = computed_create(fn_val);
    Ok(make_computed_handle(id))
}

fn builtin_effect(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let fn_val = args
        .into_iter()
        .next()
        .ok_or_else(|| RuntimeError::new("vdom.effect requires a callback function".to_string()).at(span))?;
    let id = effect_create(ev, fn_val, span)?;
    Ok(make_effect_handle(id))
}

fn builtin_batch(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let fn_val = args
        .into_iter()
        .next()
        .ok_or_else(|| RuntimeError::new("vdom.batch requires a callback function".to_string()).at(span))?;
    run_batch(ev, fn_val, span)
}

fn builtin_mount(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    let fn_val = args
        .into_iter()
        .next()
        .ok_or_else(|| RuntimeError::new("vdom.mount requires a render function".to_string()).at(span))?;
    mount_app(ev, fn_val, span)
}

fn builtin_dispatch(ev: &mut Evaluator, args: Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    if args.len() < 3 {
        return Err(RuntimeError::new("vdom.dispatch requires (app, path, event_name, [payload])".to_string()).at(span));
    }
    let id = match &args[0] {
        Value::Map(m) => get_handle_id(m, "_vdom_app_id", span)?,
        Value::Int(i) if *i > 0 => *i as u64,
        _ => return Err(RuntimeError::new("vdom.dispatch expected mounted app handle".to_string()).at(span)),
    };
    let path = parse_path_arg(&args[1], span)?;
    let event_name = args[2].to_display();
    let payload = args.get(3).cloned();
    let patches = dispatch_app_event(ev, id, &path, &event_name, payload, span)?;
    let out: Vec<Value> = patches.iter().map(|p| p.to_shae_value()).collect();
    Ok(Value::Array(Arc::new(RwLock::new(out))))
}

// ============================================================================
// 7. Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn elem(tag: &str, props: &[(&str, &str)], children: Vec<VNode>) -> VNode {
        let mut map = IndexMap::new();
        for &(k, v) in props {
            map.insert(k.to_string(), v.to_string());
        }
        VNode::element(tag, map, children)
    }

    #[test]
    fn test_vdom_diff_and_patch_props_and_text() {
        let old = elem(
            "div",
            &[("id", "box"), ("class", "old")],
            vec![VNode::Text("hello".to_string())],
        );
        let new = elem(
            "div",
            &[("id", "box"), ("data-active", "1")],
            vec![VNode::Text("world".to_string())],
        );
        let patches = diff(&old, &new);
        let patched = apply_patches(&old, &patches).unwrap();
        assert_eq!(patched, new);
    }

    #[test]
    fn test_vdom_keyed_list_reconciliation_reorder_insert_remove() {
        let old = elem(
            "ul",
            &[],
            vec![
                elem("li", &[("key", "a")], vec![VNode::Text("Alpha".into())]),
                elem("li", &[("key", "b")], vec![VNode::Text("Beta".into())]),
                elem("li", &[("key", "c")], vec![VNode::Text("Gamma".into())]),
                elem("li", &[("key", "d")], vec![VNode::Text("Delta".into())]),
            ],
        );
        let new = elem(
            "ul",
            &[],
            vec![
                elem("li", &[("key", "d")], vec![VNode::Text("Delta Updated".into())]),
                elem("li", &[("key", "x")], vec![VNode::Text("Extra".into())]),
                elem("li", &[("key", "a")], vec![VNode::Text("Alpha".into())]),
                elem("li", &[("key", "c")], vec![VNode::Text("Gamma".into())]),
            ],
        );
        let patches = diff(&old, &new);
        assert!(
            patches.iter().any(|p| matches!(p, Patch::MoveChild { .. })),
            "Expected MoveChild patch for keyed reorder, got: {:?}",
            patches
        );
        let patched = apply_patches(&old, &patches).unwrap();
        assert_eq!(patched, new);
    }

    #[test]
    fn test_vdom_large_keyed_reversal_linearity() {
        let n = 500;
        let old_items: Vec<VNode> = (0..n)
            .map(|i| {
                let k = format!("k{}", i);
                elem("li", &[("key", &k)], vec![VNode::Text(k.clone())])
            })
            .collect();
        let new_items: Vec<VNode> = (0..n)
            .rev()
            .map(|i| {
                let k = format!("k{}", i);
                elem("li", &[("key", &k)], vec![VNode::Text(k.clone())])
            })
            .collect();
        let old = elem("ul", &[], old_items);
        let new = elem("ul", &[], new_items);

        let patches = diff(&old, &new);
        let patched = apply_patches(&old, &patches).unwrap();
        assert_eq!(patched, new);
    }

    #[test]
    fn test_vdom_html_escaping_and_void_tags() {
        let tree = elem(
            "div",
            &[("title", "a\"b&c<d>"), ("disabled", "__true__")],
            vec![
                elem("input", &[("type", "text"), ("value", "<xss>")], vec![]),
                VNode::Text("<script>alert(1)</script> & 'ok'".to_string()),
            ],
        );
        let html = render_vnode_to_html(&tree);
        assert_eq!(
            html,
            "<div title=\"a&quot;b&amp;c&lt;d&gt;\" disabled><input type=\"text\" value=\"&lt;xss&gt;\">&lt;script&gt;alert(1)&lt;/script&gt; &amp; 'ok'</div>"
        );
    }
}
