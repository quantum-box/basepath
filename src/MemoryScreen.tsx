/**
 * Personal memory, in Basepath.
 *
 * This is the person's own. The screen's job is to make two things
 * unmistakable: what an AI suggested versus what they confirmed, and what
 * still stands versus what was true before. Neither distinction survives if
 * the list flattens into "things we know about you".
 *
 * It only appears for a personal workspace, because memory only exists there.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { ApiError, request, type Workspace } from "./api";
import type { WorkspaceStore } from "./useWorkspace";
import {
  countsByKind,
  duplicateGroupsFrom,
  inactiveReason,
  kindLabel,
  kinds,
  memoryListFrom,
  statusLabel,
  type DuplicateGroup,
  type Memory,
  type MemoryKind,
  type MemoryList,
} from "./shared/memoryView";

export function MemoryScreen({
  store,
  workspace,
}: {
  store: WorkspaceStore;
  workspace?: Workspace;
}) {
  const [list, setList] = useState<MemoryList | null>(null);
  const [duplicates, setDuplicates] = useState<DuplicateGroup[]>([]);
  const [error, setError] = useState("");
  const [lens, setLens] = useState<MemoryKind | "all">("all");
  const [showPast, setShowPast] = useState(false);
  const [draft, setDraft] = useState({
    kind: "preference" as MemoryKind,
    title: "",
    body: "",
    source: "",
  });

  const personal = workspace?.scope === "個人";

  const load = useCallback(async () => {
    if (!workspace || !personal) return;
    setError("");
    try {
      const [all, dupes] = await Promise.all([
        request<unknown>(
          "GET",
          `/v1/workspaces/${workspace.id}/memories?archived=true`,
        ),
        request<unknown>(
          "GET",
          `/v1/workspaces/${workspace.id}/memories/duplicates`,
        ),
      ]);
      setList(memoryListFrom(all));
      setDuplicates(duplicateGroupsFrom(dupes));
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "記憶を読み込めませんでした",
      );
    }
  }, [workspace, personal]);

  useEffect(() => {
    void load();
  }, [load]);

  const shown = useMemo(() => {
    if (!list) return [];
    return list.memories.filter(
      (memory) =>
        (lens === "all" || memory.kind === lens) &&
        (showPast || inactiveReason(memory) === null),
    );
  }, [list, lens, showPast]);

  const act = async (run: () => Promise<unknown>) => {
    setError("");
    try {
      await store.run(run);
      await load();
    } catch (failure) {
      setError(
        failure instanceof ApiError ? failure.message : "処理できませんでした",
      );
    }
  };

  const save = () =>
    act(async () => {
      await store.write(
        "POST",
        `/v1/workspaces/${workspace!.id}/memories`,
        Object.fromEntries(
          Object.entries(draft).filter(([, value]) => String(value).trim()),
        ),
      );
      setDraft({ kind: "preference", title: "", body: "", source: "" });
    });

  if (!workspace) return null;

  if (!personal) {
    return (
      <div className="page-content memory-screen">
        <section className="panel memory-intro">
          <p>
            記憶は個人のワークスペースにだけ保存されます。共有のワークスペースには
            存在しません。個人のワークスペースに切り替えてください。
          </p>
        </section>
      </div>
    );
  }

  const counts = list ? countsByKind(list) : null;

  return (
    <div className="page-content memory-screen">
      <section className="panel memory-intro">
        <div>
          <span className="eyebrow">{workspace.name}</span>
          <p>
            あなた自身の記憶です。共有ワークスペースへは移動も同期もされません。
            AIの候補は、あなたが確認するまで「あなたが言ったこと」にはなりません。
          </p>
        </div>
      </section>

      {error && (
        <p className="auth-error" role="alert">
          {error}
        </p>
      )}

      <section className="panel memory-form">
        <h3>記憶を追加</h3>
        <div className="memory-form-grid">
          <label>
            <span>種類</span>
            <select
              value={draft.kind}
              onChange={(event) =>
                setDraft({ ...draft, kind: event.target.value as MemoryKind })
              }
            >
              {kinds().map((kind) => (
                <option key={kind} value={kind}>
                  {kindLabel(kind)}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>タイトル</span>
            <input
              value={draft.title}
              onChange={(event) =>
                setDraft({ ...draft, title: event.target.value })
              }
            />
          </label>
          <label>
            <span>出典</span>
            <input
              value={draft.source}
              onChange={(event) =>
                setDraft({ ...draft, source: event.target.value })
              }
              placeholder="本人の申告、会議のメモ、など"
            />
          </label>
          <label className="memory-body-field">
            <span>内容</span>
            <textarea
              aria-label="内容"
              value={draft.body}
              onChange={(event) =>
                setDraft({ ...draft, body: event.target.value })
              }
            />
          </label>
        </div>
        <p className="memory-hint">
          「事実」には出典が必要です。根拠のない推測は「背景」や「学び」として
          記録してください。
        </p>
        <div className="memory-form-actions">
          <button
            className="primary-button"
            disabled={store.pending || !draft.title.trim()}
            onClick={() => void save()}
          >
            記録する
          </button>
        </div>
      </section>

      {duplicates.length > 0 && (
        <section className="panel memory-duplicates">
          <div className="section-header">
            <h3>似ている記憶</h3>
            <span>{duplicates.length}件</span>
          </div>
          <p className="memory-hint">
            同じことを言っているかもしれません。どちらが正しいかはあなたが決めます。
            自動では統合も削除もしません。
          </p>
          <ul>
            {duplicates.map((group) => (
              <li key={group.id}>
                <strong>{group.title}</strong>
                {group.similar.map((other) => (
                  <small key={other.id}>
                    ↔ {other.title}（{other.overlap}%一致）
                  </small>
                ))}
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="panel memory-list">
        <div className="memory-lenses">
          <button aria-pressed={lens === "all"} onClick={() => setLens("all")}>
            すべて
          </button>
          {kinds().map((kind) => (
            <button
              key={kind}
              aria-pressed={lens === kind}
              onClick={() => setLens(kind)}
            >
              {kindLabel(kind)}
              {counts && counts[kind] > 0 && ` ${counts[kind]}`}
            </button>
          ))}
          <label className="memory-past">
            <input
              type="checkbox"
              checked={showPast}
              onChange={(event) => setShowPast(event.target.checked)}
            />
            <span>過去の記憶も表示</span>
          </label>
        </div>

        {shown.length === 0 ? (
          <p className="empty-value">まだ記憶がありません。</p>
        ) : (
          <ul>
            {shown.map((memory) => {
              const inactive = inactiveReason(memory);
              return (
                <li
                  key={memory.id}
                  data-status={memory.status}
                  data-inactive={inactive !== null}
                >
                  <div className="memory-head">
                    <span className="memory-kind">
                      {kindLabel(memory.kind)}
                    </span>
                    <strong>{memory.title}</strong>
                    <span className="memory-status" data-status={memory.status}>
                      {statusLabel(memory.status)}
                      {memory.confidence !== null &&
                        `（確度 ${Math.round(memory.confidence * 100)}%）`}
                    </span>
                    {/* Never "wrong": it was true when it was recorded. */}
                    {inactive && (
                      <span className="memory-inactive">{inactive}</span>
                    )}
                    {memory.excludedFromRetrieval && (
                      <span className="memory-excluded">AIには渡さない</span>
                    )}
                  </div>
                  {memory.body && <p>{memory.body}</p>}
                  <small>
                    {memory.source && `出典: ${memory.source} ・ `}
                    {memory.createdAt.slice(0, 10)}
                  </small>
                  <div className="memory-actions">
                    {memory.status === "proposed" && (
                      <button
                        disabled={store.pending}
                        onClick={() =>
                          void act(() =>
                            store.write(
                              "POST",
                              `/v1/workspaces/${workspace.id}/memories/${memory.id}/verify`,
                              { expected_version: memory.version },
                            ),
                          )
                        }
                      >
                        自分の記憶として確認
                      </button>
                    )}
                    <button
                      disabled={store.pending}
                      onClick={() =>
                        void act(() =>
                          store.write(
                            "PATCH",
                            `/v1/workspaces/${workspace.id}/memories/${memory.id}`,
                            {
                              expected_version: memory.version,
                              excluded_from_retrieval:
                                !memory.excludedFromRetrieval,
                            },
                          ),
                        )
                      }
                    >
                      {memory.excludedFromRetrieval
                        ? "AIに渡してよい"
                        : "AIには渡さない"}
                    </button>
                    {!memory.archivedAt && (
                      <button
                        disabled={store.pending}
                        onClick={() =>
                          void act(() =>
                            store.write(
                              "PATCH",
                              `/v1/workspaces/${workspace.id}/memories/${memory.id}`,
                              {
                                expected_version: memory.version,
                                archived_at: "now",
                              },
                            ),
                          )
                        }
                      >
                        整理する
                      </button>
                    )}
                    <button
                      className="memory-delete"
                      disabled={store.pending}
                      onClick={() =>
                        void act(() =>
                          store.write(
                            "DELETE",
                            `/v1/workspaces/${workspace.id}/memories/${memory.id}`,
                            {},
                          ),
                        )
                      }
                    >
                      削除
                    </button>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>
    </div>
  );
}
