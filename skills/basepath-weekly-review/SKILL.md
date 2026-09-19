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

### Show it, and give them the link

Every change tool opens Basepath's view, so where the host renders it the diff
appears on its own — say what you assumed and what you want checked, not what
the rows already say.

Where the host renders nothing, the diff is still in the tool result and so is
`approval_url`. **Give them that URL.** "Approve it in Basepath" without the
link leaves them nothing to click, and a proposal expires in thirty minutes.
That is not hypothetical; it is why this paragraph exists.

`auto_apply_eligible: true` means the person already decided, in Basepath, that
changes of this shape may be reflected without being asked again. Say it is
inside a range they set — not that you have permission — and reflect it with
`pathbase_apply_changes`. If that is refused, the range is gone or narrower
than it was: nothing was written, and the way on is their approval screen.

Even a range they set in advance cannot finalize a week: the range covers text
they agreed could be written, and declaring the week reviewed is a different
statement.

## Corrections

A finalized review is corrected by starting a new revision, never by editing
the old one. What the earlier version said stays readable.
