---
name: basepath-record-progress
description: Record what actually happened in Basepath — an action completed or skipped, a note or learning, a measured value. Use when the person reports progress in conversation. Records evidence faithfully and never turns a report into a claim about a goal.
---

# Recording what happened

## The distinction this skill exists for

Completing actions is not achieving a goal. Basepath keeps them apart on
purpose, and so must anything written here:

- **A completion** says an occurrence happened on a date.
- **A self-assessment** is the person's judgement of an outcome. Only they set
  it. Never derive it from completions.
- **An observation** is a measured value with a unit, a time, and where it came
  from. If it was not measured, there is no observation to record.

## Completing an action

`pathbase_complete_action` with `workspace_id`, `item_id`,
`expected_version`, the `local_date` in the workspace's timezone, and a stable
`idempotency_key`. Read the item first — `pathbase_get_today` or
`pathbase_get_item` — so the version is the real one and the occurrence is the
one the person means.

Report the result as what it is: a proposal awaiting their approval in
Basepath. Not "done".

## Notes and learnings

`pathbase_record_checkin` for a note, a learning or a check-in. Write what the
person said. Where you add something they did not say, mark it:

> 観測: 火・木・土に実施。
> 推測: 移動のある日に抜けている可能性。
> 質問: 朝に動かせますか。

A record is evidence. Rewriting what someone told you into a tidier claim
destroys the only thing it was for.

## Measurements

`pathbase_record_observation` needs a value, the metric's own unit, when it was
observed, and its source. All four come from the person or from a document they
point at.

Never:

- convert an unmeasured value to 0,
- carry last week's number forward as this week's,
- estimate a value and record it without saying so — an estimate is not an
  observation, and there is no field that makes it one.

If a metric has no observation, say it is unmeasured and offer to record one.

## Where it becomes real

Everything above is a proposal. The person approves it in Basepath, on
Basepath's own origin, with their own session. Nothing they type in the
conversation is that approval, and telling them a record is saved when it is
still waiting is the one mistake that makes the whole record useless as
evidence.

### After proposing, show it

A change set is only useful if the person can see it. Every change tool opens
Basepath's view, so in a host that renders it the diff appears on its own and
you do not need to retell it — say what you assumed and what you want them to
check, not what the rows already say.

In a host that renders nothing, the diff is still in the tool result and so is
`approval_url`. **Give them that URL.** Saying "approve it in Basepath" without
it leaves them with nothing to click, and a proposal expires in thirty minutes.
That failure is why this paragraph exists.

If the change set comes back with `auto_apply_eligible: true`, the person has
already decided, in Basepath, that changes of this shape may be reflected
without being asked again. Say that it is inside a range they set — not that
you have permission — and reflect it with `pathbase_apply_changes`. If that is
refused, the range is gone or narrower than it was: nothing was written, and
the answer is their approval screen, not a retry.

## Corrections

A wrong record is superseded, not edited: the new one carries `supersedes_id`
and the original stays. The history of what was believed at the time is part of
the evidence.
