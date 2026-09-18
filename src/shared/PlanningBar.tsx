/**
 * The period a plan is being looked at in.
 *
 * Presentational only. It shows which period this is, what is on either side
 * of it, and how much of it has elapsed — never how much of the *work* is
 * done. Those are different claims and a bar that blurs them would be the
 * product lying quietly.
 *
 * A workspace that has never used periods gets one line saying so and no
 * prompt to start. Most plans do not need quarters.
 */
import type { PlanningCycle, PlanningView } from "./planningView";
import {
  cadenceLabel,
  elapsedPercent,
  spanLabel,
  statusLabel,
} from "./planningView";

function Step({
  cycle,
  relation,
  onSelect,
}: {
  cycle: PlanningCycle | null;
  relation: "previous" | "next";
  onSelect?: (id: string) => void;
}) {
  if (!cycle) return null;
  const label = relation === "previous" ? "前の期間" : "次の期間";
  return (
    <button
      type="button"
      className="planning-step"
      data-relation={relation}
      onClick={() => onSelect?.(cycle.id)}
      disabled={!onSelect}
      title={spanLabel(cycle)}
    >
      <small>{label}</small>
      <span>{cycle.label}</span>
    </button>
  );
}

export type PlanningBarProps = {
  planning: PlanningView | null;
  /** Present where the surface can switch periods. */
  onSelectCycle?: (id: string) => void;
  /** Present where the person can create the next period themselves. */
  onCreateNext?: () => void;
  busy?: boolean;
  compact?: boolean;
};

export function PlanningBar({
  planning,
  onSelectCycle,
  onCreateNext,
  busy,
  compact,
}: PlanningBarProps) {
  if (!planning) return null;
  const root = compact ? "planning-bar planning-compact" : "planning-bar";

  if (planning.unused) {
    return (
      <section className={root} aria-label="計画期間">
        <p className="planning-none">
          このワークスペースは計画期間を使っていません。
          {planning.unassignedItems > 0 &&
            `${planning.unassignedItems}件の項目は期間に属していません。`}
        </p>
      </section>
    );
  }

  const current = planning.current;
  const elapsed = current ? elapsedPercent(current, planning.today) : null;

  return (
    <section className={root} aria-label="計画期間">
      <div className="planning-row">
        <Step
          cycle={planning.previous}
          relation="previous"
          onSelect={onSelectCycle}
        />
        {current ? (
          <div className="planning-current">
            <div className="planning-head">
              <strong>{current.label}</strong>
              <span className="planning-cadence">
                {cadenceLabel(current.cadence)}
              </span>
              <span className="planning-status" data-status={current.status}>
                {statusLabel(current.status)}
              </span>
            </div>
            <small>{spanLabel(current)}</small>
            <small>{current.itemCount}件</small>
            {elapsed !== null && (
              <div className="planning-elapsed">
                <div
                  className="planning-elapsed-fill"
                  style={{ inlineSize: `${elapsed}%` }}
                />
                {/* Time, not progress. Saying which is the whole point. */}
                <small>期間の経過 {elapsed}%</small>
              </div>
            )}
          </div>
        ) : (
          <div className="planning-current">
            <strong>期間外</strong>
            <small>
              今日（{planning.today}）はどの期間にも入っていません。
            </small>
          </div>
        )}
        {planning.next ? (
          <Step
            cycle={planning.next}
            relation="next"
            onSelect={onSelectCycle}
          />
        ) : (
          onCreateNext &&
          current && (
            <button
              type="button"
              className="planning-step planning-create"
              onClick={onCreateNext}
              disabled={busy}
            >
              <small>次の期間</small>
              <span>作成する</span>
            </button>
          )
        )}
      </div>

      {planning.unassignedItems > 0 && (
        <p className="planning-unassigned">
          期間に属していない項目が{planning.unassignedItems}件あります。
        </p>
      )}

      {planning.lastReview && planning.lastReview.nextFocus.trim() && (
        <p className="planning-focus">
          <span>前回の重点（{planning.lastReview.weekStart}）</span>
          {planning.lastReview.nextFocus}
        </p>
      )}
    </section>
  );
}
