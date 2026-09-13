import { useCallback, useEffect, useState } from "react";
import { Icon } from "./icons";
import { ApiError, localDate, request, type WeeklyReview, type Workspace } from "./api";
import type { WorkspaceStore } from "./useWorkspace";

type Summary = {
  workspace_id: string;
  timezone: string;
  week_start: string;
  week_end: string;
  actions: { total: number; completed: number; skipped: number; incomplete: number; items: Array<{ item_id: string; title: string; date: string; status: string; record_id: string | null }> };
  goals: Array<{ item_id: string; title: string; self_assessment: number | null; assessed_at: string | null }>;
  metrics: Array<{ metric_id: string; item_id: string; name: string; unit: string; latest: number | null; previous: number | null; delta: number | null; status: "current" | "stale" | "unmeasured"; latest_observation_id: string | null }>;
  members: Array<{ actor: string; role: string; completed: number; skipped: number; incomplete: number }>;
  review: WeeklyReview | null;
  history: WeeklyReview[];
};

function monday(timezone: string, value = new Date()) {
  const current = localDate(timezone, value);
  const date = new Date(`${current}T12:00:00Z`);
  const offset = (date.getUTCDay() + 6) % 7;
  date.setUTCDate(date.getUTCDate() - offset);
  return date.toISOString().slice(0, 10);
}

