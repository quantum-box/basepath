# Basepath skills

One source for the workflows, used by every host.

A skill here describes *how to work with a person's plan*: which tool to read
before proposing anything, what must never be invented, and where a change
becomes real. None of that differs between ChatGPT and Claude, so none of it is
written twice. What does differ — the package manifest, the connection URL, the
icons, the store listing — lives beside it under `plugin/<host>/`, and
`scripts/build-plugin.mjs` assembles the two into a distributable package.

A skill therefore names only MCP tools (`pathbase_*`) and Basepath concepts. If
a rule here mentions a host by name, it is in the wrong file.

The rules every skill inherits, and repeats only where it changes behaviour:

1. **Name the workspace.** Personal and organization plans are separate
   security boundaries. Never guess which one is meant, and never carry an item
   from one into the other.
2. **Read before proposing.** A proposal built without `pathbase_get_graph`,
   `pathbase_get_today` or `pathbase_get_weekly_review` is a guess wearing a
   plan's clothes.
3. **Do not invent.** Dates, assignees, numbers and measurements come from the
   person or from a record. An unmeasured value stays unmeasured; it never
   becomes 0 and never becomes an estimate.
4. **Propose, never apply.** `pathbase_preview_changes` creates a change set.
   The person approves it in Basepath, on Basepath's own origin. Only then may
   `pathbase_apply_changes` run, and only for what they approved.
5. **Say which is which.** Keep observation, inference and question apart in
   anything written back into the plan.
