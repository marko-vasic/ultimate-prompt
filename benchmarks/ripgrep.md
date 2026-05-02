# Project Setup — ripgrep

This document provides instructions for fetching, installing, and building the **target project** — the codebase that the Ultimate Prompt process will be applied to.

All paths in this document are relative to `BASE_DIR` — the root of this repository.

## Configuration

| Variable | Description | Value |
|----------|-------------|-------|
| `BASE_DIR` | Root of this repository (absolute path) | *(set before running)* |
| `TARGET_DIR` | Path to the cloned target project | `${BASE_DIR}/working_dir/ripgrep` |

## Target Project

| Field | Value |
|-------|-------|
| **Name** | ripgrep (rg) |
| **Repository** | `https://github.com/BurntSushi/ripgrep.git` |
| **Language** | Rust |
| **Build System** | Cargo |
| **Description** | A line-oriented search tool that recursively searches directories for a regex pattern. Fast, Unicode-aware, and respects `.gitignore` rules. |

---

## Prerequisites

The following must be available on the system before proceeding:

- **Git** — for cloning the repository. Verify with:
  ```bash
  git --version
  ```
  Expected: any version output (e.g., `git version 2.x.x`). If this fails, install git first.

- **Rust toolchain** — `rustc` and `cargo`. Verify with:
  ```bash
  rustc --version && cargo --version
  ```
  Expected: version output for both. If not installed, install non-interactively via [rustup](https://rustup.rs/):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
  ```
  The `-y` flag accepts defaults without prompting. After install, re-verify with the commands above.

---

## Step 1: Clone the Repository

Run from `BASE_DIR`:

```bash
mkdir -p ${BASE_DIR}/working_dir
git clone https://github.com/BurntSushi/ripgrep.git ${BASE_DIR}/working_dir/ripgrep
```

### Verification

```bash
ls ${BASE_DIR}/working_dir/ripgrep/Cargo.toml
```

**Success criterion**: The file exists (exit code 0). If this fails, the clone did not succeed — check network connectivity and retry.

---

## Step 2: Build the Project

```bash
cargo build --manifest-path ${BASE_DIR}/working_dir/ripgrep/Cargo.toml
```

### Verification

```bash
${BASE_DIR}/working_dir/ripgrep/target/debug/rg --version
```

**Success criterion**: Output contains `ripgrep` and a version number (e.g., `ripgrep 14.1.1`). If the build failed, check that the Rust toolchain is installed and up to date.

---

## Step 3: Run Tests

```bash
cargo test --manifest-path ${BASE_DIR}/working_dir/ripgrep/Cargo.toml
```

**Success criterion**: All tests pass (exit code 0). If tests fail on a clean clone, this indicates an environment issue (missing system dependencies, incompatible Rust version, etc.) — not a problem with the project itself.

---

## Completion Checklist

Before proceeding to the Coordinator workflow, confirm all of the following:

- [ ] `${BASE_DIR}/working_dir/ripgrep/Cargo.toml` exists
- [ ] `cargo build` succeeded (exit code 0)
- [ ] `rg --version` prints version info
- [ ] `cargo test` passed (exit code 0)

Once all checks pass, the project is ready. Update `TARGET_DIR` in [COORDINATOR_INSTRUCTIONS.md](../COORDINATOR_INSTRUCTIONS.md) to:

```
working_dir/ripgrep
```

---

## Directory Layout After Setup

```
${BASE_DIR}/
├── ULTIMATE_PROMPT_INSTRUCTIONS.md   # Original reference
├── COORDINATOR_INSTRUCTIONS.md       # Coordinator worker
├── GENERATOR_INSTRUCTIONS.md         # Generator worker
├── benchmarks/
│   └── ripgrep.md                    # This file
├── .gitignore                        # Ignores working_dir/
└── working_dir/
    └── ripgrep/                      # Cloned target project
        ├── Cargo.toml
        ├── Cargo.lock
        ├── src/
        ├── crates/
        └── ...
```
