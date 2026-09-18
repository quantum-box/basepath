---
name: basepath-weekly-review
description: Run a weekly review from Basepath's own aggregation — completion, goal self-assessment, metric deltas and stale observations — and draft the learnings for the person to approve. Use at the end of a week, or when they ask how a week went.
---

# Reviewing a week

## The numbers are already computed

`pathbase_get_weekly_review` with `workspace_id` and a `week_start` that is a
**Monday** in the workspace's timezone. It returns completion counts, each
goal's self-assessment, and each metric's latest value with its delta and
staleness, aggregated for that workspace's own week.

Report those numbers. Do not recompute them from the rows you can see: the
response lists occurrences for context, and the totals cover the whole week. A
number you derive yourself will disagree with the one in Basepath's own screen,
and yours will be the wrong one.

## What the fields mean

| Field | Meaning | What it is not |
| --- | --- | --- |
| `latest: null` | never measured | not 0 |
| `delta: null` | nothing to compare with | not "no change" |
| `status: "stale"` | the last observation is over two weeks old | not current |
| `self_assessment: null` | the person has not assessed it | not 0%, not derived from completions |
| `actions.total: 0` | nothing was planned | not a 0% week |

When nothing was planned, say nothing was planned. A completion rate over an
empty week does not exist.

## Drafting the review

Write the learnings, challenges and next focus in three separable registers,
and mark which is which:

- **観測 (observed)** — only what the summary and the records say.
- **推測 (inferred)** — your reading of it, stated as a reading.
- **質問 (question)** — what you would need to know to be less uncertain.

Do not blend them. A guess laid out as a finding is the failure mode this
format exists to prevent.

## Saving it

The review text is proposed like any other change:

```
pathbase_preview_changes(
  workspace_id: <w>,
  title: "<week_start>の週次レビュー案",
  idempotency_key: <stable for this text>,
  operations: [{
    method: "POST",
    path: "/v1/workspaces/<w>/weekly-reviews/draft",
    body: {week_start, learnings, challenges, next_focus, expected_version?}
  }]
)
```

Include `expected_version` when the summary already shows a draft, so a
proposal written against an older version is refused rather than overwriting
what the person wrote in the meantime.

**Finalizing is not proposable.** Marking a week reviewed is the person's act,
in Basepath. A change set cannot contain it and the server refuses one that
tries. Never tell the person a week is complete because they approved the text.

## Corrections

A finalized review is corrected by starting a new revision, never by editing
the old one. What the earlier version said stays readable.
