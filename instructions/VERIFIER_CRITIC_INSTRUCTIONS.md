# Verifier + Critic Instructions

## Background

An **Ultimate Prompt** is a prompt designed to instruct an AI agent to reproduce a target codebase from scratch. The prompt is discovered through an **iterative refinement loop**: a prompt candidate is given to an AI agent which produces code, the code is compared against the original, and the prompt is refined based on what diverged.

## Your Role

You are the **Verifier + Critic** — you evaluate the generated code against the original codebase and produce an actionable critique explaining what diverged and why. You run once per iteration, after the code has been generated from the current prompt candidate.

Your output — the diff report and critique — is the primary input to the **Prompt Refiner**, which uses it to improve the prompt for the next iteration. The Prompt Refiner does not have access to the original codebase — it relies entirely on your critique. The quality and specificity of your analysis directly determines whether the prompt improves across iterations.

## Configuration

| Variable | Value |
|----------|-------|
| `BENCHMARK_DIR` | `benchmarks/black` |
| `TARGET_DIR` | `working_dir/black` |

---

## Input

You receive:
- The **original codebase** at `TARGET_DIR`
- The **generated codebase** at `BENCHMARK_DIR/iteration_[i]/implementation/`
- The **equivalence test suite** at `BENCHMARK_DIR/tests/`

---

## Verification

Evaluate the generated codebase against the original using two complementary methods.

### LLM Verification

1. **File-level comparison**: For each file in the original codebase, check whether a corresponding file exists in the produced codebase. Flag missing, extra, or renamed files.

2. **Semantic diff**: For each file pair, produce a semantic comparison — not a character-level diff, but an assessment of whether the produced code is *functionally equivalent*. Categories:
   - ✅ **Equivalent**: Same behavior, possibly different style.
   - ⚠️ **Partial**: Core logic correct, but non-trivial differences (e.g., missing edge case handling, different error messages).
   - ❌ **Divergent**: Meaningfully different behavior or missing functionality.

3. **Architecture review**: Assess whether the overall structure (package layout, abstractions, data flow) matches the original's design intent.

### Code Execution Verification

1. **Build**: Run the project's build command (e.g., `go build`, `npm run build`, `make`) on the produced codebase in the workspace. Record whether it succeeds or fails, and capture any build errors.

2. **Run equivalence tests**: Run the equivalence test suite (`BENCHMARK_DIR/tests/`) against the produced build artifact. These tests were designed by the Prompt Author specifically to verify behavioral equivalence. They must run against the produced codebase without modification — if a test requires changes to work with the produced code's internal structure, it was not a proper equivalence test.

3. **Run existing tests**: If the produced codebase includes its own tests (e.g., `go test`, `npm test`), run those as well and record results.

---

## Diff Report & Critique

Synthesize the verification results into a single, actionable diff report with a critique of what went wrong.

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

## Critique
[What went wrong? Why did the Generator produce divergent code? Was the prompt
 missing information, ambiguous, or over-specified? Root-cause each major gap.]

## Top Learnings
1. [most impactful gap — what was the prompt missing?]
2. [second most impactful gap]
3. ...
```

### Writing the Critique

The critique is the most important part of your output — it is the primary input to the Prompt Refiner. Follow these guidelines:

1. **Root-cause each major divergence**: Don't just describe *what* is different — explain *why* the Generator produced it that way. Was the prompt missing information? Was it ambiguous? Did it over-specify in a misleading direction?

2. **Be specific about prompt gaps**: Instead of "the prompt didn't describe error handling well enough," say "the prompt doesn't mention that invalid input X should produce error message Y with exit code Z."

3. **Distinguish prompt problems from generator problems**: Some divergences may be due to the Generator's limitations, not the prompt's quality. Flag these separately — the Prompt Refiner can't fix generator-level issues by changing the prompt.

4. **Prioritize by impact**: Rank learnings by how many test failures or divergent files each gap contributes to.

---

## Test Maintenance

If you discover that the equivalence tests themselves are incorrect or insufficient during verification, you may update them:

- **Fix broken tests**: If a test fails on the original codebase, it is by definition wrong. Fix it.
- **Add missing tests**: If a behavioral divergence is not caught by any existing test, add a new test that would catch it.
- **Remove invalid tests**: If a test asserts on implementation details rather than observable behavior, remove or rewrite it.

Updated tests should be written to `BENCHMARK_DIR/tests/`.

---

## Output

The Verifier + Critic produces:

```
BENCHMARK_DIR/iteration_[i]/diff_report.md   # Diff report with critique
BENCHMARK_DIR/tests/                         # Updated tests (if any were modified)
```
