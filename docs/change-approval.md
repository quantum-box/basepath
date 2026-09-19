# Change approval — where a proposal becomes a plan

An AI can propose. Only a person can approve. This document is the boundary
between the two, and the threat model it is built to.

## The problem a picture does not solve

MCP Apps let Basepath render a change set inside the conversation, which is a
real improvement: a person sees the diff where they are already reading. It is
tempting to put an "Approve" button there too.

That button cannot be trusted, and not because the host is untrustworthy. When
an app calls a tool, the call reaches this server **on the same connection,
with the same access token, in the same shape** as a call the model made. There
is no field in it the server can check that the model could not also set.

- `_meta.ui.visibility: ["app"]` is a hint the *host* honours when deciding
  what to show the model. It is not a signal the server receives.
- A header, a `_meta` entry, or an `approved: true` argument is chosen by the
  caller.
- "The host asked the user to allow this tool" is consent to a *class of
  calls*, not approval of *this specific change*.

So the server does not try to tell them apart. It assumes every MCP call might
be the model's, and designs accordingly.

## Where approval happens

**In Basepath, on Basepath's own origin, with the person's own session.**

`/changes/{workspace}/{id}` is a deep-linkable screen showing the same diff the
conversation showed. Approving there sends a request carrying the session
cookie, the same-origin CSRF header, and the digest of the content that was
rendered. All three are things an AI host cannot supply.

The conversation's job is to get someone there with context: it shows the diff,
says plainly that approving happens in Basepath, and offers the link.

**Approving is applying.** The operations run in the same transaction as the
approval, and the change set comes back as `applied`. There is no second
button, and no state where someone approved something that is not in their
plan.

That was not always true, and the reason it changed is worth keeping. The two
steps exist so an *agent* can only apply what the person approved — the server
checks `approved_hash` and `approved_by`. What they were never for was making
the **person** act twice. A person who has read the diff and pressed approve
has decided; a further button between that decision and their plan is one they
forget, and the proposal then expires having written nothing. That happened
(PLT-4916): a goal was approved in Basepath and never existed.

So the guarantee is unchanged and its cost moved: an agent still cannot apply
anything a person has not approved, and a person is no longer asked to confirm
a decision they have already made.

## What each surface may do

| Action | AI host (MCP App) | Basepath |
| --- | --- | --- |
| See the diff | yes | yes |
| Propose / re-propose | yes | yes |
| Withdraw a proposal | yes | yes |
| **Approve** (which applies) | **no** | yes |
| Apply a change set approved before approving applied | yes, if *that person* approved it | yes |

Withdrawing is allowed from the app because it only discards a proposal:
nothing is applied, and anyone can propose again.

Applying is allowed from the app because the server checks that the change set
was approved **by this same actor**, with a digest matching its current
content, before the expiry, against an unchanged plan. The app cannot cause an
apply the person did not already authorize.

In practice there is now nothing for it to apply: the approval did that. The
path remains for change sets approved before this was so, and an apply that
arrives for one already applied by this same person returns the change set with
`already_applied: true` rather than an error — a model relaying "I approved it"
should not report a failure about something that is in the plan.

## Deciding in advance

A person can say, in Basepath, that proposals of a particular shape from a
particular AI client may be reflected without being asked again. `auto_apply.rs`
holds the rule and `api/tests/auto_apply.rs` is written from the attacker's
side, the same way this file's table is.

This is **not** an exception to anything above, and the distinction is worth
being exact about, because it is the only place an apply proceeds without a
per-change approval.

The argument against a button in the app is that the server cannot attribute
the click. That argument is untouched. What a range changes is *when* the
person decides, not *where*: the row is written on Basepath's origin, with
their session and the same-origin CSRF header — the identical evidence an
approval carries. Applying under it is the server reading a decision they
already made, in the one place it can read decisions. Approval moved from
one件ずつ to 範囲ごと; the boundary did not move.

So the app may now show a trigger, and the trigger proves nothing. The range
does. The server re-reads it on every apply, which is why revoking takes effect
on the next call rather than the next session.

### What a range cannot reach

| | |
| --- | --- |
| A deletion | Never. There is no column for it. A proposal containing one falls outside every range that can be expressed |
| `due_date` / `start_date` / `scheduled_date` / `assignee_id` / `self_assessment` / `target` / `baseline` | Only if the person turned that on for that one range, as a separate decision. These read afterwards as things they decided |
| Another workspace, or another AI client | Never. Both are part of the key, and the proposal must have arrived on the connection now applying it |
| Part of a proposal | Never. A range covers every operation or none: applying the covered half leaves a plan nobody described |
| Forever | Never. Every range expires, at most 90 days out |
| An AI connection reading or writing one | Never. `GET` is a 404 — not a 403, which would confirm there is something to widen — and a write gets the same refusal every agent write gets, so the answer carries no information either way |

### Telling the two apart afterwards

`approved_by` stays null on an auto-applied change set, and `auto_applied` and
`auto_apply_rule` are set instead. The trail has to answer "did they approve
this one, or had they already decided about this kind?" and it can only do that
if the two are not written into the same field. Both appear in the change list
in Basepath, and the diff stays readable either way: "気づいたら変わっていた"
is the failure this is built not to cause.

