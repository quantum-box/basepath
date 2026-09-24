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

export type ChangeInterpretationStatus =
  | "decided"
  | "considering"
  | "hypothesis"
  | "suggested"
  | "question"
  | "conflict";

export type ChangeInterpretation = {
  status: ChangeInterpretationStatus;
  origin: "person" | "assistant" | "inference";
  sourceRef: string;
  sourceUrl: string;
  speaker: string;
  quote: string;
  at: string;
  reason: string;
};

export type BranchImpact = {
  descendantCount: number;
  actionCount: number;
  dependencyCount: number;
  contributionCount: number;
  items: { id: string; title: string; kind: string; depth: number }[];
  truncated: boolean;
};

export type ChangeRow = {
  id: string;
  title: string;
  effect: ChangeEffect;
  collection: string;
  method: string;
  path: string;
  fields: FieldChange[];
  /**
   * Values in this row that will be read afterwards as commitments — a date,
   * an owner, a target. Named so a person can look at those first.
   */
  guardedValues: string[];
  /** Where those values came from, in the proposer's words. */
  basis: string[];
  /** Why this operation matches an existing item or is genuinely new. */
  matchRationale: string[];
  /** Ordinary edits collapse to a net row; history-producing operations stay separate. */
  steps: number;
  interpretations: ChangeInterpretation[];
  /** Exact active subtree context captured on both sides of the preview. */
  impact: { before: BranchImpact; after: BranchImpact } | null;
  beforeSnapshot: unknown;
  afterSnapshot: unknown;
};

export type ChangeSet = {
  id: string;
  workspaceId: string;
  title: string;
  status: "pending" | "approved" | "applied" | "rejected" | string;
  /** Digest of the content. An approval is bound to this exact value. */
  hash: string;
  conversationId: string | null;
  approvedBy: string | null;
  approvedAt: string | null;
  rejectedBy: string | null;
  appliedAt: string | null;
  proposedBy: string | null;
  proposedByConnection: string | null;
  createdAt: string;
  expiresAt: string;
  /**
   * Whether this proposal falls inside a range the person set in Basepath in
   * advance, and could therefore be reflected from the conversation.
   *
   * The server answers this; the app never decides it. What the app does with
   * the answer is offer the right control instead of a button that would fail
   * — and a button that is present is still only a trigger. The evidence is
   * the range, which only the person could have written.
   */
  autoApplyEligible: boolean;
  /** Applied under such a range rather than approved one at a time. */
  autoApplied: boolean;
  /** Which range it was applied under, for reading the history afterwards. */
  autoApplyRule: string | null;
  /**
   * Where to approve it, as the server built it.
   *
   * Preferred over composing one locally: a host that renders nothing still
   * passes this to the model, so the same URL is what the person is told
   * whether or not they can see this view at all.
   */
  approvalUrl: string | null;
  rows: ChangeRow[];
  /**
   * What the proposer assumed, in their own words.
   *
   * Approving a breakdown is agreeing to the reasoning as much as to the rows,
   * and reasoning that is not shown is reasoning that is not agreed to.
   */
  assumptions: string[];
};

type Unknown = Record<string, unknown>;

const text = (value: unknown, fallback = "") =>
  typeof value === "string" ? value : fallback;

function object(value: unknown): Unknown | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Unknown)
    : null;
}

const INTERPRETATION_STATUSES = new Set<ChangeInterpretationStatus>([
  "decided",
  "considering",
  "hypothesis",
  "suggested",
  "question",
  "conflict",
]);

function interpretationFrom(value: unknown): ChangeInterpretation | null {
  const source = object(value);
  if (
    !source ||
    typeof source.status !== "string" ||
    !INTERPRETATION_STATUSES.has(source.status as ChangeInterpretationStatus) ||
    !["person", "assistant", "inference"].includes(text(source.origin))
  ) {
    return null;
  }
  return {
    status: source.status as ChangeInterpretationStatus,
    origin: source.origin as ChangeInterpretation["origin"],
    sourceRef: text(source.source_ref),
    sourceUrl: text(source.source_url),
    speaker: text(source.speaker),
    quote: text(source.quote),
    at: text(source.at),
    reason: text(source.reason),
  };
}

