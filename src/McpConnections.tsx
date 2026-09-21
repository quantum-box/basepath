import { useCallback, useEffect, useState } from "react";
import { request, type ApiError } from "./api";
import type { WorkspaceStore } from "./useWorkspace";
import { Icon } from "./icons";
import { AutoApplyRanges } from "./AutoApplyRanges";

/** One AI client's delegation. */
export type McpConnection = {
  id: string;
  actor: string;
  display_name?: string;
  client_id: string;
  client_name: string;
  scopes: string[];
  status: "pending" | "active" | "revoked";
  created_at: string;
  updated_at: string;
  last_used_at: string;
  version: number;
};

/**
 * What each scope lets the AI attempt. None of them approves a change: a plan
 * change still has to be reviewed and approved as a specific proposal.
 */
const SCOPES: { id: string; label: string; detail: string }[] = [
  {
    id: "pathbase.read",
    label: "目標と行動を読む",
    detail: "参加しているワークスペースの目標・行動・記録を読み取ります。",
  },
  {
    id: "pathbase.propose",
    label: "変更案を作る",
    detail:
      "計画の変更案を作成します。案のままでは反映されず、あなたが内容を確認して承認するまで何も変わりません。",
  },
  {
    id: "pathbase.apply",
    label: "承認済みの変更案を適用する",
    detail:
      "あなたが承認した変更案だけを適用できます。承認していない案は適用できません。",
  },
  {
    id: "pathbase.context",
    label: "会話を業務コンテキストにリンクする",
    detail: "会話の対象ワークスペースを記憶し、次のターンから同じ業務コンテキストを開けるようにします。",
  },
];

function statusLabel(status: McpConnection["status"]) {
  if (status === "active") return "接続中";
  if (status === "pending") return "許可待ち";
  return "解除済み";
}

function when(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "―" : date.toLocaleString("ja-JP");
}

/**
 * The screen where a person decides which AI clients may act for them, and
 * disconnects the ones they no longer want.
 */
export function McpConnections({ store }: { store: WorkspaceStore }) {
  const [connections, setConnections] = useState<McpConnection[] | null>(null);
  const [selected, setSelected] = useState<Record<string, string[]>>({});
  const [failure, setFailure] = useState("");

  const load = useCallback(async () => {
    try {
      const list = await request<McpConnection[]>("GET", "/v1/mcp/connections");
      setConnections(list);
      setSelected((current) => {
        const next = { ...current };
        for (const connection of list) {
          if (!next[connection.id]) {
            next[connection.id] =
              connection.scopes.length > 0
                ? connection.scopes
                : ["pathbase.read"];
          }
        }
        return next;
      });
      setFailure("");
    } catch (error) {
      setFailure((error as ApiError).message || "接続一覧を取得できません");
      setConnections([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  function toggle(connection: McpConnection, scope: string) {
    setSelected((current) => {
      const chosen = new Set(current[connection.id] ?? []);
      if (chosen.has(scope)) chosen.delete(scope);
      else chosen.add(scope);
      return {
        ...current,
        [connection.id]: SCOPES.map((s) => s.id).filter((id) => chosen.has(id)),
      };
    });
  }

  async function save(connection: McpConnection) {
    const scopes = selected[connection.id] ?? [];
    await store.run(async () => {
      await store.write(
        "POST",
        `/v1/mcp/connections/${connection.id}/approve`,
        { scopes, expected_version: connection.version },
      );
      await load();
    });
  }

  async function disconnect(connection: McpConnection) {
    await store.run(async () => {
      await store.write(
        "POST",
        `/v1/mcp/connections/${connection.id}/revoke`,
        {},
      );
      await load();
    });
  }

  return (
    <section className="mcp-connections">
      <header>
        <h3>AIクライアントの接続</h3>
        <p>
          ChatGPTやClaudeからBasepathへ接続すると、許可画面で選んだ権限がここに表示されます。
          許可していないAIクライアントは何も読み取れません。権限はいつでも狭められ、接続を解除すると、
          有効期限の残っているトークンもその場で使えなくなります。権限を渡しても、計画の変更は案として
          作られるだけで、あなたが差分を確認して承認するまで反映されません。
        </p>
      </header>

      {failure && <p className="empty-value">{failure}</p>}
      {connections !== null && connections.length === 0 && !failure && (
        <p className="empty-value">
          まだ接続はありません。AI側でBasepathへの接続を開始してください。
        </p>
      )}

      <ul>
        {(connections ?? []).map((connection) => (
          <li key={connection.id} data-status={connection.status}>
            <div className="mcp-connection-head">
              <span className="related-icon orange">
                <Icon name="settings" size={20} weight="duotone" />
              </span>
              <div>
                <strong>{connection.client_name}</strong>
                <small>
                  {statusLabel(connection.status)}・最終利用{" "}
                  {when(connection.last_used_at)}
                </small>
                <small>
                  承認したアカウント：
                  {connection.display_name ||
                    (store.me.mode === "local-preview"
                      ? "ローカルプレビュー"
                      : "Tachyonアカウント")}（
                  {connection.actor}）
                </small>
              </div>
            </div>

            {connection.status !== "revoked" ? (
              <>
                <fieldset>
                  <legend>許可する操作</legend>
                  {SCOPES.map((scope) => (
                    <label key={scope.id}>
                      <input
                        type="checkbox"
                        checked={(selected[connection.id] ?? []).includes(
                          scope.id,
                        )}
                        onChange={() => toggle(connection, scope.id)}
                      />
                      <span>
                        {scope.label}
                        <small>{scope.detail}</small>
                      </span>
                    </label>
                  ))}
                </fieldset>
                <div className="mcp-connection-actions">
                  <button
                    type="button"
                    disabled={
                      store.pending ||
                      (selected[connection.id] ?? []).length === 0
                    }
                    onClick={() => void save(connection)}
                  >
                    {connection.status === "active"
                      ? "権限を更新"
                      : "この接続を許可"}
                  </button>
                  <button
                    type="button"
                    className="secondary"
                    disabled={store.pending}
                    onClick={() => void disconnect(connection)}
                  >
                    接続を解除
                  </button>
                </div>
              </>
            ) : (
              <p className="empty-value">
                解除済みです。再び使うにはAI側から接続し直してください。
              </p>
            )}
          </li>
        ))}
      </ul>

      {/* The range lives beside the delegation it narrows, because it only
          means anything in terms of one: "this AI client, this workspace,
          this shape of change". */}
      <AutoApplyRanges store={store} connections={connections ?? []} />
    </section>
  );
}
