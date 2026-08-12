use std::collections::HashMap;
use std::path::Path;
use globset::{Glob, GlobSet, GlobSetBuilder};

#[derive(Clone, Debug)]
pub struct TypeDef {
    name: String,
    globs: Vec<String>,
}

impl TypeDef {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn globs(&self) -> &[String] {
        &self.globs
    }
}

#[derive(Clone, Debug, Default)]
pub struct Types {
    definitions: Vec<TypeDef>,
    selected: GlobSet,
}

impl Types {
    pub fn definitions(&self) -> &[TypeDef] {
        &self.definitions
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    pub fn matched(&self, path: &Path, _is_dir: bool) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        let path_str = path.to_string_lossy();
        self.selected.is_match(&path_str)
    }
}

#[derive(Clone, Debug)]
pub struct TypesBuilder {
    definitions: HashMap<String, Vec<String>>,
    selected_names: Vec<String>,
    negated_names: Vec<String>,
}

impl Default for TypesBuilder {
    fn default() -> Self {
        TypesBuilder {
            definitions: HashMap::new(),
            selected_names: Vec::new(),
            negated_names: Vec::new(),
        }
    }
}

impl TypesBuilder {
    pub fn new() -> Self { Self::default() }

    pub fn add_defaults(&mut self) -> &mut Self {
        let defaults: &[(&str, &[&str])] = &[
            ("rust", &["*.rs"]),
            ("python", &["*.py", "*.pyi"]),
            ("c", &["*.c", "*.h"]),
            ("cpp", &["*.cpp", "*.cc", "*.cxx", "*.hpp", "*.hh", "*.hxx", "*.h"]),
            ("js", &["*.js", "*.mjs", "*.cjs", "*.jsx"]),
            ("ts", &["*.ts", "*.tsx", "*.mts", "*.cts"]),
            ("html", &["*.html", "*.htm"]),
            ("css", &["*.css", "*.scss", "*.less"]),
            ("json", &["*.json", "*.jsonl"]),
            ("toml", &["*.toml"]),
            ("yaml", &["*.yml", "*.yaml"]),
            ("md", &["*.md", "*.markdown"]),
            ("go", &["*.go"]),
            ("java", &["*.java"]),
            ("sh", &["*.sh", "*.bash", "*.zsh", "*.fish"]),
            ("txt", &["*.txt"]),
        ];

        for (name, globs) in defaults {
            let glob_vec: Vec<String> = globs.iter().map(|s| s.to_string()).collect();
            self.definitions.insert(name.to_string(), glob_vec);
        }
        self
    }

    pub fn add(&mut self, name: &str, glob: &str) -> Result<&mut Self, String> {
        self.definitions
            .entry(name.to_string())
            .or_default()
            .push(glob.to_string());
        Ok(self)
    }

    pub fn clear(&mut self, name: &str) -> &mut Self {
        self.definitions.remove(name);
        self
    }

    pub fn select(&mut self, name: &str) -> &mut Self {
        self.selected_names.push(name.to_string());
        self
    }

    pub fn negate(&mut self, name: &str) -> &mut Self {
        self.negated_names.push(name.to_string());
        self
    }

    pub fn build(&self) -> Result<Types, String> {
        let mut builder = GlobSetBuilder::new();
        let mut defs = Vec::new();

        for (name, globs) in &self.definitions {
            defs.push(TypeDef {
                name: name.clone(),
                globs: globs.clone(),
            });
            if self.selected_names.contains(name) {
                for g_str in globs {
                    if let Ok(g) = Glob::new(g_str) {
                        builder.add(g);
                    }
                }
            }
        }

        let selected = builder.build().unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap());
        Ok(Types {
            definitions: defs,
            selected,
        })
    }
}
