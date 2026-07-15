# Project Setup — Black

This document provides instructions for fetching, installing, and building the **target project** — the codebase that the Ultimate Prompt process will be applied to.

All paths in this document are relative to `BASE_DIR` — the root of this repository.

## Configuration

| Variable | Description | Value |
|----------|-------------|-------|
| `BASE_DIR` | Root of this repository (absolute path) | *(set before running)* |
| `TARGET_DIR` | Path to the cloned target project | `${BASE_DIR}/working_dir/black` |

Before running any commands, export `BASE_DIR`:

```bash
export BASE_DIR=/absolute/path/to/this/repo
```

## Target Project

| Field | Value |
|-------|-------|
| **Name** | Black |
| **Repository** | `https://github.com/psf/black.git` |
| **Language** | Python |
| **Build System** | Hatchling (Build backend), pip |
| **Description** | The uncompromising code formatter. Fast, deterministic, and takes away style arguments. |

---

## Prerequisites

The following must be available on the system before proceeding:

- **Git** — for cloning the repository. Verify with:
  ```bash
  git --version
  ```
  Expected: any version output (e.g., `git version 2.x.x`). If this fails, install git first.

- **Python 3.10+** — Black requires modern Python. Verify with:
  ```bash
  python3 --version
  ```
  Expected: `Python 3.10.x` or higher.

---

## Step 1: Clone the Repository

Run from `BASE_DIR`:

```bash
mkdir -p ${BASE_DIR}/working_dir
git clone https://github.com/psf/black.git ${BASE_DIR}/working_dir/black
```

### Verification

```bash
ls ${BASE_DIR}/working_dir/black/pyproject.toml
```

**Success criterion**: The file exists (exit code 0). If this fails, the clone did not succeed — check network connectivity and retry.

---

## Step 2: Build the Project (Install Dependencies)

We use a virtual environment to isolate dependencies.

```bash
# Troubleshooting venv creation:
# If python3 -m venv fails due to missing ensurepip, try creating it without pip and installing pip manually:
# python3 -m venv --without-pip ${BASE_DIR}/working_dir/black/.venv
# ${BASE_DIR}/working_dir/black/.venv/bin/python3 /tmp/get-pip.py
# (Download get-pip.py from https://bootstrap.pypa.io/get-pip.py first)

python3 -m venv ${BASE_DIR}/working_dir/black/.venv

# Note on Pip Index (e.g., Airlock):
# If packages are missing from your default index, you may need to specify --index-url https://pypi.org/simple

# Note: --group requires the path to pyproject.toml if not run from the project root.
${BASE_DIR}/working_dir/black/.venv/bin/pip install --group ${BASE_DIR}/working_dir/black/pyproject.toml:dev
${BASE_DIR}/working_dir/black/.venv/bin/pip install -e "${BASE_DIR}/working_dir/black[d]"
```

### Verification

```bash
${BASE_DIR}/working_dir/black/.venv/bin/black --version
```

**Success criterion**: Output contains `black` and a version number. If the installation failed, check Python version and permissions.

---

## Step 3: Run Tests

```bash
${BASE_DIR}/working_dir/black/.venv/bin/pytest ${BASE_DIR}/working_dir/black/tests
```

**Success criterion**: All tests pass (exit code 0). If tests fail on a clean clone, this indicates an environment issue (missing system dependencies, incompatible Python version, etc.) — not a problem with the project itself.

---

## Completion Checklist

Before proceeding to the Coordinator workflow, confirm all of the following:

- [ ] `${BASE_DIR}/working_dir/black/pyproject.toml` exists
- [ ] Dependencies installed successfully
- [ ] `black --version` prints version info
- [ ] `pytest` passed (exit code 0)

Once all checks pass, the project is ready. Update the following variables in [COORDINATOR_INSTRUCTIONS.md](../../COORDINATOR_INSTRUCTIONS.md):

| Variable | Value |
|----------|-------|
| `BENCHMARK_DIR` | `benchmarks/black` |
| `TARGET_DIR` | `working_dir/black` |
| `HANDOFF_DIR` | `/cns/oz-d/home/vasic/ultimate-prompt/black/handoff` |

---

## Directory Layout After Setup

```
${BASE_DIR}/
├── COORDINATOR_INSTRUCTIONS.md       # Coordinator worker
├── GENERATOR_INSTRUCTIONS.md         # Generator worker
├── benchmarks/
│   └── black/
│       └── setup_instructions.md    # This file
├── .gitignore                        # Ignores working_dir/
└── working_dir/
    └── black/                        # Cloned target project
        ├── pyproject.toml
        ├── src/
        ├── tests/
        └── ...
```
