/**
 * The view model every surface renders from.
 *
 * Web, Tauri and the MCP App show the same plan, so the shape of what is shown
 * is defined once here and built from the same server responses. Only *how the
 * data arrives* differs, and that is the host adapter's job. No business rule
 * lives here: the Rust service decides what is true, this decides how to read
 * it.
 */

export type PlanNodeKind =
  "idea" | "outcome" | "initiative" | "action" | "milestone";

export type PlanNode = {
  id: string;
  title: string;
  kind: PlanNodeKind;
  state: string;
  dueDate: string | null;
  /** Percent, or null when the person has not assessed it. */
  selfAssessment: number | null;
  children: PlanNode[];
};

export type PlanAction = {
  id: string;
  title: string;
  completed: boolean;
  occurrenceKey: string;
  scheduledTime: string | null;
};

export type PlanWorkspace = { id: string; name: string; scope: string };

export type PlanView = {
  workspaces: PlanWorkspace[];
  workspace: PlanWorkspace | null;
  nodes: PlanNode[];
  /** True when the graph is a slice of the plan rather than all of it. */
  truncated: boolean;
  limit: number;
  localDate: string;
  actions: PlanAction[];
};

export const emptyPlanView: PlanView = {
  workspaces: [],
  workspace: null,
  nodes: [],
  truncated: false,
  limit: 0,
  localDate: "",
  actions: [],
};

type Unknown = Record<string, unknown>;

function asArray(value: unknown): Unknown[] {
  return Array.isArray(value) ? (value as Unknown[]) : [];
}

function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

export function workspacesFrom(context: unknown): PlanWorkspace[] {
  const source = (context as Unknown | undefined)?.workspaces;
  return asArray(source).map((workspace) => ({
    id: asString(workspace.id),
    name: asString(workspace.name, asString(workspace.id)),
    scope: asString(workspace.scope),
  }));
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
  const items = asArray(source.items);
  const relations = asArray(source.relations);

  const byId = new Map<string, PlanNode>();
  for (const item of items) {
    const fields = (item.fields as Unknown | undefined) ?? {};
    const assessment = fields.self_assessment;
    byId.set(asString(item.id), {
      id: asString(item.id),
      title: asString(item.title),
      kind: asString(item.kind, "outcome") as PlanNodeKind,
      state: asString(item.state, "active"),
      dueDate: typeof item.due_date === "string" ? item.due_date : null,
      selfAssessment: typeof assessment === "number" ? assessment : null,
      children: [],
    });
  }

  const attached = new Set<string>();
  for (const relation of relations) {
    if (relation.type !== "part_of") continue;
    const child = byId.get(asString(relation.source_id));
    const parent = byId.get(asString(relation.target_id));
    // A parent outside the slice leaves the child at the top level.
    if (!child || !parent || child === parent) continue;
    parent.children.push(child);
    attached.add(child.id);
  }

  const order = (a: PlanNode, b: PlanNode) => a.title.localeCompare(b.title);
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
      return {
        id: asString(item.id),
        title: asString(item.title),
        completed: entry.completed === true,
        occurrenceKey: asString(entry.occurrence_key),
        scheduledTime:
          typeof item.scheduled_time === "string" ? item.scheduled_time : null,
      };
    }),
  };
}

export function buildPlanView(input: {
  context?: unknown;
  graph?: unknown;
  today?: unknown;
  workspaceId?: string;
}): PlanView {
  const workspaces = workspacesFrom(input.context);
  const tree = treeFrom(input.graph);
  const today = actionsFrom(input.today);
  return {
    workspaces,
    workspace:
      workspaces.find((workspace) => workspace.id === input.workspaceId) ??
      workspaces[0] ??
      null,
    nodes: tree.nodes,
    truncated: tree.truncated,
    limit: tree.limit,
    localDate: today.localDate,
    actions: today.actions,
  };
}

/** Total nodes in a tree, so a view can say how much it is showing. */
export function countNodes(nodes: PlanNode[]): number {
  return nodes.reduce(
    (total, node) => total + 1 + countNodes(node.children),
    0,
  );
}
