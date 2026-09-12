import { useEffect, useRef, useState, type FormEvent } from "react";
import { Icon } from "./icons";
import {
  ApiError,
  dateLabel,
  itemPath,
  localDate,
  uiId,
  type Item,
  type Metric,
  type Snapshot,
  type Observation,
  type Operation,
} from "./api";
import type { WorkspaceStore } from "./useWorkspace";
export const stateNames: Record<string, string> = {
  draft: "下書き",
  active: "進行中",
  paused: "休止中",
  done: "完了",
  abandoned: "見送り",
};
const kindNames: Record<string, string> = {
  idea: "アイデア",
  outcome: "目標",
  initiative: "取り組み",
  action: "行動",
  milestone: "節目",
};
export function MemoEditor({
  item,
  store,
}: {
  item: Item;
  store: WorkspaceStore;
}) {
  const key = `pathbase:memo:${store.me.id}:${uiId(item)}`;
  const [initial] = useState(() => {
    const value = localStorage.getItem(key);
    if (value !== null) {
      try {
        const saved = JSON.parse(value);
        if (
          typeof saved.value === "string" &&
          typeof saved.version === "number"
        )
          return saved as { value: string; version: number; original: string };
      } catch {}
      return { value, version: -1, original: item.fields.memo || "" };
    }
    return {
      value: item.fields.memo || "",
      version: item.version,
      original: item.fields.memo || "",
    };
  });
  const [draft, setDraft] = useState(initial.value);
  const [original, setOriginal] = useState(initial.original);
  const [base, setBase] = useState(initial.version);
  const dirty = draft !== original;
  const conflict = dirty && base !== item.version;
  useEffect(() => {
    if (!dirty && item.version > base) {
      setDraft(item.fields.memo || "");
      setOriginal(item.fields.memo || "");
      setBase(item.version);
    }
  }, [item.version, item.fields.memo, dirty, base]);
  function persist(value: string, version = base) {
    localStorage.setItem(key, JSON.stringify({ value, version, original }));
  }
  return (
    <>
      <textarea
        className="field-box memo-input"
        aria-label="目標のメモ"
        readOnly={store.workspaces.find((w) => w.id === item.workspace_id)?.role === "viewer"}
        value={draft}
        placeholder="この目標についてメモを残す"
        onChange={(e) => {
          setDraft(e.target.value);
          persist(e.target.value);
        }}
      />
      {conflict && (
        <div className="conflict-note">
          <p>保存後に更新があります。最新のメモ：</p>
          <p>{item.fields.memo || "（空欄）"}</p>
          <button
            type="button"
            onClick={() => {
              setBase(item.version);
              persist(draft, item.version);
            }}
          >
            内容を確認し、自分の入力を保存対象にする
          </button>
        </div>
      )}
      {dirty && (
        <button
          className="text-link"
          disabled={store.pending || conflict || store.workspaces.find((w) => w.id === item.workspace_id)?.role === "viewer"}
          onClick={() =>
            void store.run(async () => {
              const saved = await store.write<Item>("PATCH", itemPath(item), {
                expected_version: base,
                fields: { memo: draft },
              });
              setBase(saved.version);
              setOriginal(draft);
              localStorage.removeItem(key);
            })
          }
        >
          メモを保存 <Icon name="check" size={13} />
        </button>
      )}
    </>
  );
}
function latestObservation(metric: Metric, observations: Observation[]) {
  const own = observations.filter(
    (o) => o.metric_id === metric.id && o.workspace_id === metric.workspace_id,
  );
  const superseded = new Set(own.map((o) => o.supersedes_id).filter(Boolean));
  return own
    .filter((o) => !superseded.has(o.id))
    .sort(
      (a, b) =>
        b.observed_at.localeCompare(a.observed_at) ||
        b.created_at.localeCompare(a.created_at),
    )[0];
}
function MetricValue({
  metric,
  observations,
}: {
  metric: Metric;
  observations: Observation[];
}) {
  const latest = latestObservation(metric, observations);
  if (!latest)
    return (
      <div className="metric-value">
        <strong>{metric.name}</strong>
        <span>
          未計測 · 目標 {metric.target} {metric.unit}
        </span>
      </div>
    );
  const ratio =
    metric.direction === "threshold"
      ? latest.value >= metric.target
        ? 100
        : 0
      : (100 * (latest.value - metric.baseline)) /
        (metric.target - metric.baseline);
  const stale =
    Date.now() - new Date(latest.observed_at).getTime() > 30 * 86400000;
  return (
    <div className="metric-value">
      <strong>{metric.name}</strong>
      <div className="progress purple">
        <span
          className="progress-track"
          role="progressbar"
          aria-label={metric.name}
          aria-valuenow={Math.max(0, Math.min(100, ratio))}
        >
          <span style={{ width: `${Math.max(0, Math.min(100, ratio))}%` }} />
        </span>
        <strong>{Math.round(ratio)}%</strong>
      </div>
      <span>
        {latest.value} / {metric.target} {metric.unit}
      </span>
      <small>
        {new Date(latest.observed_at).toLocaleDateString("ja-JP")} ·{" "}
        {latest.source}
        {stale ? " · 更新が必要" : ""}
      </small>
    </div>
  );
}
export function Evaluation({
  item,
  snapshots,
}: {
  item: Item;
  snapshots: Snapshot[];
}) {
  const snapshot = snapshots.find((s) => s.workspace_id === item.workspace_id);
  const metrics = snapshot?.metrics.filter((m) => m.item_id === item.id) || [];
  return (
    <>
      {metrics.map((m) => (
        <MetricValue
          key={m.id}
          metric={m}
          observations={snapshot?.observations || []}
        />
      ))}
      {!metrics.length &&
        (item.fields.self_assessment == null ? (
          <span className="empty-value">評価未設定</span>
        ) : (
          <>
            <div className="progress purple">
              <span
                className="progress-track"
                role="progressbar"
                aria-label="目標の自己評価"
                aria-valuenow={item.fields.self_assessment}
              >
                <span style={{ width: `${item.fields.self_assessment}%` }} />
              </span>
              <strong>{item.fields.self_assessment}%</strong>
            </div>
            <small className="evaluation-label">
              自己評価 ·{" "}
              {item.fields.assessed_at
                ? new Date(item.fields.assessed_at).toLocaleDateString("ja-JP")
                : "日時未記録"}
            </small>
          </>
        ))}
    </>
  );
}
export function MetricEditor({
  item,
  store,
}: {
  item: Item;
  store: WorkspaceStore;
}) {
  const snapshot = store.snapshots.find(
    (s) => s.workspace_id === item.workspace_id,
  )!;
  const metrics = snapshot.metrics.filter((m) => m.item_id === item.id);
  const [metricId, setMetricId] = useState(metrics[0]?.id || "");
  const [adding, setAdding] = useState(!metrics.length);
  const base = `/v1/workspaces/${item.workspace_id}`;
  const current = metrics.find((m) => m.id === metricId) || metrics[0];
  async function add(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const form = e.currentTarget;
    const d = new FormData(form);
    await store.run(
      async () => {
        const m = await store.write<Metric>("POST", `${base}/metrics`, {
          item_id: item.id,
          name: d.get("name"),
          unit: d.get("unit"),
          baseline: Number(d.get("baseline")),
          target: Number(d.get("target")),
          direction: d.get("direction"),
          period_start: d.get("period_start") || null,
          period_end: d.get("period_end") || null,
        });
        setMetricId(m.id);
      },
      () => {
        setAdding(false);
      },
    );
  }
  async function observe(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    if (!current) return;
    const form = e.currentTarget;
    const d = new FormData(form);
    await store.run(
      () =>
        store.write("POST", `${base}/observations`, {
          metric_id: current.id,
          value: Number(d.get("value")),
          unit: current.unit,
          source: d.get("source"),
          observed_at: new Date(String(d.get("observed_at"))).toISOString(),
          supersedes_id: d.get("supersedes_id") || null,
        }),
      () => form.reset(),
    );
  }
  return (
    <div className="feature-editor">
      <p className="modal-intro">
        行動の完了と、実際の成果は別に記録します。訂正した観測も履歴に残ります。
      </p>
      <Evaluation item={item} snapshots={store.snapshots} />
      <button className="text-link" onClick={() => setAdding(!adding)}>
        {adding ? "指標の入力を閉じる" : "指標を追加"}
      </button>
      {adding && (
        <form className="editor-form" onSubmit={add}>
          <fieldset disabled={store.pending}>
            <label>
              指標の名前
              <input
                name="name"
                required
                maxLength={200}
                placeholder="例：英語で会話できた時間"
              />
            </label>
            <div className="form-columns">
              <label>
                単位
                <input
                  name="unit"
                  required
                  maxLength={40}
                  placeholder="分、件、円など"
                />
              </label>
              <label>
                評価方向
                <select name="direction">
                  <option value="increase">増やす</option>
                  <option value="decrease">減らす</option>
                  <option value="threshold">閾値以上で達成</option>
                </select>
              </label>
            </div>
            <div className="form-columns">
              <label>
                基準値
                <input
                  name="baseline"
                  type="number"
                  step="any"
                  required
                  defaultValue="0"
                />
              </label>
              <label>
                目標値
                <input name="target" type="number" step="any" required />
              </label>
            </div>
            <div className="form-columns">
              <label>
                期間の開始
                <input name="period_start" type="date" />
              </label>
              <label>
                期間の終了
                <input name="period_end" type="date" />
              </label>
            </div>
            <button className="primary-button">指標を保存</button>
          </fieldset>
        </form>
      )}
      {!!metrics.length && (
        <form className="editor-form" onSubmit={observe}>
          <fieldset disabled={store.pending}>
            <label>
              記録する指標
              <select
                value={current?.id}
                onChange={(e) => setMetricId(e.target.value)}
              >
                {metrics.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}（{m.unit}）
                  </option>
                ))}
              </select>
            </label>
            <div className="form-columns">
              <label>
                観測値
                <input name="value" type="number" step="any" required />
              </label>
              <label>
                観測日時
                <input
                  name="observed_at"
                  type="datetime-local"
                  required
                  defaultValue={new Date(
                    Date.now() - new Date().getTimezoneOffset() * 60000,
                  )
                    .toISOString()
                    .slice(0, 16)}
                />
              </label>
            </div>
            <label>
              出典・確かめた方法
              <input
                name="source"
                required
                maxLength={500}
                placeholder="例：英会話レッスンの記録"
              />
            </label>
            <label>
              訂正対象
              <select name="supersedes_id">
                <option value="">新しい観測として記録</option>
                {snapshot.observations
                  .filter(
                    (o) =>
                      o.metric_id === current?.id &&
                      !snapshot.observations.some(
                        (x) => x.supersedes_id === o.id,
                      ),
                  )
                  .map((o) => (
                    <option key={o.id} value={o.id}>
                      {o.value} {o.unit} ·{" "}
                      {new Date(o.observed_at).toLocaleDateString("ja-JP")}
                    </option>
                  ))}
              </select>
            </label>
            <button className="primary-button">成果を記録</button>
          </fieldset>
        </form>
      )}
      <details>
        <summary>観測履歴（訂正前を含む）</summary>
        {snapshot.observations
          .filter((o) => metrics.some((m) => m.id === o.metric_id))
          .map((o) => (
            <p key={o.id}>
              {o.value} {o.unit} · {o.source} ·{" "}
              {new Date(o.observed_at).toLocaleString("ja-JP")}
              {o.supersedes_id ? "（訂正）" : ""}
            </p>
          ))}
      </details>
    </div>
  );
}
const relationNames = {
  part_of: "整理上の親",
  contributes_to: "貢献する目標",
  depends_on: "前提になる項目",
  relates_to: "関連する項目",
};
export function RelationsEditor({
  item,
  store,
}: {
  item: Item;
  store: WorkspaceStore;
}) {
  const snapshot = store.snapshots.find(
    (s) => s.workspace_id === item.workspace_id,
  )!;
  const relations = snapshot.relations.filter(
    (r) => r.source_id === item.id || r.target_id === item.id,
  );
  const candidates = snapshot.items.filter(
    (i) => i.id !== item.id && !i.archived_at,
  );
  const base = `/v1/workspaces/${item.workspace_id}`;
  const [showRemove, setShowRemove] = useState<string | null>(null);
  async function add(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const f = e.currentTarget;
    const d = new FormData(f);
    await store.run(
      () =>
        store.write("POST", `${base}/relations`, {
          source_id: item.id,
          target_id: d.get("target_id"),
          type: d.get("type"),
          rationale: d.get("rationale"),
        }),
      () => f.reset(),
    );
  }
  return (
    <div className="feature-editor">
      <p className="modal-intro">
        {item.title}
        から見たつながり。同じ領域の項目を関連付けます。成果や権限は自動で移りません。
      </p>
      <div className="relation-list">
        {!relations.length && (
          <p className="empty-value">まだつながりはありません。</p>
        )}
        {relations.map((r) => {
          const other = snapshot.items.find(
            (i) =>
              i.id === (r.source_id === item.id ? r.target_id : r.source_id),
          );
          return (
            <div key={r.id}>
              <span>
                <small>
                  {r.source_id === item.id ? "→" : "←"} {relationNames[r.type]}
                </small>
                {other?.title || "アーカイブした項目"}
                {r.rationale && <small>{r.rationale}</small>}
              </span>
              <button
                type="button"
                className="text-link"
                onClick={() => setShowRemove(r.id)}
              >
                解除
              </button>
              {showRemove === r.id && (
                <button
                  type="button"
                  disabled={store.pending}
                  onClick={() =>
                    void store.run(
                      () =>
                        store.write("DELETE", `${base}/relations/${r.id}`, {
                          expected_version: r.version,
                        }),
                      () => setShowRemove(null),
                    )
                  }
                >
                  このつながりを解除する
                </button>
              )}
            </div>
          );
        })}
      </div>
      <form className="editor-form" onSubmit={add}>
        <fieldset disabled={store.pending || !candidates.length}>
          <label>
            関係
            <select name="type">
              {Object.entries(relationNames).map(([v, l]) => (
                <option key={v} value={v}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <label>
            対象
            <select name="target_id" required>
              {candidates.map((i) => (
                <option key={i.id} value={i.id}>
                  {kindNames[i.kind]} · {i.title}
                </option>
              ))}
            </select>
          </label>
          <label>
            つながりの理由
            <input name="rationale" maxLength={500} />
          </label>
          <button className="primary-button">つながりを追加</button>
        </fieldset>
      </form>
      {item.kind !== "action" && (
        <form
          className="editor-form"
          onSubmit={(e) => {
            e.preventDefault();
            const d = new FormData(e.currentTarget);
            void store.run(() =>
              store.write("PATCH", itemPath(item), {
                expected_version: item.version,
                fields: { next_action_id: d.get("next_action_id") || null },
              }),
            );
          }}
        >
          <fieldset disabled={store.pending}>
            <label>
              次の一歩
              <select
                name="next_action_id"
                defaultValue={item.fields.next_action_id || ""}
              >
                <option value="">未設定</option>
                {candidates
                  .filter((i) => i.kind === "action")
                  .map((i) => (
                    <option key={i.id} value={i.id}>
                      {i.title}
                    </option>
                  ))}
              </select>
            </label>
            <button className="secondary-button">次の一歩を保存</button>
          </fieldset>
        </form>
      )}
    </div>
  );
}
export function ItemEditor({
  item,
  store,
  onClose,
}: {
  item: Item;
  store: WorkspaceStore;
  onClose: () => void;
}) {
  const [version, setVersion] = useState(item.version);
  const [archiving, setArchiving] = useState(false);
  const [record, setRecord] = useState("");
  const [recordType, setRecordType] = useState("note");
  const [day, setDay] = useState(localDate(store.settings.timezone));
  const snapshot = store.snapshots.find(
    (s) => s.workspace_id === item.workspace_id,
  )!;
  const history = snapshot.records
    .filter((r) => r.item_ids.includes(item.id))
    .sort((a, b) => b.created_at.localeCompare(a.created_at));
  const conflict = version !== item.version;
  async function save(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const d = new FormData(e.currentTarget);
    const frequency = Number(d.get("frequency") || 0);
    const assessment = String(d.get("assessment") || "");
    await store.run(
      () =>
        store.write("PATCH", itemPath(item), {
          expected_version: version,
          title: d.get("title"),
          description: d.get("description"),
          state: d.get("state"),
          start_date: d.get("start_date") || null,
          due_date: d.get("due_date") || null,
          scheduled_date: d.get("scheduled_date") || null,
          scheduled_time: d.get("scheduled_time") || null,
          fields: {
            self_assessment: assessment === "" ? null : Number(assessment),
            ...(item.kind === "action"
              ? {
                  recurrence: frequency
                    ? frequency === item.fields.recurrence?.times_per_week
                      ? item.fields.recurrence
                      : {
                          mode: "period_quota",
                          times_per_week: frequency,
                          weekdays: [],
                          timezone: store.settings.timezone,
                        }
                    : null,
                }
              : {}),
          },
        }),
      onClose,
    );
  }
  return (
    <div className="feature-editor">
      {item.fields.field_reference && (
        <p className="empty-value">
          Fieldの営業タスクを参照しています。元のタスクの更新・完了はFieldで行えます。
          <br />
          参照元の状態：
          {item.fields.field_reference.source_status === "done"
            ? "完了"
            : "未完了"}{" "}
          ·{" "}
          {new Date(
            item.fields.field_reference.source_updated_at,
          ).toLocaleDateString("ja-JP")}
          時点
        </p>
      )}
      <p className="modal-intro">
        {kindNames[item.kind]} · {stateNames[item.state]} ·{" "}
        {dateLabel(item.due_date)}
      </p>
      {conflict && (
        <div className="conflict-note">
          <p>
            更新があります。最新：{item.title} · {stateNames[item.state]}
          </p>
          <button onClick={() => setVersion(item.version)}>
            最新の内容を確認し、自分の入力で更新する
          </button>
        </div>
      )}
      <form className="editor-form" onSubmit={save}>
        <fieldset disabled={store.pending || conflict}>
          <label>
            名前
            <input
              name="title"
              defaultValue={item.title}
              required
              maxLength={200}
            />
          </label>
          <label>
            目的・説明
            <textarea
              name="description"
              defaultValue={item.description}
              maxLength={10000}
            />
          </label>
          <label>
            状態
            <select name="state" defaultValue={item.state}>
              {Object.entries(stateNames)
                .filter(
                  ([v]) =>
                    v !== "done" ||
                    item.kind !== "action" ||
                    item.state === "done",
                )
                .map(([v, l]) => (
                  <option key={v} value={v}>
                    {l}
                  </option>
                ))}
            </select>
          </label>
          <div className="form-columns">
            <label>
              開始日
              <input
                type="date"
                name="start_date"
                defaultValue={item.start_date || ""}
              />
            </label>
            <label>
              終了日・期限
              <input
                type="date"
                name="due_date"
                defaultValue={item.due_date || ""}
              />
            </label>
          </div>
          {item.kind === "action" ? (
            <>
              <div className="form-columns">
                <label>
                  行動の日付
                  <input
                    type="date"
                    name="scheduled_date"
                    defaultValue={item.scheduled_date || ""}
                  />
                </label>
                <label>
                  時刻
                  <input
                    type="time"
                    name="scheduled_time"
                    defaultValue={item.scheduled_time || ""}
                  />
                </label>
              </div>
              <label>
                繰り返し
                <select
                  name="frequency"
                  defaultValue={item.fields.recurrence?.times_per_week || 0}
                >
                  <option value="0">繰り返さない</option>
                  {[1, 2, 3, 4, 5, 6, 7].map((n) => (
                    <option value={n} key={n}>
                      週{n}回
                    </option>
                  ))}
                </select>
              </label>
            </>
          ) : (
            <label>
              自己評価（%・任意）
              <input
                name="assessment"
                type="number"
                min="0"
                max="100"
                defaultValue={item.fields.self_assessment ?? ""}
              />
            </label>
          )}
          <button className="primary-button">変更を保存</button>
        </fieldset>
      </form>
      {item.kind === "action" && (
        <div className="occurrence-editor">
          <label>
            実施した日
            <input
              type="date"
              value={day}
              onChange={(e) => setDay(e.target.value)}
            />
          </label>
          <div className="button-row">
            {["complete", "skip", "reopen"].map((action, i) => (
              <button
                key={action}
                disabled={store.pending || !day}
                className="secondary-button"
                onClick={() =>
                  void store.run(async () => {
                    const saved = await store.write<{ item: Item }>(
                      "POST",
                      `/v1/workspaces/${item.workspace_id}/actions/${item.id}/${action}`,
                      { expected_version: item.version, local_date: day },
                    );
                    setVersion(saved.item.version);
                  })
                }
              >
                {
                  ["この日の完了を記録", "今回は見送り", "記録を未完了に戻す"][
                    i
                  ]
                }
              </button>
            ))}
          </div>
          <p className="empty-value">
            習慣は実施日ごとに保存します。休止中は記録されません。
          </p>
        </div>
      )}
      <details>
        <summary>関連・依存関係</summary>
        <RelationsEditor item={item} store={store} />
      </details>
      <details>
        <summary>メモ・学び・履歴</summary>
        <form
          className="editor-form"
          onSubmit={(e) => {
            e.preventDefault();
            void store.run(
              () =>
                store.write(
                  "POST",
                  `/v1/workspaces/${item.workspace_id}/records`,
                  {
                    item_ids: [item.id],
                    record_type: recordType,
                    body: record,
                  },
                ),
              () => setRecord(""),
            );
          }}
        >
          <label>
            種類
            <select
              value={recordType}
              onChange={(e) => setRecordType(e.target.value)}
            >
              <option value="note">メモ</option>
              <option value="learning">学び</option>
              <option value="review">振り返り</option>
            </select>
          </label>
          <textarea
            aria-label="項目に記録を追加"
            required
            value={record}
            onChange={(e) => setRecord(e.target.value)}
          />
          <button disabled={store.pending} className="primary-button">
            記録する
          </button>
        </form>
        {history.map((r) => (
          <p key={r.id}>
            {r.body ||
              { completion: "完了", skip: "見送り", reopen: "未完了に戻す" }[
                r.record_type
              ] ||
              r.record_type}{" "}
            <small>{new Date(r.happened_at).toLocaleString("ja-JP")}</small>
          </p>
        ))}
      </details>
      <button className="text-link" onClick={() => setArchiving(!archiving)}>
        {item.archived_at ? "アーカイブから戻す" : "アーカイブする"}
      </button>
      {archiving && (
        <div className="conflict-note">
          <p>
            {item.archived_at
              ? "一覧に戻します。"
              : "通常の一覧から外します。設定画面で復元できます。"}
          </p>
          <button
            className="secondary-button"
            disabled={store.pending}
            onClick={() =>
              void store.run(
                () =>
                  store.write("PATCH", itemPath(item), {
                    expected_version: item.version,
                    archived_at: item.archived_at
                      ? null
                      : new Date().toISOString(),
                  }),
                onClose,
              )
            }
          >
            {item.archived_at ? "復元する" : "アーカイブを確定"}
          </button>
        </div>
      )}
    </div>
  );
}
export function ItemList({
  items,
  onSelect,
}: {
  items: Item[];
  onSelect: (id: string) => void;
}) {
  const [filter, setFilter] = useState("all");
  const [query, setQuery] = useState("");
  const filtered = items.filter(
    (i) =>
      (filter === "all" || i.state === filter) &&
      i.title.toLowerCase().includes(query.toLowerCase()),
  );
  return (
    <div className="item-list">
      <div className="list-tools">
        <input
          aria-label="一覧を検索"
          placeholder="名前で絞り込み"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <select
          aria-label="状態で絞り込み"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        >
          <option value="all">すべての状態</option>
          {Object.entries(stateNames).map(([v, l]) => (
            <option key={v} value={v}>
              {l}
            </option>
          ))}
        </select>
      </div>
      {!filtered.length && (
        <p className="empty-value">
          {items.length
            ? "条件に一致する項目はありません。"
            : "項目を追加して、ここから始めましょう。"}
        </p>
      )}
      {filtered.map((i) => (
        <button
          className="item-list-row"
          key={uiId(i)}
          onClick={() => onSelect(uiId(i))}
        >
          <Icon name={i.fields.icon || "target"} size={18} />
          <span>
            <strong>{i.title}</strong>
            <small>
              {kindNames[i.kind]} · {stateNames[i.state]}
            </small>
          </span>
          <time>{dateLabel(i.due_date || i.scheduled_date)}</time>
        </button>
      ))}
    </div>
  );
}
function ChangePreview({
  operations,
  store,
}: {
  operations: Operation[];
  store: WorkspaceStore;
}) {
  const labels: Record<string, string> = {
    title: "名前",
    description: "目的・説明",
    kind: "種類",
    state: "状態",
    start_date: "開始日",
    due_date: "期限",
    scheduled_date: "実施日",
    scheduled_time: "時刻",
    local_date: "記録する日",
    archived_at: "アーカイブ日時",
    parent_id: "親の項目",
    source_id: "起点",
    target_id: "対象",
    type: "関係",
    rationale: "つながりの理由",
    item_ids: "関連する項目",
    item_id: "項目",
    body: "記録内容",
    record_type: "記録の種類",
    happened_at: "出来事の日時",
    metric_id: "指標",
    value: "観測値",
    unit: "単位",
    source: "出典",
    observed_at: "観測日時",
    supersedes_id: "訂正する記録",
    name: "名前",
    baseline: "基準値",
    target: "目標値",
    direction: "評価方向",
    period_start: "期間の開始",
    period_end: "期間の終了",
    fields: "補足",
    memo: "メモ",
    self_assessment: "自己評価",
    next_action_id: "次の一歩",
    recurrence: "繰り返し",
    external_url: "参考リンク",
    mode: "繰り返し方",
    times_per_week: "週あたりの回数",
    weekdays: "曜日",
    timezone: "日付の基準",
    icon: "アイコン",
    subtitle: "副題",
    template: "テンプレート",
    template_version: "テンプレートの版",
  };
  return (
    <ul className="change-preview">
      {operations.map((op, n) => {
        const parts = op.path.split("/");
        const snapshot = store.snapshots.find(
          (s) => s.workspace_id === parts[3],
        );
        const item = snapshot?.items.find((i) => i.id === parts[5]);
        const body = op.body as Record<string, unknown>;
        const display = (v: unknown): string => {
          if (v === null) return "未設定";
          if (Array.isArray(v)) return v.map(display).join("、");
          if (typeof v === "object")
            return Object.entries(v as Record<string, unknown>)
              .map(([k, x]) => `${labels[k] || k}：${display(x)}`)
              .join(" / ");
          if (typeof v === "string")
            return (
              snapshot?.items.find((i) => i.id === v)?.title ||
              snapshot?.metrics.find((m) => m.id === v)?.name ||
              stateNames[v] ||
              kindNames[v] ||
              (relationNames as Record<string, string>)[v] ||
              (
                {
                  increase: "増やす",
                  decrease: "減らす",
                  threshold: "閾値以上",
                  period_quota: "週の回数",
                  fixed_schedule: "曜日指定",
                  note: "メモ",
                  review: "振り返り",
                  learning: "学び",
                } as Record<string, string>
              )[v] ||
              v
            );
          return String(v);
        };
        return (
          <li key={n}>
            <strong>
              {item?.title ||
                (typeof body.title === "string"
                  ? body.title
                  : "新しい項目・記録")}
            </strong>
            <span>
              {op.method === "PATCH"
                ? "内容を更新"
                : op.method === "DELETE"
                  ? "つながりを解除"
                  : op.path.endsWith("/complete")
                    ? "行動を完了"
                    : op.path.endsWith("/skip")
                      ? "行動を見送り"
                      : op.path.endsWith("/reopen")
                        ? "行動を未完了に戻す"
                        : "追加"}
            </span>
            {item && (
              <small>
                現在：{item.title} · {stateNames[item.state]} ·{" "}
                {item.description || "説明なし"}
              </small>
            )}
            <dl>
              {Object.entries(body)
                .filter(([k]) => k !== "expected_version")
                .map(([k, v]) => (
                  <div key={k}>
                    <dt>{labels[k] || k}</dt>
                    <dd>{display(v)}</dd>
                  </div>
                ))}
            </dl>
          </li>
        );
      })}
    </ul>
  );
}
export function StorageSettings({
  store,
  workspaceId,
  onSelect,
}: {
  store: WorkspaceStore;
  workspaceId: string;
  onSelect: (id: string) => void;
}) {
  const [importText, setImportText] = useState("");
  const [fileError, setFileError] = useState("");
  const [exportWs, setExportWs] = useState(workspaceId);
  const input = useRef<HTMLInputElement>(null);
  const snapshot = store.snapshots.find((s) => s.workspace_id === exportWs);
  async function exportData() {
    await store.run(async () => {
      const data = await store.write(
        "POST",
        `/v1/workspaces/${exportWs}/exports`,
        {},
      );
      const url = URL.createObjectURL(
        new Blob([JSON.stringify(data, null, 2)], { type: "application/json" }),
      );
      const a = document.createElement("a");
      a.href = url;
      a.download = `pathbase-${exportWs}-${localDate()}.json`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    });
  }
  return (
    <div className="settings-list feature-editor">
      <div>
        <span>
          <strong>コンパクト表示</strong>
          <p>タスクの行間を小さく表示します</p>
        </span>
        <input
          type="checkbox"
          aria-label="コンパクト表示"
          checked={store.settings.compact}
          disabled={store.pending}
          onChange={(e) =>
            void store.run(() =>
              store.write("PATCH", "/v1/settings", {
                ...store.settings,
                compact: e.target.checked,
              }),
            )
          }
        />
      </div>
      <div>
        <span>
          <strong>アプリ内のお知らせ</strong>
          <p>今日の未完了の行動を表示</p>
        </span>
        <input
          type="checkbox"
          aria-label="アプリ内のお知らせ"
          checked={store.settings.notifications}
          disabled={store.pending}
          onChange={(e) =>
            void store.run(() =>
              store.write("PATCH", "/v1/settings", {
                ...store.settings,
                notifications: e.target.checked,
              }),
            )
          }
        />
      </div>
      <label>
        日付の基準
        <select
          aria-label="日付の基準"
          value={store.settings.timezone}
          disabled={store.pending}
          onChange={(e) =>
            void store.run(() =>
              store.write("PATCH", "/v1/settings", {
                ...store.settings,
                timezone: e.target.value,
              }),
            )
          }
        >
          {[
            "Asia/Tokyo",
            "UTC",
            "America/New_York",
            "America/Los_Angeles",
            "Europe/London",
          ].map((zone) => (
            <option value={zone} key={zone}>
              {zone}
            </option>
          ))}
        </select>
      </label>
      <p className="demo-note">
        {store.me.mode === "local-preview"
          ? "この端末に保存"
          : "アカウントの領域に保存"}
      </p>
      <details open>
        <summary>バックアップと復元</summary>
        <label>
          対象のワークスペース
          <select
            value={exportWs}
            onChange={(e) => {
              setExportWs(e.target.value);
              setImportText("");
            }}
          >
            {store.workspaces.map((w) => (
              <option key={w.id} value={w.id}>
                {w.name}
              </option>
            ))}
          </select>
        </label>
        <p className="empty-value">
          選んだ領域の項目・つながり・記録・評価方法をJSONに保存します。
        </p>
        <button
          className="secondary-button"
          onClick={() => void exportData()}
          disabled={store.pending}
        >
          JSONを書き出す
        </button>
        <input
          ref={input}
          aria-label="バックアップファイル"
          type="file"
          accept="application/json,.json"
          onChange={async (e) => {
            setFileError("");
            setImportText("");
            const f = e.target.files?.[0];
            if (!f) return;
            if (f.size > 8 * 1024 * 1024) {
              setFileError("8MB以下のファイルを選んでください");
              return;
            }
            const text = await f.text();
            try {
              const b = JSON.parse(text);
              if (b.schema_version !== 1 || !Array.isArray(b.items))
                throw new Error();
              setImportText(text);
            } catch {
              setFileError("PathBaseのバックアップ形式を確認してください");
            }
          }}
        />
        {fileError && <p role="alert">{fileError}</p>}
        {importText && (
          <div className="conflict-note">
            <p>
              {JSON.parse(importText).items.length}項目を
              {store.workspaces.find((w) => w.id === exportWs)?.name}
              へ追加します。同じIDがあれば全件取り消され、既存データは変更しません。
            </p>
            <button
              className="primary-button"
              disabled={store.pending}
              onClick={() =>
                void store.run(
                  () =>
                    store.write(
                      "POST",
                      `/v1/workspaces/${exportWs}/imports`,
                      JSON.parse(importText),
                    ),
                  () => {
                    setImportText("");
                    if (input.current) input.current.value = "";
                  },
                )
              }
            >
              内容を確認して取り込む
            </button>
          </div>
        )}
      </details>
      <details>
        <summary>アーカイブした項目</summary>
        {snapshot?.items
          .filter((i) => i.archived_at)
          .map((i) => (
            <button
              className="item-list-row"
              key={i.id}
              onClick={() => onSelect(uiId(i))}
            >
              {i.title}
              <span>詳細・復元</span>
            </button>
          ))}
      </details>
      <details>
        <summary>AIからの変更案</summary>
        <p className="empty-value">
          提案はこの画面で承認するまで適用されません。有効期限は30分です。
        </p>
        {store.snapshots
          .flatMap((s) => s.changesets)
          .filter((c) => c.status !== "applied")
          .map((c) => (
            <div className="pending-change" key={c.id}>
              <strong>{c.title}</strong>
              <small>
                {store.workspaces.find((w) => w.id === c.workspace_id)?.name} ·{" "}
                {new Date(c.expires_at).toLocaleString("ja-JP")}まで
              </small>
              <ChangePreview operations={c.operations} store={store} />
              <button
                className="primary-button"
                disabled={store.pending}
                onClick={() =>
                  void store.run(async () => {
                    if (c.status === "pending")
                      await store.write(
                        "POST",
                        `/v1/workspaces/${c.workspace_id}/changesets/${c.id}/approve`,
                        {},
                      );
                    await store.write(
                      "POST",
                      `/v1/workspaces/${c.workspace_id}/changesets/${c.id}/apply`,
                      {},
                    );
                  })
                }
              >
                差分を承認して適用
              </button>
            </div>
          ))}
      </details>
    </div>
  );
}
