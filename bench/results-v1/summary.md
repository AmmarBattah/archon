# Archon benchmark

- Run `20260926T200340Z`: 2026-09-26T20:03:40.884977145+00:00 → 2026-09-26T21:15:03.728438624+00:00
- Archon commit: not published (the build predates the public repository)
- Effort: `high`; trials per task and arm: 3; parallel: 3
- Model `aether`: `claude-code/claude-opus-5-5`
- Model `archon`: `claude-code/claude-opus-5-5`
- Model `claude`: `claude-code/claude-opus-5-5`
- Model `codex`: `codex/gpt-5.6-sol`
- Version `aetherd`: `0.1.1`
- Version `archon`: pre-release build (before 1.0.0)
- Version `claude`: `2.1.283 (Claude Code)`
- Version `codex`: `codex-cli 0.155.1`
- Trials: 96 planned, 52 completed, 44 skipped; spent $40.01 of $37.00
- **Stopped early:** --max-usd 37.00 reached after $40.01; 44 trials skipped

## Per arm

| Arm | Model | Trials | Success | pass@1 | pass@1 | Score | Wall s | Input tok | Output tok | Cost $ | Operator msgs | Infra | Timeouts |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| archon | `claude-code/claude-opus-5-5` | 12 | 12/12 (100%) | 100% | 100% | 1.00 ± 0.00 | 279 ± 199 | 1350763 ± 940220 | 29221 ± 22419 | 2.02 ± 1.39 | 3.7 ± 1.6 | 1 | 0 |
| aether | `claude-code/claude-opus-5-5` | 13 | 13/13 (100%) | 100% | 100% | 1.00 ± 0.00 | 91 ± 49 | 274846 ± 164522 | 8379 ± 5688 | 0.42 ± 0.20 | 3.5 ± 2.4 | 0 | 0 |
| claude | `claude-code/claude-opus-5-5` | 12 | 12/12 (100%) | 100% | 100% | 1.00 ± 0.00 | 77 ± 50 | 231210 ± 163780 | 7478 ± 5381 | 0.38 ± 0.19 | 1.0 ± 0.0 | 1 | 0 |
| codex | `codex/gpt-5.6-sol` | 13 | 13/13 (100%) | 100% | 100% | 1.00 ± 0.00 | 215 ± 183 | 285680 ± 196260 | 8343 ± 5436 | 0.38 ± 0.22 | 1.0 ± 0.0 | 0 | 0 |

## Per task

Each cell: passes/trials · wall s · cost $ · operator msgs (mean ± stdev).

