/**
 * The view model every surface renders from.
 *
 * Web and Tauri show the same plan, so the shape of what is shown is defined
 * once here and built from the same server responses. No business rule lives
 * here: the Rust service decides what is true, this decides how to read it.
 */

export type PlanNodeKind =
  | "idea"
  | "outcome"
  | "initiative"
  | "action"
  | "milestone"
  | "criterion"
  | "constraint"
  | "question";

export type ConversationBasis = {
  origin?: string;
  source_ref?: string;
  source_url?: string;
  speaker?: string;
  quote?: string;
  at?: string;
  reason?: string;
  assumptions?: string[];
};

export type PlanRelationship = {
  sourceId: string;
  targetId: string;
  sourceTitle: string;
  targetTitle: string;
  type: string;
  position: number | null;
  rationale: string | null;
  basis: ConversationBasis | null;
};

export type PlanNode = {
  id: string;
  title: string;
  kind: PlanNodeKind;
  state: string;
  dueDate: string | null;
  /** Percent, or null when the person has not assessed it. */
  selfAssessment: number | null;
  /** How the conversation framed this item, separate from plan state. */
  conversationStatus?: string | null;
  detail?: string | null;
  basis?: ConversationBasis | null;
  /** Conversation-draft relations, including their order and evidence. */
  relationships?: PlanRelationship[];
  parentRelation?: PlanRelationship | null;
  children: PlanNode[];
};

export type PlanAction = {
  id: string;
  title: string;
  completed: boolean;
  occurrenceKey: string;
  scheduledTime: string | null;
  assignee: string | null;
  dueDate: string | null;
  /** Needed to propose a change without overwriting someone else's edit. */
  version: number;
};

export type PlanWorkspace = {
  id: string;
  name: string;
  scope: string;
  /** IANA timezone. "Today" and "this week" are this workspace's, not the
   * viewer's device's. */
  timezone: string;
  role: string;
};

/** One day of the week view. */
export type PlanDay = {
  date: string;
  entries: PlanEntry[];
};

export type PlanEntry = {
  id: string;
  title: string;
  /** Why this item appears on this day. */
  label: "start" | "due" | "scheduled" | "habit";
  assignee: string | null;
  scheduledTime: string | null;
  /** For a habit occurrence: completed, skipped or missed. */
  status: string | null;
  occurrenceKey: string | null;
  version: number;
};

export type PlanWeek = {
  start: string;
  end: string;
  timezone: string;
  days: PlanDay[];
  /** Items with no date at all, which a week view would otherwise hide. */
  unscheduled: { id: string; title: string }[];
};

export type PlanView = {
  workspaces: PlanWorkspace[];
  workspace: PlanWorkspace | null;
  nodes: PlanNode[];
  /** True when the graph is a slice of the plan rather than all of it. */
  truncated: boolean;
  limit: number;
  localDate: string;
  actions: PlanAction[];
  week: PlanWeek | null;
};

export const emptyPlanView: PlanView = {
  workspaces: [],
  workspace: null,
  nodes: [],
  truncated: false,
  limit: 0,
  localDate: "",
  actions: [],
  week: null,
};

type Unknown = Record<string, unknown>;

function asArray(value: unknown): Unknown[] {
  return Array.isArray(value) ? (value as Unknown[]) : [];
}

function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

/**
 * Reads the workspace a host tool result belongs to.
 *
 * Most plan reads receive this in the tool input rather than in the payload,
 * while change tools put it on the change set. Keeping this small adapter in
 * the shared view model lets a caller use either shape without guessing
 * from the first workspace in the context list.
 */
export function workspaceIdFrom(value: unknown): string | undefined {
  if (!value || typeof value !== "object") return undefined;
  const source = value as Unknown;
  const direct = asString(source.workspace_id);
  if (direct) return direct;
  for (const key of [
    "arguments",
    "input",
    "changeset",
    "change",
    "target",
    "workspace",
  ]) {
    const nested = workspaceIdFrom(source[key]);
    if (nested) return nested;
  }
  return undefined;
}

export function workspacesFrom(context: unknown): PlanWorkspace[] {
  const source = (context as Unknown | undefined)?.workspaces;
  const workspaces = asArray(source).map((workspace) => ({
    id: asString(workspace.id),
    name: asString(workspace.name, asString(workspace.id)),
    scope: asString(workspace.scope),
    timezone: asString(workspace.timezone, "Asia/Tokyo"),
    role: asString(workspace.role),
  }));
  // Personal is the stable fallback and the first option in the switcher, but
  // an explicit workspace_id always wins. Do not let the server's row order
  // decide which plan a conversation silently shows.
  return workspaces.sort((a, b) => {
    const personal = (workspace: PlanWorkspace) =>
      workspace.id === "personal" || workspace.scope === "個人";
    return Number(personal(b)) - Number(personal(a));
  });
}

