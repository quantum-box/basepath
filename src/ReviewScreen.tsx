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
      const [line, history] = await Promise.all([
        request<unknown>(
          "GET",
          `/v1/workspaces/${workspaceId}/items/${entry.id}/timeline`,
        ),
        request<unknown>(
          "GET",
          `/v1/workspaces/${workspaceId}/items/${entry.id}/checkins`,
        ),
      ]);
      setTimeline(timelineFrom(line));
      setCheckins(checkinsFrom(history));
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "履歴を読み込めませんでした",
      );
    }
  }, [workspaceId, entry.id]);

  useEffect(() => {
    void load();
  }, [load]);

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