function branchImpactFrom(value: unknown): BranchImpact | null {
  const source = object(value);
  if (!source) return null;
  const items = Array.isArray(source.items)
    ? source.items.flatMap((entry) => {
        const item = object(entry);
        if (!item || typeof item.id !== "string") return [];
        return [
          {
            id: item.id,
            title: text(item.title, "（無題）"),
            kind: text(item.kind),
            depth: typeof item.depth === "number" ? item.depth : 0,
          },
        ];
      })
    : [];
  return {
    descendantCount:
      typeof source.descendant_count === "number" ? source.descendant_count : 0,
    actionCount:
      typeof source.action_count === "number" ? source.action_count : 0,
    dependencyCount:
      typeof source.dependency_count === "number" ? source.dependency_count : 0,
    contributionCount:
      typeof source.contribution_count === "number"
        ? source.contribution_count
        : 0,
    items,
    truncated: source.truncated === true,
  };
}

function impactFrom(value: unknown): ChangeRow["impact"] {
  const source = object(value);
  if (!source) return null;
  const before = branchImpactFrom(source.before);
  const after = branchImpactFrom(source.after);
  return before && after ? { before, after } : null;
}

function unique(values: string[]): string[] {
  return [...new Set(values.map((value) => value.trim()).filter(Boolean))];
}

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

function relationChanges(change: Unknown): FieldChange[] {
  const delta = change.relation_delta as Unknown | null | undefined;
  if (!delta) return [];

  const parentSnapshot = (snapshot: unknown): Unknown | null =>
    snapshot && typeof snapshot === "object" ? (snapshot as Unknown) : null;
  if (delta.type === "relation") {
    const beforeRelation = parentSnapshot(delta.before);
    const afterRelation = parentSnapshot(delta.after);
    const relationLabel = (relation: Unknown | null) => {
      if (!relation) return "（関連なし）";
      const source =
        text(relation.source_title) || text(relation.source_id, "（不明）");
      const type = text(relation.relation_type, "関連");
      const target =
        text(relation.target_title) || text(relation.target_id, "（不明）");
      return `${source} — ${type} → ${target}`;
    };
    return [
      {
        field: "関連",
        before: relationLabel(beforeRelation),
        after: relationLabel(afterRelation),
      },
    ];
  }
  if (delta.type !== "part_of") return [];

  const beforeParent = parentSnapshot(delta.before);
  const afterParent = parentSnapshot(delta.after);
  const beforeId = text(beforeParent?.target_id);
  const afterId = text(afterParent?.target_id);
  if (beforeId === afterId) {
    const beforePosition = beforeParent?.position;
    const afterPosition = afterParent?.position;
    if (display(beforePosition) === display(afterPosition)) return [];
    const positionLabel = (value: unknown) =>
      typeof value === "number" ? `${value + 1}番目` : "順序指定なし";
    return [
      {
        field: "並び順",
        before: positionLabel(beforePosition),
        after: positionLabel(afterPosition),
      },
    ];
  }

  const parentTitle = (parent: Unknown | null) =>
    parent
      ? text(parent.parent_title) || text(parent.target_id, "（不明）")
      : "（親なし）";
  let before = parentTitle(beforeParent);
  let after = parentTitle(afterParent);
  if (beforeParent && afterParent && before === after) {
    before = `${before}（${beforeId}）`;
    after = `${after}（${afterId}）`;
  }
  return [{ field: "親項目", before, after }];
}

function rowFromChange(change: Unknown): ChangeRow {
  const interpretation = interpretationFrom(change.interpretation);
  return {
    id: text(change.id),
    title: text(change.title, "（無題）"),
    effect: text(change.effect, "unknown") as ChangeEffect,
    collection: text(change.collection),
    method: text(change.method),
    path: text(change.path),
    fields: [
      ...(change.effect === "deleted"
        ? []
        : fieldChanges(change.before, change.after)),
      ...relationChanges(change),
    ],
    guardedValues: Array.isArray(change.guarded_values)
      ? (change.guarded_values as unknown[]).filter(
          (value): value is string => typeof value === "string",
        )
      : [],
    basis: unique([text(change.basis)]),
    matchRationale: unique([text(change.match_rationale)]),
    steps: 1,
    interpretations: interpretation ? [interpretation] : [],
    impact: impactFrom(change.impact),
    beforeSnapshot: change.before ?? null,
    afterSnapshot: change.after ?? null,
  };
}

