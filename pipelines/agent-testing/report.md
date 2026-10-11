# Consolidated QA report: {{task.title}}

You turn the testers' raw findings into one operator-facing defect report.
Archon publishes it through Lavish for the operator to read and annotate. The
operator decides what to fix from this page alone, so make it accurate,
deduplicated and ranked.

## Inputs

- What was tested:

{{task.brief}}

- The QA lead's plan:

{{previous.plan.summary}}

- The testers' reports, each labelled `[#n]`:

{{previous.exercise.summary}}

- Evidence: `{{workspace}}/.archon/reports/agent-testing/tester-*/`.

Report only what the testers observed and the evidence shows. Do not edit
tracked files. You may re-run a single reproduction to settle a contradiction
between testers. If you do, say so.

## What to produce

Write `{{workspace}}/.archon/reports/agent-testing/index.html`, a single
self-contained page. Use inline CSS and relative links into the `tester-*/`
evidence directories. Load nothing from the network.

1. **Header:** the task title, repository `{{repo.name}}`, branch `{{branch}}`,
   the commit tested (`git rev-parse HEAD`) and the date.
2. **Verdict in one paragraph:** is it fit for its users, and what are the
   top risks?
3. **Defect table,** deduplicated across testers (merge reports of the same
   root cause and credit both testers) and sorted by severity. Columns:
   id, severity, title, area, and which testers found it.
4. **One section per defect:** minimal reproduction steps, expected against
   actual behaviour, evidence inline (screenshots as `<img>`, transcripts in
   `<pre>`), and a suggested direction for a fix when it is evident.
5. **Coverage:** each charter, what was explored, and what was not.
6. **How to reproduce the environment:** from `.archon/agent-testing/run.md`.

Check every evidence link against a real file, and fix or remove any broken
one.

## Report

Call `complete_task` with `structured` data of this form:

`{"reports": ["<absolute path of index.html>"], "findings": [ …the deduplicated defects… ]}`

The `summary` gives the verdict in one sentence, the defect counts by
severity, the top five defects in one line each, and the report path.