| Task | archon | aether | claude | codex |
|---|---|---|---|---|
| ask-cache-invalidation | 2/2 · 35 ± 0 · 0.23 ± 0.01 · 1.0 ± 0.0 | 2/2 · 34 ± 2 · 0.20 ± 0.01 · 1.0 ± 0.0 | 2/2 · 32 ± 4 · 0.20 ± 0.00 · 1.0 ± 0.0 | 2/2 · 63 ± 2 · 0.11 ± 0.01 · 1.0 ± 0.0 |
| feature-token-bucket | 2/2 · 586 ± 42 · 3.04 ± 0.56 · 4.0 ± 1.4 | 2/2 · 109 ± 20 · 0.51 ± 0.07 · 3.0 ± 0.0 | 2/2 · 116 ± 22 · 0.49 ± 0.08 · 1.0 ± 0.0 | 2/2 · 194 ± 13 · 0.37 ± 0.03 · 1.0 ± 0.0 |
| fix-off-by-one | 2/2 · 182 ± 151 · 0.64 ± 0.01 · 3.5 ± 0.7 | 2/2 · 45 ± 38 · 0.16 ± 0.00 · 1.0 ± 0.0 | 2/2 · 18 ± 2 · 0.16 ± 0.01 · 1.0 ± 0.0 | 2/2 · 57 ± 9 · 0.17 ± 0.04 · 1.0 ± 0.0 |
| long-backlog | 2/2 · 390 ± 151 · 3.97 ± 0.19 · 6.0 ± 0.0 | 2/2 · 165 ± 1 · 0.72 ± 0.01 · 4.5 ± 0.7 | 1/1 · 154 ± 0 · 0.71 ± 0.00 · 1.0 ± 0.0 | 2/2 · 563 ± 233 · 0.75 ± 0.11 · 1.0 ± 0.0 |
| multi-repo-currency | 2/2 · 193 ± 41 · 2.19 ± 0.05 · 4.0 ± 0.0 | 2/2 · 123 ± 12 · 0.56 ± 0.02 · 7.5 ± 0.7 | 2/2 · 110 ± 16 · 0.52 ± 0.06 · 1.0 ± 0.0 | 2/2 · 239 ± 1 · 0.52 ± 0.03 · 1.0 ± 0.0 |
| multi-repo-pagination | 1/1 · 388 ± 0 · 2.73 ± 0.00 · 4.0 ± 0.0 | 1/1 · 111 ± 0 · 0.49 ± 0.00 · 6.0 ± 0.0 | 1/1 · 125 ± 0 · 0.56 ± 0.00 · 1.0 ± 0.0 | 1/1 · 222 ± 0 · 0.48 ± 0.00 · 1.0 ± 0.0 |
| refactor-order-status | 1/1 · 190 ± 0 · 1.36 ± 0.00 · 3.0 ± 0.0 | 1/1 · 64 ± 0 · 0.38 ± 0.00 · 3.0 ± 0.0 | 1/1 · 55 ± 0 · 0.34 ± 0.00 · 1.0 ± 0.0 | 1/1 · 197 ± 0 · 0.42 ± 0.00 · 1.0 ± 0.0 |
| review-seeded-bug | — | 1/1 · 55 ± 0 · 0.27 ± 0.00 · 2.0 ± 0.0 | 1/1 · 41 ± 0 · 0.22 ± 0.00 · 1.0 ± 0.0 | 1/1 · 140 ± 0 · 0.23 ± 0.00 · 1.0 ± 0.0 |

## Infrastructure failures

Left out of every number above.

- review-seeded-bug / archon #1: grading: aborting the failed merge of : git ["merge", "-q", "--no-ff", "--no-edit", ""] in ~/.local/share/archon-bench/work/20260926T200340Z/e447d784a73b9fefd03a69ae/review-seeded-bug fail…
- long-backlog / claude #2: claude reported success: You've hit your session limit

## Method

- Every arm gets the same task prompt, effort and time limit, and the same model where the harness allows; `codex` runs its own model. Every arm runs with its tool permission prompts bypassed.
- `claude` keeps Claude Code's built-in subagents (the Task tool); the `aether` arm is a single leaf agent without delegation.
- Each trial starts from a fresh copy of the fixtures in an unguessably named directory outside the Archon checkout, with a one-commit git history. Hidden tests, reference solutions and rubrics are copied in only at grade time, into a separate copy the agent never sees, after the agent and every process it started have stopped. Agent processes do not inherit environment variables pointing at the checkout. There is no OS sandbox: an agent that searched the whole home directory could still find the checkout.
- Success is the hidden grader's verdict. Test graders restore the visible tests from the task, not from the agent's repository, and require the runner's own summary to report no failures and a minimum number of passing tests. Rubric graders need related facts close together and cap the answer length.
- A failed trial with a harness error (the arm could not run cleanly: a provider limit, a crash, a grading failure) is an infrastructure failure. It is left out of pass rates and every metric, counted under Infra and listed with its error.
- Wall time runs from handing over the prompt until the arm finished; Archon's daemon start-up (recorded as `setup_secs`) and the quiet window used to detect that Archon finished are excluded.
- Input tokens include cache reads and writes. Cost is billed dollars when reported, else Aether's estimate or `--price` list prices. `(n=…)` marks a metric only that many trials reported.
- Operator messages are what the operator had to read: one final result for `claude`/`codex`, the assistant messages of the Aether agent, and the default-visible chat lines for Archon.
- Values are mean ± sample standard deviation over graded trials. pass@k uses the unbiased estimator per task, averaged over tasks, with k the fewest graded trials any (task, arm) cell has.