function localTimestamp(value: string | null, timezone: string) {
  if (!value) return "";
  return new Intl.DateTimeFormat("ja-JP", {
    timeZone: timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

export function WeeklyReviewScreen({ store, workspace, onOpenItem }: { store: WorkspaceStore; workspace?: Workspace; onOpenItem: (id: string) => void }) {
  const [weekStart, setWeekStart] = useState(() => monday(workspace?.timezone || "Asia/Tokyo"));
  const [summary, setSummary] = useState<Summary | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [learnings, setLearnings] = useState("");
  const [challenges, setChallenges] = useState("");
  const [nextFocus, setNextFocus] = useState("");
  const review = summary?.review;
  const editable = !review || review.status === "draft";
  const canWrite = !!workspace && workspace.role !== "viewer";

  const load = useCallback(async () => {
    if (!workspace) return;
    setLoading(true);
    setError("");
    try {
      const value = await request<Summary>("GET", `/v1/workspaces/${workspace.id}/weekly-review?week_start=${weekStart}`);
      setSummary(value);
      setLearnings(value.review?.learnings || "");
      setChallenges(value.review?.challenges || "");
      setNextFocus(value.review?.next_focus || "");
    } catch (failure) {
      setError(failure instanceof ApiError ? failure.message : "週次レポートを読み込めませんでした");
    } finally { setLoading(false); }
  }, [workspace, weekStart]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => { if (workspace) setWeekStart(monday(workspace.timezone)); }, [workspace?.id, workspace?.timezone]);

  const save = async () => {
    if (!workspace) return null;
    let saved: WeeklyReview | null = null;
    await store.run(async () => {
      saved = await store.write<WeeklyReview>("POST", `/v1/workspaces/${workspace.id}/weekly-reviews/draft`, {
        week_start: weekStart, learnings, challenges, next_focus: nextFocus,
        ...(review?.status === "draft" ? { expected_version: review.version } : {}),
      });
    });
    await load();
    return saved;
  };
  const finalize = async () => {
    if (!workspace) return;
    let target = review?.status === "draft" ? review : await save();
    if (!target) return;
    await store.run(() => store.write("POST", `/v1/workspaces/${workspace.id}/weekly-reviews/${target!.id}/finalize`, { expected_version: target!.version }));
    await load();
  };
  const startCorrection = () => setSummary((current) => current ? { ...current, review: null } : current);

  return <div className="page-content weekly-review-screen">
    <section className="panel weekly-review-toolbar">
      <div><span className="eyebrow">{workspace?.name} · {summary?.timezone}</span><h2>週次レビュー</h2><p>行動、目標の自己評価、成果指標を分けて振り返ります。</p></div>
      <div className="weekly-review-controls"><label>週の開始<input type="date" value={weekStart} onChange={(e) => setWeekStart(monday(workspace?.timezone || "Asia/Tokyo", new Date(`${e.target.value}T12:00:00Z`)))} /></label><button className="secondary-button" onClick={() => window.print()}><Icon name="note" size={17} />印刷・共有用</button></div>
    </section>
    {error && <p className="auth-error" role="alert">{error}</p>}
    {loading && !summary ? <section className="panel weekly-empty">集計しています…</section> : summary && <>
      <div className="weekly-stat-grid">
        <section className="panel metric-card green"><small>完了した行動</small><strong>{summary.actions.completed}</strong></section>
        <section className="panel metric-card orange"><small>見送り</small><strong>{summary.actions.skipped}</strong></section>
        <section className="panel metric-card purple"><small>未完了</small><strong>{summary.actions.incomplete}</strong></section>
        <section className="panel metric-card blue"><small>完了率</small><strong>{summary.actions.total ? Math.round(summary.actions.completed / summary.actions.total * 100) : "—"}{summary.actions.total ? "%" : ""}</strong></section>
      </div>
      {!summary.actions.total && !summary.goals.length && !summary.metrics.length && <section className="panel weekly-empty"><Icon name="leaf" size={30} /><h3>この週はまだ集計できるデータがありません</h3><p>推測値は作らず、行動や観測が記録されるまで空のまま表示します。</p></section>}
      <div className="weekly-columns">
        <section className="panel weekly-section"><div className="section-header"><h3>行動の実績</h3><span>{summary.actions.total}件</span></div>{summary.actions.items.length ? <ul className="evidence-list">{summary.actions.items.map((item, index) => <li key={`${item.item_id}-${item.date}-${index}`}><button onClick={() => onOpenItem(item.item_id)}><span><strong>{item.title}</strong><small>{item.date} · {item.status === "completed" ? "完了" : item.status === "skipped" ? "見送り" : "未完了"}</small></span><Icon name="right" size={15} /></button></li>)}</ul> : <p className="empty-value">予定された行動はありません。</p>}</section>
        <section className="panel weekly-section"><div className="section-header"><h3>目標の自己評価</h3><span>行動完了率とは別指標</span></div>{summary.goals.length ? <ul className="evidence-list">{summary.goals.map((goal) => <li key={goal.item_id}><button onClick={() => onOpenItem(goal.item_id)}><span><strong>{goal.title}</strong><small>{goal.self_assessment === null ? "評価未設定" : `自己評価 ${goal.self_assessment}%`}</small></span><Icon name="right" size={15} /></button></li>)}</ul> : <p className="empty-value">目標はありません。</p>}</section>
      </div>
      <section className="panel weekly-section"><div className="section-header"><h3>成果指標</h3><span>最新値と前週差</span></div>{summary.metrics.length ? <div className="metric-review-grid">{summary.metrics.map((metric) => <button key={metric.metric_id} onClick={() => onOpenItem(metric.item_id)}><strong>{metric.name}</strong><span>{metric.latest === null ? "未計測" : `${metric.latest} ${metric.unit}`}</span><small>{metric.status === "stale" ? "古い観測" : metric.delta === null ? "前週データなし" : `前週差 ${metric.delta > 0 ? "+" : ""}${metric.delta} ${metric.unit}`}</small></button>)}</div> : <p className="empty-value">成果指標はありません。</p>}</section>
      {!!summary.members.length && <section className="panel weekly-section"><div className="section-header"><h3>担当者別</h3><span>このワークスペースのメンバーのみ</span></div><div className="member-review-grid">{summary.members.map((member) => <div key={member.actor}><strong>{member.actor}</strong><small>{member.role}</small><span>完了 {member.completed} · 見送り {member.skipped} · 未完了 {member.incomplete}</span></div>)}</div></section>}
      <section className="panel weekly-editor"><div className="section-header"><div><h3>{review?.status === "finalized" ? `確定済みレビュー · 第${review.revision}版` : "レビューをまとめる"}</h3><p>{summary.week_start}〜{summary.week_end}</p></div>{review?.status === "finalized" && canWrite && <button className="secondary-button" onClick={startCorrection}>訂正版を作成</button>}</div>
        <div className="weekly-editor-grid"><label>学び<textarea disabled={!editable || !canWrite} value={learnings} onChange={(e) => setLearnings(e.target.value)} /></label><label>課題<textarea disabled={!editable || !canWrite} value={challenges} onChange={(e) => setChallenges(e.target.value)} /></label><label>次週の重点<textarea disabled={!editable || !canWrite} value={nextFocus} onChange={(e) => setNextFocus(e.target.value)} /></label></div>
        <div className="reflection-actions"><span>{review?.status === "draft" ? "下書き保存済み" : review?.status === "finalized" ? `確定 ${localTimestamp(review.finalized_at, summary.timezone)}` : "未保存"}</span>{editable && canWrite && <><button className="secondary-button" disabled={store.pending} onClick={() => void save()}>下書き保存</button><button className="primary-button" disabled={store.pending || !(learnings.trim() || challenges.trim() || nextFocus.trim())} onClick={() => void finalize()}>レビューを確定</button></>}</div>
      </section>
      {summary.history.length > 1 && <section className="panel weekly-section"><div className="section-header"><h3>訂正履歴</h3><span>{summary.history.length}版</span></div><ol className="review-history">{[...summary.history].reverse().map((entry) => <li key={entry.id}><strong>第{entry.revision}版 · {entry.status === "finalized" ? "確定" : "下書き"}</strong><span>{localTimestamp(entry.updated_at, summary.timezone)} · {entry.author}</span></li>)}</ol></section>}
    </>}
  </div>;
}
