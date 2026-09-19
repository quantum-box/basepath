import { useState } from "react";
import { Icon } from "./icons";
import {
  dateLabel,
  localDate,
  type ActionSuggestion,
  type ChangeSet,
  type Item,
  type ReflectionSuggestion,
  type SuggestionEvidence,
  type SuggestionPreview,
} from "./api";
import type { WorkspaceStore } from "./useWorkspace";

type Candidate = ActionSuggestion | ReflectionSuggestion;

function Evidence({ value }: { value: SuggestionEvidence }) {
  return (
    <dl className="ai-evidence">
      <div>
        <dt>目標</dt>
        <dd>{value.goal.title}</dd>
      </div>
      <div>
        <dt>期限</dt>
        <dd>{value.deadline ? dateLabel(value.deadline) : "未設定"}</dd>
      </div>
      <div>
        <dt>直近記録</dt>
        <dd>{value.latest_record?.body || "関連する記録はまだありません"}</dd>
      </div>
    </dl>
  );
}

export function AiSuggestions({
  goal,
  store,
  onApplied,
}: {
  goal: Item;
  store: WorkspaceStore;
  onApplied: () => void;
}) {
  const [result, setResult] = useState<SuggestionPreview | null>(null);
  const [dismissed, setDismissed] = useState<string[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [change, setChange] = useState<ChangeSet | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function generate() {
    setBusy(true);
    setError("");
    setChange(null);
    try {
      const preview = await store.write<SuggestionPreview>(
        "POST",
        `/v1/workspaces/${goal.workspace_id}/ai/suggestions/preview`,
        { goal_id: goal.id, expected_version: goal.version },
      );
      setResult(preview);
      setDismissed([]);
      setDrafts(
        Object.fromEntries([
          ...preview.suggestions.map((item) => [item.id, item.title]),
          [preview.reflection.id, preview.reflection.body],
        ]),
      );
    } catch (failure) {
      setError(
        failure instanceof Error
          ? failure.message
          : "提案を作成できませんでした",
      );
    } finally {
      setBusy(false);
    }
  }

  async function createChange(candidate: Candidate) {
    setBusy(true);
    setError("");
    try {
      const body = drafts[candidate.id]?.trim();
      if (!body) throw new Error("提案内容を入力してください");
      const operation =
        candidate.kind === "action"
          ? {
              method: "POST",
              path: `/v1/workspaces/${goal.workspace_id}/items`,
              body: {
                title: body,
                kind: "action",
                state: "active",
                scheduled_date: localDate(store.settings.timezone),
                parent_id: goal.id,
                description: "AI提案を確認して採用",
              },
            }
          : {
              method: "POST",
              path: `/v1/workspaces/${goal.workspace_id}/records`,
              body: {
                record_type: "review",
                body,
                item_ids: [goal.id],
              },
            };
      const preview = await store.write<ChangeSet>(
        "POST",
        `/v1/workspaces/${goal.workspace_id}/changesets/preview`,
        { title: `AI提案を採用: ${candidate.title}`, operations: [operation] },
      );
      setChange(preview);
      setEditing(null);
    } catch (failure) {
      setError(
        failure instanceof Error
          ? failure.message
          : "差分を作成できませんでした",
      );
    } finally {
      setBusy(false);
    }
  }

  async function approve() {
    if (!change) return;
    setBusy(true);
    setError("");
    try {
      const approved = await store.write<ChangeSet>(
        "POST",
        `/v1/workspaces/${goal.workspace_id}/changesets/${change.id}/approve`,
        {},
      );
      setChange(approved);
      // Approving applies it. There is nothing left for the person to press,
      // so the panel closes rather than showing a button that would do what
      // has already been done.
      if (approved.status === "applied") {
        await store.refresh();
        onApplied();
      }
    } catch (failure) {
      setError(
        failure instanceof Error ? failure.message : "承認できませんでした",
      );
    } finally {
      setBusy(false);
    }
  }

  async function apply() {
    if (!change) return;
    setBusy(true);
    setError("");
    try {
      await store.write(
        "POST",
        `/v1/workspaces/${goal.workspace_id}/changesets/${change.id}/apply`,
        {},
      );
      await store.refresh();
      onApplied();
    } catch (failure) {
      setError(
        failure instanceof Error ? failure.message : "適用できませんでした",
      );
    } finally {
      setBusy(false);
    }
  }

  if (!result)
    return (
      <div className="ai-empty">
        <span className="ai-orb">
          <Icon name="sparkle" size={28} weight="duotone" />
        </span>
        <h3>次の30分を、一緒に考えます</h3>
        <p>
          この目標と期限、同じワークスペースの直近記録だけを使います。提案だけではデータは変わりません。
        </p>
        {error && (
          <p className="save-error" role="alert">
            {error}
          </p>
        )}
        <button
          className="primary-button"
          disabled={busy}
          onClick={() => void generate()}
        >
          {busy ? "考えています…" : "行動と振り返りを提案"}
        </button>
      </div>
    );

  const candidates: Candidate[] = [...result.suggestions, result.reflection];
  return (
    <div className="ai-suggestions">
      <div className="ai-notice">
        <Icon name="shield" size={17} />
        {result.provider_notice}
      </div>
      {candidates
        .filter((item) => !dismissed.includes(item.id))
        .map((item) => (
          <article className="ai-candidate" key={item.id}>
            <div className="ai-candidate-title">
              <span>
                {item.kind === "action"
                  ? `${item.duration_minutes}分`
                  : "振り返り"}
              </span>
              <h3>{item.title}</h3>
            </div>
            {editing === item.id ? (
              <textarea
                autoFocus
                aria-label="提案内容を編集"
                value={drafts[item.id] || ""}
                onChange={(event) =>
                  setDrafts({ ...drafts, [item.id]: event.target.value })
                }
              />
            ) : item.kind === "reflection" ? (
              <div className="reflection-sections">
                <strong>事実</strong>
                <ul>
                  {item.fact.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
                <strong>推測</strong>
                <ul>
                  {item.inference.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
                <strong>質問</strong>
                <ul>
                  {item.questions.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
              </div>
            ) : (
              <p>{item.reason}</p>
            )}
            <Evidence value={item.evidence} />
            <div className="ai-actions">
              <button
                className="primary-button"
                disabled={busy}
                onClick={() => void createChange(item)}
              >
                差分を確認
              </button>
              <button
                onClick={() => setEditing(editing === item.id ? null : item.id)}
              >
                {editing === item.id ? "編集を閉じる" : "編集して採用"}
              </button>
              <button onClick={() => setDismissed([...dismissed, item.id])}>
                却下
              </button>
            </div>
          </article>
        ))}
      {!candidates.some((item) => !dismissed.includes(item.id)) && (
        <p className="ai-all-dismissed">
          すべて却下しました。元のデータは変更されていません。
        </p>
      )}
      {change && (
        <section className="ai-diff" aria-label="変更差分">
          <h3>変更差分を確認</h3>
          <div>
            <span>変更前</span>
            <p>項目なし</p>
          </div>
          <div>
            <span>変更後</span>
            <p>
              {String(
                (change.operations[0]?.body &&
                  (
                    change.operations[0].body as {
                      title?: string;
                      body?: string;
                    }
                  ).title) ||
                  (change.operations[0]?.body as { body?: string })?.body ||
                  "",
              )}
            </p>
          </div>
          <small>
            有効期限: {new Date(change.expires_at).toLocaleString("ja-JP")}
          </small>
          {change.status === "pending" ? (
            <button
              className="primary-button"
              disabled={busy}
              onClick={() => void approve()}
            >
              この差分を承認
            </button>
          ) : (
            <button
              className="primary-button"
              disabled={busy}
              onClick={() => void apply()}
            >
              承認済みの差分を適用
            </button>
          )}
        </section>
      )}
      {error && (
        <p className="save-error" role="alert">
          {error}
        </p>
      )}
      <button
        className="text-link"
        disabled={busy}
        onClick={() => void generate()}
      >
        最新の内容で作り直す <Icon name="repeat" size={14} />
      </button>
    </div>
  );
}
