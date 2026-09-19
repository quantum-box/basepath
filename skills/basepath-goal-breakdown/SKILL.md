---
name: basepath-goal-breakdown
description: Break a goal in Basepath down until something can be started, however many levels that takes. Use when the person wants to turn an outcome into work they can begin, says a goal feels too big, too vague or stalled, or asks to reorganise a goal that already has work under it. Reads the existing plan first, asks about what it cannot know, and proposes changes for the person to approve rather than writing them.
---

# Breaking a goal down

## Read the brief first

`pathbase_get_breakdown_brief(workspace_id, item_id)` before anything else. It
returns the goal, what is already under it, its metrics, and `questions` — the
things to ask the person instead of deciding.

**Ask them.** A goal with no deadline and no way of being measured can be
broken down into something that looks finished and means nothing. A plausible
answer to "when is this due" is worse than no answer: once it is approved as
part of a larger change, it reads as something they decided.

`context_kind` comes from the workspace, not from you. A personal plan and an
organization plan are different plans with different rules; never carry work,
memory or context between them.

## Then read what is there

- `pathbase_get_breakdown(workspace_id, item_id)` — what already sits under the
  goal. `has_more_children` on a node means there is more below it than this
  response carried; call again with that node as `item_id`. `truncated` means
  the response stopped early. Do not conclude something is missing from a
  response that told you it was partial.
- `pathbase_get_breakdown_gaps(workspace_id)` — where the plan does not reach
  an action. The second kind, `no_action_beneath`, is the one that hides: the
  plan looks complete and nothing in it can be started.
- `pathbase_get_review_context` when the person says the goal is stuck. The
  records say what happened; your recollection of the conversation does not.

## Going down a level at a time

There is no fixed number of levels. A ten-year goal and a two-week goal are
both correct, and the structure follows the work rather than a template.

Break down **one level**, show it, and go further only where the person's
answer or the goal's own shape calls for it. A whole tree produced in one pass
is a tree nobody read: they approve the top two rows and inherit the rest.

Stop when something can be started this week. If it cannot be started, it is
not an action yet — break it down again. If several "actions" are each ten
minutes of the same thing, propose one that covers them; a plan made of
fragments is as unusable as one made of abstractions.

MECE is not the goal. Overlap is normal in real work. What is worth saying out
loud is a **duplicate** (two branches doing the same thing), a **hole** (an
outcome nothing below it reaches), an **unresolved dependency**, and a branch
whose pieces are wildly different sizes.

## Re-breaking-down something that already has work under it

`pathbase_compare_breakdown(workspace_id, item_id, children)` before proposing.
It puts your candidates next to what exists and answers keep / change / add,
and anything already there that your list does not mention comes back as
`remove_candidate`.

A remove candidate is a question for the person. Work already underway is not
deleted because you did not think of it, and proposing its deletion because it
was absent from your list is the same mistake with extra steps.

## What you must not decide

Leave it out and ask instead:

- **Dates.** Only what the person said, or what an item already carries. Never
  distribute a deadline across children.
- **Owners.** Only someone named in this workspace's membership, and only when
  the person said so.
- **Targets and baselines.** Propose the metric; leave the numbers. A target
  with no measured baseline is a number with nothing behind it.
- **Self-assessment.** It is the person's judgement of their own goal. Never
  yours, and never derived from how many actions are complete.

The server enforces this: an operation that sets a date, a start, a schedule,
an owner, a self-assessment, a target or a baseline is **refused** unless that
operation carries `basis` saying where the value came from. If you cannot write
the sentence, you do not have the value — ask for it.

## Proposing

One `pathbase_propose_plan` call with the whole level, so the person sees it as
one decision:

```
pathbase_propose_plan(
  workspace_id: <the workspace>,
  title: "<goal>の分解案",
  idempotency_key: <stable for this proposal>,
  assumptions: ["<what you assumed, in their language>"],
  operations: [
    {method: "POST", path: "/v1/workspaces/<w>/items",
     body: {kind: "initiative", title: "..."}},
    {method: "POST", path: "/v1/workspaces/<w>/relations",
     body: {type: "part_of", source_id: "...", target_id: "...",
            rationale: "なぜこれがこの目標の一部なのか"}},
    ...
  ]
)
```

Put a `rationale` on every structural relation. It is what the person reads
months later when they ask why an action exists, and it is the only place that
answer is kept. An empty one is honest when nobody said; an invented one is
not, because it reads afterwards as something someone wrote down.

`assumptions` carries your reasoning next to the diff. Approving a breakdown is
agreeing to the thinking as much as to the rows.

Then tell the person, in their language, what the change set would do and that
it is waiting for them in Basepath. It is **not applied**, and approving it
there is what applies it — so once they say they have, it is already in their
plan and there is nothing further to call. An approval they type in the
conversation is not the approval the server needs, and saying otherwise is
worse than not offering.

If they want something changed, build a new proposal. Do not describe an edit
they cannot see as a diff.
