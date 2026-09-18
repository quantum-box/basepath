/**
 * The goal dashboard, read without merging four different things.
 *
 * This is the place a goal product usually starts lying. Four tickets shipped
 * becomes "40% toward the revenue target", the number goes on a screen, and it
 * gets defended for a quarter. So the shape here keeps them apart by
 * construction — there is no field that holds "progress", only four named
 * ones that mean different things:
 *
 * - `actionCompletion` — what was planned and what happened
 * - `metricProgress` — derived from observations, by a stated method
 * - `selfAssessment` — the person's own judgement
 * - `health` — somebody's stated view, with their name and the date
 *
 * Every one of them can be absent, and absent is never rendered as zero.
 */

export type HealthStatus = "on_track" | "at_risk" | "off_track";
export type MetricStatus = "current" | "stale" | "unmeasured";

export type DashboardMetric = {
  metricId: string;
  name: string;
  unit: string;
  direction: string;
  baseline: number;
  target: number;
  /** Null when nothing has been observed. Not zero. */
  latest: number | null;
  /** Percent of the way from baseline to target. Null when unmeasured. */
  progress: number | null;
  status: MetricStatus;
  /** Where the number came from, so it can be followed back. */
  latestObservationId: string | null;
  observedAt: string | null;
};

export type DashboardGoal = {
  id: string;
  title: string;
  ownerLabel: string;
  cycleLabel: string | null;
  state: string;
  dueDate: string | null;
  /** What was planned and what happened. `rate` is null when nothing was. */
  actionCompletion: { total: number; completed: number; rate: number | null };
  /** Derived, by a method the goal names. Null method means no number. */
  metricProgress: {
    method: string | null;
    source: string;
    value: number | null;
    counted: number;
    missing: number;
    metrics: DashboardMetric[];
  };
  /** The person's own judgement of their own goal. */
  selfAssessment: number | null;
  assessedAt: string | null;
  /** Somebody's stated view. Null means unknown, which is a real answer. */
  health: {
    status: HealthStatus;
    note: string;
    setAt: string;
    setBy: string;
  } | null;
  /** Derived from the signals below. A suggestion, never the health. */
  suggestedHealth: { status: HealthStatus; reasons: string[] } | null;
  signals: {
    overdue: boolean;
    staleMetrics: number;
    unmeasuredMetrics: number;
    lastCheckinAt: string | null;
    daysSinceCheckin: number | null;
  };
};

export type DashboardView = {
  workspaceId: string;
  today: string;
  goals: DashboardGoal[];
  byHealth: {
    on_track: number;
    at_risk: number;
    off_track: number;
    unknown: number;
  };
  /** Goals that named no rollup method, and so report no derived progress. */
  withoutRollupMethod: number;
  rollupMethods: string[];
};

type Unknown = Record<string, unknown>;

const asString = (value: unknown, fallback = ""): string =>
  typeof value === "string" ? value : fallback;

const asCount = (value: unknown): number =>
  typeof value === "number" && Number.isFinite(value) ? value : 0;

/** Null rather than 0: an absent measurement is not a measured zero. */
const asNumber = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

const asStrings = (value: unknown): string[] =>
  Array.isArray(value)
    ? value.filter((entry): entry is string => typeof entry === "string")
    : [];

function ownerLabelOf(value: unknown): string {
  const owner = value as Unknown | null | undefined;
  if (!owner || typeof owner.kind !== "string") return "担当なし";
  if (owner.kind === "organization") return "組織";
  return asString(owner.id, "担当なし");
}

function healthOf(value: unknown): DashboardGoal["health"] {
  const source = value as Unknown | null | undefined;
  const status = asString(source?.status);
  if (!["on_track", "at_risk", "off_track"].includes(status)) return null;
  return {
    status: status as HealthStatus,
    note: asString(source?.note),
    setAt: asString(source?.set_at),
    setBy: asString(source?.set_by),
  };
}

