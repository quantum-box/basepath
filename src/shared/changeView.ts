/**
 * Reading a change set the way a person needs to see it.
 *
 * The server records what each proposed operation would do, captured while it
 * was actually doing it inside a rolled-back savepoint. This turns that record
 * into rows a person can check: what is added, what is changed and to what,
 * and what is removed.
 */

export type ChangeEffect = "created" | "updated" | "deleted" | "unknown";

export type FieldChange = { field: string; before: string; after: string };

export type ChangeRow = {
  id: string;
  title: string;
  effect: ChangeEffect;
  collection: string;
  method: string;
  fields: FieldChange[];
};

export type ChangeSet = {
  id: string;
  workspaceId: string;
  title: string;
  status: "pending" | "approved" | "applied" | "rejected" | string;
  /** Digest of the content. An approval is bound to this exact value. */
  hash: string;
  approvedBy: string | null;
  approvedAt: string | null;
  rejectedBy: string | null;
  appliedAt: string | null;
  proposedBy: string | null;
  proposedByConnection: string | null;
  createdAt: string;
  expiresAt: string;
  rows: ChangeRow[];
};

type Unknown = Record<string, unknown>;

const text = (value: unknown, fallback = "") =>
  typeof value === "string" ? value : fallback;

/** Fields worth showing a person. Bookkeeping is noise in a review. */
const REVIEWABLE = [
  ["title", "タイトル"],
  ["description", "説明"],
  ["state", "状態"],
  ["kind", "種類"],
  ["start_date", "開始日"],
  ["due_date", "期限"],
  ["scheduled_date", "予定日"],
  ["scheduled_time", "予定時刻"],
  ["body", "本文"],
  ["value", "値"],
  ["unit", "単位"],
  ["name", "名前"],
  ["target", "目標値"],
  ["baseline", "基準値"],
  // A weekly review has no title of its own; these are what it says.
  ["week_start", "対象週"],
  ["learnings", "学び"],
  ["challenges", "課題"],
  ["next_focus", "次週の重点"],
  // A check-in: the status, the judgement, and the words behind both.
  ["health", "状況"],
  ["self_assessment", "自己評価"],
  ["comment", "コメント"],
  ["results", "成果"],
  ["blockers", "課題"],
] as const;

const FIELD_LABELS = new Map<string, string>(
  REVIEWABLE.map(([field, label]) => [field, label]),
);

function display(value: unknown): string {
  if (value === null || value === undefined || value === "") return "—";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return JSON.stringify(value);
}

function fieldChanges(before: unknown, after: unknown): FieldChange[] {
  const start = (before as Unknown | null) ?? {};
  const end = (after as Unknown | null) ?? {};
  const changes: FieldChange[] = [];
  for (const [field] of REVIEWABLE) {
    const from = start[field];
    const to = end[field];
    if (display(from) === display(to)) continue;
    changes.push({
      field: FIELD_LABELS.get(field) ?? field,
      before: display(from),
      after: display(to),
    });
  }
  return changes;
}

export function changeSetFrom(value: unknown): ChangeSet | null {
  const source = value as Unknown | undefined;
  if (!source || typeof source.id !== "string") return null;
  const rows = Array.isArray(source.changes)
    ? (source.changes as Unknown[]).map((change) => ({
        id: text(change.id),
        title: text(change.title, "（無題）"),
        effect: text(change.effect, "unknown") as ChangeEffect,
        collection: text(change.collection),
        method: text(change.method),
        fields:
          change.effect === "deleted"
            ? []
            : fieldChanges(change.before, change.after),
      }))
    : [];
  return {
    id: text(source.id),
    workspaceId: text(source.workspace_id),
    title: text(source.title, "計画の変更案"),
    status: text(source.status, "pending"),
    hash: text(source.hash),
    approvedBy:
      typeof source.approved_by === "string" ? source.approved_by : null,
    approvedAt:
      typeof source.approved_at === "string" ? source.approved_at : null,
    rejectedBy:
      typeof source.rejected_by === "string" ? source.rejected_by : null,
    appliedAt: typeof source.applied_at === "string" ? source.applied_at : null,
    proposedBy: typeof source.actor === "string" ? source.actor : null,
    proposedByConnection:
      typeof source.proposed_by_connection === "string"
        ? source.proposed_by_connection
        : null,
    createdAt: text(source.created_at),
    expiresAt: text(source.expires_at),
    rows,
  };
}

export function changeSetsFrom(value: unknown): ChangeSet[] {
  const items = (value as Unknown | undefined)?.items;
  if (!Array.isArray(items)) return [];
  return items
    .map(changeSetFrom)
    .filter((change): change is ChangeSet => change !== null);
}

export function isExpired(change: ChangeSet, now = new Date()): boolean {
  if (!change.expiresAt) return false;
  const expiry = new Date(change.expiresAt);
  return !Number.isNaN(expiry.getTime()) && expiry.getTime() <= now.getTime();
}

/** Counts by effect, for a one-line summary above the detail. */
export function summarize(change: ChangeSet) {
  const counts = { created: 0, updated: 0, deleted: 0, unknown: 0 };
  for (const row of change.rows) counts[row.effect] += 1;
  return counts;
}

/**
 * Where a person approves this change set.
 *
 * Always Basepath's own origin: approval needs the person's own session, and
 * a call made from inside an AI host cannot supply one.
 */
export function approvalUrl(base: string, change: ChangeSet): string {
  const root = base.replace(/\/$/, "");
  return `${root}/changes/${encodeURIComponent(change.workspaceId)}/${encodeURIComponent(change.id)}`;
}
