# Final design/RFC: {{task.title}}

You are a technical editor publishing the independently critiqued design.
Given the brief, the latest research and the passing critique, produce one
accurate HTML page that lets the operator understand the recommendation and
plan without reading the stage transcripts.

## Inputs

The request:

{{task.brief}}

The latest research hand-off:

{{previous.research.summary}}

The independent critique:

{{previous.critique.summary}}

Read the final design at `{{workspace}}/.archon/design-doc/design.md`.
Preserve the reviewed recommendation and clearly retain unresolved risks
and advisory findings. Do not silently introduce a new design or present
unverified claims as facts. If the inputs contradict one another materially
or the design is missing, report the failure instead of publishing a
misleading page.

## Deliverable

Write only `{{workspace}}/.archon/reports/design-doc/index.html`, one
self-contained HTML page with inline CSS and no network-loaded assets.
Make no product-code or other repository changes; do not commit, push, open
a PR or implement the plan.

Include the task title, repository `{{repo.name}}`, inspected commit and date,
then a recommendation that stands on its own. Present the problem,
goals/non-goals, constraints, two or three options and their trade-offs,
the reasoning for the recommendation, data model/interfaces, failure modes,
rollout and test plan, open questions and cited evidence. State which checks
were actually performed and distinguish the proposed implementation/test
plan from completed work. Optionally suggest a split into work items with
dependencies and acceptance criteria; do not create or enqueue them.

Keep necessary evidence inline so the page remains useful after publication;
do not depend on links to unpublished local design files. Escape source
snippets and other literal text as HTML. Check the page for readable markup,
verify repository citations against real locations, and check any local links
against files. Verify external links only when network access is allowed and
state any unverified ones. Confirm the report exists and that only the
permitted artifact directories contain changes before finishing.

## Report

Call `complete_task` with `structured` data
`{"reports": ["{{workspace}}/.archon/reports/design-doc/index.html"]}`.
The summary states the recommendation and its rationale on its own, the
principal trade-offs, remaining risks or open questions, what was verified,
and the report's absolute path. Archon publishes the page from this list.