/**
 * Today's date *in the workspace's timezone*.
 *
 * A person in Tokyo planning a workspace set to UTC must see that workspace's
 * day, not their device's. Around midnight these differ, which is exactly when
 * getting it wrong is most visible.
 */
export function localDateIn(timezone: string, now = new Date()): string {
  try {
    return new Intl.DateTimeFormat("en-CA", {
      timeZone: timezone,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).format(now);
  } catch {
    // An unknown timezone must not blank the screen.
    return new Intl.DateTimeFormat("en-CA", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).format(now);
  }
}

/** The Monday-to-Sunday week containing `date`, as the server expects it. */
export function weekBounds(date: string): { start: string; end: string } {
  const parsed = new Date(`${date}T00:00:00Z`);
  if (Number.isNaN(parsed.getTime())) return { start: date, end: date };
  const weekday = (parsed.getUTCDay() + 6) % 7; // Monday = 0
  const start = new Date(parsed);
  start.setUTCDate(parsed.getUTCDate() - weekday);
  const end = new Date(start);
  end.setUTCDate(start.getUTCDate() + 6);
  const iso = (value: Date) => value.toISOString().slice(0, 10);
  return { start: iso(start), end: iso(end) };
}

export function weekFrom(calendar: unknown): PlanWeek | null {
  const source = calendar as Unknown | undefined;
  if (!source || !Array.isArray(source.days)) return null;
  return {
    start: asString(source.start),
    end: asString(source.end),
    timezone: asString(source.timezone),
    days: asArray(source.days).map((day) => ({
      date: asString(day.date),
      entries: asArray(day.entries).map((entry) => {
        const item = (entry.item as Unknown | undefined) ?? {};
        const fields = (item.fields as Unknown | undefined) ?? {};
        return {
          id: asString(item.id),
          title: asString(item.title),
          label: asString(entry.label, "scheduled") as PlanEntry["label"],
          assignee:
            typeof fields.assignee_id === "string"
              ? fields.assignee_id
              : typeof fields.assignee === "string"
                ? fields.assignee
                : null,
          scheduledTime:
            typeof item.scheduled_time === "string"
              ? item.scheduled_time
              : null,
          status: typeof entry.status === "string" ? entry.status : null,
          occurrenceKey:
            typeof entry.occurrence_key === "string"
              ? entry.occurrence_key
              : null,
          version: typeof item.version === "number" ? item.version : 1,
        };
      }),
    })),
    unscheduled: asArray(source.unscheduled).map((item) => ({
      id: asString(item.id),
      title: asString(item.title),
    })),
  };
}

/**
 * Builds the goal tree from the flat graph the server returns.
 *
 * `part_of` is the structural relation: a child is part of its target. Nodes
 * whose parent is outside the returned slice stay at the top level rather than
 * disappearing, so a truncated graph never silently hides work.
 */
export function treeFrom(graph: unknown): {
  nodes: PlanNode[];
  truncated: boolean;
  limit: number;
} {
  const source = (graph as Unknown | undefined) ?? {};
  // `pathbase_get_graph` returns items + relations. The focused breakdown
  // tool returns a breadth-first `nodes` list with parent_id instead. Both
  // describe the same tree, so a caller can render either result as a tree
  // rather than falling back to the first workspace's plan.
  const breakdown = !Array.isArray(source.items) && Array.isArray(source.nodes);
  const items = breakdown ? asArray(source.nodes) : asArray(source.items);
  const relations: Unknown[] = breakdown
    ? items
        .filter((item) => asString(item.parent_id))
        .map((item) => ({
          type: "part_of",
          source_id: asString(item.id),
          target_id: asString(item.parent_id),
        }))
    : asArray(source.relations);

  const byId = new Map<string, PlanNode>();
  const itemOrder = new Map<string, number>();
  const partOfPositions = new Map<string, number | null>();
  const isConversationDraft = typeof source.draft_id === "string";
  for (const [index, item] of items.entries()) {
    const fields = (item.fields as Unknown | undefined) ?? {};
    const assessment = fields.self_assessment ?? item.self_assessment;
    const id = asString(item.id);
    itemOrder.set(id, index);
    byId.set(id, {
      id,
      title: asString(item.title),
      kind: asString(item.kind, "outcome") as PlanNodeKind,
      state: asString(item.state, "active"),
      dueDate: typeof item.due_date === "string" ? item.due_date : null,
      selfAssessment: typeof assessment === "number" ? assessment : null,
      conversationStatus:
        typeof item.conversation_status === "string"
          ? item.conversation_status
          : null,
      detail: typeof item.detail === "string" ? item.detail : null,
      basis:
        item.basis && typeof item.basis === "object"
          ? (item.basis as ConversationBasis)
          : null,
      relationships: isConversationDraft ? [] : undefined,
      children: [],
    });
  }

  const attached = new Set<string>();
  for (const relation of relations) {
    const sourceId = asString(relation.source_id);
    const targetId = asString(relation.target_id);
    const child = byId.get(sourceId);
    const parent = byId.get(targetId);
    if (isConversationDraft && child && parent) {
      const relationship: PlanRelationship = {
        sourceId,
        targetId,
        sourceTitle: child.title,
        targetTitle: parent.title,
        type: asString(relation.type),
        position:
          typeof relation.position === "number" ? relation.position : null,
        rationale:
          typeof relation.rationale === "string" ? relation.rationale : null,
        basis:
          relation.basis && typeof relation.basis === "object"
            ? (relation.basis as ConversationBasis)
            : null,
      };
      child.relationships?.push(relationship);
      parent.relationships?.push(relationship);
      if (relationship.type === "part_of") child.parentRelation = relationship;
    }
    if (relation.type !== "part_of") continue;
    partOfPositions.set(
      sourceId,
      typeof relation.position === "number" ? relation.position : null,
    );
    // A parent outside the slice leaves the child at the top level.
    if (!child || !parent || child === parent) continue;
    parent.children.push(child);
    attached.add(child.id);
  }

  const order = (a: PlanNode, b: PlanNode) => {
    const aPosition = partOfPositions.get(a.id) ?? Number.MAX_SAFE_INTEGER;
    const bPosition = partOfPositions.get(b.id) ?? Number.MAX_SAFE_INTEGER;
    const aItemOrder = itemOrder.get(a.id) ?? Number.MAX_SAFE_INTEGER;
    const bItemOrder = itemOrder.get(b.id) ?? Number.MAX_SAFE_INTEGER;
    return (
      aPosition - bPosition ||
      aItemOrder - bItemOrder ||
      a.id.localeCompare(b.id)
    );
  };
  for (const node of byId.values()) node.children.sort(order);
  const roots = [...byId.values()]
    .filter((node) => !attached.has(node.id))
    .sort(order);

  return {
    nodes: roots,
    truncated: source.truncated === true,
    limit: typeof source.limit === "number" ? source.limit : 0,
  };
}

export function actionsFrom(today: unknown): {
  localDate: string;
  actions: PlanAction[];
} {
  const source = (today as Unknown | undefined) ?? {};
  return {
    localDate: asString(source.local_date),
    actions: asArray(source.items).map((entry) => {
      const item = (entry.item as Unknown | undefined) ?? {};
      const fields = (item.fields as Unknown | undefined) ?? {};
      return {
        id: asString(item.id),
        title: asString(item.title),
        completed: entry.completed === true,
        occurrenceKey: asString(entry.occurrence_key),
        scheduledTime:
          typeof item.scheduled_time === "string" ? item.scheduled_time : null,
        assignee:
          typeof fields.assignee_id === "string"
            ? fields.assignee_id
            : typeof fields.assignee === "string"
              ? fields.assignee
              : null,
        dueDate: typeof item.due_date === "string" ? item.due_date : null,
        version: typeof item.version === "number" ? item.version : 1,
      };
    }),
  };
}

export function buildPlanView(input: {
  context?: unknown;
  graph?: unknown;
  today?: unknown;
  week?: unknown;
  workspaceId?: string;
}): PlanView {
  const workspaces = workspacesFrom(input.context);
  const tree = treeFrom(input.graph);
  const today = actionsFrom(input.today);
  const personal = workspaces.find(
    (workspace) => workspace.id === "personal" || workspace.scope === "個人",
  );
  return {
    workspaces,
    workspace:
      input.workspaceId !== undefined
        ? (workspaces.find((workspace) => workspace.id === input.workspaceId) ??
          null)
        : (personal ?? workspaces[0] ?? null),
    nodes: tree.nodes,
    truncated: tree.truncated,
    limit: tree.limit,
    localDate: today.localDate,
    actions: today.actions,
    week: weekFrom(input.week),
  };
}

/** Every node id in a tree, for pruning a selection that no longer exists. */
export function nodeIds(
  nodes: PlanNode[],
  into = new Set<string>(),
): Set<string> {
  for (const node of nodes) {
    into.add(node.id);
    nodeIds(node.children, into);
  }
  return into;
}

/** Finds one node anywhere in the tree. */
export function findNode(nodes: PlanNode[], id: string): PlanNode | null {
  for (const node of nodes) {
    if (node.id === id) return node;
    const found = findNode(node.children, id);
    if (found) return found;
  }
  return null;
}

/** Total nodes in a tree, so a view can say how much it is showing. */
export function countNodes(nodes: PlanNode[]): number {
  return nodes.reduce(
    (total, node) => total + 1 + countNodes(node.children),
    0,
  );
}
