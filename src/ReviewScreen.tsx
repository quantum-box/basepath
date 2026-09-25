/**
 * The goal review: what a review meeting needs in front of it.
 *
 * Three lists, because they call for different conversations. Goals nobody has
 * said anything about are silence, not a warning — merging them into "needs
 * attention" would make every new goal look like a problem and would bury the
 * ones somebody actually flagged.
 *
 * From any of them you can open the goal's history, which is where the numbers
 * and the words that produced this list came from.
 */
import { useCallback, useEffect, useState } from "react";
import { ApiError, request, type Workspace } from "./api";
import type { WorkspaceStore } from "./useWorkspace";
import {
  checkinsFrom,
  eventLabel,
  healthLabel,
  reviewQueueFrom,
  timelineFrom,
  type Checkin,
  type ReviewEntry,
  type ReviewQueue,
  type Timeline,
} from "./shared/checkinView";

function List({
  title,
  note,
  entries,
  onOpen,
}: {
  title: string;
  note: string;
  entries: ReviewEntry[];
  onOpen: (entry: ReviewEntry) => void;
}) {
  return (
    <section className="panel review-list">
      <div className="section-header">
        <h3>{title}</h3>
        <span>{entries.length}件</span>
      </div>
      <p className="review-note">{note}</p>
      {entries.length === 0 ? (
        <p className="empty-value">該当する目標はありません。</p>
      ) : (
        <ul>
          {entries.map((entry) => (
            <li key={entry.id}>
              <button onClick={() => onOpen(entry)}>
                <span>
                  <strong>{entry.title}</strong>
                  <small>
                    {entry.ownerLabel}
                    {entry.lastCheckinAt
                      ? ` ・ ${entry.lastCheckinAt.slice(0, 10)} ${entry.lastCheckinBy ?? ""}`
                      : " ・ チェックインなし"}
                    {entry.health && ` ・ ${healthLabel(entry.health)}`}
                  </small>
                  {entry.blockers && <small>課題: {entry.blockers}</small>}
                  {entry.nextFocus && (
                    <small>次の一手: {entry.nextFocus}</small>
                  )}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

type PlanHistoryChange = {
  id: string;
  collection: string;
  title?: string;
  effect?: string;
  operation_index?: number;
  undoable?: boolean;
  basis?: string;
  match_rationale?: string;
  interpretation?: {
    status?: string;
    origin?: string;
    source_ref?: string;
    source_url?: string;
    speaker?: string;
    at?: string;
    quote?: string;
    reason?: string;
    assumptions?: string[];
  } | null;
  before?: Record<string, unknown> | null;
  after?: Record<string, unknown> | null;
  relation_delta?: {
    type?: string;
    before?: (Record<string, unknown> & {
      source_title?: string;
      target_title?: string;
      relation_type?: string;
      parent_title?: string;
      position?: number | null;
    }) | null;
    after?: (Record<string, unknown> & {
      source_title?: string;
      target_title?: string;
      relation_type?: string;
      parent_title?: string;
      position?: number | null;
    }) | null;
  } | null;
};

type PlanHistoryVersion = {
  id: string;
  title: string;
  kind?: "audit" | "changeset";
  undoable?: boolean;
  actor?: string;
  proposed_by?: string;
  approved_by?: string;
  applied_by?: string;
  applied_by_connection?: string;
  applied_at?: string;
  conversation_id?: string | null;
  source_status?: "available" | "unavailable" | "not_linked";
  undo_of?: string;
  undo_reason?: string;
  assumptions?: string[];
  changes: PlanHistoryChange[];
};

type HistoryComparison = {
  coverage_complete: boolean;
  coverage_gaps: unknown[];
  changes: PlanHistoryChange[];
};

function valueLabel(value: unknown): string {
  if (value === null || value === undefined || value === "") return "なし";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return JSON.stringify(value);
}

function changesItem(change: PlanHistoryChange, itemId: string): boolean {
  return (
    change.id === itemId ||
    change.relation_delta?.before?.source_id === itemId ||
    change.relation_delta?.before?.target_id === itemId ||
    change.relation_delta?.after?.source_id === itemId ||
    change.relation_delta?.after?.target_id === itemId
  );
}

function comparedFields(change: PlanHistoryChange): string[] {
  const relation = change.relation_delta;
  if (relation?.type === "relation") {
    const describe = (value: NonNullable<typeof relation.before>) => {
      const relationType =
        value.relation_type ?? (typeof value.type === "string" ? value.type : "関連");
      const source = valueLabel(value.source_title ?? value.source_id ?? "項目");
      const target = valueLabel(value.target_title ?? value.target_id ?? "項目");
      return `${source} — ${relationType} → ${target}`;
    };
    if (!relation.before && relation.after) return ["関連を追加: " + describe(relation.after)];
    if (relation.before && !relation.after) return ["関連を解除: " + describe(relation.before)];
    if (relation.before && relation.after) {
      const before = describe(relation.before);
      const after = describe(relation.after);
      return before === after
        ? ["関連の内容に変更なし"]
        : ["関連: " + before + " → " + after];
    }
  }
  const before = change.before ?? null;
  const after = change.after ?? null;
  const changed: string[] = [];
  if (!before) changed.push("項目を追加");
  else if (!after) changed.push("項目を削除");
  if (!before || !after) {
    if (relation?.type === "part_of") {
      changed.push(
        "親: " + valueLabel(relation.before?.parent_title) + " → " + valueLabel(relation.after?.parent_title),
      );
    }
    return changed.length ? changed : ["変更内容を確認できません"];
  }
  const labels: Record<string, string> = {
    title: "名前",
    description: "説明",
    state: "状態",
    start_date: "開始日",
    due_date: "期限",
    scheduled_date: "予定日",
    scheduled_time: "予定時刻",
    archived_at: "アーカイブ",
  };
  changed.push(...Object.keys(labels)
    .filter((key) => JSON.stringify(before[key]) !== JSON.stringify(after[key]))
    .map((key) => labels[key] + ": " + valueLabel(before[key]) + " → " + valueLabel(after[key])));
  const beforeFields = (before.fields ?? {}) as Record<string, unknown>;
  const afterFields = (after.fields ?? {}) as Record<string, unknown>;
  const fieldLabels: Record<string, string> = {
    memo: "メモ",
    assignee_id: "担当者",
    priority: "優先度",
    self_assessment: "自己評価",
    cycle_id: "計画期間",
    owner: "担当範囲",
  };
  for (const key of new Set([...Object.keys(beforeFields), ...Object.keys(afterFields)])) {
    if (JSON.stringify(beforeFields[key]) !== JSON.stringify(afterFields[key])) {
      changed.push(
        (fieldLabels[key] ?? key) + ": " + valueLabel(beforeFields[key]) + " → " + valueLabel(afterFields[key]),
      );
    }
  }
  if (relation?.type === "part_of") {
    changed.push(
      "親: " + valueLabel(relation.before?.parent_title) + "（順序 " + valueLabel(relation.before?.position) + "） → " +
        valueLabel(relation.after?.parent_title) + "（順序 " + valueLabel(relation.after?.position) + "）",
    );
  }
  return changed.length ? changed : ["版間で内容の変更なし"];
}

function safeSourceUrl(value?: string): string | undefined {
  if (!value) return undefined;
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : undefined;
  } catch {
    return undefined;
  }
}

function History({
  workspaceId,
  entry,
  store,
  onClose,
}: {
  workspaceId: string;
  entry: ReviewEntry;
  store: WorkspaceStore;
  onClose: () => void;
}) {
  const [timeline, setTimeline] = useState<Timeline | null>(null);
  const [checkins, setCheckins] = useState<Checkin[]>([]);
  const [versions, setVersions] = useState<PlanHistoryVersion[]>([]);
  const [compareFrom, setCompareFrom] = useState("");
  const [compareTo, setCompareTo] = useState("");
  const [comparison, setComparison] = useState<HistoryComparison | null>(null);
  const [undoTarget, setUndoTarget] = useState<{
    versionId: string;
    operationIndex: number;
  } | null>(null);
  const [undoReason, setUndoReason] = useState("");
  const [undoPending, setUndoPending] = useState(false);
  const [draft, setDraft] = useState({
    health: "",
    comment: "",
    results: "",
    blockers: "",
    next_focus: "",
  });
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    setError("");
    try {
      const [line, checkinHistory, planHistory] = await Promise.all([
        request<unknown>(
          "GET",
          `/v1/workspaces/${workspaceId}/items/${entry.id}/timeline`,
        ),
        request<unknown>(
          "GET",
          `/v1/workspaces/${workspaceId}/items/${entry.id}/checkins`,
        ),
        request<{ items: PlanHistoryVersion[] }>(
          "GET",
          `/v1/workspaces/${workspaceId}/history`,
        ),
      ]);
      setTimeline(timelineFrom(line));
      setCheckins(checkinsFrom(checkinHistory));
      const nextVersions = planHistory.items ?? [];
      setVersions(nextVersions);
      const itemChanges = nextVersions.filter((version) =>
        version.changes.some((change) => changesItem(change, entry.id)),
      );
      setCompareFrom((current) =>
        itemChanges.some((version) => version.id === current)
          ? current
          : itemChanges.at(-2)?.id || "",
      );
      setCompareTo((current) =>
        itemChanges.some((version) => version.id === current)
          ? current
          : itemChanges.at(-1)?.id || "",
      );
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "履歴を読み込めませんでした",
      );
    }
  }, [workspaceId, entry.id]);

  useEffect(() => {
    setCompareFrom("");
    setCompareTo("");
    setComparison(null);
    setUndoTarget(null);
    setUndoReason("");
  }, [workspaceId, entry.id]);

  useEffect(() => {
    void load();
  }, [load]);

  const itemVersions = versions
    .map((version) => ({
      ...version,
      changes: version.changes.filter((change) => changesItem(change, entry.id)),
    }))
    .filter((version) => version.changes.length > 0);

  const compare = async () => {
    setError("");
    setComparison(null);
    try {
      const query = new URLSearchParams({ from: compareFrom, to: compareTo });
      setComparison(
        await request<HistoryComparison>(
          "GET",
          "/v1/workspaces/" + workspaceId + "/history/compare?" + query.toString(),
        ),
      );
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "版間の比較を読み込めませんでした",
      );
    }
  };

  const createUndoProposal = async () => {
    if (!undoTarget || !undoReason.trim()) return;
    setError("");
    setUndoPending(true);
    try {
      const graph = await request<{ plan_version: string }>(
        "GET",
        "/v1/workspaces/" + workspaceId + "/graph",
      );
      const proposal = await store.write<{ id: string }>(
        "POST",
        "/v1/workspaces/" + workspaceId + "/changesets/" + undoTarget.versionId + "/undo-preview",
        {
          expected_base_version: graph.plan_version,
          reason: undoReason.trim(),
          operation_indexes: [undoTarget.operationIndex],
        },
      );
      const approvalUrl = new URL(
        "/changes/" + encodeURIComponent(workspaceId) + "/" + encodeURIComponent(proposal.id),
        window.location.origin,
      );
      const tenantId = new URLSearchParams(window.location.search).get("tenant_id");
      if (tenantId) approvalUrl.searchParams.set("tenant_id", tenantId);
      window.location.assign(approvalUrl.pathname + approvalUrl.search);
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "取り消し案を作成できませんでした",
      );
      setUndoPending(false);
    }
  };

  const save = async () => {
    setError("");
    try {
      await store.run(() =>
        store.write(
          "POST",
          `/v1/workspaces/${workspaceId}/items/${entry.id}/checkins`,
          Object.fromEntries(
            Object.entries(draft).filter(([, value]) => value.trim()),
          ),
        ),
      );
      setDraft({
        health: "",
        comment: "",
        results: "",
        blockers: "",
        next_focus: "",
      });
      await load();
    } catch (failure) {
      setError(
        failure instanceof ApiError ? failure.message : "記録できませんでした",
      );
    }
  };

  return (
    <section className="panel review-history">
      <div className="section-header">
        <h3>{entry.title}</h3>
        <button className="secondary-button" onClick={onClose}>
          閉じる
        </button>
      </div>

      {error && (
        <p className="auth-error" role="alert">
          {error}
        </p>
      )}

      <div className="review-checkin-form">
        <label>
          <span>状況</span>
          <select
            value={draft.health}
            onChange={(event) =>
              setDraft({ ...draft, health: event.target.value })
            }
          >
            <option value="">記入しない</option>
            <option value="on_track">順調</option>
            <option value="at_risk">注意</option>
            <option value="off_track">遅れ</option>
          </select>
        </label>
        {(
          [
            ["comment", "コメント"],
            ["results", "成果"],
            ["blockers", "課題"],
            ["next_focus", "次の一手"],
          ] as const
        ).map(([field, label]) => (
          <label key={field}>
            <span>{label}</span>
            <textarea
              aria-label={label}
              value={draft[field]}
              onChange={(event) =>
                setDraft({ ...draft, [field]: event.target.value })
              }
            />
          </label>
        ))}
        <div className="review-form-actions">
          <button
            className="primary-button"
            disabled={
              store.pending ||
              Object.values(draft).every((value) => !value.trim())
            }
            onClick={() => void save()}
          >
            チェックインを記録
          </button>
        </div>
      </div>

      {checkins.length > 0 && (
        <div className="review-checkins">
          <h4>チェックインの履歴</h4>
          <ul>
            {checkins.map((checkin) => (
              <li
                key={checkin.id}
                data-standing={checkin.standing}
                data-superseded={checkin.superseded}
              >
                <div className="review-checkin-head">
                  <strong>{healthLabel(checkin.health)}</strong>
                  <small>
                    {checkin.createdAt.slice(0, 16).replace("T", " ")} ・{" "}
                    {checkin.author}
                  </small>
                  {checkin.superseded && <small>訂正済み</small>}
                  {checkin.supersedesId && <small>訂正</small>}
                </div>
                {checkin.comment && <p>{checkin.comment}</p>}
                {checkin.results && <p>成果: {checkin.results}</p>}
                {checkin.blockers && <p>課題: {checkin.blockers}</p>}
                {checkin.nextFocus && <p>次の一手: {checkin.nextFocus}</p>}
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="review-plan-history">
        <div className="section-header">
          <h4>方針の確定履歴</h4>
          <span>{itemVersions.length}版</span>
        </div>
        <p className="review-note">
          この目標に反映された変更案の版、根拠、確認者、適用者を確認できます。チェックインや実行記録は取り消しません。
        </p>
        {itemVersions.length === 0 ? (
          <p className="empty-value">この目標に反映された変更案の履歴はありません。</p>
        ) : (
          <ol className="review-plan-history-list">
            {[...itemVersions].reverse().map((version) => (
              <li key={version.id} className="review-plan-history-version">
                <strong>{version.title}</strong>
                <small>
                  {version.applied_at
                    ? new Date(version.applied_at).toLocaleString("ja-JP")
                    : "日時不明"}
                  {version.kind === "audit"
                    ? " · 直接操作 "
                    : " · 提案者 "}
                  {version.actor ?? version.proposed_by ?? "不明"}
                  {version.kind !== "audit" && (
                    <>
                      {" · 承認者 "}
                      {version.approved_by ??
                        (version.applied_by_connection ? "事前承認" : "承認記録なし")}
                    </>
                  )}
                  {" · 適用者 "}
                  {version.applied_by ?? "不明"}
                </small>
                {version.undo_of && <small>変更 {version.undo_of} の取り消しを反映</small>}
                {version.conversation_id && version.source_status === "available" ? (
                  <small>出典会話: {version.conversation_id}</small>
                ) : version.conversation_id && version.source_status === "unavailable" ? (
                  <small>出典会話は失効または参照権限がなく、引用を表示できません。</small>
                ) : null}
                <p>
                  {version.undo_reason ||
                    version.assumptions?.join(" / ") ||
                    "変更理由の記録はありません。"}
                </p>
                <ul>
                  {version.changes.map((change, index) => (
                    <li key={change.operation_index ?? index} className="review-plan-history-change">
                      <div>
                        <strong>{change.title || entry.title}</strong>
                        <ul>
                          {comparedFields(change).map((field, fieldIndex) => (
                            <li key={fieldIndex}>{field}</li>
                          ))}
                        </ul>
                        {change.match_rationale && (
                          <small className="review-history-evidence">
                            一致・変更の根拠: {change.match_rationale}
                          </small>
                        )}
                        {change.basis && (
                          <small className="review-history-evidence">
                            出典の説明: {change.basis}
                          </small>
                        )}
                        {change.interpretation && (
                          <div className="review-history-evidence">
                            {(change.interpretation.status || change.interpretation.origin) && (
                              <small>
                                解釈: {change.interpretation.status ?? "状態不明"}
                                {change.interpretation.origin
                                  ? ` · ${change.interpretation.origin}`
                                  : ""}
                              </small>
                            )}
                            {(change.interpretation.speaker || change.interpretation.at) && (
                              <small>
                                {change.interpretation.speaker ?? "話者不明"}
                                {change.interpretation.at ? ` · ${change.interpretation.at}` : ""}
                              </small>
                            )}
                            {change.interpretation.source_ref && (
                              <small>参照: {change.interpretation.source_ref}</small>
                            )}
                            {change.interpretation.quote && (
                              <blockquote>{change.interpretation.quote}</blockquote>
                            )}
                            {change.interpretation.reason && (
                              <small>判断理由: {change.interpretation.reason}</small>
                            )}
                            {change.interpretation.assumptions?.map((assumption, assumptionIndex) => (
                              <small key={assumptionIndex}>前提: {assumption}</small>
                            ))}
                            {safeSourceUrl(change.interpretation.source_url) && (
                              <a
                                href={safeSourceUrl(change.interpretation.source_url)}
                                target="_blank"
                                rel="noreferrer"
                              >
                                根拠ソースを開く
                              </a>
                            )}
                          </div>
                        )}
                      </div>
                      {version.undoable !== false && change.undoable !== false &&
                        typeof change.operation_index === "number" && (
                        <button
                          className="secondary-button"
                          onClick={() => {
                            setUndoTarget({
                              versionId: version.id,
                              operationIndex: change.operation_index!,
                            });
                            setUndoReason("");
                          }}
                        >
                          この変更を取り消す
                        </button>
                      )}
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ol>
        )}

        {itemVersions.length > 1 && (
          <div className="review-history-compare">
            <h5>確定版を比較</h5>
            <div className="review-checkin-form">
              <label>
                <span>比較元</span>
                <select
                  value={compareFrom}
                  onChange={(event) => {
                    setCompareFrom(event.target.value);
                    setComparison(null);
                  }}
                >
                  {itemVersions.map((version) => (
                    <option key={version.id} value={version.id}>
                      {version.title} · {version.applied_at?.slice(0, 10) ?? "日時不明"}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                <span>比較先</span>
                <select
                  value={compareTo}
                  onChange={(event) => {
                    setCompareTo(event.target.value);
                    setComparison(null);
                  }}
                >
                  {itemVersions.map((version) => (
                    <option key={version.id} value={version.id}>
                      {version.title} · {version.applied_at?.slice(0, 10) ?? "日時不明"}
                    </option>
                  ))}
                </select>
              </label>
              <div className="review-form-actions">
                <button
                  className="secondary-button"
                  disabled={!compareFrom || !compareTo || compareFrom === compareTo}
                  onClick={() => void compare()}
                >
                  版間の差分を見る
                </button>
              </div>
            </div>
            {comparison && (
              <div className="review-history-comparison">
                {!comparison.coverage_complete && (
                  <p className="conflict-note">
                    版の間にchangesetで記録されていない変更が含まれる可能性があります。以下は保存された変更記録の差分です。
                  </p>
                )}
                <ul>
                  {comparison.changes
                    .filter((change) => changesItem(change, entry.id))
                    .map((change, index) => (
                      <li key={change.id + ":" + index}>
                        <strong>{change.title || entry.title}</strong>
                        <ul>
                          {comparedFields(change).map((field, fieldIndex) => (
                            <li key={fieldIndex}>{field}</li>
                          ))}
                        </ul>
                      </li>
                    ))}
                </ul>
              </div>
            )}
          </div>
        )}

        {undoTarget && (
          <div className="review-history-undo">
            <label>
              <span>取り消す理由</span>
              <textarea
                value={undoReason}
                maxLength={500}
                onChange={(event) => setUndoReason(event.target.value)}
              />
            </label>
            <p className="review-note">
              逆変更案を作成して差分を開きます。内容を確認して明示的に承認するまで計画は変わりません。
            </p>
            <div className="review-form-actions">
              <button
                className="secondary-button"
                onClick={() => setUndoTarget(null)}
                disabled={undoPending}
              >
                戻る
              </button>
              <button
                className="primary-button"
                disabled={undoPending || !undoReason.trim()}
                onClick={() => void createUndoProposal()}
              >
                {undoPending ? "作成中…" : "逆変更案を作って差分を確認"}
              </button>
            </div>
          </div>
        )}
      </div>

      {timeline && timeline.events.length > 0 && (
        <div className="review-timeline">
          <h4>この目標に起きたこと</h4>
          <ol>
            {[...timeline.events].reverse().map((event, index) => (
              <li key={`${event.at}-${index}`} data-kind={event.kind}>
                <span className="review-event-kind">
                  {eventLabel(event.kind)}
                </span>
                <span className="review-event-at">{event.at.slice(0, 10)}</span>
                <span className="review-event-summary">{event.summary}</span>
                {event.actor && (
                  <small>
                    {event.actor}
                    {event.connection && `（接続 ${event.connection}）`}
                  </small>
                )}
              </li>
            ))}
          </ol>
        </div>
      )}
    </section>
  );
}

export function ReviewScreen({
  store,
  workspace,
}: {
  store: WorkspaceStore;
  workspace?: Workspace;
}) {
  const [queue, setQueue] = useState<ReviewQueue | null>(null);
  const [open, setOpen] = useState<ReviewEntry | null>(null);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    if (!workspace) return;
    setError("");
    try {
      const value = await request<unknown>(
        "GET",
        `/v1/workspaces/${workspace.id}/review`,
      );
      setQueue(reviewQueueFrom(value));
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "レビュー一覧を読み込めませんでした",
      );
    }
  }, [workspace]);

  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    setOpen(null);
  }, [workspace?.id]);

  return (
    <div className="page-content review-screen">
      <section className="panel review-intro">
        <div>
          <span className="eyebrow">{workspace?.name}</span>
          <p>
            誰も何も言っていない目標と、問題があると言われた目標は別のリストです。
            沈黙は警告ではありません。
          </p>
        </div>
      </section>

      {error && (
        <p className="auth-error" role="alert">
          {error}
        </p>
      )}

      {open && workspace && (
        <History
          workspaceId={workspace.id}
          entry={open}
          store={store}
          onClose={() => {
            setOpen(null);
            void load();
          }}
        />
      )}

      {queue && (
        <>
          <List
            title="問題があると言われた目標"
            note="誰かが注意・遅れとして記録したものです。"
            entries={queue.atRisk}
            onOpen={setOpen}
          />
          <List
            title="誰も何も言っていない目標"
            note="一度もチェックインがない目標です。問題があるとは限りません。"
            entries={queue.neverCheckedIn}
            onOpen={setOpen}
          />
          <List
            title={`${queue.staleDays}日以上更新のない目標`}
            note="以前は記録があり、そこから止まっているものです。"
            entries={queue.stale}
            onOpen={setOpen}
          />
          <List
            title="最近更新された目標"
            note="この期間に記録があった目標です。"
            entries={queue.recentlyUpdated}
            onOpen={setOpen}
          />
        </>
      )}
    </div>
  );
}
