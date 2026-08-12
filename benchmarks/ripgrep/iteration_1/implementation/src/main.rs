use std::io::{self, Write};
use std::process;

mod app;
mod args;
mod search;

use args::{Args, Mode};

#[cfg(all(target_env = "musl", target_pointer_width = "64"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

fn main() {
    if let Err(err) = run() {
        if let Some(io_err) = err.downcast_ref::<io::Error>() {
            if io_err.kind() == io::ErrorKind::BrokenPipe {
                process::exit(0);
            }
        }
        eprintln!("{:#}", err);
        process::exit(2);
    }
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse()?;

    match args.mode {
        Mode::Search => search::run_search(&args),
        Mode::Files => search::run_files(&args),
        Mode::Types => run_types(),
        Mode::Version => run_version(),
        Mode::Help => run_help(),
        Mode::Generate(ref kind) => run_generate(kind),
    }
}

fn run_version() -> anyhow::Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let git_hash = option_env!("RIPGREP_BUILD_GIT_HASH").unwrap_or("unknown");
    writeln!(io::stdout(), "ripgrep {} (rev {})", version, git_hash)?;
    Ok(())
}

fn run_help() -> anyhow::Result<()> {
    let help = app::generate_help();
    write!(io::stdout(), "{}", help)?;
    Ok(())
}

fn run_types() -> anyhow::Result<()> {
    let mut builder = ignore::types::TypesBuilder::new();
    builder.add_defaults();
    let types = builder.build().map_err(|e| anyhow::anyhow!("{}", e))?;
    let defs = types.definitions();
    let mut stdout = io::stdout().lock();
    for def in defs {
        writeln!(stdout, "{}: {}", def.name(), def.globs().join(", "))?;
    }
    Ok(())
}

fn run_generate(kind: &str) -> anyhow::Result<()> {
    match kind {
        "man" => {
            writeln!(io::stdout(), ".TH RG 1")?;
            writeln!(io::stdout(), ".SH NAME")?;
            writeln!(io::stdout(), "rg \\- recursively search the current directory for lines matching a pattern")?;
            writeln!(io::stdout(), ".SH SYNOPSIS")?;
            writeln!(io::stdout(), ".B rg")?;
            writeln!(io::stdout(), "[OPTIONS] PATTERN [PATH ...]")?;
            writeln!(io::stdout(), ".SH DESCRIPTION")?;
            writeln!(io::stdout(), "ripgrep (rg) recursively searches the current directory for a regex pattern.")?;
        }
        "complete-bash" => {
            writeln!(io::stdout(), "# Bash completion for rg")?;
            writeln!(io::stdout(), "_rg() {{")?;
            writeln!(io::stdout(), "    local cur=${{COMP_WORDS[COMP_CWORD]}}")?;
            writeln!(io::stdout(), "    COMPREPLY=( $(compgen -f -- \"$cur\") )")?;
            writeln!(io::stdout(), "}}")?;
            writeln!(io::stdout(), "complete -F _rg rg")?;
        }
        "complete-zsh" => {
            writeln!(io::stdout(), "#compdef rg")?;
            writeln!(io::stdout(), "_rg() {{ _arguments '*:filename:_files' }}")?;
            writeln!(io::stdout(), "_rg")?;
        }
        "complete-fish" => {
            writeln!(io::stdout(), "# Fish completion for rg")?;
            writeln!(io::stdout(), "complete -c rg -s h -l help -d 'Show help'")?;
        }
        "complete-powershell" => {
            writeln!(io::stdout(), "# PowerShell completion for rg")?;
        }
        _ => anyhow::bail!("unknown generate mode: {}", kind),
    }
    Ok(())
}