export function dashboardViewFrom(value: unknown): DashboardView | null {
  const source = value as Unknown | undefined;
  if (!source || typeof source.workspace_id !== "string") return null;
  const listed = Array.isArray(source.goals) ? (source.goals as Unknown[]) : [];
  const counts = (source.by_health as Unknown | undefined) ?? {};
  return {
    workspaceId: source.workspace_id,
    today: asString(source.today),
    goals: listed.map((goal) => {
      const completion = (goal.action_completion as Unknown | undefined) ?? {};
      const progress = (goal.metric_progress as Unknown | undefined) ?? {};
      const signals = (goal.signals as Unknown | undefined) ?? {};
      const suggested = goal.suggested_health as Unknown | null | undefined;
      const suggestedStatus = asString(suggested?.status);
      return {
        id: asString(goal.id),
        title: asString(goal.title),
        ownerLabel: ownerLabelOf(goal.owner),
        cycleLabel: goal.cycle ? asString((goal.cycle as Unknown).label) : null,
        state: asString(goal.state, "active"),
        dueDate: typeof goal.due_date === "string" ? goal.due_date : null,
        actionCompletion: {
          total: asCount(completion.total),
          completed: asCount(completion.completed),
          rate: asNumber(completion.rate),
        },
        metricProgress: {
          method: typeof progress.method === "string" ? progress.method : null,
          source: asString(progress.source, "none"),
          value: asNumber(progress.value),
          counted: asCount(progress.counted),
          missing: asCount(progress.missing),
          metrics: (Array.isArray(progress.metrics)
            ? (progress.metrics as Unknown[])
            : []
          ).map((metric) => {
            const status = asString(metric.status, "unmeasured");
            return {
              metricId: asString(metric.metric_id),
              name: asString(metric.name),
              unit: asString(metric.unit),
              direction: asString(metric.direction),
              baseline: asCount(metric.baseline),
              target: asCount(metric.target),
              latest: asNumber(metric.latest),
              progress: asNumber(metric.progress),
              status: (["current", "stale", "unmeasured"].includes(status)
                ? status
                : "unmeasured") as MetricStatus,
              latestObservationId:
                typeof metric.latest_observation_id === "string"
                  ? metric.latest_observation_id
                  : null,
              observedAt:
                typeof metric.observed_at === "string"
                  ? metric.observed_at
                  : null,
            };
          }),
        },
        selfAssessment: asNumber(goal.self_assessment),
        assessedAt:
          typeof goal.assessed_at === "string" ? goal.assessed_at : null,
        health: healthOf(goal.health),
        suggestedHealth: ["on_track", "at_risk", "off_track"].includes(
          suggestedStatus,
        )
          ? {
              status: suggestedStatus as HealthStatus,
              reasons: asStrings(suggested?.reasons),
            }
          : null,
        signals: {
          overdue: signals.overdue === true,
          staleMetrics: asCount(signals.stale_metrics),
          unmeasuredMetrics: asCount(signals.unmeasured_metrics),
          lastCheckinAt:
            typeof signals.last_checkin_at === "string"
              ? signals.last_checkin_at
              : null,
          daysSinceCheckin: asNumber(signals.days_since_checkin),
        },
      };
    }),
    byHealth: {
      on_track: asCount(counts.on_track),
      at_risk: asCount(counts.at_risk),
      off_track: asCount(counts.off_track),
      unknown: asCount(counts.unknown),
    },
    withoutRollupMethod: asCount(source.without_rollup_method),
    rollupMethods: asStrings(source.rollup_methods),
  };
}

export function healthLabel(status: HealthStatus | null): string {
  if (status === "on_track") return "順調";
  if (status === "at_risk") return "注意";
  if (status === "off_track") return "遅れ";
  return "未記入";
}

const METHOD_LABELS: Record<string, string> = {
  metric_average: "指標の平均",
  metric_worst: "指標の最小",
  children_average: "下位目標の平均",
  children_worst: "下位目標の最小",
};

export function methodLabel(method: string | null): string {
  if (!method) return "集計方法なし";
  return METHOD_LABELS[method] ?? method;
}

/**
 * A percentage, or the reason there is not one.
 *
 * Never "0%" as a stand-in for "we do not know". The two read the same on a
 * screen and mean opposite things.
 */
export function percent(value: number | null, absent = "—"): string {
  return value === null ? absent : `${Math.round(value)}%`;
}

/** What a metric says, including when it says nothing. */
export function metricValueLabel(metric: DashboardMetric): string {
  if (metric.latest === null) return "未計測";
  return metric.unit
    ? `${metric.latest} ${metric.unit}`
    : String(metric.latest);
}
