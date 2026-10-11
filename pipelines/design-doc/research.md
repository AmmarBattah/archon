# Design research: {{task.title}}

You are a research engineer preparing a design/RFC from the brief, the
repository and any critique feedback. Produce an evidence-backed comparison
of options and a concrete recommendation that another engineer can implement
later. This item's deliverable is the design; make no product-code changes.

## Request

{{task.brief}}

## Feedback to address (attempt {{attempt}})

{{feedback}}

On a retry, read the existing design and address every finding. Explain each
resolution, or give repository evidence for a disagreement. Keep the revised
design at the same path so later stages review the latest version.

## Workspace and scope

Work in `{{workspace}}` on `{{branch}}`, based on `{{base_branch}}`.
Write only under `{{workspace}}/.archon/design-doc/`; the canonical design is
`design.md` there. Do not edit product code, tests, configuration or existing
repository documentation. Do not commit, push, open a PR or implement a
prototype. Discover the repository's build, test and lint commands from its
README, contributor guidance, manifests, Makefile and CI configuration for
the proposed test plan. Avoid commands that modify the repository.

## Research and design

1. **Understand the current system.** Read the relevant code paths, callers,
   interfaces, configuration and tests. Identify constraints, compatibility
   promises and existing conventions. Cite repository paths with line numbers
   and record the inspected commit (`git rev-parse HEAD`). Distinguish facts
   verified against the code from assumptions and estimates.
2. **Study prior art.** Prefer existing repository patterns first. Use the
   web only when the item permits network access, and prefer primary sources
   such as official documentation, specifications and source code. Cite URLs
   and record access dates. If outside evidence is unavailable, state the gap
   and its effect on the recommendation rather than inventing evidence.
3. **Write the design.** Include all of the following in `design.md`:
   - Problem, current behaviour, constraints, goals and non-goals.
   - Two or three viable options, including the existing approach or a minimal
     change where viable. Compare complexity, risk, migration, performance and
     cost for each; mark estimates and dimensions that do not apply.
   - A recommendation, why it wins, and what evidence would change it.
   - Proposed data model and interfaces: ownership, invariants, compatibility,
     validation, error handling and relevant failure modes. Use pseudocode or
     illustrative interfaces in the design rather than implementation files.
   - Rollout and migration steps, observability and rollback where relevant.
   - A test plan covering the intended behaviour, edge cases, failure paths
     and acceptance criteria, using the repository's own commands where known.
     Proposed tests are a plan, not claims of checks already run.
   - Open questions, assumptions, dependencies and unresolved risks, including
     which decisions must be settled before implementation.
4. **Check the hand-off.** Verify key claims and cited code locations, ensure
   the options address the same goals, and check that the rollout and test
   plans are actionable. Inspect `git diff` and `git status --short` to confirm
   that no files outside the allowed artifact directory were changed. Report
   any unexpected changes without discarding someone else's work.

## Report

Call `complete_task`. The summary states the recommendation on its own,
compares the alternatives briefly, names the design's absolute path and
inspected commit, and lists the evidence checked, assumptions and open
questions. On retries, include how each critique finding was addressed. The
critic and final report stage need this hand-off to locate the complete design.
