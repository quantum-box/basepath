/**
 * Deciding in advance which proposals may be reflected without being asked
 * again.
 *
 * This screen is the evidence. Everything the server allows under a range it
 * allows because a row was written here — on Basepath's own origin, with this
 * person's session and the same-origin CSRF header — and for no other reason.
 * An AI connection cannot read what is on this screen or change it.
 *
 * So the screen has one job beyond collecting the answer: make the edges of
 * what is being granted visible while it is being granted. A permission whose
 * shape a person cannot see is one they cannot mean.
 */
import { useCallback, useEffect, useState } from "react";
import { request, type ApiError } from "./api";
import type { McpConnection } from "./McpConnections";
import type { WorkspaceStore } from "./useWorkspace";

export type AutoApplyRule = {
  id: string;
  actor: string;
  workspace_id: string;
  connection_id: string;
  allow_create: boolean;
  allow_update: boolean;
  allow_guarded: boolean;
  expires_at: string;
  created_at: string;
  updated_at: string;
  revoked_at: string | null;
  version: number;
};

type Draft = {
  workspace_id: string;
  allow_create: boolean;
  allow_update: boolean;
  allow_guarded: boolean;
  days: number;
};

const DEFAULT_DRAFT: Draft = {
  workspace_id: "",
  allow_create: true,
  allow_update: false,
  allow_guarded: false,
  days: 30,
};

function when(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "―" : date.toLocaleString("ja-JP");
}

function live(rule: AutoApplyRule) {
  return !rule.revoked_at && new Date(rule.expires_at).getTime() > Date.now();
}

