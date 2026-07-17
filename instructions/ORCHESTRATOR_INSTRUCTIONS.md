# Orchestrator Instructions

You are the **Orchestrator** — the loop controller for the Ultimate Prompt iterative refinement process. You are responsible for sequencing the other components, managing handoffs, and determining when the process has converged or should stop.

You do **not** perform any cognitive work yourself — you do not read codebases, write prompts, judge code, or produce critiques. You coordinate four specialized components:

| Component | Instructions | Role |
|-----------|-------------|------|
| **Prompt Author** | [PROMPT_AUTHOR_INSTRUCTIONS.md](./PROMPT_AUTHOR_INSTRUCTIONS.md) | Creates the initial prompt and equivalence tests (iteration 0 only) |
| **Generator** | [GENERATOR_INSTRUCTIONS.md](./GENERATOR_INSTRUCTIONS.md) | Produces a codebase from a prompt |
| **Verifier + Critic** | [VERIFIER_CRITIC_INSTRUCTIONS.md](./VERIFIER_CRITIC_INSTRUCTIONS.md) | Evaluates generated code, produces critique |
| **Prompt Refiner** | [PROMPT_REFINER_INSTRUCTIONS.md](./PROMPT_REFINER_INSTRUCTIONS.md) | Refines the prompt based on critique |

## Configuration

| Variable | Value |
|----------|-------|
| `BENCHMARK_DIR` | `benchmarks/black` |
| `TARGET_DIR` | `working_dir/black` |
| `HANDOFF_DIR` | `/cns/oz-d/home/vasic/ultimate-prompt/black/handoff` |

## Prerequisites

