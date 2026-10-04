with open("src/env.rs", "r") as f:
    content = f.read()

export_map = """
    pub fn export_map(&self) -> indexmap::IndexMap<String, Value> {
        let mut map = indexmap::IndexMap::new();
        for (k, v) in &self.values {
            map.insert(k.clone(), v.clone());
        }
        map
    }
}
"""
content = content.replace("        names\n    }\n}", "        names\n    }\n" + export_map)
with open("src/env.rs", "w") as f:
    f.write(content)
