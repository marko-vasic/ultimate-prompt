# Ultimate Prompt Instructions

## Configuration

| Variable | Value |
|----------|-------|
| `TARGET_DIR` | `` |

## What is an Ultimate Prompt?

An **Ultimate Prompt** is a prompt that, with high likelihood, would lead to the creation of the current codebase in one go when given to an AI coding agent.

In other words, it is the minimal yet sufficiently detailed set of instructions such that an agent — starting from an empty workspace — could produce the existing code, architecture, and configuration of this project as its output.

## Iterative Refinement Loop

The ultimate prompt is discovered through an iterative refinement process. **Step 0** generates the initial prompt from the existing codebase. The loop then runs from `i = 0`, refining the prompt each iteration until **convergence** (the produced codebase matches the original) or a maximum of **10 iterations** is reached.

```
┌─────────────────────────────────────────────────────┐
│                   START (i = 0)                     │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
              ┌────────────────┐
              │  Ultimate      │
              │  Prompt v[i]   │
              └───────┬────────┘
                      │
                      ▼
         ┌────────────────────────┐
         │  AI Agent               │
         │  (produces codebase    │
         │   from prompt)         │
         └───────────┬────────────┘
                     │
                     ▼
         ┌────────────────────────┐
         │  Judge                 │
         │  ┌──────────────────┐  │
         │  │ LLM Verification │  │
         │  └──────────────────┘  │
         │  ┌──────────────────┐  │
         │  │ Code Execution   │  │
         │  │ Verification     │  │
         │  └──────────────────┘  │
         └───────────┬────────────┘
                     │
                     ▼
         ┌────────────────────────┐
         │  Diff Report           │
         │  (produced codebase    │
         │   vs. original)        │
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
     │ Refine prompt   │
     │ using learnings │
     │ from diff report│
     └────────┬────────┘
              │
              │  i = i + 1
              └──────► (back to top)
```

### Step-by-Step

