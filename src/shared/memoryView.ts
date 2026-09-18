/**
 * What a person's own Basepath remembers for them.
 *
 * Two distinctions this file exists to keep:
 *
 * - **Confirmed and suggested are different.** A proposal is what an AI
 *   thought; it is not something the person said, and it never renders as one.
 * - **No longer current is not wrong.** A preference from two jobs ago was
 *   true then. Superseding or expiring it takes it out of "what stands now",
 *   not out of the history.
 */

export type MemoryKind =
  "fact" | "preference" | "decision" | "learning" | "context" | "episode";

export type MemoryStatus = "verified" | "proposed";

export type Memory = {
  id: string;
  kind: MemoryKind;
  title: string;
  body: string;
  status: MemoryStatus;
  source: string;
  evidenceIds: string[];
  observedAt: string | null;
  validFrom: string | null;
  validTo: string | null;
  /** Only a proposal carries one. */
  confidence: number | null;
  supersedesId: string | null;
  archivedAt: string | null;
  /** Kept, but never handed to an AI. */
  excludedFromRetrieval: boolean;
  itemIds: string[];
  topics: string[];
  people: string[];
  author: string;
  createdAt: string;
  version: number;
  /** True once something later has superseded it. */
  superseded: boolean;
};

export type MemoryList = {
  memories: Memory[];
  supersededIds: string[];
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

const KINDS: MemoryKind[] = [
  "fact",
  "preference",
  "decision",
  "learning",
  "context",
  "episode",
];

export function memoryListFrom(value: unknown): MemoryList | null {
  const source = value as Unknown | undefined;
  if (!source || !Array.isArray(source.items)) return null;
  const superseded = new Set(asStrings(source.superseded_ids));
  return {
    supersededIds: [...superseded],
    memories: (source.items as Unknown[]).map((entry) => {
      const kind = asString(entry.kind);
      const status = asString(entry.status, "proposed");
      const id = asString(entry.id);
      return {
        id,
        // An unknown kind reads as context rather than as a fact: the one
        // direction this must never guess in is toward "the person said so".
        kind: (KINDS.includes(kind as MemoryKind)
          ? kind
          : "context") as MemoryKind,
        title: asString(entry.title),
        body: asString(entry.body),
        status: (status === "verified"
          ? "verified"
          : "proposed") as MemoryStatus,
        source: asString(entry.source),
        evidenceIds: asStrings(entry.evidence_ids),
        observedAt:
          typeof entry.observed_at === "string" ? entry.observed_at : null,
        validFrom:
          typeof entry.valid_from === "string" ? entry.valid_from : null,
        validTo: typeof entry.valid_to === "string" ? entry.valid_to : null,
        confidence: asNumber(entry.confidence),
        supersedesId:
          typeof entry.supersedes_id === "string" ? entry.supersedes_id : null,
        archivedAt:
          typeof entry.archived_at === "string" ? entry.archived_at : null,
        excludedFromRetrieval: entry.excluded_from_retrieval === true,
        itemIds: asStrings(entry.item_ids),
        topics: asStrings(entry.topics),
        people: asStrings(entry.people),
        author: asString(entry.author),
        createdAt: asString(entry.created_at),
        version: asNumber(entry.version) ?? 1,
        superseded: superseded.has(id),
      };
    }),
  };
}

export type DuplicateGroup = {
  id: string;
  title: string;
  kind: MemoryKind;
  similar: { id: string; title: string; overlap: number }[];
};

export function duplicateGroupsFrom(value: unknown): DuplicateGroup[] {
  const source = value as Unknown | undefined;
  const groups = Array.isArray(source?.groups)
    ? (source.groups as Unknown[])
    : [];
  return groups.map((group) => ({
    id: asString(group.id),
    title: asString(group.title),
    kind: asString(group.kind, "context") as MemoryKind,
    similar: (Array.isArray(group.similar)
      ? (group.similar as Unknown[])
      : []
    ).map((entry) => ({
      id: asString(entry.id),
      title: asString(entry.title),
      overlap: asNumber(entry.overlap) ?? 0,
    })),
  }));
}

const KIND_LABELS: Record<MemoryKind, string> = {
  fact: "事実",
  preference: "好み",
  decision: "決定",
  learning: "学び",
  context: "背景",
  episode: "出来事",
};

export function kindLabel(kind: MemoryKind): string {
  return KIND_LABELS[kind];
}

export function kinds(): MemoryKind[] {
  return [...KINDS];
}

export function statusLabel(status: MemoryStatus): string {
  return status === "verified" ? "本人が確認" : "AIの候補";
}

/**
 * Why this memory is not part of "what stands now".
 *
 * Null when it is. Never the word "wrong": a preference that expired was true
 * when it was recorded, and saying otherwise rewrites the person's past.
 */
export function inactiveReason(
  memory: Memory,
  now = new Date(),
): string | null {
  if (memory.archivedAt) return "整理済み";
  if (memory.superseded) return "更新済み";
  const stamp = now.toISOString();
  if (memory.validTo && memory.validTo < stamp) return "この期間は過ぎました";
  if (memory.validFrom && memory.validFrom > stamp) return "まだ先の期間です";
  return null;
}

/** What still stands, for a screen that wants only that. */
export function current(list: MemoryList, now = new Date()): Memory[] {
  return list.memories.filter((memory) => inactiveReason(memory, now) === null);
}

/** Counts by kind, so a screen can say what is in here without listing it. */
export function countsByKind(list: MemoryList): Record<MemoryKind, number> {
  const counts = Object.fromEntries(KINDS.map((kind) => [kind, 0])) as Record<
    MemoryKind,
    number
  >;
  for (const memory of list.memories) counts[memory.kind] += 1;
  return counts;
}
