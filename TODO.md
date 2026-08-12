## Potential Benchmarks

- Easier codebase to start with (github)
    - sindresorhus/awesome-lint
    - tj/commander.js
    - httpie/cli
    - Textualize/rich
    - pydantic/pydantic
    - ekstaziSharp
    - psf/black
- Coordinator vs Generator vs Verifier problem?
    1. Coordinator bad => bad prompt suggestions
    2. Generator bad => current LLMs not performing well enough
    3. Verifier bad => false positives/negatives

iteration 0
repo -> coordinator -> prompt_v0.md
prompt_v0.md -> generator -> code
code -> verifier -> pass/fail, critique

iteration 1
pass/fail + critique + prompt_v0.md -> coordinator -> prompt_v1.md

---

## Roadmap: Study Conclusion & Public Release

### 1. Benchmark Convergence
- [ ] **ripgrep (`benchmarks/ripgrep` — Rust)**
  - [ ] Generate **Prompt v2** incorporating critical default wiring rules:
    - Omit file path prefix on single-file search and `--no-filename`
    - Read from stdin when piped and no positional paths supplied
    - Integrate `.rgignore` / `.ignore` precedence & sorting in walker
  - [ ] Run Generator on Prompt v2 and verify test suite pass rate (>85/89)
  - [ ] Finalize convergence diff report and learnings
- [ ] **black (`benchmarks/black` — Python)**
  - [ ] Execute Prompt Author iteration 0 (prompt + test suite)
  - [ ] Run Generator on Prompt v0
  - [ ] Run Verifier/Critic and generate diff report
  - [ ] Refine to Prompt v1 and run until convergence

### 2. Cross-Ecosystem Validation (1–2 Additional Repos)
- [ ] Select and onboard 1–2 lightweight target codebases in other languages:
  - [ ] **TypeScript/JavaScript**: `tj/commander.js` or `sindresorhus/awesome-lint`
  - [ ] **Go**: CLI / microservice (e.g. `rakyll/hey` or `schollz/croc`)
- [ ] Run full refinement loop to demonstrate cross-language generality

### 3. Empirical Analysis & Core Research Findings
- [ ] **Information Density & Code Compression Ratio**:
  - Measure prompt LoC vs. generated LoC across benchmarks (e.g., ~450 LoC prompt → ~13,000 LoC Rust).
- [ ] **Structural Independence & Emergent Architecture**:
  - Document how agents discover modular crate/package layouts from behavioral requirements without file-level prescription.
- [ ] **The Test Contamination Phenomenon**:
  - Document the impact of test leakage on code quality (shallow monolith with hollow stubs vs. genuine modular synthesis).
- [ ] **Convergence Dynamics**:
  - Plot test pass rates, compilation warnings, and diff size across iterations ($v0 \rightarrow v1 \rightarrow v2$).

### 4. Public Release Deliverables
- [ ] **Technical Report / Blog Post**:
  - "The Ultimate Prompt: Compressing Real-World Software into Pure Behavioral Prompts"
  - Methodology overview (Author → Generator → Verifier/Critic → Refiner loop).
  - Empirical results, architectural comparisons, and case studies.
- [ ] **Open-Source Benchmark Suite**:
  - Release equivalence test suites, prompts, and verification harness for frontier model evaluation.
- [ ] **Showcase Dashboard / Visual Comparison**:
  - Side-by-side comparison of original repositories vs. agent-synthesized codebases.