The revoked range is kept rather than deleted, so a change set applied under it
can still name what it was applied under.

### Weekly review text

A proposal may also carry the week's review text, as
`POST /v1/workspaces/{w}/weekly-reviews/draft`. It is the same contract: the
app writes a draft nobody has saved, the person reads the diff in Basepath —
their own words on the left, the proposed ones on the right — and approves it
there.

Finalizing a week is **not** proposable. Approving a change set means agreeing
to the text, which is a different statement from declaring the week reviewed,
so `.../weekly-reviews/{id}/finalize` is refused inside a change set
(`422 VALIDATION_ERROR`) and stays something the person does in Basepath. The
draft carries `expected_version` when one already exists, so a proposal written
against an older draft is refused at approval with `409 VERSION_CONFLICT`
rather than overwriting what the person wrote in the meantime — while they are
still in front of the screen to read the refusal.

## What the server refuses

`api/tests/approval.rs` is written from the attacker's side. Each of these is a
test:

| Attempt | Refusal |
| --- | --- |
| Apply a change set nobody approved | `403 APPROVAL_REQUIRED` |
| Approve as the agent actor | `403` |
| `approved: true` / `approved_by` / `status: approved` / `approved_hash` in the body | `403` or `422` |
| `PATCH` the stored change set to say it is approved | `403` |
| Approve content other than what was shown (`hash` mismatch) | `409 CHANGESET_SUPERSEDED` |
| Apply someone else's approval | `403` / `404` |
| Use a change-set id from another workspace | `404` |
| Approve or apply after the plan changed underneath | `409 VERSION_CONFLICT` |
| Approve or apply after the proposal was withdrawn | `409` / `403 APPROVAL_REQUIRED` |
| Approve or apply an expired proposal | `409 VERSION_CONFLICT` |
| Approve an already-applied change set | `409 VERSION_CONFLICT` |
| Apply someone else's already-applied change set | refused, not `already_applied` |
| Propose finalizing a weekly review | `422 VALIDATION_ERROR` |
| Approve a review draft written against an older version | `409 VERSION_CONFLICT` |
| Re-send the same approval (double click, retry) | the stored result, written once |
| Apply what this person's approval already applied | the change set, `already_applied: true`, nothing written |
| Apply with no range, or one that does not cover every operation | `403 APPROVAL_REQUIRED` |
| Apply a deletion under the widest range that can be saved | `403 APPROVAL_REQUIRED` |
| Apply a proposal that arrived on another connection, under this one's range | `403 APPROVAL_REQUIRED` |
| Apply under a range revoked or expired since the proposal | `403 APPROVAL_REQUIRED` |
| `auto_applied` / `auto_apply_rule` / `auto_apply_eligible` in a proposal body | `422` |
| Read or create a range from an AI connection | `404` / the blanket agent-write refusal |

And one more, which is the point of the whole design: **previewing does not
change the plan.** The operations run inside a savepoint that is rolled back;
what survives is the description of what they did.

## Seeing the diff where the decision is made

The conversation shows the change set. That is the point of the whole MCP Apps
surface, and for a while it did not happen: the tools that *create* a change set
carried no UI resource, so a host had nothing to render. The model answered in
prose, the person never saw the diff, and the proposal expired
(PLT-4943). Every change tool now names a view, in both the MCP Apps and the
OpenAI Apps SDK spelling, because a host reads one or the other and not both.

A host that renders nothing must still not be a dead end. Every change set a
tool returns carries `approval_url` — the absolute Basepath link — and a
sentence saying what to do with it, so the model has somewhere to send the
person even when there is no view at all. "It told me to approve in Basepath
and did not say where" is what that field exists to prevent.

Whether a given host draws the view is now recorded rather than argued:
reading a `ui://` resource stamps `mcp_connections.ui_read_at`, and 設定 →
AIクライアントの接続 shows it per connection. A host reads that resource only in
order to draw it, so the timestamp separates "this client does not support it"
from "it does, and something else went wrong".

## The diff a person sees

The description is captured while the operations actually run, because that is
the only moment both the state before an operation and the state after it
exist, in order, without touching the live plan. Each row carries:

- the effect — created, updated or deleted, derived from whether the target
  existed before and after, not from the HTTP method;
- the item's title;
- the fields that changed, before and after.

A deletion is labelled a deletion and marked, because "this proposal removes
something" is the thing most worth not missing.

## Audit

Every write records the actor, the origin (`ui` or `mcp`) and — for MCP — the
connection id. So for one change set the trail answers: which AI connection
proposed it, and that the person approved it in the browser — which is also
where it was written, so `applied_by_connection` is null for an approval made
in Basepath rather than naming the connection the proposal arrived on. The same
history is visible in Basepath and through `pathbase_get_change`.

## Recovering from a lost connection

Status is always re-readable: `pathbase_get_change` and the approval screen both
report `pending` / `approved` / `applied` / `rejected`, with `approved_at` and
`applied_at`. A client that lost its connection mid-approval re-reads rather
than guessing, and re-sending the same idempotency key returns the original
result instead of writing twice.

`approved` now only appears on change sets approved before approving applied.
They are shown as 承認済み・未反映 and, once past their expiry, as expired with
an invitation to propose again — an approval that was never written does not
disappear quietly.
