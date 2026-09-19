/**
 * A plan, however many levels deep it happens to be.
 *
 * Nothing in this file names a level. There is no `Goal | Initiative |
 * Milestone | Action` union deciding what may sit under what, because a
 * person with a ten-year goal and a person with a two-week one are both
 * right, and a type that picks between them is wrong for one of them.
 *
 * What it does keep apart is **structure** from **contribution**. `part_of`
 * is where something sits and has one answer; `contributes_to` is what it
 * helps and has many. Drawing the second as if it were the first would turn
 * one plan into several overlapping ones.
 */

export type BreakdownLink = {
  id: string;
  rationale: string;
};

export type BreakdownNode = {
  id: string;
  kind: string;
  title: string;
  state: string;
  dueDate: string | null;
  archivedAt: string | null;
  /** Distance below the root of this response, not a level in a hierarchy. */
  depth: number;
  parentId: string | null;
  /** Why it is part of its parent, as recorded on the link. May be empty. */
  rationale: string;
  childCount: number;
  loadedChildren: number;
  /** There is more below than this response carries. */
  hasMoreChildren: boolean;
  contributesTo: BreakdownLink[];
  dependsOn: BreakdownLink[];
};

export type Breakdown = {
  rootId: string;
  rootTitle: string;
  nodes: BreakdownNode[];
  depth: number;
  /** The response stopped early. Said, never inferred from a short list. */
  truncated: boolean;
};

type Unknown = Record<string, unknown>;

const asString = (value: unknown, fallback = ""): string =>
  typeof value === "string" ? value : fallback;

const asNumber = (value: unknown): number =>
  typeof value === "number" ? value : 0;

const asLinks = (value: unknown): BreakdownLink[] =>
  Array.isArray(value)
    ? value.map((entry) => ({
        id: asString((entry as Unknown)?.id),
        rationale: asString((entry as Unknown)?.rationale),
      }))
    : [];

export function breakdownFrom(value: unknown): Breakdown | null {
  const source = value as Unknown | undefined;
  if (!source || !Array.isArray(source.nodes)) return null;
  const root = (source.root ?? {}) as Unknown;
  return {
    rootId: asString(root.id),
    rootTitle: asString(root.title),
    depth: asNumber(source.depth),
    truncated: source.truncated === true,
    nodes: (source.nodes as Unknown[]).map((entry) => ({
      id: asString(entry.id),
      kind: asString(entry.kind),
      title: asString(entry.title),
      state: asString(entry.state),
      dueDate: typeof entry.due_date === "string" ? entry.due_date : null,
      archivedAt:
        typeof entry.archived_at === "string" ? entry.archived_at : null,
      depth: asNumber(entry.depth),
      parentId: typeof entry.parent_id === "string" ? entry.parent_id : null,
      rationale: asString(entry.rationale),
      childCount: asNumber(entry.child_count),
      loadedChildren: asNumber(entry.loaded_children),
      hasMoreChildren: entry.has_more_children === true,
      contributesTo: asLinks(entry.contributes_to),
      dependsOn: asLinks(entry.depends_on),
    })),
  };
}

export type Ancestor = {
  id: string;
  title: string;
  kind: string;
  /** Which lower item the rationale explains. */
  childId: string;
  rationale: string;
};

export type Rationale = {
  itemId: string;
  itemTitle: string;
  /** Top first, so it reads as an explanation rather than a traversal. */
  ancestors: Ancestor[];
  contributesTo: { id: string; title: string; rationale: string }[];
  dependsOn: { id: string; title: string; rationale: string }[];
  topLevel: boolean;
};

export function rationaleFrom(value: unknown): Rationale | null {
  const source = value as Unknown | undefined;
  if (!source || !Array.isArray(source.ancestors)) return null;
  const item = (source.item ?? {}) as Unknown;
  const named = (list: unknown) =>
    Array.isArray(list)
      ? list.map((entry) => ({
          id: asString((entry as Unknown).id),
          title: asString((entry as Unknown).title),
          rationale: asString((entry as Unknown).rationale),
        }))
      : [];
  return {
    itemId: asString(item.id),
    itemTitle: asString(item.title),
    topLevel: source.top_level === true,
    ancestors: (source.ancestors as Unknown[]).map((entry) => ({
      id: asString(entry.id),
      title: asString(entry.title),
      kind: asString(entry.kind),
      childId: asString(entry.child_id),
      rationale: asString(entry.rationale),
    })),
    contributesTo: named(source.contributes_to),
    dependsOn: named(source.depends_on),
  };
}

export type Gap = {
  itemId: string;
  title: string;
  kind: string;
  gap: string;
  detail: string;
  targetId: string | null;
};

export function gapsFrom(value: unknown): Gap[] {
  const source = value as Unknown | undefined;
  if (!source || !Array.isArray(source.gaps)) return [];
  return (source.gaps as Unknown[]).map((entry) => ({
    itemId: asString(entry.item_id),
    title: asString(entry.title),
    kind: asString(entry.kind),
    gap: asString(entry.gap),
    detail: asString(entry.detail),
    targetId: typeof entry.target_id === "string" ? entry.target_id : null,
  }));
}

/**
 * The tree the nodes describe, as a list of children per parent.
 *
 * The response is flat and each node names its parent, so the shape is
 * rebuilt here rather than being sent twice.
 */
export function childrenOf(breakdown: Breakdown): Map<string, BreakdownNode[]> {
  const byParent = new Map<string, BreakdownNode[]>();
  for (const node of breakdown.nodes) {
    if (node.id === breakdown.rootId) continue;
    const key = node.parentId ?? "";
    const list = byParent.get(key) ?? [];
    list.push(node);
    byParent.set(key, list);
  }
  return byParent;
}

/** Plain words for a kind, without asserting it is a fixed level. */
export function kindLabel(kind: string): string {
  const labels: Record<string, string> = {
    outcome: "目標",
    initiative: "取り組み",
    milestone: "節目",
    action: "行動",
    idea: "アイデア",
  };
  return labels[kind] ?? kind;
}

/** What a gap is, in words a person can act on. */
export function gapLabel(gap: string): string {
  const labels: Record<string, string> = {
    not_broken_down: "分解されていません",
    no_action_beneath: "実行できる行動まで降りていません",
    dangling_dependency: "待っている相手が見当たりません",
  };
  return labels[gap] ?? gap;
}
