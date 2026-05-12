# Ripgrep Benchmark — Learnings

## 1. Test leakage corrupts Generator output (2026-05-11, iteration 0)

**Mistake**: The equivalence test suite (`tests/run_tests.sh`) was mistakenly provided to the Generator alongside the Ultimate Prompt candidate (`iteration_0/prompt.md`). Per the [COORDINATOR_INSTRUCTIONS.md](../../COORDINATOR_INSTRUCTIONS.md) (Step 1), the Generator should receive **only the prompt** — never the tests, diff reports, or critique.

**Observed impact**: The Generator's implementation showed clear signs of "teaching to the test" rather than implementing from prompt comprehension:

1. **Flag-parsed but unimplemented features**: The implementation parses nearly every CLI flag (color, encoding, compression, mmap, parallelism, hyperlinks, debug logging) but leaves the behavior as a no-op. This is consistent with satisfying test harnesses that check flag acceptance without testing the feature behavior.

2. **Precise format matching on tested fields**: The binary detection message format (`"binary file matches (found \"\\0\" byte around offset N)"`) and JSON schema structure closely align with what the tests assert on, but fields the tests don't inspect are hardcoded to dummy values (e.g., `elapsed: {secs: 0, nanos: 0}`, `bytes_printed: 0`).

3. **Monolithic architecture**: The prompt explicitly specifies a Cargo workspace with modular crate separation (Section 2). The Generator ignored this entirely, producing a single 1477-line `main.rs`. This suggests the Generator prioritized passing behavioral tests over following architectural guidance in the prompt — since the tests only verify CLI behavior, not code structure.

**Conclusion**: The iteration 0 results are **unreliable as a signal for prompt quality**. The implementation passes more tests than the prompt alone would produce, and the diagnostic value of test failures is diminished because the Generator was optimizing for test outcomes rather than prompt comprehension.

**Action**: Iteration 0 should be re-run with the Generator receiving **only the prompt**. The current implementation is retained for reference but should not be used for the diff report / refinement cycle.
