/**
 * Which planning period this is, read the way a person asks it.
 *
 * The server decides what the periods are and which one today falls in — it
 * knows the workspace's timezone, and "this quarter" is a question about the
 * workspace's calendar, not the viewer's device. This shapes that answer and
 * adds the one thing a screen needs that a server response does not carry:
 * how to describe a period in a sentence.
 *
 * A workspace with no periods is the ordinary case, not a missing setup. It
 * reads as "no period", and nothing here suggests creating one.
 */

export type CycleCadence = "quarter" | "month" | "week" | "custom";
export type CycleStatus = "planned" | "active" | "closed";

export type PlanningCycle = {
  id: string;
  cadence: CycleCadence;
  label: string;
  /** Inclusive, in the workspace's own local dates. */
  startDate: string;
  endDate: string;
  status: CycleStatus;
  /** The period this one continues, when it was created as the next one. */
  previousId: string | null;
  /** How many live items belong to it. */
  itemCount: number;
  /** Required to change the period without discarding someone else's edit. */
  version: number;
};

export type PlanningView = {
  workspaceId: string;
  timezone: string;
  /** Today, in the workspace's timezone. */
  today: string;
  cycles: PlanningCycle[];
  /** The period being looked at: today's, or the one that was asked for. */
  current: PlanningCycle | null;
  previous: PlanningCycle | null;
  next: PlanningCycle | null;
  /** Work that belongs to no period. Allowed, but a period view would hide it. */
  unassignedItems: number;
  /** The person's own words from their last finalized review. Not a plan. */
  lastReview: {
    weekStart: string;
    nextFocus: string;
    learnings: string;
    challenges: string;
  } | null;
  /** True when this workspace has never used periods. */
  unused: boolean;
};

type Unknown = Record<string, unknown>;

const asString = (value: unknown, fallback = ""): string =>
  typeof value === "string" ? value : fallback;

const asCount = (value: unknown): number =>
  typeof value === "number" && Number.isFinite(value) ? value : 0;

function cycleFrom(value: unknown, itemCount = 0): PlanningCycle | null {
  const source = value as Unknown | undefined;
  if (!source || typeof source.id !== "string") return null;
  const cadence = asString(source.cadence, "custom");
  const status = asString(source.status, "planned");
  return {
    id: source.id,
    cadence: (["quarter", "month", "week", "custom"].includes(cadence)
      ? cadence
      : "custom") as CycleCadence,
    label: asString(source.label, source.id),
    startDate: asString(source.start_date),
    endDate: asString(source.end_date),
    status: (["planned", "active", "closed"].includes(status)
      ? status
      : "planned") as CycleStatus,
    previousId:
      typeof source.previous_id === "string" ? source.previous_id : null,
    itemCount,
    version: asCount(source.version) || 1,
  };
}

/** One of `current` / `previous` / `next`, which carry their own item count. */
function slotFrom(value: unknown): PlanningCycle | null {
  const source = value as Unknown | undefined;
  if (!source) return null;
  return cycleFrom(source.cycle, asCount(source.item_count));
}

export function planningViewFrom(value: unknown): PlanningView | null {
  const source = value as Unknown | undefined;
  if (!source || typeof source.workspace_id !== "string") return null;
  const listed = Array.isArray(source.cycles) ? source.cycles : [];
  const cycles = listed
    .map((cycle) => cycleFrom(cycle))
    .filter((cycle): cycle is PlanningCycle => cycle !== null);
  const review = source.last_finalized_review as Unknown | null | undefined;
  return {
    workspaceId: source.workspace_id,
    timezone: asString(source.timezone, "Asia/Tokyo"),
    today: asString(source.today),
    cycles,
    current: slotFrom(source.current),
    previous: slotFrom(source.previous),
    next: slotFrom(source.next),
    unassignedItems: asCount(source.unassigned_items),
    lastReview: review
      ? {
          weekStart: asString(review.week_start),
          nextFocus: asString(review.next_focus),
          learnings: asString(review.learnings),
          challenges: asString(review.challenges),
        }
      : null,
    unused: cycles.length === 0,
  };
}

const CADENCE_LABELS: Record<CycleCadence, string> = {
  quarter: "四半期",
  month: "月次",
  week: "週次",
  custom: "任意期間",
};

export function cadenceLabel(cadence: CycleCadence): string {
  return CADENCE_LABELS[cadence];
}

export function statusLabel(status: CycleStatus): string {
  if (status === "active") return "進行中";
  if (status === "closed") return "終了";
  return "予定";
}

/** `2026-10-01〜2026-12-31`, which is how the period is actually bounded. */
export function spanLabel(cycle: PlanningCycle): string {
  return `${cycle.startDate}〜${cycle.endDate}`;
}

/**
 * Where today sits in the period, as a percentage.
 *
 * Null outside the period, and null for a period of no length, because a
 * progress bar over neither is a number nobody asked for. This is elapsed
 * time, never progress on the work — the two are different claims and the
 * screen must not let one stand in for the other.
 */
export function elapsedPercent(
  cycle: PlanningCycle,
  today: string,
): number | null {
  const start = Date.parse(`${cycle.startDate}T00:00:00Z`);
  const end = Date.parse(`${cycle.endDate}T00:00:00Z`);
  const now = Date.parse(`${today}T00:00:00Z`);
  if ([start, end, now].some(Number.isNaN)) return null;
  if (now < start || now > end || end < start) return null;
  const days = (end - start) / 86_400_000 + 1;
  if (days <= 0) return null;
  return Math.round((((now - start) / 86_400_000 + 1) / days) * 100);
}

/** The next period's start, for proposing one that continues this. */
export function dayAfter(date: string): string {
  const parsed = new Date(`${date}T00:00:00Z`);
  if (Number.isNaN(parsed.getTime())) return date;
  parsed.setUTCDate(parsed.getUTCDate() + 1);
  return parsed.toISOString().slice(0, 10);
}

/**
 * The body that creates the period after this one.
 *
 * Only the cadence and the start are decided here, because only those follow
 * from "the next one". A custom period has nothing to follow from, so it
 * carries no derived name and the caller has to supply one.
 */
export function nextCycleBody(cycle: PlanningCycle): Record<string, unknown> {
  return {
    cadence: cycle.cadence,
    start_date: dayAfter(cycle.endDate),
    previous_id: cycle.id,
  };
}
