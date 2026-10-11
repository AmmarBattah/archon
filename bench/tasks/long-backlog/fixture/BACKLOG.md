# Backlog

## TT-101: Reject impossible due dates

`tasktracker.dates.parse_due(text, today)` silently clamps impossible dates:
`2026-02-30` becomes `2026-02-28`. Instead:

- An impossible calendar date (`2026-02-30`, `2025-02-29`, `2026-04-31`,
  `2026-13-01`, `2026-00-10`) raises `InvalidDate`, a `ValueError` subclass
  defined in `tasktracker.dates`, whose message contains the original text.
- Anything else that does not match a supported form also raises
  `InvalidDate` (today it raises a bare `ValueError`).
- Relative offsets: `+Nd` is N days after `today`, `+0d` is `today`. Negative
  or unsigned offsets (`-3d`, `3d`) raise `InvalidDate`.
- `today` and `tomorrow` keep working; matching stays case-insensitive and
  ignores surrounding whitespace.

## TT-102: RFC 4180 CSV export

`tasktracker.export.to_csv(tasks)` joins fields with commas and breaks on
titles containing commas, quotes or newlines. It must produce RFC 4180 CSV:

- Header `id,title,tags,status,due`; one row per task in the given order.
- Tags joined with `;`, due date as ISO `YYYY-MM-DD` or empty.
- A field containing a comma, double quote, CR or LF is wrapped in double
  quotes, with embedded double quotes doubled. Other fields are not quoted.
- Every line (header included) ends with CRLF.

## TT-103: Search query syntax

`tasktracker.search.search(tasks, query)` only does a case-sensitive substring
match on titles. Support a small query language instead:

- The query is split into terms on whitespace, except inside double quotes:
  `"fix login"` is one phrase term. All terms must match (AND).
- A bare word or phrase matches when it appears in the title,
  case-insensitively.
- `tag:NAME` matches tasks that have the tag NAME (exact tag, case-insensitive).
- `status:open` / `status:done` match on status.
- Any other `field:value` term, an unknown status, or an unterminated quote
  raises `QueryError` (defined in `tasktracker.search`).
- An empty or blank query matches every task. Results keep the input order.

## TT-104: Undo

`tasktracker.store.TaskStore` has `add`, `complete` and `delete`. Add
`undo()`:

- It reverts the most recent not-yet-undone `add`, `complete` or `delete`;
  repeated calls walk further back (LIFO), across all three kinds.
- Undoing an `add` removes that task; undoing a `complete` makes the task
  open again; undoing a `delete` restores the task exactly as it was, under
  the same id.
- Ids are never reused, even after undoing an `add`.
- With nothing left to undo, `undo()` raises `NothingToUndo` (defined in
  `tasktracker.store`).
- Failed operations (e.g. completing an unknown id) are not recorded.
