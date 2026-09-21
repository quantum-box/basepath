---
name: basepath-week-planning
description: Plan the coming week against the person's Basepath goals. Use when they ask what to do this week, want to rebalance a week that is too full, or are starting a week and want their existing plan in front of them. Works from what is already scheduled rather than inventing a new week.
---

# Planning a week

## Read the week that already exists

1. `pathbase_get_context` — the workspace and its timezone. The week is the
   **workspace's**, not the device's: `pathbase_get_week` runs Monday to
   Sunday in that timezone, and around midnight the two differ.
2. `pathbase_get_week` with `start` and `end` — what is already scheduled,
   what is due, and which habit occurrences are still open. Also returns
   `unscheduled`: work with no date at all, which a week view would otherwise
   hide.
3. `pathbase_get_graph` — what these actions are *for*. An action nothing
   points at is worth asking about; it is not automatically worth deleting.

Do not propose a plan before reading all three. A week built from the
conversation alone will quietly duplicate what is already there.

## Planning

Work from capacity the person stated, not from a number you chose. If they have
not said how much time they have, ask — one question, then plan.

- Keep what is already scheduled unless the person wants it moved.
- An overfull week is a fact to report, not a problem to solve silently. Say
  what does not fit and let them choose what drops.
- Habits are occurrences, not tasks. Completing one is
  `pathbase_complete_action` with its `local_date`, never an edit to the item.
- Do not assign a date to something the person left undated because they have
  not decided yet. Ask, or leave it in `unscheduled`.

## Proposing

One `pathbase_preview_changes` call for the week, with an
`idempotency_key` that is stable for this plan so a retry does not create a
second copy. Every write carries `expected_version` where the API asks for one;
if it comes back `VERSION_CONFLICT`, the plan changed underneath — re-read and
show the person what is different rather than overwriting it.

The change set is a proposal. The person approves it in Basepath. Say so
plainly, and do not describe the week as planned until they have.

### After proposing, summarize it

A change set is only useful if the person can understand it. The result
contains the diff in `structuredContent` and text: summarize what is different,
say what you assumed and what you want them to check, and give them
`approval_url`. Saying "approve it in Basepath" without the link leaves them
with nothing to click, and a proposal expires in thirty minutes.

If the change set comes back with `auto_apply_eligible: true`, the person has
already decided, in Basepath, that changes of this shape may be reflected
without being asked again. Say that it is inside a range they set — not that
you have permission — and reflect it with `pathbase_apply_changes`. If that is
refused, the range is gone or narrower than it was: nothing was written, and
the answer is their approval screen, not a retry.
