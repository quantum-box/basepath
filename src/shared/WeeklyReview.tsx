/**
 * The weekly review, rendered the same way wherever it is shown.
 *
 * Presentational only: it takes the view model and emits markup. It fetches
 * nothing and aggregates nothing — every number on screen is one the server
 * already computed for the workspace's own week.
 *
 * What a person can *do* differs by surface, and deliberately so:
 *
 * - In Basepath they are signed in, on Basepath's origin, with a session the
 *   server can verify. They can save a draft and finalize it.
 * - In an AI host they are not. A click there reaches the server as an
 *   ordinary tool call, indistinguishable from the model's, so it cannot
 *   finalize a review. It can only propose the text as a change set, which
 *   the person then approves in Basepath.
 *
 * That is why `onFinalize` and `onProposeDraft` are separate props rather than
 * one "submit": the caller states which of the two it can actually back.
 */
import type {
  WeeklyDraftInput,
  WeeklyMetric,
  WeeklyReviewView,
} from "./weeklyView";
import { draftDiffers, hasDraftText, shiftWeek } from "./weeklyView";

function statusLabel(status: string) {
  switch (status) {
    case "completed":
      return "完了";
    case "skipped":
      return "見送り";
    default:
      return "未完了";
  }
}

/**
 * What a metric is worth saying.
 *
 * An unmeasured metric says so. It never borrows the previous week's number,
 * and it never becomes 0 — a plan is not improved by inventing a measurement.
 */
function metricValue(metric: WeeklyMetric) {
  if (metric.latest === null) return "未計測";
  return metric.unit
    ? `${metric.latest} ${metric.unit}`
    : String(metric.latest);
}

function metricNote(metric: WeeklyMetric) {
  if (metric.status === "unmeasured") return "観測がありません";
  if (metric.status === "stale") return "古い観測（2週間以上前）";
  if (metric.delta === null) return "前週データなし";
  const sign = metric.delta > 0 ? "+" : "";
  return `前週差 ${sign}${metric.delta}${metric.unit ? ` ${metric.unit}` : ""}`;
}

