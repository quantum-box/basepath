/**
 * Check-ins and the history they leave.
 *
 * A check-in history is only worth having if it says what was believed at the
 * time. So a correction is a new entry that supersedes an old one, and the old
 * one stays readable — this shapes that into something a screen can show
 * without flattening it into "the current value".
 */

export type CheckinHealth = "on_track" | "at_risk" | "off_track";

export type Checkin = {
  id: string;
  itemId: string;
  /** Absent when the check-in recorded only words. */
  health: CheckinHealth | null;
  selfAssessment: number | null;
  comment: string;
  results: string;
  blockers: string;
  nextFocus: string;
  /** The measurements it was written against. Values are not copied here. */
  observationIds: string[];
  author: string;
  createdAt: string;
  /** The check-in this corrects. */
  supersedesId: string | null;
  /** True for the one that currently stands. */
  standing: boolean;
  /** True once something later has superseded it. */
  superseded: boolean;
};

export type TimelineEvent = {
  at: string;
  kind: string;
  actor: string | null;
  summary: string;
  ref: string | null;
  origin: string | null;
  connection: string | null;
};

export type Timeline = {
  itemId: string;
  title: string;
  /** The moment this was replayed to, when it was. */
  asOf: string | null;
  events: TimelineEvent[];
  /** What the goal said at that moment. All absent when nobody had spoken. */
  state: {
    health: CheckinHealth | null;
    selfAssessment: number | null;
    checkinId: string | null;
    checkedInAt: string | null;
    checkedInBy: string | null;
  };
};

export type ReviewEntry = {
  id: string;
  title: string;
  ownerLabel: string;
  health: CheckinHealth | null;
  lastCheckinAt: string | null;
  lastCheckinBy: string | null;
  blockers: string;
  nextFocus: string;
};

export type ReviewQueue = {
  workspaceId: string;
  staleDays: number;
  /** Nobody has ever said anything. Silence, not a warning. */
  neverCheckedIn: ReviewEntry[];
  /** Spoken about once, but not lately. */
  stale: ReviewEntry[];
  /** Somebody said these are in trouble. */
  atRisk: ReviewEntry[];
  recentlyUpdated: ReviewEntry[];
};

type Unknown = Record<string, unknown>;

const asString = (value: unknown, fallback = ""): string =>
  typeof value === "string" ? value : fallback;

const asNumber = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

const asStrings = (value: unknown): string[] =>
  Array.isArray(value)
    ? value.filter((v): v is string => typeof v === "string")
    : [];

function healthFrom(value: unknown): CheckinHealth | null {
  const status = asString(value);
  return ["on_track", "at_risk", "off_track"].includes(status)
    ? (status as CheckinHealth)
    : null;
}

export function checkinsFrom(value: unknown): Checkin[] {
  const source = value as Unknown | undefined;
  const listed = Array.isArray(source?.items)
    ? (source.items as Unknown[])
    : [];
  const standingId = asString(source?.standing_id);
  const supersededIds = new Set(
    listed
      .map((entry) => asString(entry.supersedes_id))
      .filter((id) => id.length > 0),
  );
  return listed.map((entry) => {
    const id = asString(entry.id);
    return {
      id,
      itemId: asString(entry.item_id),
      health: healthFrom(entry.health),
      selfAssessment: asNumber(entry.self_assessment),
      comment: asString(entry.comment),
      results: asString(entry.results),
      blockers: asString(entry.blockers),
      nextFocus: asString(entry.next_focus),
      observationIds: asStrings(entry.observation_ids),
      author: asString(entry.author),
      createdAt: asString(entry.created_at),
      supersedesId:
        typeof entry.supersedes_id === "string" ? entry.supersedes_id : null,
      standing: id === standingId,
      superseded: supersededIds.has(id),
    };
  });
}

export function timelineFrom(value: unknown): Timeline | null {
  const source = value as Unknown | undefined;
  if (!source || typeof source.item_id !== "string") return null;
  const state = (source.state as Unknown | undefined) ?? {};
  return {
    itemId: source.item_id,
    title: asString(source.title),
    asOf: typeof source.as_of === "string" ? source.as_of : null,
    events: (Array.isArray(source.events)
      ? (source.events as Unknown[])
      : []
    ).map((event) => ({
      at: asString(event.at),
      kind: asString(event.kind, "changed"),
      actor: typeof event.actor === "string" ? event.actor : null,
      summary: asString(event.summary),
      ref: typeof event.ref === "string" ? event.ref : null,
      origin: typeof event.origin === "string" ? event.origin : null,
      connection:
        typeof event.connection === "string" ? event.connection : null,
    })),
    state: {
      health: healthFrom(state.health),
      selfAssessment: asNumber(state.self_assessment),
      checkinId: typeof state.checkin_id === "string" ? state.checkin_id : null,
      checkedInAt:
        typeof state.checked_in_at === "string" ? state.checked_in_at : null,
      checkedInBy:
        typeof state.checked_in_by === "string" ? state.checked_in_by : null,
    },
  };
}

function entryFrom(value: unknown): ReviewEntry {
  const source = (value as Unknown | undefined) ?? {};
  const owner = source.owner as Unknown | null | undefined;
  return {
    id: asString(source.id),
    title: asString(source.title),
    ownerLabel: !owner
      ? "担当なし"
      : owner.kind === "organization"
        ? "組織"
        : asString(owner.id, "担当なし"),
    health: healthFrom(source.health),
    lastCheckinAt:
      typeof source.last_checkin_at === "string"
        ? source.last_checkin_at
        : null,
    lastCheckinBy:
      typeof source.last_checkin_by === "string"
        ? source.last_checkin_by
        : null,
    blockers: asString(source.blockers),
    nextFocus: asString(source.next_focus),
  };
}

export function reviewQueueFrom(value: unknown): ReviewQueue | null {
  const source = value as Unknown | undefined;
  if (!source || typeof source.workspace_id !== "string") return null;
  const list = (key: string) =>
    (Array.isArray(source[key]) ? (source[key] as unknown[]) : []).map(
      entryFrom,
    );
  return {
    workspaceId: source.workspace_id,
    staleDays: asNumber(source.stale_days) ?? 14,
    neverCheckedIn: list("never_checked_in"),
    stale: list("stale"),
    atRisk: list("at_risk"),
    recentlyUpdated: list("recently_updated"),
  };
}

const EVENT_LABELS: Record<string, string> = {
  created: "作成",
  carried_over: "前の期間から引き継ぎ",
  checkin: "チェックイン",
  checkin_correction: "チェックインの訂正",
  observation: "観測",
  observation_correction: "観測の訂正",
  alignment_changed: "つながりの変更",
  edited: "編集",
  record_note: "メモ",
  record_learning: "学び",
  record_review: "振り返り",
  record_checkin: "記録",
  record_completion: "実施",
  record_skip: "見送り",
};

export function eventLabel(kind: string): string {
  return EVENT_LABELS[kind] ?? kind;
}

export function healthLabel(status: CheckinHealth | null): string {
  if (status === "on_track") return "順調";
  if (status === "at_risk") return "注意";
  if (status === "off_track") return "遅れ";
  return "未記入";
}

/** How many entries a review needs to look at, kept as separate counts. */
export function reviewCounts(queue: ReviewQueue) {
  return {
    silent: queue.neverCheckedIn.length,
    stale: queue.stale.length,
    atRisk: queue.atRisk.length,
    updated: queue.recentlyUpdated.length,
  };
}
