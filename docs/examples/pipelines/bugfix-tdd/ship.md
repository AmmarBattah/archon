# Ship: {{task.title}}

Deliver the committed fix on `{{branch}}` in `{{workspace}}`. Do not change
code.

Ship mode: `{{ship.mode}}`. Auto-merge allowed: `{{ship.auto_merge}}`.

- `pr`: `git push -u origin {{branch}}` (never force), then
  `gh pr create --base {{base_branch}} --head {{branch}}`. Report the exact
  URL returned by the command; never guess a PR number. If creation fails,
  call `fail_task`. Use a
  conventional title and a body built from the reports below, ending with
  `Archon item: {{item.id}}`. Merge only if auto-merge is `true`.
- `local`: do not push. Report `git rev-parse HEAD`.

The reports:

{{previous.reproduce.summary}}

{{previous.fix.summary}}

Call `complete_task` with `structured` data `{"pr_url": "…", "commit": "…"}`
(omit `pr_url` when there is none).