function timestamp(value: string | null, timezone: string) {
  if (!value) return "";
  try {
    return new Intl.DateTimeFormat("ja-JP", {
      timeZone: timezone,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(value));
  } catch {
    return value;
  }
}

function Evidence({
  title,
  detail,
  onOpen,
}: {
  title: string;
  detail: string;
  onOpen?: () => void;
}) {
  const body = (
    <>
      <span>
        <strong>{title}</strong>
        <small>{detail}</small>
      </span>
    </>
  );
  return (
    <li>
      {onOpen ? (
        <button type="button" onClick={onOpen}>
          {body}
        </button>
      ) : (
        <div className="evidence-static">{body}</div>
      )}
    </li>
  );
}

export type WeeklyReviewPanelProps = {
  review: WeeklyReviewView | null;
  workspaceName?: string;
  /** The conversation surface: its own heading, one column, host colours. */
  compact?: boolean;
  loading?: boolean;
  problem?: { title: string; detail: string; retry?: () => void } | null;
  /** A newer tool result has superseded what is shown. */
  stale?: boolean;
  /** Week navigation, when the surface offers it. */
  onSelectWeek?: (weekStart: string) => void;
  draft: WeeklyDraftInput;
  onDraftChange?: (next: WeeklyDraftInput) => void;
  /** False for a viewer, or once the shown revision is finalized. */
  canEdit?: boolean;
  /** Present only where a session can back a save. */
  onSave?: () => void;
  onFinalize?: () => void;
  /** Present where it cannot: the text becomes a change set to approve. */
  onProposeDraft?: () => void;
  onCorrect?: () => void;
  onOpenItem?: (itemId: string) => void;
  busy?: boolean;
  notice?: string | null;
};

export function WeeklyReviewPanel({
  review,
  workspaceName,
  compact,
  loading,
  problem,
  stale,
  onSelectWeek,
  draft,
  onDraftChange,
  canEdit = true,
  onSave,
  onFinalize,
  onProposeDraft,
  onCorrect,
  onOpenItem,
  busy,
  notice,
}: WeeklyReviewPanelProps) {
  const root = compact ? "weekly-review weekly-panel" : "weekly-review";
  if (problem) {
    return (
      <div className={`${root} plan-problem`} role="alert">
        <h3>{problem.title}</h3>
        <p>{problem.detail}</p>
        {problem.retry && (
          <button type="button" onClick={problem.retry}>
            再試行
          </button>
        )}
      </div>
    );
  }
  if (loading && !review) {
    return (
      <div className={`${root} plan-loading`} aria-busy="true">
        <p>集計しています…</p>
      </div>
    );
  }
  if (!review) {
    return (
      <div className={`${root} weekly-empty`}>
        <p>表示できる週がありません。</p>
      </div>
    );
  }

  const saved = review.review;
  const finalized = saved?.status === "finalized";
  // A finalized revision is never edited in place: correcting it starts a new
  // revision, so the history of what was said stays intact.
  const editable = canEdit && !finalized;
  const dirty = draftDiffers(draft, saved);
  const patch = (next: Partial<WeeklyDraftInput>) =>
    onDraftChange?.({ ...draft, ...next });

  return (
    <div className={root} data-stale={stale ? "true" : undefined}>
      {compact && (
        <header className="weekly-header">
          <h2>週次レビュー</h2>
          <span>
            {workspaceName ?? review.workspaceId} · {review.timezone}
          </span>
          <span>
            {review.weekStart}〜{review.weekEnd}
          </span>
        </header>
      )}
      {onSelectWeek && (
        <div className="weekly-week-nav">
          <button
            type="button"
            onClick={() => onSelectWeek(shiftWeek(review.weekStart, -1))}
          >
            前の週
          </button>
          <span>{review.weekStart}</span>
          <button
            type="button"
            onClick={() => onSelectWeek(shiftWeek(review.weekStart, 1))}
          >
            次の週
          </button>
        </div>
      )}
      {stale && (
        <p className="plan-stale" role="status">
          新しい結果が届いています。表示は1つ前の内容です。
        </p>
      )}

      <div className="weekly-stat-grid">
        <section className="panel metric-card green">
          <small>完了した行動</small>
          <strong>{review.totals.completed}</strong>
        </section>
        <section className="panel metric-card orange">
          <small>見送り</small>
          <strong>{review.totals.skipped}</strong>
        </section>
        <section className="panel metric-card purple">
          <small>未完了</small>
          <strong>{review.totals.incomplete}</strong>
        </section>
        <section className="panel metric-card blue">
          <small>完了率</small>
          <strong>
            {review.completionRate === null ? "—" : `${review.completionRate}%`}
          </strong>
        </section>
      </div>

      {review.empty && (
        <section className="panel weekly-empty">
          <h3>この週はまだ集計できるデータがありません</h3>
          <p>推測値は作らず、行動や観測が記録されるまで空のまま表示します。</p>
        </section>
      )}

      <div className="weekly-columns">
        <section className="panel weekly-section">
          <div className="section-header">
            <h3>行動の実績</h3>
            <span>{review.totals.total}件</span>
          </div>
          {review.actions.length ? (
            <ul className="evidence-list">
              {review.actions.map((action, index) => (
                <Evidence
                  key={`${action.itemId}-${action.date}-${index}`}
                  title={action.title}
                  detail={`${action.date} · ${statusLabel(action.status)}${
                    action.recordId ? " · 記録あり" : ""
                  }`}
                  onOpen={
                    onOpenItem ? () => onOpenItem(action.itemId) : undefined
                  }
                />
              ))}
            </ul>
          ) : (
            <p className="empty-value">予定された行動はありません。</p>
          )}
        </section>
        <section className="panel weekly-section">
          <div className="section-header">
            <h3>目標の自己評価</h3>
            <span>行動完了率とは別指標</span>
          </div>
          {review.goals.length ? (
            <ul className="evidence-list">
              {review.goals.map((goal) => (
                <Evidence
                  key={goal.itemId}
                  title={goal.title}
                  detail={
                    goal.selfAssessment === null
                      ? "評価未設定"
                      : `自己評価 ${goal.selfAssessment}%`
                  }
                  onOpen={
                    onOpenItem ? () => onOpenItem(goal.itemId) : undefined
                  }
                />
              ))}
            </ul>
          ) : (
            <p className="empty-value">目標はありません。</p>
          )}
        </section>
      </div>

      <section className="panel weekly-section">
        <div className="section-header">
          <h3>成果指標</h3>
          <span>最新値と前週差</span>
        </div>
        {review.metrics.length ? (
          <div className="metric-review-grid">
            {review.metrics.map((metric) => {
              const content = (
                <>
                  <strong>{metric.name}</strong>
                  <span>{metricValue(metric)}</span>
                  <small>{metricNote(metric)}</small>
                </>
              );
              return onOpenItem ? (
                <button
                  key={metric.metricId}
                  type="button"
                  data-status={metric.status}
                  onClick={() => onOpenItem(metric.itemId)}
                >
                  {content}
                </button>
              ) : (
                <div key={metric.metricId} data-status={metric.status}>
                  {content}
                </div>
              );
            })}
          </div>
        ) : (
          <p className="empty-value">成果指標はありません。</p>
        )}
      </section>

      {review.members.length > 0 && (
        <section className="panel weekly-section">
          <div className="section-header">
            <h3>担当者別</h3>
            <span>このワークスペースのメンバーのみ</span>
          </div>
          <div className="member-review-grid">
            {review.members.map((member) => (
              <div key={member.actor}>
                <strong>{member.actor}</strong>
                <small>{member.role}</small>
                <span>
                  完了 {member.completed} · 見送り {member.skipped} · 未完了{" "}
                  {member.incomplete}
                </span>
              </div>
            ))}
          </div>
        </section>
      )}

      <section className="panel weekly-editor">
        <div className="section-header">
          <div>
            <h3>
              {finalized && saved
                ? `確定済みレビュー · 第${saved.revision}版`
                : "レビューをまとめる"}
            </h3>
            <p>
              {review.weekStart}〜{review.weekEnd}
            </p>
          </div>
          {finalized && canEdit && onCorrect && (
            <button
              type="button"
              className="secondary-button"
              onClick={onCorrect}
            >
              訂正版を作成
            </button>
          )}
        </div>
        <div className="weekly-editor-grid">
          {(
            [
              ["learnings", "学び"],
              ["challenges", "課題"],
              ["nextFocus", "次週の重点"],
            ] as const
          ).map(([field, label]) => (
            <label key={field}>
              <span>{label}</span>
              {/* Named explicitly: a wrapping label would otherwise take its
                  name from the text inside it, which includes what the person
                  has typed. */}
              <textarea
                aria-label={label}
                disabled={!editable}
                value={draft[field]}
                onChange={(event) => patch({ [field]: event.target.value })}
              />
            </label>
          ))}
        </div>
        {notice && (
          <p className="weekly-notice" role="status">
            {notice}
          </p>
        )}
        <div className="reflection-actions">
          <span>
            {finalized && saved
              ? `確定 ${timestamp(saved.finalizedAt, review.timezone)}`
              : saved
                ? dirty
                  ? "未保存の変更があります"
                  : "下書き保存済み"
                : "未保存"}
          </span>
          {editable && onSave && (
            <button
              type="button"
              className="secondary-button"
              disabled={busy}
              onClick={onSave}
            >
              下書き保存
            </button>
          )}
          {editable && onFinalize && (
            <button
              type="button"
              className="primary-button"
              disabled={busy || !hasDraftText(draft)}
              onClick={onFinalize}
            >
              レビューを確定
            </button>
          )}
          {editable && onProposeDraft && (
            <button
              type="button"
              className="primary-button"
              disabled={busy || !hasDraftText(draft)}
              onClick={onProposeDraft}
            >
              変更案にする
            </button>
          )}
        </div>
        {onProposeDraft && (
          <p className="weekly-note">
            ここでの入力は変更案になります。確定はBasepathで本人が承認したときだけ行われます。
          </p>
        )}
      </section>

      {review.history.length > 1 && (
        <section className="panel weekly-section">
          <div className="section-header">
            <h3>訂正履歴</h3>
            <span>{review.history.length}版</span>
          </div>
          <ol className="review-history">
            {[...review.history].reverse().map((entry) => (
              <li key={entry.id}>
                <strong>
                  第{entry.revision}版 ·{" "}
                  {entry.status === "finalized" ? "確定" : "下書き"}
                </strong>
                <span>
                  {timestamp(entry.updatedAt, review.timezone)} · {entry.author}
                </span>
              </li>
            ))}
          </ol>
        </section>
      )}
    </div>
  );
}
