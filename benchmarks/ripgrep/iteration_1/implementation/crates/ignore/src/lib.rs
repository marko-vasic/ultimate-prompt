pub mod error;
pub mod gitignore;
pub mod overrides;
pub mod types;

use std::ffi::OsStr;
use std::fs::{self, FileType, Metadata};
use std::io;
use std::path::{Path, PathBuf};

pub use error::Error;
pub use gitignore::{Gitignore, GitignoreBuilder};
pub use overrides::{Override, OverrideBuilder};
pub use types::{Types, TypesBuilder};

pub mod dir {
    pub use super::IgnoreBuilder;
}

pub mod walk {
    pub use super::{DirEntry, Walk, WalkParallel, WalkState};
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalkState {
    Continue,
    Skip,
    Quit,
}

#[derive(Clone, Debug)]
pub struct DirEntry {
    path: PathBuf,
    is_dir: bool,
    is_stdin: bool,
    depth: usize,
}

impl DirEntry {
    pub fn new_stdin() -> DirEntry {
        DirEntry {
            path: PathBuf::from("<stdin>"),
            is_dir: false,
            is_stdin: true,
            depth: 0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn file_name(&self) -> &OsStr {
        self.path.file_name().unwrap_or_else(|| self.path.as_os_str())
    }
    pub fn file_type(&self) -> Option<FileType> {
        fs::metadata(&self.path).ok().map(|m| m.file_type())
    }
    pub fn metadata(&self) -> Result<Metadata, Error> {
        fs::metadata(&self.path).map_err(Error::io_plain)
    }
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }
    pub fn depth(&self) -> usize {
        self.depth
    }
    pub fn is_stdin(&self) -> bool {
        self.is_stdin
    }
}

#[derive(Clone, Debug)]
pub struct IgnoreBuilder {
    paths: Vec<PathBuf>,
    git_ignore: bool,
    git_global: bool,
    ignore_file: bool,
    rg_ignore_file: bool,
    hidden: bool,
    follow_links: bool,
    same_file_system: bool,
    parents: bool,
    max_depth: Option<usize>,
    max_filesize: Option<u64>,
    overrides: Override,
    types: Types,
    threads: usize,
}

impl Default for IgnoreBuilder {
    fn default() -> Self {
        IgnoreBuilder {
            paths: Vec::new(),
            git_ignore: true,
            git_global: true,
            ignore_file: true,
            rg_ignore_file: true,
            hidden: true,
            follow_links: false,
            same_file_system: false,
            parents: true,
            max_depth: None,
            max_filesize: None,
            overrides: Override::empty(),
            types: Types::default(),
            threads: 0,
        }
    }
}

impl IgnoreBuilder {
    pub fn new() -> Self { Self::default() }

    pub fn add_path<P: AsRef<Path>>(&mut self, path: P) -> &mut Self {
        self.paths.push(path.as_ref().to_path_buf());
        self
    }
    pub fn git_ignore(&mut self, yes: bool) -> &mut Self {
        self.git_ignore = yes;
        self
    }
    pub fn git_global(&mut self, yes: bool) -> &mut Self {
        self.git_global = yes;
        self
    }
    pub fn ignore_file(&mut self, yes: bool) -> &mut Self {
        self.ignore_file = yes;
        self
    }
    pub fn rg_ignore_file(&mut self, yes: bool) -> &mut Self {
        self.rg_ignore_file = yes;
        self
    }
    pub fn hidden(&mut self, yes: bool) -> &mut Self {
        self.hidden = yes;
        self
    }
    pub fn follow_links(&mut self, yes: bool) -> &mut Self {
        self.follow_links = yes;
        self
    }
    pub fn same_file_system(&mut self, yes: bool) -> &mut Self {
        self.same_file_system = yes;
        self
    }
    pub fn parents(&mut self, yes: bool) -> &mut Self {
        self.parents = yes;
        self
    }
    pub fn max_depth(&mut self, depth: Option<usize>) -> &mut Self {
        self.max_depth = depth;
        self
    }
    pub fn max_filesize(&mut self, size: Option<u64>) -> &mut Self {
        self.max_filesize = size;
        self
    }
    pub fn overrides(&mut self, ov: Override) -> &mut Self {
        self.overrides = ov;
        self
    }
    pub fn types(&mut self, types: Types) -> &mut Self {
        self.types = types;
        self
    }
    pub fn threads(&mut self, n: usize) -> &mut Self {
        self.threads = n;
        self
    }

    pub fn build(&self) -> Walk {
        let mut entries = Vec::new();
        let targets = if self.paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            self.paths.clone()
        };

        for root in targets {
            if root == Path::new("-") {
                entries.push(Ok(DirEntry::new_stdin()));
                continue;
            }

            if !root.exists() {
                continue;
            }

            let mut walkdir_builder = walkdir::WalkDir::new(&root);
            if self.follow_links {
                walkdir_builder = walkdir_builder.follow_links(true);
            }
            if let Some(depth) = self.max_depth {
                walkdir_builder = walkdir_builder.max_depth(depth);
            }

            for result in walkdir_builder {
                match result {
                    Ok(entry) => {
                        let path = entry.path().to_path_buf();
                        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

                        // Skip hidden files if hidden setting is enabled
                        if self.hidden && file_name.starts_with('.') && file_name != "." && file_name != ".." {
                            continue;
                        }

                        // Check max filesize
                        if let Some(max_size) = self.max_filesize {
                            if let Ok(meta) = entry.metadata() {
                                if meta.is_file() && meta.len() > max_size {
                                    continue;
                                }
                            }
                        }

                        entries.push(Ok(DirEntry {
                            path,
                            is_dir: entry.file_type().is_dir(),
                            is_stdin: false,
                            depth: entry.depth(),
                        }));
                    }
                    Err(err) => {
                        if let Some(io_err) = err.io_error() {
                            entries.push(Err(Error::io_plain(io::Error::new(io_err.kind(), io_err.to_string()))));
                        }
                    }
                }
            }
        }

        Walk {
            entries: entries.into_iter(),
        }
    }

    pub fn build_parallel(&self) -> WalkParallel {
        WalkParallel {
            builder: self.clone(),
        }
    }
}

pub struct Walk {
    entries: std::vec::IntoIter<Result<DirEntry, Error>>,
}

impl Iterator for Walk {
    type Item = Result<DirEntry, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.entries.next()
    }
}

pub struct WalkParallel {
    builder: IgnoreBuilder,
}

impl WalkParallel {
    pub fn run<F>(&self, mut mkf: F)
    where
        F: FnMut() -> Box<dyn FnMut(Result<DirEntry, Error>) -> WalkState + Send>,
    {
        let walk = self.builder.build();
        let mut consumer = mkf();
        for entry in walk {
            if consumer(entry) == WalkState::Quit {
                break;
            }
        }
    }
}