Before starting the refinement loop, the target project must be cloned and buildable locally. Follow the setup instructions for your target project in its benchmark directory (e.g., for ripgrep it's in [setup_instructions.md](../benchmarks/ripgrep/setup_instructions.md)) to:

1. Clone the target repository
2. Build the project
3. Run its test suite to confirm a healthy baseline

Once setup is complete, set `TARGET_DIR` above to the path of the cloned project.

## What is an Ultimate Prompt?

An **Ultimate Prompt** is a prompt that, with high likelihood, would lead to the creation of the current codebase in one go when given to an AI coding agent.

In other words, it is the minimal yet sufficiently detailed set of instructions such that an agent — starting from an empty workspace — could produce the existing code, architecture, and configuration of this project as its output.

---

## Iterative Refinement Loop — Overview

The ultimate prompt is discovered through an iterative refinement process. The **Prompt Author** generates the initial prompt from the existing codebase. The loop then runs from `i = 0`, refining the prompt each iteration until **convergence** (the produced codebase matches the original) or a maximum of **10 iterations** is reached.

```
┌─────────────────────────────────────────────────────┐
│                   START (i = 0)                     │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
              ┌────────────────┐
              │  Prompt Author │  (iteration 0 only)
              │  → prompt v[0] │
              │  → tests/      │
              └───────┬────────┘
                      │
                      ▼
         ┌────────────────────────┐
         │  Generator             │
         │  (produces codebase   │
         │   from prompt)        │
         └───────────┬────────────┘
                     │
                     ▼
         ┌────────────────────────┐
         │  Verifier + Critic     │
         │  ┌──────────────────┐  │
         │  │ LLM Verification │  │
         │  └──────────────────┘  │
         │  ┌──────────────────┐  │
         │  │ Code Execution   │  │
         │  │ Verification     │  │
         │  └──────────────────┘  │
         │  ┌──────────────────┐  │
         │  │ Diff Report +    │  │
         │  │ Critique         │  │
         │  └──────────────────┘  │
         └───────────┬────────────┘
                     │
            ┌────────┴────────┐
            │                 │
            ▼                 ▼
     ┌────────────┐   ┌─────────────┐
     │ Converged? │   │ i >= 10?    │
     │ (no diff)  │   │ (max steps) │
     └─────┬──────┘   └──────┬──────┘
       yes │              yes │
           ▼                  ▼
       ┌───────┐         ┌────────┐
       │ DONE  │         │ STOP   │
       └───────┘         └────────┘
           no │
              ▼
     ┌─────────────────┐
     │  Prompt Refiner  │
     │  → prompt v[i+1] │
     └────────┬────────┘
              │
              │  i = i + 1
              └──────► (back to Generator)
```

---

## Step-by-Step Sequencing

### Iteration 0: Bootstrap

1. **Invoke the Prompt Author**: The Prompt Author reads the original codebase and produces:
   - `BENCHMARK_DIR/iteration_0/prompt.md` — the initial prompt
   - `BENCHMARK_DIR/tests/` — the equivalence test suite

2. **Dispatch to the Generator**: Hand off `prompt.md` to the Generator via the handoff protocol (see below).

3. **Receive generated codebase**: Archive the Generator's output to `BENCHMARK_DIR/iteration_0/implementation/`.

4. **Invoke the Verifier + Critic**: The Verifier + Critic evaluates the generated codebase and produces `BENCHMARK_DIR/iteration_0/diff_report.md`.

5. **Check termination**: If converged, stop. Otherwise, proceed to iteration 1.

### Iteration i (i ≥ 1): Refine

1. **Invoke the Prompt Refiner**: The Prompt Refiner reads the previous critique and prompt, and produces `BENCHMARK_DIR/iteration_[i]/prompt.md`.

2. **Dispatch to the Generator**: Hand off the refined prompt via the handoff protocol.

3. **Receive generated codebase**: Archive to `BENCHMARK_DIR/iteration_[i]/implementation/`.

4. **Invoke the Verifier + Critic**: Produces `BENCHMARK_DIR/iteration_[i]/diff_report.md`.

5. **Check termination**: If converged or `i >= 10`, stop. Otherwise, proceed to iteration `i + 1`.

---

## Handoff Protocol (Generator)

The Orchestrator and Generator communicate through a single shared directory (`HANDOFF_DIR`) on CNS. The directory contains a `STATE` file whose contents represent the current phase of the handoff. The lifecycle for each iteration is:

```
EMPTY ──→ READY ──→ PROCESSING ──→ COMPLETED ──→ (archive & cleanup) ──→ EMPTY
          Orch.      Generator      Generator      Orchestrator
```

### Dispatch Procedure

1. **Ensure the handoff directory is clean**: The directory should be empty (or not exist) before starting. If it contains artifacts from a previous iteration, something went wrong — investigate before proceeding.

2. **Write the prompt**: Copy the current prompt candidate to `HANDOFF_DIR/prompt.md`. This file must contain **only the prompt** — no metadata, no iteration numbers, no test suites, no diff reports. The Generator must not be able to infer anything about the iteration history.

3. **Signal readiness**: Write `READY` to `HANDOFF_DIR/STATE`. This tells the Generator that a task is available.

4. **Wait for completion**: Poll `HANDOFF_DIR/STATE` until its contents change to `COMPLETED` (or `FAILED`). Do not modify anything in the handoff directory while the Generator is working.

### Handoff Directory Layout

After the Generator completes, the handoff directory will contain:

```
HANDOFF_DIR/
├── STATE          # "COMPLETED" (or "FAILED")
├── prompt.md      # The prompt (written by Orchestrator)
└── workspace/     # The produced codebase (written by Generator)
```

### Receiving Results

1. **Read `STATE`**: If `COMPLETED`, proceed to verification. If `FAILED`, inspect the handoff directory for error information and decide whether to retry or refine the prompt.

2. **Archive**: Copy the contents of the handoff directory to the iteration's archive directory:
   - `HANDOFF_DIR/prompt.md` → `BENCHMARK_DIR/iteration_[i]/prompt.md`
   - `HANDOFF_DIR/workspace/` → `BENCHMARK_DIR/iteration_[i]/implementation/`

3. **Clean up**: Delete the contents of `HANDOFF_DIR` (or the directory itself) to reset for the next iteration.

4. **Proceed to verification**: Invoke the Verifier + Critic on the archived implementation.

> **Critical rule**: Never place tests, diff reports, learnings, or any file other than `prompt.md` and `STATE` into the handoff directory. The Generator must see only the prompt.

---

## Termination

The loop terminates when one of the following conditions is met.

### Convergence

The process has **converged** when the diff report shows:
- All files are present and assessed as ✅ **Equivalent**.
- Build command succeeds.
- All equivalence tests pass.

When converged, the final prompt is the **ultimate prompt** for this codebase.

### Max Steps Reached

If 10 iterations have been completed without convergence, the loop stops. The output is:
- The best prompt so far (the one with the fewest divergences).
- A summary of remaining gaps that the prompt could not resolve within the iteration budget.

### Final Output

```
BENCHMARK_DIR/final/prompt.md        # The converged (or best) ultimate prompt
BENCHMARK_DIR/tests/                 # The final equivalence test suite
BENCHMARK_DIR/final/diff_report.md   # The final diff report
```
