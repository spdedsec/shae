//! Standard Documentation Engine (`src/doc.rs`, `shae doc`)
//!
//! Extracts doc-comments (`///` and `//!`), function signatures, struct definitions,
//! and enum variants from Shae source files, and renders static HTML sites,
//! GitHub Flavored Markdown references, and JSON documentation models.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DocItemKind {
    Function,
    AsyncFunction,
    Struct,
    Enum,
    Constant,
}

impl DocItemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::AsyncFunction => "async function",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Constant => "constant",
        }
    }

    pub fn badge_class(&self) -> &'static str {
        match self {
            Self::Function => "badge-fn",
            Self::AsyncFunction => "badge-async",
            Self::Struct => "badge-struct",
            Self::Enum => "badge-enum",
            Self::Constant => "badge-const",
        }
    }
}

/// A documented symbol in a Shae source file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocItem {
    pub kind: DocItemKind,
    pub name: String,
    pub signature: String,
    pub description: String,
    pub params: Vec<String>,
    pub fields: Vec<String>,
    pub variants: Vec<String>,
    pub line: usize,
}

/// Documentation for an individual Shae source file or module.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleDoc {
    pub name: String,
    pub path: String,
    pub description: String,
    pub items: Vec<DocItem>,
}

/// Aggregated documentation for a Shae project or package.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageDoc {
    pub package_name: String,
    pub version: String,
    pub description: String,
    pub modules: Vec<ModuleDoc>,
}

impl ModuleDoc {
    /// Extracts documentation comments and signatures from a Shae source string.
    pub fn extract(source: &str, module_name: &str, file_path: &str) -> Self {
        let mut module_doc_lines = Vec::new();
        let mut pending_doc_lines = Vec::new();
        let mut items = Vec::new();

        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            // 1. Module-level doc comment: //!
            if let Some(comment) = line.strip_prefix("//!") {
                module_doc_lines.push(comment.trim().to_string());
                i += 1;
                continue;
            }

            // 2. Item-level doc comment: ///
            if let Some(comment) = line.strip_prefix("///") {
                pending_doc_lines.push(comment.trim().to_string());
                i += 1;
                continue;
            }

            // If empty line between comments and decl, keep comments if next line is decl,
            // else flush if blank line after code
            if line.is_empty() {
                if !pending_doc_lines.is_empty() {
                    // Peek next non-empty line
                    let mut next_i = i + 1;
                    let mut has_next_decl = false;
                    while next_i < lines.len() {
                        let next_line = lines[next_i].trim();
                        if next_line.starts_with("///") {
                            break;
                        }
                        if next_line.starts_with("fn ")
                            || next_line.starts_with("async fn ")
                            || next_line.starts_with("struct ")
                            || next_line.starts_with("enum ")
                            || next_line.starts_with("let ")
                            || next_line.starts_with("const ")
                        {
                            has_next_decl = true;
                            break;
                        }
                        if !next_line.is_empty() {
                            break;
                        }
                        next_i += 1;
                    }
                    if !has_next_decl {
                        pending_doc_lines.clear();
                    }
                }
                i += 1;
                continue;
            }

            // Skip standard comments
            if line.starts_with("//") {
                i += 1;
                continue;
            }

            let doc_text = pending_doc_lines.join("\n");
            pending_doc_lines.clear();

            // 3. Declarations
            // A. Async Function: async fn name(params)
            if let Some(rest) = line.strip_prefix("async fn ") {
                if let Some(item) = parse_fn_decl(rest, true, doc_text, i + 1) {
                    items.push(item);
                }
            }
            // B. Sync Function: fn name(params)
            else if let Some(rest) = line.strip_prefix("fn ") {
                if let Some(item) = parse_fn_decl(rest, false, doc_text, i + 1) {
                    items.push(item);
                }
            }
            // C. Struct: struct Name { ... }
            else if let Some(rest) = line.strip_prefix("struct ") {
                let start_line = i + 1;
                if let Some(item) = parse_struct_decl(rest, &lines, &mut i, doc_text, start_line) {
                    items.push(item);
                }
            }
            // D. Enum: enum Name { ... }
            else if let Some(rest) = line.strip_prefix("enum ") {
                let start_line = i + 1;
                if let Some(item) = parse_enum_decl(rest, &lines, &mut i, doc_text, start_line) {
                    items.push(item);
                }
            }
            // E. Constant or Global: const NAME = ... / let NAME = ...
            else if line.starts_with("const ")
                || (line.starts_with("let ") && !doc_text.is_empty())
            {
                let is_const = line.starts_with("const ");
                let prefix = if is_const { "const " } else { "let " };
                let decl_part = line.strip_prefix(prefix).unwrap();
                let var_name = decl_part.split('=').next().unwrap_or("").trim();
                if !var_name.is_empty() && is_identifier(var_name) {
                    items.push(DocItem {
                        kind: DocItemKind::Constant,
                        name: var_name.to_string(),
                        signature: line.split('{').next().unwrap_or(line).trim().to_string(),
                        description: doc_text,
                        params: Vec::new(),
                        fields: Vec::new(),
                        variants: Vec::new(),
                        line: i + 1,
                    });
                }
            }

            i += 1;
        }

