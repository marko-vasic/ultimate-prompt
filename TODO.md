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

