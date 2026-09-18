/**
 * Reading one week of a plan the way a person needs to see it.
 *
 * The server has already aggregated the week against the workspace's timezone:
 * which occurrences were completed, skipped or left incomplete, what each goal
 * was assessed at, and what each metric last measured with its delta and
 * staleness. Nothing here recomputes any of that. Re-deriving an aggregate in
 * the UI is how two surfaces end up disagreeing about the same week, and the
 * conversation surface would be the one quietly wrong.
 *
 * What this file does add is the distinction the numbers depend on:
 *
 * - **Unmeasured is not zero.** A metric with no observation reads as
 *   unmeasured, and a week with no planned action has no completion rate.
 *   Turning either into `0` invents a measurement.
 * - **Completing actions is not achieving a goal.** The two live in separate
 *   fields here and are never combined into one score.
 */

import { weekBounds } from "./viewModel";

export type WeeklyActionStatus = "completed" | "skipped" | "incomplete";

export type WeeklyAction = {
  itemId: string;
  title: string;
  date: string;
  status: WeeklyActionStatus;
  /** The record that evidences the status, when one exists. */
  recordId: string | null;
  /** Who it counted for. Null when nobody is assigned. */
  actor: string | null;
};

export type WeeklyGoal = {
  itemId: string;
  title: string;
  /** Percent, or null when the person has not assessed it. Never 0 by default. */
  selfAssessment: number | null;
  assessedAt: string | null;
};

export type WeeklyMetricStatus = "current" | "stale" | "unmeasured";

export type WeeklyMetric = {
  metricId: string;
  itemId: string;
  name: string;
  unit: string;
  /** Null means unmeasured, which is not the same as measured zero. */
  latest: number | null;
  previous: number | null;
  /** Null when either side is missing; a delta needs two measurements. */
  delta: number | null;
  status: WeeklyMetricStatus;
  latestObservationId: string | null;
};

export type WeeklyMember = {
  actor: string;
  role: string;
  completed: number;
  skipped: number;
  incomplete: number;
};

export type WeeklyDraft = {
  id: string;
  status: "draft" | "finalized";
  revision: number;
  /** Required to save over this draft without discarding someone else's edit. */
  version: number;
  learnings: string;
  challenges: string;
  nextFocus: string;
  author: string;
  updatedAt: string;
  finalizedAt: string | null;
};

export type WeeklyReviewView = {
  workspaceId: string;
  /** The week boundary is this workspace's, not the viewer's device's. */
  timezone: string;
  weekStart: string;
  weekEnd: string;
  totals: {
    total: number;
    completed: number;
    skipped: number;
    incomplete: number;
  };
  /** Percent of planned occurrences completed, or null when nothing was planned. */
  completionRate: number | null;
  actions: WeeklyAction[];
  goals: WeeklyGoal[];
  metrics: WeeklyMetric[];
  /** Only the members the viewer is already allowed to see. */
  members: WeeklyMember[];
  /** The newest revision for this week, draft or finalized. */
  review: WeeklyDraft | null;
  /** Every revision, oldest first, so a correction keeps its history. */
  history: WeeklyDraft[];
  /** Nothing was planned, assessed or measured in this week. */
  empty: boolean;
};

type Unknown = Record<string, unknown>;

const asArray = (value: unknown): Unknown[] =>
  Array.isArray(value) ? (value as Unknown[]) : [];

const asString = (value: unknown, fallback = ""): string =>
  typeof value === "string" ? value : fallback;

/** Null rather than 0: a missing number must not read as a measured one. */
const asNumber = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

const asCount = (value: unknown): number => asNumber(value) ?? 0;

function draftFrom(value: unknown): WeeklyDraft | null {
  const source = value as Unknown | undefined;
  if (!source || typeof source.id !== "string") return null;
  return {
    id: source.id,
    status:
      asString(source.status, "draft") === "finalized" ? "finalized" : "draft",
    revision: asCount(source.revision) || 1,
    version: asCount(source.version) || 1,
    learnings: asString(source.learnings),
    challenges: asString(source.challenges),
    nextFocus: asString(source.next_focus),
    author: asString(source.author),
    updatedAt: asString(source.updated_at),
    finalizedAt:
      typeof source.finalized_at === "string" ? source.finalized_at : null,
  };
}

/** True when a payload looks like a weekly summary rather than some other tool result. */
export function isWeeklyReview(value: unknown): boolean {
  const source = value as Unknown | undefined;
  return (
    !!source &&
    typeof source.week_start === "string" &&
    typeof source.actions === "object" &&
    source.actions !== null
  );
}