0. **Initial Prompt Generation (Step 0)**: Before the loop begins, an initial ultimate prompt (`v[0]`) is generated from the existing codebase. See [Creating the Initial Prompt](#creating-the-initial-prompt) below for detailed instructions.

1. **Prompt → AI Agent**: The current iteration of the ultimate prompt (`v[i]`) is given to an AI agent, which works in a clean workspace to produce the codebase from scratch based solely on the prompt.

2. **Judge — Verification**: Once Jetski finishes, a **judge** evaluates the produced codebase against the original using two methods:
   - **LLM Verification**: An LLM compares the produced code against the original for semantic correctness, architectural alignment, and completeness.
   - **Code Execution Verification**: The produced code is built and tested (`blaze build`, `blaze test`) to verify it compiles and passes the same tests as the original.

3. **Diff Report**: The judge produces a structured report detailing the differences between the produced codebase and the original — missing files, incorrect logic, structural deviations, failing tests, etc.

4. **Refinement**: The learnings from the diff report are used to refine the ultimate prompt into the next iteration (`v[i+1]`). The refinement targets the gaps identified by the judge.

5. **Termination**: The loop terminates when either:
   - **Convergence**: The diff report shows no meaningful differences.
   - **Max steps reached**: 10 iterations have been completed.

## Step 0: Creating the Initial Prompt

The initial ultimate prompt (`v[0]`) is bootstrapped from the existing codebase. The goal is to produce a comprehensive, self-contained prompt that captures the project's intent, architecture, and key implementation details — without simply copying the source code verbatim.

### Process

1. **Survey the codebase**: Read all source files, configuration, and BUILD files in the target directory tree. Identify the full set of files that constitute the project.

2. **Extract high-level architecture**: Identify the major components, their responsibilities, and how they interact. Document:
   - Entry points (e.g., `main.go`, CLI flags)
   - Core abstractions and interfaces
   - Data flow and control flow
   - External dependencies and integrations

3. **Capture design decisions**: Note non-obvious choices that a naive implementation would miss:
   - Why certain patterns were used (e.g., concurrency model, error handling strategy)
   - Configuration layering and precedence
   - Protocol/API contracts
   - Edge cases and invariants

4. **Document file structure**: List every file that should be produced, with a brief description of its purpose.

5. **Specify behavior, not implementation**: The prompt should describe *what* the code does and *why*, not *how* line-by-line. The agent should be free to arrive at the implementation details on its own — the test is whether it converges on an equivalent result.

6. **Include build and test expectations**: Specify which build targets should succeed and which tests should pass, so the judge has clear acceptance criteria.

7. **Generate equivalence tests**: Produce a test suite that the agent deems sufficient and necessary to verify equivalence between the produced codebase and the original. These tests serve as the ground-truth acceptance criteria for the judge and should cover:
   - Core functionality and business logic
   - API contracts and interface compliance
   - Configuration parsing and validation
   - Error handling and edge cases
   - Integration points between components

### Output Format

Step 0 produces two files, saved alongside this file:

```
ULTIMATE_PROMPT_v0.md          # The initial ultimate prompt
ULTIMATE_PROMPT_TESTS_v0.go    # Equivalence test suite
```

## Step 1: Prompt → AI Agent Execution

The current ultimate prompt (`v[i]`) is given to a fresh AI agent session to produce the codebase from scratch.

### Process

1. **Create a fresh workspace**: Prepare a clean, empty directory (e.g., `mkdir fresh_workspace`) to serve as the environment for the agent. This ensures the agent starts with no existing source files from the project.

2. **Provide the prompt**: Give the agent the full contents of `ULTIMATE_PROMPT_v[i].md` as its task. Instruct it to generate the codebase based solely on the prompt.

3. **Let the agent work autonomously**: The AI agent reads the prompt and produces all source files, configuration, build files, and any other artifacts specified in the prompt. No human intervention.

4. **Capture the output**: Once the agent signals completion (or times out), the workspace contains the produced codebase for this iteration.

> **Note**: The agent still has access to its internal knowledge and any search tools it might have. The prompt should include a clear instruction not to search for or reference the original implementation from any external sources or existing indexed versions of the codebase if available.

### Output

- The produced codebase in the workspace, ready for evaluation.

## Step 2: Judge — Verification

The judge evaluates the produced codebase against the original. It uses two complementary verification methods.

### LLM Verification

1. **File-level comparison**: For each file in the original codebase, check whether a corresponding file exists in the produced codebase. Flag missing, extra, or renamed files.

2. **Semantic diff**: For each file pair, use an LLM to produce a semantic comparison — not a character-level diff, but an assessment of whether the produced code is *functionally equivalent*. Categories:
   - ✅ **Equivalent**: Same behavior, possibly different style.
   - ⚠️ **Partial**: Core logic correct, but non-trivial differences (e.g., missing edge case handling, different error messages).
   - ❌ **Divergent**: Meaningfully different behavior or missing functionality.

3. **Architecture review**: Assess whether the overall structure (package layout, abstractions, data flow) matches the original's design intent.

### Code Execution Verification

1. **Build**: Run the project's build command (e.g., `go build`, `npm run build`, `make`) on the produced codebase in the workspace. Record whether it succeeds or fails, and capture any build errors.

2. **Run equivalence tests**: Run the equivalence test suite (e.g., `ULTIMATE_PROMPT_TESTS_v[i].go`) in the workspace. These tests were designed in Step 0 specifically to verify behavioral equivalence.

3. **Run existing tests**: If the produced codebase includes its own tests (e.g., `go test`, `npm test`), run those as well and record results.

### Output

- A structured verification report with per-file semantic assessments and build/test results.

## Step 3: Diff Report

The judge synthesizes the verification results into a single, actionable diff report.

### Report Structure

```markdown
# Diff Report — Iteration [i]

## Summary
- Files in original: N
- Files produced: M
- Equivalent: X
- Partial: Y
- Divergent: Z
- Missing: W
- Build status: PASS/FAIL
- Equivalence tests: P/Q passed

## Missing Files
- [list of files present in original but absent in produced codebase]

## Extra Files
- [list of files produced that don't exist in the original]

## Per-File Assessment
### [filename]
- Status: ✅ / ⚠️ / ❌
- Summary: [brief description of differences]
- Key gaps: [specific issues the prompt should address]

## Build Errors
- [any compilation errors]

## Test Failures
- [failing test names and error summaries]

## Top Learnings
1. [most impactful gap — what was the prompt missing?]
2. [second most impactful gap]
3. ...
```

### Output

- `DIFF_REPORT_v[i].md` — saved alongside this file.

## Step 4: Refinement

The diff report's learnings are used to produce the next iteration of the ultimate prompt.

### Process

1. **Analyze the diff report**: Identify the root causes of divergence. Distinguish between:
   - **Prompt gaps**: Information that was absent from the prompt entirely.
   - **Prompt ambiguity**: Instructions that were present but unclear, leading to a different interpretation.
   - **Unnecessary detail**: Over-specification that constrained the agent in the wrong direction.

2. **Prioritize fixes**: Rank the issues by impact. Focus on changes that would fix the most divergent files or the most test failures.

3. **Revise the prompt**: Update the prompt to address the identified gaps and ambiguities. Follow these principles:
   - **Add missing context** where the agent lacked information to make the right choice.
   - **Clarify ambiguous instructions** where the agent made a reasonable but wrong interpretation.
   - **Remove over-specification** where unnecessary detail led the agent astray.
   - **Maintain minimality** — do not over-correct by adding line-by-line descriptions. The prompt should remain design-doc-level.

4. **Update equivalence tests** (if needed): If the diff report revealed that the equivalence tests themselves were insufficient or incorrect, update them as well.

### Output

- `ULTIMATE_PROMPT_v[i+1].md` — the refined prompt for the next iteration.
- `ULTIMATE_PROMPT_TESTS_v[i+1].go` — updated equivalence tests (if changed, otherwise carry forward).

## Step 5: Termination

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
ULTIMATE_PROMPT_FINAL.md       # The converged (or best) ultimate prompt
ULTIMATE_PROMPT_TESTS_FINAL.go # The final equivalence test suite
DIFF_REPORT_v[last].md        # The final diff report
```

---

## Learnings

This section captures observations and lessons learned during the ultimate prompt process. These inform future iterations and improvements to the methodology.

### 1. Prompt should not be too revealing (2026-03-23)

**Observation**: The v0 prompt included a file manifest with exact line counts (e.g., `kernel.go | ~120 | Kernel struct, AddGoal, Wake`). This is *too revealing* — it leaks implementation-level detail about the expected size of each file, which biases the agent toward matching a specific implementation rather than producing correct behavior. The ultimate prompt should specify **what** a file does, not **how big** it is. More generally, implementation details can leak unintentionally (line counts, exact variable names, internal comments, etc.).

**Action**: Remove line counts from the file manifest in subsequent prompt versions. Keep only the file path and a brief purpose description.

**Proposed remedy — Post-creation prompt review stage (Step 0.5)**:
1. After generating `ULTIMATE_PROMPT_v[i].md`, run a review pass.
2. Flag any content that specifies implementation detail rather than behavior:
   - Exact line counts or file sizes
   - Exact internal variable/field names (unless they are part of the public API or interface)
   - Copy-pasted code snippets from the original implementation
   - Internal comments or doc strings
3. Strip or generalize flagged content.
4. The prompt should pass the test: "Could a competent engineer produce a *functionally equivalent* but *structurally different* implementation from this prompt?" If the prompt over-constrains the structure, it's too revealing.