        Self {
            name: module_name.to_string(),
            path: file_path.to_string(),
            description: module_doc_lines.join("\n"),
            items,
        }
    }

    /// Renders module documentation as GitHub Flavored Markdown.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!("# Module `{}`\n\n", self.name));

        if !self.description.is_empty() {
            md.push_str(&self.description);
            md.push_str("\n\n");
        }

        if self.items.is_empty() {
            md.push_str("*No documented items found.*\n");
            return md;
        }

        // Table of contents
        md.push_str("## Table of Contents\n\n");
        for item in &self.items {
            let anchor = item.name.to_lowercase().replace(' ', "-");
            md.push_str(&format!(
                "- [`{}`](#{}) *({})*\n",
                item.name,
                anchor,
                item.kind.as_str()
            ));
        }
        md.push_str("\n---\n\n");

        // Items detailed
        for item in &self.items {
            md.push_str(&format!(
                "### `{}` <small>({} at line {})</small>\n\n",
                item.name,
                item.kind.as_str(),
                item.line
            ));

            md.push_str("```shae\n");
            md.push_str(&item.signature);
            md.push_str("\n```\n\n");

            if !item.description.is_empty() {
                md.push_str(&item.description);
                md.push_str("\n\n");
            }

            if !item.params.is_empty() {
                md.push_str("**Parameters:**\n\n");
                for param in &item.params {
                    md.push_str(&format!("- `{}`\n", param));
                }
                md.push_str("\n");
            }

            if !item.fields.is_empty() {
                md.push_str("**Fields:**\n\n");
                for field in &item.fields {
                    md.push_str(&format!("- `{}`\n", field));
                }
                md.push_str("\n");
            }

            if !item.variants.is_empty() {
                md.push_str("**Variants:**\n\n");
                for variant in &item.variants {
                    md.push_str(&format!("- `{}`\n", variant));
                }
                md.push_str("\n");
            }

            md.push_str("---\n\n");
        }

        md
    }
}

