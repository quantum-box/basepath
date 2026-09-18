/**
 * Whose goal is this, and what does it roll up to.
 *
 * One workspace, one graph. A goal in someone's personal workspace is not in
 * here and cannot be reached from here — putting a goal in a shared workspace
 * is the act of sharing it, and a company goal pointing at a private one would
 * make that choice on someone's behalf.
 *
 * `partOf` and `contributesTo` stay separate all the way to the screen. One is
 * structure ("this is part of that", at most one), the other is contribution
 * ("this helps that along", as many as are true). A view that merged them
 * would answer neither question.
 */

export type OwnerKind = "organization" | "team" | "person";

export type GoalOwner = {
  kind: OwnerKind;
  /** Empty for the organization: the workspace is the organization. */
  id: string;
};

export type AlignedGoal = {
  id: string;
  title: string;
  kind: string;
  state: string;
  owner: GoalOwner | null;
  cycleLabel: string | null;
  cycleId: string | null;
  /** The person's own judgement, or null when they have not made one. */
  selfAssessment: number | null;
  dueDate: string | null;
  /** Structure: at most one. */
  partOf: string[];
  /** Contribution: as many as are true. */
  contributesTo: string[];
  /** What rolls up into this, by either edge. */
  supportedBy: string[];
  /** Initiatives and actions beneath it. */
  descendantWork: number;
  /** Nothing above it. Normal for a top-level goal; worth finding otherwise. */
  orphan: boolean;
};

export type AlignmentView = {
  workspaceId: string;
  goals: AlignedGoal[];
  teams: string[];
  people: string[];
  unownedGoals: number;
  orphanGoals: number;
};

type Unknown = Record<string, unknown>;

const asString = (value: unknown, fallback = ""): string =>
  typeof value === "string" ? value : fallback;

const asCount = (value: unknown): number =>
  typeof value === "number" && Number.isFinite(value) ? value : 0;

const asIds = (value: unknown): string[] =>
  Array.isArray(value)
    ? value.filter((id): id is string => typeof id === "string")
    : [];

function ownerFrom(value: unknown): GoalOwner | null {
  const source = value as Unknown | undefined;
  if (!source || typeof source.kind !== "string") return null;
  if (!["organization", "team", "person"].includes(source.kind)) return null;
  return { kind: source.kind as OwnerKind, id: asString(source.id) };
}

export function alignmentViewFrom(value: unknown): AlignmentView | null {
  const source = value as Unknown | undefined;
  if (!source || typeof source.workspace_id !== "string") return null;
  const listed = Array.isArray(source.goals) ? (source.goals as Unknown[]) : [];
  return {
    workspaceId: source.workspace_id,
    goals: listed.map((goal) => {
      const cycle = goal.cycle as Unknown | null | undefined;
      const assessment = goal.self_assessment;
      return {
        id: asString(goal.id),
        title: asString(goal.title),
        kind: asString(goal.kind, "outcome"),
        state: asString(goal.state, "active"),
        owner: ownerFrom(goal.owner),
        cycleLabel: cycle ? asString(cycle.label) : null,
        cycleId: cycle ? asString(cycle.id) : null,
        selfAssessment:
          typeof assessment === "number" && Number.isFinite(assessment)
            ? assessment
            : null,
        dueDate: typeof goal.due_date === "string" ? goal.due_date : null,
        partOf: asIds(goal.part_of),
        contributesTo: asIds(goal.contributes_to),
        supportedBy: asIds(goal.supported_by),
        descendantWork: asCount(goal.descendant_work),
        orphan: goal.orphan === true,
      };
    }),
    teams: asIds(source.teams),
    people: asIds(source.people),
    unownedGoals: asCount(source.unowned_goals),
    orphanGoals: asCount(source.orphan_goals),
  };
}

export function ownerLabel(owner: GoalOwner | null): string {
  if (!owner) return "担当なし";
  if (owner.kind === "organization") return "組織";
  if (owner.kind === "team") return owner.id;
  return owner.id;
}

export function ownerKindLabel(kind: OwnerKind): string {
  if (kind === "organization") return "組織の目標";
  if (kind === "team") return "チームの目標";
  return "個人の目標";
}

/** The goals with nothing above them: where reading the map starts. */
export function roots(view: AlignmentView): AlignedGoal[] {
  return view.goals.filter((goal) => goal.orphan);
}

/** What rolls up into this goal, resolved to the goals themselves. */
export function supporters(
  view: AlignmentView,
  goal: AlignedGoal,
): AlignedGoal[] {
  return goal.supportedBy
    .map((id) => view.goals.find((other) => other.id === id))
    .filter((other): other is AlignedGoal => other !== undefined);
}

/**
 * The chain from a goal up to whatever it is ultimately part of.
 *
 * Structure only — `partOf`. Following contribution as well would produce
 * several chains and answer a different question, and the graph refuses cycles
 * so this terminates.
 */
export function chainUpward(
  view: AlignmentView,
  goal: AlignedGoal,
): AlignedGoal[] {
  const chain: AlignedGoal[] = [];
  const seen = new Set<string>([goal.id]);
  let current = goal;
  for (;;) {
    const parentId = current.partOf[0];
    if (!parentId || seen.has(parentId)) return chain;
    const parent = view.goals.find((other) => other.id === parentId);
    if (!parent) return chain;
    chain.push(parent);
    seen.add(parent.id);
    current = parent;
  }
}
