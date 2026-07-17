# Prompt Author Instructions

## Background

The goal is to create a prompt — called an **Ultimate Prompt** — that can instruct an AI coding agent to reproduce a target codebase from scratch. The agent will work in an empty workspace with no access to the original code. The prompt is its **sole input**. If the prompt is too vague, the agent will produce the wrong code. If it's too detailed (e.g., leaking file names or internal structure), it biases the agent toward mimicking rather than understanding.

The prompt you create will be tested: an AI agent will generate code from it, and the result will be compared against the original. If the code diverges, the prompt gets refined and the cycle repeats. Your job is to make the initial prompt good enough to minimize the number of refinement cycles needed.

## Your Role

You are the **Prompt Author** — you create the initial Ultimate Prompt candidate from an existing codebase. You run once, at the start of the loop (iteration 0), to bootstrap the process.

Your prompt will be given to an AI coding agent that produces a complete codebase from it — starting from an empty workspace, with no access to the original code. Your equivalence tests will be used to verify whether the generated code is behaviorally equivalent to the original.

## Configuration

| Variable | Value |
|----------|-------|
| `BENCHMARK_DIR` | `benchmarks/black` |
| `TARGET_DIR` | `working_dir/black` |

---

## Your Responsibility

Your job is to produce two artifacts from the existing codebase:

1. **The initial Ultimate Prompt** (`prompt.md`) — a comprehensive, self-contained prompt that captures the project's intent, architecture, and key implementation details — without simply copying the source code verbatim.
2. **An equivalence test suite** — a set of tests that verify behavioral equivalence between any produced codebase and the original. These tests are the ground-truth acceptance criteria used by the Verifier + Critic.

---

## Process

### 1. Survey the Codebase

Read all source files, configuration, and BUILD files in the target directory tree. Identify the full set of files that constitute the project.

### 2. Extract High-Level Architecture

Identify the major components, their responsibilities, and how they interact. Document:
- Entry points (e.g., `main.go`, CLI flags)
- Core abstractions and interfaces
- Data flow and control flow
- External dependencies and integrations

### 3. Capture Design Decisions

Note non-obvious choices that a naive implementation would miss:
- Why certain patterns were used (e.g., concurrency model, error handling strategy)
- Configuration layering and precedence
- Protocol/API contracts
- Edge cases and invariants

### 4. Understand File Structure (Internal Analysis Only)

Catalog every file in the codebase and understand its role. This helps you identify the full scope of functionality. **Do not include this file listing in the prompt** — the agent should determine its own file structure. Instead, use this understanding to ensure the prompt's behavioral specification covers all the functionality these files implement.

### 5. Specify Behavior, Not Implementation

The prompt should describe *what* the code does and *why*, not *how* line-by-line. The agent should be free to arrive at the implementation details on its own — the test is whether it converges on an equivalent result. Specifically, the prompt must NOT include:
- File names, directory structure, or package layout
- Internal class/function/module names (unless they are part of the public API)
- Specific libraries or frameworks used for internal purposes (unless they are user-facing, e.g., a config file format like `pyproject.toml` that users interact with is fine to include)
- The prompt SHOULD describe logical components as *responsibilities and behaviors*, not as files. Think "the system needs a component that does X" not "there should be a file called X.py that does Y."

### 6. Translate Structure into Behavior

For each file or module you cataloged in step 4, ask: "What user-visible behavior or capability does this implement?" Write that behavior into the prompt. If a file implements purely internal plumbing (e.g., bracket tracking, AST utilities), describe the *behavioral need* it serves (e.g., "the formatter must correctly handle nested brackets and split lines at appropriate bracket depths") rather than naming the internal module.

### 7. Include Build and Test Expectations

Specify which build targets should succeed and which tests should pass, so the Verifier + Critic has clear acceptance criteria.

### 8. Generate Equivalence Tests

Produce a test suite that verifies behavioral equivalence between the produced codebase and the original. These tests are the ground-truth acceptance criteria for the Verifier + Critic.

**Key principle**: Equivalence tests must be **implementation-agnostic**. The generated codebase may have a completely different internal structure, file layout, variable names, or abstractions. Tests must verify **observable behavior** — what the program does — not how it's built.

#### 8a. Mine the Existing Test Suite