export function weeklyReviewFrom(value: unknown): WeeklyReviewView | null {
  if (!isWeeklyReview(value)) return null;
  const source = value as Unknown;
  const actions = (source.actions as Unknown) ?? {};
  const totals = {
    total: asCount(actions.total),
    completed: asCount(actions.completed),
    skipped: asCount(actions.skipped),
    incomplete: asCount(actions.incomplete),
  };
  const history = asArray(source.history)
    .map(draftFrom)
    .filter((entry): entry is WeeklyDraft => entry !== null);
  const goals = asArray(source.goals).map((goal) => ({
    itemId: asString(goal.item_id),
    title: asString(goal.title),
    selfAssessment: asNumber(goal.self_assessment),
    assessedAt: typeof goal.assessed_at === "string" ? goal.assessed_at : null,
  }));
  const metrics = asArray(source.metrics).map((metric) => {
    const status = asString(metric.status, "unmeasured");
    return {
      metricId: asString(metric.metric_id),
      itemId: asString(metric.item_id),
      name: asString(metric.name),
      unit: asString(metric.unit),
      latest: asNumber(metric.latest),
      previous: asNumber(metric.previous),
      delta: asNumber(metric.delta),
      status: (status === "current" || status === "stale"
        ? status
        : "unmeasured") as WeeklyMetricStatus,
      latestObservationId:
        typeof metric.latest_observation_id === "string"
          ? metric.latest_observation_id
          : null,
    };
  });
  return {
    workspaceId: asString(source.workspace_id),
    timezone: asString(source.timezone, "Asia/Tokyo"),
    weekStart: asString(source.week_start),
    weekEnd: asString(source.week_end),
    totals,
    // A rate over nothing is not 0%: there was nothing to complete.
    completionRate: totals.total
      ? Math.round((totals.completed / totals.total) * 100)
      : null,
    actions: asArray(actions.items).map((item) => {
      const status = asString(item.status, "incomplete");
      return {
        itemId: asString(item.item_id),
        title: asString(item.title),
        date: asString(item.date),
        status: (status === "completed" || status === "skipped"
          ? status
          : "incomplete") as WeeklyActionStatus,
        recordId: typeof item.record_id === "string" ? item.record_id : null,
        actor: typeof item.actor === "string" ? item.actor : null,
      };
    }),
    goals,
    metrics,
    members: asArray(source.members).map((member) => ({
      actor: asString(member.actor),
      role: asString(member.role),
      completed: asCount(member.completed),
      skipped: asCount(member.skipped),
      incomplete: asCount(member.incomplete),
    })),
    review: draftFrom(source.review) ?? history[history.length - 1] ?? null,
    history,
    empty: totals.total === 0 && goals.length === 0 && metrics.length === 0,
  };
}

/** The Monday the server requires for `week_start`, for any date in that week. */
export function mondayOf(date: string): string {
  return weekBounds(date).start;
}

/** The Monday `weeks` weeks away, for stepping through the review history. */
export function shiftWeek(weekStart: string, weeks: number): string {
  const parsed = new Date(`${weekStart}T00:00:00Z`);
  if (Number.isNaN(parsed.getTime())) return weekStart;
  parsed.setUTCDate(parsed.getUTCDate() + weeks * 7);
  return mondayOf(parsed.toISOString().slice(0, 10));
}

/** What the person typed, as the review endpoint expects it. */
export type WeeklyDraftInput = {
  learnings: string;
  challenges: string;
  nextFocus: string;
};

export const emptyDraftInput: WeeklyDraftInput = {
  learnings: "",
  challenges: "",
  nextFocus: "",
};

export function draftInputFrom(review: WeeklyDraft | null): WeeklyDraftInput {
  if (!review) return emptyDraftInput;
  return {
    learnings: review.learnings,
    challenges: review.challenges,
    nextFocus: review.nextFocus,
  };
}

export function hasDraftText(input: WeeklyDraftInput): boolean {
  return Boolean(
    input.learnings.trim() || input.challenges.trim() || input.nextFocus.trim(),
  );
}

export function draftDiffers(
  input: WeeklyDraftInput,
  review: WeeklyDraft | null,
): boolean {
  const saved = draftInputFrom(review);
  return (
    input.learnings !== saved.learnings ||
    input.challenges !== saved.challenges ||
    input.nextFocus !== saved.nextFocus
  );
}

/**
 * The request body for saving this draft.
 *
 * `expected_version` is sent only when a draft already exists, because that is
 * exactly when a save could overwrite someone else's edit. Finalized revisions
 * are never edited in place: a correction starts a new revision.
 */
export function draftBody(
  weekStart: string,
  input: WeeklyDraftInput,
  review: WeeklyDraft | null,
): Record<string, unknown> {
  return {
    week_start: weekStart,
    learnings: input.learnings,
    challenges: input.challenges,
    next_focus: input.nextFocus,
    ...(review && review.status === "draft"
      ? { expected_version: review.version }
      : {}),
  };
}

/**
 * A short stable digest of what the person typed.
 *
 * Used as part of an idempotency key: resending the same text must not create
 * a second proposal, but edited text is a different proposal and has to be
 * able to reach the person.
 */
export function draftKey(input: WeeklyDraftInput): string {
  const text = [input.learnings, input.challenges, input.nextFocus].join(
    "\u0000",
  );
  let hash = 5381;
  for (let index = 0; index < text.length; index += 1) {
    hash = ((hash * 33) ^ text.charCodeAt(index)) >>> 0;
  }
  return hash.toString(16);
}