function effectBetween(before: unknown, after: unknown): ChangeEffect {
  const hadBefore = object(before) !== null;
  const hasAfter = object(after) !== null;
  if (!hadBefore && hasAfter) return "created";
  if (hadBefore && !hasAfter) return "deleted";
  if (hadBefore && hasAfter) return "updated";
  return "unknown";
}

/** Combine repeated edits to one stable item into its net before/after state. */
function aggregateRows(rows: ChangeRow[]): ChangeRow[] {
  const groups = new Map<string, ChangeRow>();
  rows.forEach((row, index) => {
    const preservesSideEffects =
      /\/actions\/[^/]+\/(?:complete|reopen|skip)$/.test(row.path) ||
      /\/items\/[^/]+\/reparent$/.test(row.path);
    const key = row.id && !preservesSideEffects
      ? `${row.collection}:${row.id}`
      : `${row.collection}:row-${index}`;
    const existing = groups.get(key);
    if (!existing) {
      groups.set(key, row);
      return;
    }

    const fields = new Map<string, FieldChange>();
    for (const field of existing.fields) fields.set(field.field, field);
    for (const field of row.fields) {
      const prior = fields.get(field.field);
      fields.set(field.field, {
        field: field.field,
        before: prior?.before ?? field.before,
        after: field.after,
      });
    }
    const mergedFields = [...fields.values()].filter(
      (field) => field.before !== field.after,
    );
    const beforeSnapshot = existing.beforeSnapshot;
    const afterSnapshot = row.afterSnapshot;
    const interpretations = new Map<string, ChangeInterpretation>();
    for (const interpretation of [
      ...existing.interpretations,
      ...row.interpretations,
    ]) {
      interpretations.set(JSON.stringify(interpretation), interpretation);
    }
    const impact =
      existing.impact || row.impact
        ? {
            before: existing.impact?.before ?? row.impact!.before,
            after: row.impact?.after ?? existing.impact!.after,
          }
        : null;
    groups.set(key, {
      ...existing,
      title: row.title || existing.title,
      effect: effectBetween(beforeSnapshot, afterSnapshot),
      method: existing.method === row.method ? row.method : "複数操作",
      fields: mergedFields,
      guardedValues: unique([...existing.guardedValues, ...row.guardedValues]),
      basis: unique([...existing.basis, ...row.basis]),
      matchRationale: unique([
        ...existing.matchRationale,
        ...row.matchRationale,
      ]),
      steps: existing.steps + row.steps,
      interpretations: [...interpretations.values()],
      impact,
      beforeSnapshot,
      afterSnapshot,
    });
  });
  return [...groups.values()];
}

export function changeSetFrom(value: unknown): ChangeSet | null {
  const source = value as Unknown | undefined;
  // Recognised by what a person needs in order to act on it, not by having an
  // id. Every other record has an id too, and a tool result is read here
  // without knowing which tool produced it.
  if (
    !source ||
    typeof source.id !== "string" ||
    typeof source.status !== "string" ||
    typeof source.workspace_id !== "string" ||
    !Array.isArray(source.changes)
  ) {
    return null;
  }
  const rows = Array.isArray(source.changes)
    ? aggregateRows((source.changes as Unknown[]).map(rowFromChange))
    : [];
  return {
    id: text(source.id),
    workspaceId: text(source.workspace_id),
    title: text(source.title, "計画の変更案"),
    status: text(source.status, "pending"),
    hash: text(source.hash),
    conversationId:
      typeof source.conversation_id === "string"
        ? source.conversation_id
        : null,
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
    autoApplyEligible: source.auto_apply_eligible === true,
    autoApplied: source.auto_applied === true,
    autoApplyRule:
      typeof source.auto_apply_rule === "string"
        ? source.auto_apply_rule
        : null,
    approvalUrl:
      typeof source.approval_url === "string" ? source.approval_url : null,
    rows,
    assumptions: Array.isArray(source.assumptions)
      ? (source.assumptions as unknown[]).filter(
          (line): line is string => typeof line === "string",
        )
      : [],
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
  // The server's own link wins when there is one: it is the same string the
  // model is given for a host that renders nothing, and two places composing
  // the same URL is two places to get it wrong.
  if (change.approvalUrl) return change.approvalUrl;
  const root = base.replace(/\/$/, "");
  return `${root}/changes/${encodeURIComponent(change.workspaceId)}/${encodeURIComponent(change.id)}`;
}
