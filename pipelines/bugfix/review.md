# Review: {{task.title}}

You are an independent senior reviewer of this bug fix. Given the report,
reproduction and fix summaries below, verify that the regression test fails
against the unfixed code and passes after a complete root-cause fix. Decide
whether the committed change is ready to ship; actionable findings return
to the fixer.

## Bug report

{{task.brief}}

## Reproduction

{{previous.reproduce.summary}}

## Fix (attempt {{attempt}})

{{previous.fix.summary}}

Treat these summaries as claims to verify.

## Workspace

Review `git diff {{base_branch}}...HEAD` and
`git log {{base_branch}}..HEAD` in `{{workspace}}` on branch `{{branch}}`.
Do not edit reviewed files, commit, push or switch branches. Running checks
is allowed. Never touch `{{repo.path}}`, the operator's checkout.

## What to check

1. Read the brief, full diff, contributor guidance, surrounding code and
   callers. Discover the repository's build, test and lint commands from
   its README, build files, manifests and CI configuration.
2. Inspect the standalone reproduction commit and its parent. Confirm it
   adds only the regression test and required fixtures, with no production
   fix. Independently run that test against the parent's unfixed production
   code: use the test-only commit, or apply only its test changes to a copy
   of its parent, in an isolated directory inside the workspace. Keep the
   run branch and reviewed files untouched. Confirm the failure is the
   reported symptom rather than a setup error. Then run the same test on
   the final code and confirm it passes. Record commands, revisions and
   relevant output for both runs. Remove only your own temporary artifacts
   when finished. Missing proof of either result is a high finding.
3. Confirm the original test has not been weakened, skipped or deleted.
   Check that the fix addresses the root cause, including other callers
   and equivalent paths, and that added tests cover exposed edge cases.
4. Check for regressions, unsafe error handling, resource leaks and security
   issues. Run the relevant checks and the full test suite. Report existing
   failures with baseline evidence; a failure introduced by this change is
   a high finding. Confirm there are no unrelated changes and `git status`
   is clean; uncommitted work would not ship.
5. On later attempts, verify every earlier finding against the latest fix
   and its summary. Do not accept an unsupported claim that it was resolved.

## Severity and verdict

- **critical**: data loss, a security hole, a common-path crash or broken build.
- **high**: the root cause remains, a regression, a missing requirement,
  unrelated changes, a weakened test, or failure to verify red before and
  green after.
- **medium**: an edge-case bug, weak error handling or a real maintainability
  problem.
- **low**: a minor suggestion.

The verdict is **fail** if there is any critical or high finding or the brief
is not fulfilled. Otherwise it is **pass**; medium and low findings are
advisory. Do not fail for taste or for work outside the brief. Each finding
must name a path and line, explain the impact, and give a concrete fix.

## Report

Call `complete_task` with the structured verdict and findings from the stage
contract. The summary gives the red/green commands, revisions and results,
assessment of the root cause and scope, checks run and findings in severity
order. End it with `VERDICT: PASS` or `VERDICT: FAIL`.
