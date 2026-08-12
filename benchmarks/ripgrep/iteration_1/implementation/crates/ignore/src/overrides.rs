use std::path::Path;
use globset::{Glob, GlobSet, GlobSetBuilder};

#[derive(Clone, Debug, Default)]
pub struct Override {
    glob_set: GlobSet,
}

impl Override {
    pub fn empty() -> Override {
        Override {
            glob_set: GlobSetBuilder::new().build().unwrap(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.glob_set.is_empty()
    }

    pub fn matched(&self, path: &Path, _is_dir: bool) -> bool {
        if self.glob_set.is_empty() {
            return false;
        }
        let path_str = path.to_string_lossy();
        self.glob_set.is_match(&path_str)
    }
}

#[derive(Clone, Debug)]
pub struct OverrideBuilder {
    builder: GlobSetBuilder,
}

impl OverrideBuilder {
    pub fn new(_root: &Path) -> OverrideBuilder {
        OverrideBuilder {
            builder: GlobSetBuilder::new(),
        }
    }

    pub fn add(&mut self, glob_str: &str) -> Result<&mut Self, String> {
        let g = Glob::new(glob_str).map_err(|e| e.to_string())?;
        self.builder.add(g);
        Ok(self)
    }

    pub fn case_insensitive(&mut self, _yes: bool) -> &mut Self {
        self
    }

    pub fn build(&self) -> Result<Override, String> {
        let glob_set = self.builder.build().map_err(|e| e.to_string())?;
        Ok(Override { glob_set })
    }
}
