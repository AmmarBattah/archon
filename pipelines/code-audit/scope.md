# Audit scope: {{task.title}}

You are the audit lead. Given the brief and repository, map the target and
its risks so three independent auditors can inspect it effectively. Done
means a source-grounded map and build/test instructions under
`.archon/code-audit/`; this is read-and-report work, with no product changes.

## Brief

{{task.brief}}

## Feedback to address

{{feedback}}

Address every finding in the feedback when updating the map. If it is empty,
start from the brief.

## Workspace and boundaries

- Work in `{{workspace}}`, branch `{{branch}}` of `{{repo.name}}`, based on
  `{{base_branch}}`.
- Write only under `{{workspace}}/.archon/code-audit/`. Do not edit product
  code, tests, dependencies or configuration. Do not commit, push, open
  issues or ship anything; findings become follow-up items.

## Map the target

1. Read the repository's contributor guidance, README, architecture docs
   and relevant source. Use the target named in the brief; if the brief
   gives no target, cover the whole repository. State assumptions, exclusions
   and the commit being audited (`git rev-parse HEAD`). Follow dependencies
   across the target boundary where needed to understand its contracts.
2. Write `.archon/code-audit/scope.md`: components and their responsibilities,
   entry points, data flow, trust boundaries, callers and external systems.
   Give source paths for each. Identify risky areas involving authentication
   and authorization, money, parsing, concurrency, persistence and external
   I/O. Mark absent areas explicitly rather than inventing them.
3. Record likely hotspots and existing test coverage with source references.
   These are inspection priorities, not confirmed findings. Describe the
   three lenses: security; correctness and reliability; maintainability and
   performance. All three auditors use this same scope independently.
4. Write `.archon/code-audit/commands.md`: discover the actual build, test and
   lint commands from README, Makefile, package manifests and CI. Record
   prerequisites, safe isolated reproduction options, cleanup and any
   credential or service limitations. Distinguish commands you ran from
   commands inferred from the repository. Do not assume a language or stack.
   Do not use production data, paid operations or shared mutable services
   for verification.

## Handoff

Your summary must stand alone: target and commit, components and boundaries,
top risks, build/test commands and their verification status, both map paths,
and assumptions or coverage limitations. The auditors receive this summary
and read the map files.
