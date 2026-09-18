/**
 * Planning periods, in Basepath.
 *
 * A period is a frame, not a container: work exists whether or not a period
 * contains it, and most workspaces never create one. So this screen leads with
 * what is there, offers a period only as something to add, and never treats
 * "no periods" as a setup step left undone.
 *
 * Carrying work forward copies it. The period that was already reviewed keeps
 * saying what was in it, and the copy points back at where it came from.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { ApiError, request, type Item, type Workspace } from "./api";
import type { WorkspaceStore } from "./useWorkspace";
import { PlanningBar } from "./shared/PlanningBar";
import {
  cadenceLabel,
  nextCycleBody,
  planningViewFrom,
  spanLabel,
  statusLabel,
  type CycleCadence,
  type PlanningView,
} from "./shared/planningView";

const CADENCES: CycleCadence[] = ["quarter", "month", "week", "custom"];

export function PlanningScreen({
  store,
  workspace,
  items,
  onOpenItem,
}: {
  store: WorkspaceStore;
  workspace?: Workspace;
  items: Item[];
  onOpenItem: (id: string) => void;
}) {
  const [planning, setPlanning] = useState<PlanningView | null>(null);
  const [cycleId, setCycleId] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState({
    cadence: "quarter" as CycleCadence,
    start_date: "",
    end_date: "",
    label: "",
  });
  const [chosen, setChosen] = useState<string[]>([]);

  const canWrite = !!workspace && workspace.role !== "viewer";

  const load = useCallback(async () => {
    if (!workspace) return;
    setError("");
    try {
      const value = await request<unknown>(
        "GET",
        `/v1/workspaces/${workspace.id}/planning${cycleId ? `?cycle_id=${encodeURIComponent(cycleId)}` : ""}`,
      );
      setPlanning(planningViewFrom(value));
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "計画期間を読み込めませんでした",
      );
    }
  }, [workspace, cycleId]);

  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    setCycleId("");
    setChosen([]);
  }, [workspace?.id]);

  const current = planning?.current ?? null;

  /** Live items in this workspace that are not already in the shown period. */
  const carryable = useMemo(() => {
    if (!workspace || !current) return [];
    return items.filter(
      (item) =>
        item.workspace_id === workspace.id &&
        !item.archived_at &&
        item.fields?.cycle_id !== current.id,
    );
  }, [items, workspace, current]);

  const act = async (run: () => Promise<unknown>, done: string) => {
    if (!workspace) return;
    setNotice("");
    setError("");
    try {
      await store.run(run);
      setNotice(done);
      await load();
    } catch (failure) {
      setError(
        failure instanceof ApiError ? failure.message : "処理できませんでした",
      );
    }
  };

  const createNext = () =>
    current &&
    act(
      () =>
        store.write(
          "POST",
          `/v1/workspaces/${workspace!.id}/cycles`,
          nextCycleBody(current),
        ),
      "次の期間を作成しました。",
    );

  const create = () =>
    act(async () => {
      const body: Record<string, unknown> = {
        cadence: draft.cadence,
        start_date: draft.start_date,
      };
      if (draft.cadence === "custom") {
        body.end_date = draft.end_date;
        body.label = draft.label;
      } else if (draft.label.trim()) {
        body.label = draft.label.trim();
      }
      await store.write("POST", `/v1/workspaces/${workspace!.id}/cycles`, body);
      setCreating(false);
      setDraft({ cadence: "quarter", start_date: "", end_date: "", label: "" });
    }, "期間を作成しました。");

  const carryOver = () =>
    current &&
    act(async () => {
      await store.write(
        "POST",
        `/v1/workspaces/${workspace!.id}/cycles/${current.id}/carry-over`,
        { item_ids: chosen, expected_version: versionOf(current.id) },
      );
      setChosen([]);
    }, `${chosen.length}件を引き継ぎました。元の項目はそのまま残ります。`);

  /** The version the shown period was read at, for the write to check. */
  const versionOf = (id: string) =>
    planning?.cycles.find((cycle) => cycle.id === id)?.version ?? 1;

  return (
    <div className="page-content planning-screen">
      <section className="panel planning-intro">
        <div>
          {/* The page header already names this screen; repeating it here
              would just be the product talking to itself. */}
          <span className="eyebrow">{workspace?.name}</span>
          <p>
            四半期・月・週などの期間で計画を運用します。期間を使わないワークスペースは
            これまでどおり動きます。
          </p>
        </div>
        {canWrite && (
          <button
            className="secondary-button"
            onClick={() => setCreating((open) => !open)}
          >
            {creating ? "閉じる" : "期間を追加"}
          </button>
        )}
      </section>

      {error && (
        <p className="auth-error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="planning-notice" role="status">
          {notice}
        </p>
      )}

      <section className="panel">
        <PlanningBar
          planning={planning}
          onSelectCycle={(id) => setCycleId(id)}
          onCreateNext={canWrite ? () => void createNext() : undefined}
          busy={store.pending}
        />
      </section>

      {creating && canWrite && (
        <section className="panel planning-form">
          <h3>期間を追加</h3>
          <div className="planning-form-grid">
            <label>
              <span>種類</span>
              <select
                value={draft.cadence}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    cadence: event.target.value as CycleCadence,
                  })
                }
              >
                {CADENCES.map((cadence) => (
                  <option key={cadence} value={cadence}>
                    {cadenceLabel(cadence)}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>開始日</span>
              <input
                type="date"
                value={draft.start_date}
                onChange={(event) =>
                  setDraft({ ...draft, start_date: event.target.value })
                }
              />
            </label>
            {draft.cadence === "custom" && (
              <>
                <label>
                  <span>終了日</span>
                  <input
                    type="date"
                    value={draft.end_date}
                    onChange={(event) =>
                      setDraft({ ...draft, end_date: event.target.value })
                    }
                  />
                </label>
                <label>
                  <span>名前</span>
                  <input
                    value={draft.label}
                    onChange={(event) =>
                      setDraft({ ...draft, label: event.target.value })
                    }
                    placeholder="FY26 Q1"
                  />
                </label>
              </>
            )}
          </div>
          <p className="planning-hint">
            四半期・月・週は開始日から長さが決まります。会計年度が暦年と違う場合は
            「任意期間」で開始日・終了日・名前を指定してください。
          </p>
          <div className="planning-form-actions">
            <button
              className="primary-button"
              disabled={store.pending || !draft.start_date}
              onClick={() => void create()}
            >
              作成
            </button>
          </div>
        </section>
      )}

      {planning && planning.cycles.length > 0 && (
        <section className="panel planning-list">
          <div className="section-header">
            <h3>すべての期間</h3>
            <span>{planning.cycles.length}件</span>
          </div>
          <ul>
            {planning.cycles.map((cycle) => (
              <li key={cycle.id} data-status={cycle.status}>
                <button onClick={() => setCycleId(cycle.id)}>
                  <span>
                    <strong>{cycle.label}</strong>
                    <small>
                      {cadenceLabel(cycle.cadence)} · {spanLabel(cycle)} ·{" "}
                      {statusLabel(cycle.status)}
                    </small>
                  </span>
                </button>
                {canWrite && cycle.status !== "closed" && (
                  <button
                    className="secondary-button"
                    disabled={store.pending}
                    onClick={() =>
                      void act(
                        () =>
                          store.write(
                            "PATCH",
                            `/v1/workspaces/${workspace!.id}/cycles/${cycle.id}`,
                            {
                              status: "closed",
                              expected_version: versionOf(cycle.id),
                            },
                          ),
                        "期間を終了しました。中身はそのまま残ります。",
                      )
                    }
                  >
                    終了する
                  </button>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}

      {current && canWrite && current.status !== "closed" && (
        <section className="panel planning-carry">
          <div className="section-header">
            <h3>{current.label}へ引き継ぐ</h3>
            <span>元の項目は変更されません</span>
          </div>
          {carryable.length === 0 ? (
            <p className="empty-value">引き継げる項目がありません。</p>
          ) : (
            <>
              <ul className="planning-carry-list">
                {carryable.slice(0, 50).map((item) => (
                  <li key={item.id}>
                    <label>
                      <input
                        type="checkbox"
                        checked={chosen.includes(item.id)}
                        onChange={(event) =>
                          setChosen((held) =>
                            event.target.checked
                              ? [...held, item.id]
                              : held.filter((id) => id !== item.id),
                          )
                        }
                      />
                      <span>
                        <strong>{item.title}</strong>
                        <small>
                          {item.fields?.cycle_id
                            ? (planning?.cycles.find(
                                (cycle) => cycle.id === item.fields?.cycle_id,
                              )?.label ?? "他の期間")
                            : "期間なし"}
                        </small>
                      </span>
                    </label>
                    <button
                      className="link-button"
                      onClick={() => onOpenItem(item.id)}
                    >
                      開く
                    </button>
                  </li>
                ))}
              </ul>
              <div className="planning-form-actions">
                <button
                  className="primary-button"
                  disabled={store.pending || chosen.length === 0}
                  onClick={() => void carryOver()}
                >
                  {chosen.length}件を引き継ぐ
                </button>
              </div>
            </>
          )}
        </section>
      )}
    </div>
  );
}