export function AutoApplyRanges({
  store,
  connections,
}: {
  store: WorkspaceStore;
  connections: McpConnection[];
}) {
  const [rules, setRules] = useState<AutoApplyRule[] | null>(null);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [failure, setFailure] = useState("");

  const load = useCallback(async () => {
    try {
      setRules(await request<AutoApplyRule[]>("GET", "/v1/mcp/auto-apply"));
      setFailure("");
    } catch (error) {
      setFailure((error as ApiError).message || "設定を取得できません");
      setRules([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const active = connections.filter(
    (connection) => connection.status === "active",
  );

  /** Every range still in force for one connection, newest first. */
  function liveFor(connection: McpConnection): AutoApplyRule[] {
    return (rules ?? [])
      .filter((rule) => rule.connection_id === connection.id && live(rule))
      .sort((a, b) => b.created_at.localeCompare(a.created_at));
  }

  function draftFor(connection: McpConnection): Draft {
    return (
      drafts[connection.id] ?? {
        ...DEFAULT_DRAFT,
        workspace_id: store.workspaces[0]?.id ?? "",
      }
    );
  }

  function edit(connection: McpConnection, patch: Partial<Draft>) {
    setDrafts((current) => ({
      ...current,
      [connection.id]: { ...draftFor(connection), ...patch },
    }));
  }

  async function save(connection: McpConnection) {
    const draft = draftFor(connection);
    await store.run(async () => {
      await store.write("POST", "/v1/mcp/auto-apply", {
        workspace_id: draft.workspace_id,
        connection_id: connection.id,
        allow_create: draft.allow_create,
        allow_update: draft.allow_update,
        allow_guarded: draft.allow_guarded,
        days: draft.days,
      });
      await load();
    });
  }

  async function revoke(rule: AutoApplyRule) {
    await store.run(async () => {
      await store.write("POST", `/v1/mcp/auto-apply/${rule.id}/revoke`, {});
      await load();
    });
  }

  function workspaceName(id: string) {
    return (
      store.workspaces.find((workspace) => workspace.id === id)?.name ?? id
    );
  }

  return (
    <section className="auto-apply">
      <header>
        <h3>確認なしで反映してよい範囲</h3>
        <p>
          いつもの承認を1件ずつではなく、範囲ごとに済ませておく設定です。
          ここで決めた範囲に入る変更案は、会話の中からそのまま反映できます。
          範囲の外にある変更案は、これまで通り1件ずつ差分を確認して承認します。
        </p>
        <p>
          <strong>削除は、どの設定でも自動反映されません。</strong>
          期限・開始日・予定日・担当・自己評価・目標値・基準値を設定する変更も、
          あとから「本人が決めた」と読まれる値なので、別に許可しない限り自動では反映されません。
          自動で反映されたものも差分は残り、あとから読めます。設定の解除はすぐに効きます。
        </p>
      </header>

      {failure && <p className="empty-value">{failure}</p>}
      {active.length === 0 && (
        <p className="empty-value">
          許可済みのAIクライアントがありません。先に接続を許可してください。
        </p>
      )}

      <ul>
        {active.map((connection) => {
          const draft = draftFor(connection);
          const current = liveFor(connection);
          return (
            <li key={connection.id} data-connection={connection.id}>
              <div className="auto-apply-head">
                <strong>{connection.client_name}</strong>
                {current.length === 0 && (
                  <small>設定なし。すべて1件ずつ承認します。</small>
                )}
              </div>

              {/* Every range in force, each one removable on its own.
                  Ranges are keyed by workspace, so one connection can hold
                  several — and a permission the screen does not show is one
                  nobody can take back. */}
              {current.length > 0 && (
                <ul className="auto-apply-current">
                  {current.map((rule) => (
                    <li key={rule.id}>
                      <div>
                        <strong>{workspaceName(rule.workspace_id)}</strong>
                        <small>
                          {[
                            rule.allow_create ? "追加" : null,
                            rule.allow_update ? "更新" : null,
                            rule.allow_guarded ? "期限や担当も含む" : null,
                          ]
                            .filter(Boolean)
                            .join("・")}
                          ・{when(rule.expires_at)}まで
                        </small>
                      </div>
                      <button
                        type="button"
                        className="secondary"
                        disabled={store.pending}
                        onClick={() => void revoke(rule)}
                      >
                        この範囲を解除
                      </button>
                    </li>
                  ))}
                </ul>
              )}

              <fieldset>
                <legend>
                  {current.length > 0 ? "範囲を追加・変更する" : "範囲"}
                </legend>
                <label>
                  <span>ワークスペース</span>
                  {/* One workspace, chosen explicitly. A personal plan and a
                      shared one are different plans with different
                      consequences, and "everywhere" would cross that boundary
                      for the sake of one fewer click. Saving for a workspace
                      that already has a range replaces it; another workspace
                      becomes a second range, listed above. */}
                  <select
                    value={draft.workspace_id}
                    onChange={(event) =>
                      edit(connection, { workspace_id: event.target.value })
                    }
                  >
                    {store.workspaces.map((workspace) => (
                      <option key={workspace.id} value={workspace.id}>
                        {workspace.name}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  <input
                    type="checkbox"
                    checked={draft.allow_create}
                    onChange={(event) =>
                      edit(connection, { allow_create: event.target.checked })
                    }
                  />
                  <span>
                    追加を自動で反映する
                    <small>
                      目標・行動・記録の追加。増えたものは一覧に残り、いつでも消せます。
                    </small>
                  </span>
                </label>
                <label>
                  <input
                    type="checkbox"
                    checked={draft.allow_update}
                    onChange={(event) =>
                      edit(connection, { allow_update: event.target.checked })
                    }
                  />
                  <span>
                    更新も自動で反映する
                    <small>
                      すでにあるものの書き換え。行動の完了もここに入ります。あなたが書いた文言が置き換わることがあります。
                    </small>
                  </span>
                </label>
                <label>
                  <input
                    type="checkbox"
                    checked={draft.allow_guarded}
                    onChange={(event) =>
                      edit(connection, { allow_guarded: event.target.checked })
                    }
                  />
                  <span>
                    期限・担当・目標値なども含める
                    <small>
                      これらの値は、あとから「あなたが決めた」と読まれます。設定するときも、消すときも同じです。必要がなければ外したままにしてください。
                    </small>
                  </span>
                </label>
                <label>
                  <span>有効期限</span>
                  {/* No "indefinitely". A standing permission nobody revisits
                      is one nobody remembers granting. */}
                  <select
                    value={String(draft.days)}
                    onChange={(event) =>
                      edit(connection, { days: Number(event.target.value) })
                    }
                  >
                    <option value="7">7日</option>
                    <option value="30">30日</option>
                    <option value="90">90日</option>
                  </select>
                </label>
              </fieldset>

              <div className="auto-apply-actions">
                <button
                  type="button"
                  disabled={
                    store.pending ||
                    !draft.workspace_id ||
                    (!draft.allow_create && !draft.allow_update)
                  }
                  onClick={() => void save(connection)}
                >
                  {current.some(
                    (rule) => rule.workspace_id === draft.workspace_id,
                  )
                    ? "このワークスペースの範囲を更新"
                    : "この範囲で許可する"}
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