impl PackageDoc {
    /// Renders aggregated documentation as a self-contained static HTML site.
    pub fn to_html(&self) -> String {
        let mut html = String::new();
        html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
        html.push_str("  <meta charset=\"UTF-8\">\n");
        html.push_str(
            "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n",
        );
        html.push_str(&format!(
            "  <title>{} v{} — Shae Documentation</title>\n",
            escape_html(&self.package_name),
            escape_html(&self.version)
        ));
        html.push_str("  <style>\n");
        html.push_str(EMBEDDED_CSS);
        html.push_str("  </style>\n");
        html.push_str("</head>\n<body>\n");

        // Top Navigation Header
        html.push_str("  <header class=\"top-nav\">\n");
        html.push_str("    <div class=\"brand\">\n");
        html.push_str("      <span class=\"logo\">⚡</span>\n");
        html.push_str(&format!(
            "      <h1>{}</h1>\n",
            escape_html(&self.package_name)
        ));
        html.push_str(&format!(
            "      <span class=\"version\">v{}</span>\n",
            escape_html(&self.version)
        ));
        html.push_str("    </div>\n");
        html.push_str("    <div class=\"search-box\">\n");
        html.push_str("      <input type=\"text\" id=\"doc-search\" placeholder=\"Filter symbols (e.g. fn, struct)...\" onkeyup=\"filterSymbols()\" />\n");
        html.push_str("    </div>\n");
        html.push_str("  </header>\n");

        html.push_str("  <div class=\"layout\">\n");

        // Sidebar Navigation
        html.push_str("    <aside class=\"sidebar\">\n");
        for module in &self.modules {
            html.push_str(&format!(
                "      <div class=\"sidebar-section\">\n        <h3>{}</h3>\n        <ul>\n",
                escape_html(&module.name)
            ));
            for item in &module.items {
                let item_id = format!("{}-{}", module.name, item.name);
                html.push_str(&format!(
                    "          <li><a href=\"#{}\" class=\"sidebar-item\"><span class=\"badge {}\">{}</span> {}</a></li>\n",
                    escape_html(&item_id),
                    item.kind.badge_class(),
                    escape_html(item.kind.as_str()),
                    escape_html(&item.name)
                ));
            }
            html.push_str("        </ul>\n      </div>\n");
        }
        html.push_str("    </aside>\n");

        // Main Documentation Content
        html.push_str("    <main class=\"content\">\n");

        if !self.description.is_empty() {
            html.push_str(&format!(
                "      <div class=\"pkg-desc\">{}</div>\n",
                escape_html(&self.description)
            ));
        }

        for module in &self.modules {
            html.push_str(&format!("      <section class=\"module-block\">\n        <h2>Module: <code>{}</code></h2>\n", escape_html(&module.name)));
            if !module.description.is_empty() {
                html.push_str(&format!(
                    "        <p class=\"module-desc\">{}</p>\n",
                    format_markdown_p(&module.description)
                ));
            }

            for item in &module.items {
                let item_id = format!("{}-{}", module.name, item.name);
                html.push_str(&format!("        <article class=\"item-card\" id=\"{}\" data-name=\"{}\" data-kind=\"{}\">\n",
                    escape_html(&item_id),
                    escape_html(&item.name.to_lowercase()),
                    escape_html(item.kind.as_str())
                ));

                html.push_str("          <div class=\"item-header\">\n");
                html.push_str(&format!(
                    "            <span class=\"badge {}\">{}</span>\n",
                    item.kind.badge_class(),
                    escape_html(item.kind.as_str())
                ));
                html.push_str(&format!(
                    "            <h3>{}</h3>\n",
                    escape_html(&item.name)
                ));
                html.push_str(&format!(
                    "            <span class=\"line-no\">line {}</span>\n",
                    item.line
                ));
                html.push_str("          </div>\n");

                html.push_str(&format!(
                    "          <pre class=\"signature\"><code>{}</code></pre>\n",
                    escape_html(&item.signature)
                ));

                if !item.description.is_empty() {
                    html.push_str(&format!(
                        "          <div class=\"item-desc\">{}</div>\n",
                        format_markdown_p(&item.description)
                    ));
                }

                if !item.params.is_empty() {
                    html.push_str("          <div class=\"params-section\">\n            <strong>Parameters:</strong>\n            <ul>\n");
                    for p in &item.params {
                        html.push_str(&format!(
                            "              <li><code>{}</code></li>\n",
                            escape_html(p)
                        ));
                    }
                    html.push_str("            </ul>\n          </div>\n");
                }

                if !item.fields.is_empty() {
                    html.push_str("          <div class=\"fields-section\">\n            <strong>Fields:</strong>\n            <ul>\n");
                    for f in &item.fields {
                        html.push_str(&format!(
                            "              <li><code>{}</code></li>\n",
                            escape_html(f)
                        ));
                    }
                    html.push_str("            </ul>\n          </div>\n");
                }

                if !item.variants.is_empty() {
                    html.push_str("          <div class=\"variants-section\">\n            <strong>Variants:</strong>\n            <ul>\n");
                    for v in &item.variants {
                        html.push_str(&format!(
                            "              <li><code>{}</code></li>\n",
                            escape_html(v)
                        ));
                    }
                    html.push_str("            </ul>\n          </div>\n");
                }

                html.push_str("        </article>\n");
            }

            html.push_str("      </section>\n");
        }

        html.push_str("    </main>\n");
        html.push_str("  </div>\n");

        // Search Filter JavaScript
        html.push_str("  <script>\n");
        html.push_str(EMBEDDED_JS);
        html.push_str("  </script>\n");

        html.push_str("</body>\n</html>\n");
        html
    }