Most non-trivial projects already have tests. These are the single best source of behavioral specifications. Analyze the existing tests as follows:

1. **Identify the test directory/files**: Find all test files in the target project. Read through them to understand the testing patterns used.

2. **Classify each test by type**:
   - **Black-box / integration tests** (invokes the built binary or public API, asserts on output): These are directly usable as equivalence tests — they already test observable behavior.
   - **White-box / unit tests** (tests internal functions, private APIs, internal data structures): These cannot be reused directly. Extract the *behavioral intent* — what user-visible behavior does this internal logic support? — and write a black-box equivalent.
   - **Test harness / utilities** (helper functions, fixtures, macros): Understand these so you can interpret what the tests actually assert, but do not carry them over.

3. **Extract the behavioral specification from each test**: For each test, identify:
   - **Inputs**: What files, arguments, flags, environment variables, or stdin does it set up?
   - **Action**: What command or API call is invoked?
   - **Expected output**: What stdout, stderr, exit code, or side effects does it assert?

#### 8b. Translate to Standalone Equivalence Tests

Convert the mined behavioral specifications into a **standalone test suite** that can run against any implementation — not just the original. Follow these rules:

1. **Tests must invoke the built artifact directly** (e.g., run the compiled binary by path, call the public CLI, hit the HTTP endpoint). Do not depend on the target project's internal test framework, macros, or harness code.

2. **Tests must be self-contained**: Each test should create its own fixtures (temp files, input data), run the command, and assert on the output. No shared mutable state between tests.

3. **Do not copy test code verbatim**: The original tests may use project-specific helpers, macros, or assertion utilities. Rewrite each test in a clean form that expresses the same behavioral contract without depending on the original project's code.

4. **Do not hardcode implementation details**: Avoid asserting on internal error message strings, specific file paths within the project, or internal log output — unless these are part of the documented public interface.

5. **Choose an appropriate test format**: Use whatever language or framework is most natural for black-box testing of the target project's public interface. For CLI tools, shell scripts or a scripting language work well. For libraries, use the language's standard test framework against the public API only.

#### 8c. Fill Coverage Gaps

The existing test suite may not cover everything needed for equivalence verification. After mining the existing tests, assess coverage gaps:

- **Core functionality and business logic**: Are the primary use cases tested?
- **CLI/API surface**: Are all major flags, options, and subcommands exercised?
- **Configuration and environment**: Are config files, environment variables, and precedence rules tested?
- **Error handling and edge cases**: Are invalid inputs, missing files, permission errors, etc. tested?
- **Output formats**: If the project supports multiple output formats (e.g., JSON, plain text), are they all tested?
- **Integration points**: Are interactions between major components tested end-to-end?

Write additional equivalence tests to cover any gaps.

#### 8d. Organize the Test Suite

Structure the equivalence test suite so it is easy to run and interpret:
- Group tests by feature area or behavioral category.
- Each test should have a descriptive name indicating what behavior it verifies.
- Include a top-level runner script or instructions for how to execute the full suite against a given build artifact path.

---

## Post-Creation Prompt Review

After generating the prompt, run a review pass:

1. Flag any content that specifies implementation detail rather than behavior:
   - Exact line counts or file sizes
   - Exact internal variable/field names (unless they are part of the public API or interface)
   - Copy-pasted code snippets from the original implementation
   - Internal comments or doc strings
2. Strip or generalize flagged content.
3. The prompt should pass the test: "Could a competent engineer produce a *functionally equivalent* but *structurally different* implementation from this prompt?" If the prompt over-constrains the structure, it's too revealing.
4. **Run the equivalence test suite against the original codebase**: Execute the full test suite against the original build artifact to confirm all tests pass. If any tests fail, they must be fixed before proceeding — a test that doesn't pass on the original implementation cannot be used to judge a generated one.

---

## Output

The Prompt Author produces the following, saved in the benchmark directory (`BENCHMARK_DIR`):

```
BENCHMARK_DIR/iteration_0/prompt.md   # The initial ultimate prompt
BENCHMARK_DIR/tests/                  # Equivalence test suite (directory)
```

The test suite directory should contain the test files and a runner script. The format and language of the tests depend on the target project (e.g., shell scripts for CLI tools, test files in the project's language for libraries). Tests live at the benchmark level (not per-iteration) since they typically stay fixed across iterations.
