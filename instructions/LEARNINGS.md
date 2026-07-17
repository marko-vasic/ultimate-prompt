# Learnings

This file captures observations and lessons learned during the Ultimate Prompt process. These inform future iterations and improvements to the methodology. Learnings from all components and all benchmarks are collected here.

---

### 1. Prompt should not be too revealing (2026-03-23)

**Component**: Prompt Author

**Observation**: The v0 prompt included a file manifest with exact line counts (e.g., `kernel.go | ~120 | Kernel struct, AddGoal, Wake`) and, more broadly, a complete list of files with their internal responsibilities. Both are *too revealing* — they leak implementation-level detail about the expected structure, which biases the agent toward matching a specific implementation rather than producing correct behavior. The file listing is useful as an *analysis step* for the Prompt Author to understand the codebase's full scope, but it should not appear in the prompt. The ultimate prompt should specify **what** the system does, not **how it's decomposed into files**. More generally, implementation details can leak unintentionally (line counts, exact variable names, file names, directory layout, internal class names, internal comments, etc.).

**Action**: The file structure analysis is internal working material. The prompt should describe logical components as behavioral responsibilities, not as a file manifest. Remove file names, internal class names, and directory structure from prompts.

---

### 2. Test leakage corrupts Generator output (2026-05-11)

**Component**: Orchestrator / Generator

**Observation**: The equivalence test suite was mistakenly provided to the Generator alongside the Ultimate Prompt candidate. Per the handoff protocol, the Generator should receive **only the prompt** — never the tests, diff reports, or critique.

**Observed impact**: The Generator's implementation showed clear signs of "teaching to the test" rather than implementing from prompt comprehension:
1. Flag-parsed but unimplemented features — consistent with satisfying test harnesses that check flag acceptance without testing feature behavior.
2. Precise format matching on tested fields, but hardcoded dummy values for untested fields.
3. Monolithic architecture ignoring the prompt's structural guidance — since tests only verify CLI behavior, not code structure.

**Conclusion**: When the Generator sees the tests, iteration results become unreliable as a signal for prompt quality.

**Action**: Strict enforcement of handoff protocol — only `prompt.md` and `STATE` in the handoff directory. Never include tests.