    /// Renders aggregated package documentation as Markdown.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!("# {} `v{}`\n\n", self.package_name, self.version));
        if !self.description.is_empty() {
            md.push_str(&self.description);
            md.push_str("\n\n");
        }
        for module in &self.modules {
            md.push_str(&module.to_markdown());
            md.push_str("\n\n");
        }
        md
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_fn_decl(rest: &str, is_async: bool, doc_text: String, line: usize) -> Option<DocItem> {
    let name_part = rest.split('(').next()?.trim();
    if !is_identifier(name_part) {
        return None;
    }
    let params_str = rest.split('(').nth(1)?.split(')').next()?.trim();
    let params = if params_str.is_empty() {
        Vec::new()
    } else {
        params_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    let sig_prefix = if is_async { "async fn " } else { "fn " };
    let signature = format!("{}{}({})", sig_prefix, name_part, params.join(", "));

    Some(DocItem {
        kind: if is_async {
            DocItemKind::AsyncFunction
        } else {
            DocItemKind::Function
        },
        name: name_part.to_string(),
        signature,
        description: doc_text,
        params,
        fields: Vec::new(),
        variants: Vec::new(),
        line,
    })
}

fn parse_struct_decl(
    rest: &str,
    lines: &[&str],
    line_idx: &mut usize,
    doc_text: String,
    line: usize,
) -> Option<DocItem> {
    let name_part = rest.split('{').next()?.trim();
    if !is_identifier(name_part) {
        return None;
    }

    let mut fields = Vec::new();
    if rest.contains('{') && rest.contains('}') {
        let inner = rest.split('{').nth(1)?.split('}').next()?.trim();
        for field in inner.split(',') {
            let f = field.trim();
            if !f.is_empty() {
                fields.push(f.to_string());
            }
        }
    } else {
        while *line_idx + 1 < lines.len() {
            *line_idx += 1;
            let l = lines[*line_idx].trim();
            if l.starts_with('}') {
                break;
            }
            let f = l.trim_matches(',').trim();
            if !f.is_empty() && !f.starts_with("//") {
                fields.push(f.to_string());
            }
        }
    }

    let signature = format!("struct {} {{ {} }}", name_part, fields.join(", "));

    Some(DocItem {
        kind: DocItemKind::Struct,
        name: name_part.to_string(),
        signature,
        description: doc_text,
        params: Vec::new(),
        fields,
        variants: Vec::new(),
        line,
    })
}

fn parse_enum_decl(
    rest: &str,
    lines: &[&str],
    line_idx: &mut usize,
    doc_text: String,
    line: usize,
) -> Option<DocItem> {
    let name_part = rest.split('{').next()?.trim();
    if !is_identifier(name_part) {
        return None;
    }

    let mut variants = Vec::new();
    if rest.contains('{') && rest.contains('}') {
        let inner = rest.split('{').nth(1)?.split('}').next()?.trim();
        for variant in inner.split(',') {
            let v = variant.trim();
            if !v.is_empty() {
                variants.push(v.to_string());
            }
        }
    } else {
        while *line_idx + 1 < lines.len() {
            *line_idx += 1;
            let l = lines[*line_idx].trim();
            if l.starts_with('}') {
                break;
            }
            let v = l.trim_matches(',').trim();
            if !v.is_empty() && !v.starts_with("//") {
                variants.push(v.to_string());
            }
        }
    }

    let signature = format!("enum {} {{ {} }}", name_part, variants.join(", "));

    Some(DocItem {
        kind: DocItemKind::Enum,
        name: name_part.to_string(),
        signature,
        description: doc_text,
        params: Vec::new(),
        fields: Vec::new(),
        variants,
        line,
    })
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => chars.all(|c| c.is_alphanumeric() || c == '_'),
        _ => false,
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn format_markdown_p(s: &str) -> String {
    let escaped = escape_html(s);
    escaped.replace("\n", "<br/>")
}

const EMBEDDED_CSS: &str = r#"
:root {
  --bg: #0d1117;
  --panel: #161b22;
  --border: #30363d;
  --text: #c9d1d9;
  --heading: #f0f6fc;
  --accent: #58a6ff;
  --fn-bg: #238636;
  --async-bg: #8957e5;
  --struct-bg: #1f6feb;
  --enum-bg: #d29922;
  --const-bg: #da3633;
}
* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
  background: var(--bg);
  color: var(--text);
  line-height: 1.6;
}
.top-nav {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1rem 2rem;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
  position: sticky;
  top: 0;
  z-index: 100;
}
.top-nav .brand { display: flex; align-items: center; gap: 0.75rem; }
.top-nav .logo { font-size: 1.5rem; }
.top-nav h1 { font-size: 1.25rem; color: var(--heading); font-weight: 600; }
.top-nav .version {
  background: var(--border);
  font-size: 0.75rem;
  padding: 0.15rem 0.5rem;
  border-radius: 12px;
}
.search-box input {
  background: var(--bg);
  border: 1px solid var(--border);
  color: var(--text);
  padding: 0.5rem 1rem;
  border-radius: 6px;
  width: 320px;
  outline: none;
}
.search-box input:focus { border-color: var(--accent); }
.layout { display: flex; min-height: calc(100vh - 65px); }
.sidebar {
  width: 280px;
  background: var(--panel);
  border-right: 1px solid var(--border);
  padding: 1.5rem 1rem;
  overflow-y: auto;
  position: sticky;
  top: 65px;
  height: calc(100vh - 65px);
}
.sidebar-section h3 {
  font-size: 0.85rem;
  text-transform: uppercase;
  color: #8b949e;
  margin-bottom: 0.5rem;
  letter-spacing: 0.5px;
}
.sidebar ul { list-style: none; margin-bottom: 1.5rem; }
.sidebar-item {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.35rem 0.5rem;
  color: var(--text);
  text-decoration: none;
  font-size: 0.9rem;
  border-radius: 4px;
}
.sidebar-item:hover { background: #21262d; color: var(--heading); }
.content { flex: 1; padding: 2rem 3rem; max-width: 960px; }
.pkg-desc { font-size: 1.1rem; color: #8b949e; margin-bottom: 2rem; }
.module-block { margin-bottom: 3rem; }
.module-block h2 { font-size: 1.5rem; color: var(--heading); border-bottom: 1px solid var(--border); padding-bottom: 0.5rem; margin-bottom: 1rem; }
.module-desc { color: #8b949e; margin-bottom: 1.5rem; }
.item-card {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 1.25rem;
  margin-bottom: 1.5rem;
}
.item-header { display: flex; align-items: center; gap: 0.75rem; margin-bottom: 0.75rem; }
.item-header h3 { font-size: 1.15rem; color: var(--heading); font-family: ui-monospace, SFMono-Regular, monospace; }
.line-no { margin-left: auto; font-size: 0.8rem; color: #8b949e; }
.signature {
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 0.75rem 1rem;
  margin-bottom: 0.75rem;
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 0.9rem;
  overflow-x: auto;
}
.item-desc { margin-bottom: 0.75rem; }
.params-section, .fields-section, .variants-section {
  font-size: 0.9rem;
  margin-top: 0.75rem;
  background: var(--bg);
  padding: 0.5rem 1rem;
  border-radius: 4px;
}
.params-section ul, .fields-section ul, .variants-section ul { margin-left: 1.25rem; margin-top: 0.25rem; }
.badge {
  font-size: 0.7rem;
  padding: 0.15rem 0.45rem;
  border-radius: 10px;
  font-weight: 600;
  text-transform: uppercase;
  color: #fff;
}
.badge-fn { background: var(--fn-bg); }
.badge-async { background: var(--async-bg); }
.badge-struct { background: var(--struct-bg); }
.badge-enum { background: var(--enum-bg); }
.badge-const { background: var(--const-bg); }
"#;

const EMBEDDED_JS: &str = r#"
function filterSymbols() {
  const query = document.getElementById('doc-search').value.toLowerCase().trim();
  const items = document.querySelectorAll('.item-card');
  items.forEach(item => {
    const name = item.getAttribute('data-name') || '';
    const kind = item.getAttribute('data-kind') || '';
    if (!query || name.includes(query) || kind.includes(query)) {
      item.style.display = '';
    } else {
      item.style.display = 'none';
    }
  });
}
"#;

/// Generates documentation files for a path (file or project directory).
pub fn generate_docs(
    input_path: &Path,
    output_dir: &Path,
    format: Option<&str>,
) -> Result<PackageDoc, String> {
    let (package_name, version, description) = if input_path.is_dir() {
        let manifest_path = input_path.join("shae.toml");
        if manifest_path.exists() {
            if let Ok(manifest) = crate::pkg::PackageManifest::load_from_file(&manifest_path) {
                (
                    manifest.package.name,
                    manifest.package.version,
                    manifest.package.description.unwrap_or_default(),
                )
            } else {
                ("project".into(), "0.1.0".into(), "".into())
            }
        } else {
            ("project".into(), "0.1.0".into(), "".into())
        }
    } else {
        (
            input_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "module".into()),
            "0.1.0".into(),
            "".into(),
        )
    };

    let mut modules = Vec::new();

    if input_path.is_file() {
        let content = fs::read_to_string(input_path)
            .map_err(|e| format!("Failed to read file {}: {}", input_path.display(), e))?;
        let mod_name = input_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "main".into());
        modules.push(ModuleDoc::extract(
            &content,
            &mod_name,
            &input_path.to_string_lossy(),
        ));
    } else {
        collect_shae_files(input_path, &mut modules)?;
    }

    let pkg_doc = PackageDoc {
        package_name,
        version,
        description,
        modules,
    };

    fs::create_dir_all(output_dir).map_err(|e| {
        format!(
            "Failed to create output dir {}: {}",
            output_dir.display(),
            e
        )
    })?;

    let fmt = format.unwrap_or("all");
    match fmt {
        "html" => {
            let html_content = pkg_doc.to_html();
            fs::write(output_dir.join("index.html"), html_content)
                .map_err(|e| format!("Failed to write index.html: {}", e))?;
        }
        "md" | "markdown" => {
            let md_content = pkg_doc.to_markdown();
            fs::write(output_dir.join("README.md"), md_content)
                .map_err(|e| format!("Failed to write README.md: {}", e))?;
        }
        "json" => {
            let json_content = serde_json::to_string_pretty(&pkg_doc)
                .map_err(|e| format!("JSON serialization error: {}", e))?;
            fs::write(output_dir.join("doc.json"), json_content)
                .map_err(|e| format!("Failed to write doc.json: {}", e))?;
        }
        _ => {
            // Default "all": write both HTML, Markdown, and JSON
            fs::write(output_dir.join("index.html"), pkg_doc.to_html())
                .map_err(|e| format!("Failed to write index.html: {}", e))?;
            fs::write(output_dir.join("README.md"), pkg_doc.to_markdown())
                .map_err(|e| format!("Failed to write README.md: {}", e))?;
            if let Ok(json_content) = serde_json::to_string_pretty(&pkg_doc) {
                let _ = fs::write(output_dir.join("doc.json"), json_content);
            }
        }
    }

    Ok(pkg_doc)
}

fn collect_shae_files(dir: &Path, modules: &mut Vec<ModuleDoc>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read dir: {}", e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            if file_name.starts_with('.') || file_name == "target" || file_name == "doc" {
                continue;
            }
            collect_shae_files(&path, modules)?;
        } else if path.extension().map_or(false, |ext| ext == "shae") {
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            if file_name.ends_with("_test.shae") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                let mod_name = path.file_stem().unwrap().to_string_lossy().to_string();
                modules.push(ModuleDoc::extract(
                    &content,
                    &mod_name,
                    &path.to_string_lossy(),
                ));
            }
        }
    }
    Ok(())
}

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

