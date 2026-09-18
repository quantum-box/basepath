---
name: basepath-goal-breakdown
description: Break a goal in Basepath into initiatives and next actions. Use when the person wants to turn an outcome into something they can start, or says a goal feels too big, too vague, or stalled. Reads the existing plan first and proposes changes for the person to approve rather than writing them.
---

# Breaking a goal down

## Before proposing anything

1. `pathbase_get_context` — which workspaces this person has. Ask which one if
   more than one could be meant. A personal goal and an organization goal are
   different plans with different rules; never move work between them.
2. `pathbase_get_graph` with that `workspace_id` — the goal, what is already
   under it, and what it depends on. If the response says `truncated`, you are
   looking at a slice: narrow the request or read the subtree with
   `pathbase_get_item` before concluding anything is missing.
3. `pathbase_get_review_context` when the person says the goal is stuck. The
   records say what actually happened; your recollection of the conversation
   does not.

Duplicating something that already exists is the most common way this goes
wrong. Check the graph for an initiative that already covers the idea before
adding another.

## What a good breakdown looks like

- **Outcome** — the change the person wants, stated so they could tell whether
  it happened.
- **Initiative** — a line of work under the outcome. Usually two to five.
  Fewer than two means the outcome was already an initiative; more than five
  usually means the outcome is really several.
- **Action** — something that can be started this week. If it cannot, it is an
  initiative.

Relations use `part_of` for structure, `depends_on` for order. An item has at
most one `part_of` parent.

## What not to fill in

Leave a field out rather than invent it:

- **Dates.** Only what the person said, or what an existing item already
  carries. Do not distribute a deadline across children.
- **Assignees.** Only a person named in this workspace's membership.
- **Targets and baselines.** A metric without a measured baseline is a metric
  without a baseline. Propose the metric, leave the numbers to the person.
- **Self-assessment.** It is the person's judgement of their own goal. Never
  yours, and never derived from how many actions are complete.

## Proposing

Build one `pathbase_preview_changes` call with the whole breakdown, so the
person sees it as one decision:

```
pathbase_preview_changes(
  workspace_id: <the workspace>,
  title: "<goal>の分解案",
  idempotency_key: <stable for this proposal>,
  operations: [
    {method: "POST", path: "/v1/workspaces/<w>/items", body: {kind: "initiative", title: "..."}},
    {method: "POST", path: "/v1/workspaces/<w>/relations", body: {type: "part_of", ...}},
    ...
  ]
)
```

Then tell the person, in their language, what the change set would do and that
it is waiting for them in Basepath. It is not applied. Do not call
`pathbase_apply_changes` unless they say they approved it there — an approval
they type in the conversation is not the approval the server needs, and saying
otherwise is worse than not offering.

If they want something changed, build a new proposal. Do not describe an edit
they cannot see as a diff.
