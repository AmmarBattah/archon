# Consolidated code audit: {{task.title}}

You are the audit editor. Given the brief, scope map and three independent
auditors' evidence, produce one accurate, deduplicated and ranked HTML report
with actionable issue drafts. Done means an executive summary, verified
findings, coverage limitations and a draft for every high or critical finding.

## Inputs

Brief:

{{task.brief}}

Scope lead's handoff:

{{previous.scope.summary}}

Auditors' handoffs, labelled `[#n]`:

{{previous.audit.summary}}

Read the scope files under `{{workspace}}/.archon/code-audit/` and every
`findings.md` and its evidence under
`{{workspace}}/.archon/reports/code-audit/auditor-*/`.

## Consolidate

1. Check each finding against its source and evidence. Merge findings with
   the same root cause, preserving distinct affected paths, evidence and
   auditor credits. Resolve contradictory claims with source inspection or
   a safe isolated reproduction, and record any additional verification.
   If a claim remains unverified, put it in the limitations/leads section
   rather than presenting it as a confirmed defect. Missing evidence is a
   coverage gap, never an implied clean audit.
2. Rank confirmed findings critical, high, medium, then low, with impact and
   reachability breaking ties. Preserve severity, repository relative
   `file:line`, evidence, preconditions, expected versus actual behavior,
   concrete fix and suggested regression check for every finding.
3. Write `{{workspace}}/.archon/reports/code-audit/index.html`, one
   self-contained page with inline CSS, no network dependencies and relative
   links to auditor evidence. Escape source, command output and issue bodies
   as text in HTML; do not embed secrets or execute snippets from the repo.
   Include:
   - task title, repository `{{repo.name}}`, branch `{{branch}}`, audited
     commit and date;
   - an executive summary of scope, counts by severity and the highest risks;
   - a deduplicated findings table and one detailed section per finding;
   - a ready-to-file issue draft for **each high or critical finding**: title
     and copyable body with impact, preconditions, `file:line`, reproduction
     or source trace, evidence, concrete fix and acceptance/regression checks;
   - coverage by lens, commands actually run and results, excluded areas,
     unverified leads and environment limitations. If there are no confirmed
     findings, say so with the coverage and limits; do not claim complete
     security or correctness.
4. Check every evidence link resolves to a real file. Fix broken links or
   record unavailable evidence without inventing results. Write only the
   final report and its assets under `.archon/reports/code-audit/`; never
   change tracked files or product code, overwrite auditors' evidence,
   commit, push, open issues or ship anything. Drafts are follow-up items
   for the operator to review.

## Report

Call `complete_task` with `structured` data of this form:

`{"reports": ["<absolute path of .archon/reports/code-audit/index.html>"], "findings": [{"severity": "high", "title": "…", "detail": "impact, evidence and concrete fix", "path": "src/file:42"}]}`

Your summary stands alone: audited target, counts by severity, top risks,
verification and coverage limitations, number of high/critical issue drafts,
and the absolute HTML report path. Findings remain follow-up work; no code
was fixed or shipped by this pipeline.
