import { useCallback, useEffect, useMemo, useState } from "react";
import { Icon } from "./icons";
import { ApiError, request, type WeeklyReview, type Workspace } from "./api";
import type { WorkspaceStore } from "./useWorkspace";
import { WeeklyReviewPanel } from "./shared/WeeklyReview";
import {
  draftBody,
  draftInputFrom,
  mondayOf,
  weeklyReviewFrom,
  type WeeklyDraftInput,
  type WeeklyReviewView,
} from "./shared/weeklyView";
import { localDateIn } from "./shared/viewModel";

/**
 * The weekly review, in Basepath itself.
 *
 * The screen owns loading and saving; what is shown comes from the shared
 * panel, built from the shared view model, so the conversation surface and
 * this one cannot disagree about the same week. Nothing is aggregated here.
 */
export function WeeklyReviewScreen({
  store,
  workspace,
  onOpenItem,
}: {
  store: WorkspaceStore;
  workspace?: Workspace;
  onOpenItem: (id: string) => void;
}) {
  const timezone = workspace?.timezone || "Asia/Tokyo";
  const [weekStart, setWeekStart] = useState(() =>
    mondayOf(localDateIn(timezone)),
  );
  const [summary, setSummary] = useState<WeeklyReviewView | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [draft, setDraft] = useState<WeeklyDraftInput>(draftInputFrom(null));
  // A finalized revision is corrected by starting a new one, never by editing
  // it in place, so the earlier version keeps saying what it said.
  const [correcting, setCorrecting] = useState(false);

  const load = useCallback(async () => {
    if (!workspace) return;
    setLoading(true);
    setError("");
    try {
      const value = await request<unknown>(
        "GET",
        `/v1/workspaces/${workspace.id}/weekly-review?week_start=${weekStart}`,
      );
      const view = weeklyReviewFrom(value);
      setSummary(view);
      setCorrecting(false);
      setDraft(draftInputFrom(view?.review ?? null));
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "週次レポートを読み込めませんでした",
      );
    } finally {
      setLoading(false);
    }
  }, [workspace, weekStart]);
  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    if (workspace) setWeekStart(mondayOf(localDateIn(workspace.timezone)));
  }, [workspace?.id, workspace?.timezone]);

  // While correcting, the shown revision is not the one being edited.
  const view = useMemo(
    () => (summary && correcting ? { ...summary, review: null } : summary),
    [summary, correcting],
  );
  const saved = view?.review ?? null;
  const canWrite = !!workspace && workspace.role !== "viewer";

  const save = async (): Promise<WeeklyReview | null> => {
    if (!workspace) return null;
    const stored: { value: WeeklyReview | null } = { value: null };
    await store.run(async () => {
      stored.value = await store.write<WeeklyReview>(
        "POST",
        `/v1/workspaces/${workspace.id}/weekly-reviews/draft`,
        draftBody(weekStart, draft, saved),
      );
    });
    await load();
    return stored.value;
  };
  const finalize = async () => {
    if (!workspace) return;
    const target =
      saved && saved.status === "draft"
        ? { id: saved.id, version: saved.version }
        : await save().then((stored) =>
            stored ? { id: stored.id, version: stored.version } : null,
          );
    if (!target) return;
    await store.run(() =>
      store.write(
        "POST",
        `/v1/workspaces/${workspace.id}/weekly-reviews/${target.id}/finalize`,
        { expected_version: target.version },
      ),
    );
    await load();
  };

  return (
    <div className="page-content weekly-review-screen">
      <section className="panel weekly-review-toolbar">
        <div>
          <span className="eyebrow">
            {workspace?.name} · {view?.timezone ?? timezone}
          </span>
          <h2>週次レビュー</h2>
          <p>行動、目標の自己評価、成果指標を分けて振り返ります。</p>
        </div>
        <div className="weekly-review-controls">
          <label>
            週の開始
            <input
              type="date"
              value={weekStart}
              onChange={(event) =>
                setWeekStart(mondayOf(event.target.value || weekStart))
              }
            />
          </label>
          <button className="secondary-button" onClick={() => window.print()}>
            <Icon name="note" size={17} />
            印刷・共有用
          </button>
        </div>
      </section>
      {error && (
        <p className="auth-error" role="alert">
          {error}
        </p>
      )}
      <WeeklyReviewPanel
        review={view}
        workspaceName={workspace?.name}
        loading={loading}
        draft={draft}
        onDraftChange={setDraft}
        canEdit={canWrite}
        onSave={() => void save()}
        onFinalize={() => void finalize()}
        onCorrect={() => {
          setCorrecting(true);
          setDraft(draftInputFrom(null));
        }}
        onOpenItem={onOpenItem}
        busy={store.pending}
      />
    </div>
  );
}