/// Starts a live static HTTP dev server serving generated documentation.
pub fn start_doc_server(
    dir: &Path,
    host: &str,
    port: u16,
    shutdown: Option<Arc<AtomicBool>>,
) -> Result<(SocketAddr, JoinHandle<()>), std::io::Error> {
    let root = fs::canonicalize(dir)?;
    if !root.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Path '{}' is not a directory", dir.display()),
        ));
    }

    let bind_addr = format!("{}:{}", host, port);
    let listener = TcpListener::bind(&bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;

    let shutdown_flag = shutdown.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let root_arc = Arc::new(root);

    let handle = std::thread::spawn(move || {
        while !shutdown_flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let root = root_arc.clone();
                    std::thread::spawn(move || {
                        let _ = handle_doc_http_stream(&mut stream, &root);
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(15));
                }
                Err(_) => break,
            }
        }
    });

    Ok((local_addr, handle))
}

fn handle_doc_http_stream(stream: &mut TcpStream, root: &Path) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return Ok(()),
    };

    let req_str = String::from_utf8_lossy(&buf[..n]);
    let first_line = req_str.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let raw_url = parts.next().unwrap_or("/");

    if method != "GET" && method != "HEAD" {
        let _ = stream.write_all(
            b"HTTP/1.1 405 Method Not Allowed
Content-Length: 0

",
        );
        return Ok(());
    }

    let path_part = raw_url.split('?').next().unwrap_or("/");
    let rel_path = path_part.trim_start_matches('/');
    let rel_path = if rel_path.is_empty() {
        "index.html"
    } else {
        rel_path
    };

    // Security check: reject explicit directory traversal sequences
    if rel_path.contains("..") || rel_path.contains('\0') {
        let body = "<!DOCTYPE html><html><head><title>403 Forbidden</title></head><body><h1>403 Forbidden</h1></body></html>";
        let resp = format!(
            "HTTP/1.1 403 Forbidden\nContent-Type: text/html; charset=utf-8\nContent-Length: {}\nConnection: close\n\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        return Ok(());
    }

    let mut target = root.join(rel_path);
    if target.is_dir() {
        let idx = target.join("index.html");
        if idx.is_file() {
            target = idx;
        }
    }

    let canonical_target = match fs::canonicalize(&target) {
        Ok(c) => c,
        Err(_) => {
            let body = format!(
                "<!DOCTYPE html><html><head><title>404 Not Found</title></head><body><h1>404 Not Found</h1><p>Doc file not found: {}</p></body></html>",
                path_part
            );
            let resp = format!(
                "HTTP/1.1 404 Not Found\nContent-Type: text/html; charset=utf-8\nContent-Length: {}\nConnection: close\n\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            return Ok(());
        }
    };

    if !canonical_target.starts_with(root) {
        let body = "<!DOCTYPE html><html><head><title>403 Forbidden</title></head><body><h1>403 Forbidden</h1></body></html>";
        let resp = format!(
            "HTTP/1.1 403 Forbidden\nContent-Type: text/html; charset=utf-8\nContent-Length: {}\nConnection: close\n\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        return Ok(());
    }

    match fs::read(&canonical_target) {
        Ok(content) => {
            let mime = match canonical_target
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
            {
                "html" => "text/html; charset=utf-8",
                "js" => "application/javascript; charset=utf-8",
                "css" => "text/css; charset=utf-8",
                "json" => "application/json; charset=utf-8",
                "md" => "text/markdown; charset=utf-8",
                "svg" => "image/svg+xml",
                "png" => "image/png",
                _ => "text/plain; charset=utf-8",
            };
            let head = format!(
                "HTTP/1.1 200 OK
Content-Type: {}
Content-Length: {}
Connection: close

",
                mime,
                content.len()
            );
            let _ = stream.write_all(head.as_bytes());
            if method == "GET" {
                let _ = stream.write_all(&content);
            }
        }
        Err(_) => {
            let _ = stream.write_all(
                b"HTTP/1.1 500 Internal Server Error
Content-Length: 0

",
            );
        }
    }

    Ok(())
}
