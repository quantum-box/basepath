/**
 * The goal dashboard.
 *
 * Four different things are shown side by side and never merged: what was
 * planned and what happened, what the measurements say, what the person
 * thinks, and what somebody decided about how it is going. Each has its own
 * column and its own label, because the moment they share one the screen
 * starts making a claim nobody checked.
 *
 * Absent is shown as absent. A goal with no metric is not 0%, a week with no
 * planned action has no completion rate, and a goal nobody has assessed is
 * unassessed. Rendering any of those as zero would be the dashboard inventing
 * bad news — or good news — about real work.
 */
import { useCallback, useEffect, useState } from "react";
import { ApiError, request, type Workspace } from "./api";
import type { WorkspaceStore } from "./useWorkspace";
import {
  dashboardViewFrom,
  healthLabel,
  methodLabel,
  metricValueLabel,
  percent,
  type DashboardGoal,
  type DashboardView,
  type HealthStatus,
} from "./shared/dashboardView";

const STATUSES: HealthStatus[] = ["on_track", "at_risk", "off_track"];

function Row({
  goal,
  canWrite,
  busy,
  onOpen,
  onHealth,
}: {
  goal: DashboardGoal;
  canWrite: boolean;
  busy: boolean;
  onOpen: (id: string) => void;
  onHealth: (goal: DashboardGoal, status: HealthStatus) => void;
}) {
  const [open, setOpen] = useState(false);
  const metrics = goal.metricProgress.metrics;
  return (
    <li className="dashboard-goal" data-state={goal.state}>
      <div className="dashboard-head">
        <button className="dashboard-title" onClick={() => onOpen(goal.id)}>
          <strong>{goal.title}</strong>
        </button>
        <span className="dashboard-owner">{goal.ownerLabel}</span>
        {goal.cycleLabel && (
          <span className="dashboard-cycle">{goal.cycleLabel}</span>
        )}
      </div>

      <div className="dashboard-figures">
        <div className="dashboard-figure">
          <small>行動の実施</small>
          <strong>{percent(goal.actionCompletion.rate)}</strong>
          <small>
            {goal.actionCompletion.total === 0
              ? "行動なし"
              : `${goal.actionCompletion.completed}/${goal.actionCompletion.total}件`}
          </small>
        </div>
        <div className="dashboard-figure">
          <small>指標の進捗</small>
          <strong>{percent(goal.metricProgress.value)}</strong>
          <small>
            {methodLabel(goal.metricProgress.method)}
            {goal.metricProgress.missing > 0 &&
              ` ・未計測${goal.metricProgress.missing}件`}
          </small>
        </div>
        <div className="dashboard-figure">
          <small>自己評価</small>
          <strong>{percent(goal.selfAssessment, "未設定")}</strong>
          <small>本人の判断</small>
        </div>
        <div className="dashboard-figure" data-health={goal.health?.status}>
          <small>状況</small>
          <strong>{healthLabel(goal.health?.status ?? null)}</strong>
          <small>
            {goal.health
              ? `${goal.health.setBy} が記入`
              : "誰も記入していません"}
          </small>
        </div>
      </div>

      {goal.health?.note && (
        <p className="dashboard-note">{goal.health.note}</p>
      )}

      {/* Facts, listed. Whether they add up to "at risk" is a judgement, and
          the screen says whose it would be rather than making it. */}
      {goal.suggestedHealth && (
        <p className="dashboard-signals">
          <span>
            気になる点（{healthLabel(goal.suggestedHealth.status)}の可能性）
          </span>
          {goal.suggestedHealth.reasons.join(" / ")}
          {!goal.health && "。状況はまだ誰も記入していません。"}
        </p>
      )}

      {canWrite && (
        <div className="dashboard-actions">
          {STATUSES.map((status) => (
            <button
              key={status}
              disabled={busy}
              aria-pressed={goal.health?.status === status}
              onClick={() => onHealth(goal, status)}
            >
              {healthLabel(status)}として記録
            </button>
          ))}
        </div>
      )}

      {metrics.length > 0 && (
        <>
          <button
            className="dashboard-expand"
            aria-expanded={open}
            onClick={() => setOpen((shown) => !shown)}
          >
            {open ? "根拠を閉じる" : `根拠の指標 ${metrics.length}件`}
          </button>
          {open && (
            <table className="dashboard-metrics">
              <thead>
                <tr>
                  <th scope="col">指標</th>
                  <th scope="col">最新値</th>
                  <th scope="col">基準→目標</th>
                  <th scope="col">進捗</th>
                  <th scope="col">観測</th>
                </tr>
              </thead>
              <tbody>
                {metrics.map((metric) => (
                  <tr key={metric.metricId} data-status={metric.status}>
                    <th scope="row">{metric.name}</th>
                    <td>{metricValueLabel(metric)}</td>
                    <td>
                      {metric.baseline} → {metric.target}
                    </td>
                    <td>{percent(metric.progress, "未計測")}</td>
                    <td>
                      {metric.observedAt
                        ? metric.observedAt.slice(0, 10)
                        : "観測なし"}
                      {metric.status === "stale" && "（古い）"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </>
      )}
    </li>
  );
}

export function DashboardScreen({
  store,
  workspace,
  onOpenItem,
}: {
  store: WorkspaceStore;
  workspace?: Workspace;
  onOpenItem: (id: string) => void;
}) {
  const [view, setView] = useState<DashboardView | null>(null);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    if (!workspace) return;
    setError("");
    try {
      const value = await request<unknown>(
        "GET",
        `/v1/workspaces/${workspace.id}/dashboard`,
      );
      setView(dashboardViewFrom(value));
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "ダッシュボードを読み込めませんでした",
      );
    }
  }, [workspace]);

  useEffect(() => {
    void load();
  }, [load]);

  const setHealth = async (goal: DashboardGoal, status: HealthStatus) => {
    if (!workspace) return;
    try {
      await store.run(async () => {
        const item = await request<{ version: number }>(
          "GET",
          `/v1/workspaces/${workspace.id}/items/${goal.id}`,
        );
        await store.write(
          "POST",
          `/v1/workspaces/${workspace.id}/items/${goal.id}/health`,
          { status, expected_version: item.version },
        );
      });
      await load();
    } catch (failure) {
      setError(
        failure instanceof ApiError ? failure.message : "記録できませんでした",
      );
    }
  };

  const canWrite = !!workspace && workspace.role !== "viewer";

  return (
    <div className="page-content dashboard-screen">
      <section className="panel dashboard-intro">
        <div>
          <span className="eyebrow">{workspace?.name}</span>
          <p>
            行動の実施、指標の進捗、自己評価、状況は別々の事実です。ここでは混ぜずに
            並べます。分からないものは0%ではなく「—」と表示します。
          </p>
        </div>
      </section>

      {error && (
        <p className="auth-error" role="alert">
          {error}
        </p>
      )}

      {view && (
        <>
          <section className="panel dashboard-summary">
            <div className="dashboard-counts">
              {(["on_track", "at_risk", "off_track"] as const).map((status) => (
                <div key={status} data-health={status}>
                  <small>{healthLabel(status)}</small>
                  <strong>{view.byHealth[status]}</strong>
                </div>
              ))}
              <div data-health="unknown">
                <small>未記入</small>
                <strong>{view.byHealth.unknown}</strong>
              </div>
            </div>
            <p className="dashboard-summary-note">
              組織全体をひとつの数字にはしません。
              {view.withoutRollupMethod > 0 &&
                `集計方法が未設定の目標が${view.withoutRollupMethod}件あり、指標の進捗は表示していません。`}
            </p>
          </section>

          <section className="panel dashboard-list">
            {view.goals.length === 0 ? (
              <p className="empty-value">この領域にはまだ目標がありません。</p>
            ) : (
              <ul>
                {view.goals.map((goal) => (
                  <Row
                    key={goal.id}
                    goal={goal}
                    canWrite={canWrite}
                    busy={store.pending}
                    onOpen={onOpenItem}
                    onHealth={(target, status) =>
                      void setHealth(target, status)
                    }
                  />
                ))}
              </ul>
            )}
          </section>
        </>
      )}
    </div>
  );
}
