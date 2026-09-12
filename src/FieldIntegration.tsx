import { useState } from "react";
import { request, type Item } from "./api";
import type { WorkspaceStore } from "./useWorkspace";
type Tenant = { id: string; name: string; environment: string };
type FieldTask = {
  id: string;
  tenantId: string;
  title: string;
  status: string;
  dueAt: string | null;
};
const metricKeys: Record<string, string> = {
  mrr: "MRR",
  arr: "ARR",
  backlogAmount: "受注残",
  receivableOutstanding: "売掛残",
  daysSalesOutstanding: "DSO",
};
export function FieldIntegration({
  store,
  workspaceId,
}: {
  store: WorkspaceStore;
  workspaceId: string;
}) {
  const [tenants, setTenants] = useState<Tenant[]>([]);
  const [tenant, setTenant] = useState("");
  const [tasks, setTasks] = useState<FieldTask[]>([]);
  const [values, setValues] = useState<Record<string, number | null>>({});
  const [offset, setOffset] = useState(0);
  const [message, setMessage] = useState("");
  async function loadTenants() {
    await store.run(async () => {
      const result = await request<Tenant[]>(
        "GET",
        "/v1/integrations/field/tenants",
      );
      setTenants(result);
      setTenant(result[0]?.id || "");
    });
  }
  async function loadTasks(page = 0) {
    await store.run(async () => {
      const result = await request<{ items: FieldTask[] }>(
        "GET",
        `/v1/integrations/field/tasks?tenant_id=${encodeURIComponent(tenant)}&offset=${page}`,
      );
      setTasks(result.items);
      setOffset(page);
      setMessage("");
    });
  }
  const metrics =
    store.snapshots.find((s) => s.workspace_id === workspaceId)?.metrics || [];
  return (
    <div className="feature-editor field-integration">
      <h3>Tachyon・Field連携</h3>
      <p className="empty-value">
        {store.auth.configured
          ? "Tachyonで認証済み。Field側の権限を操作ごとに確認します。"
          : "ローカル確認モードです。本番の認証先はTachyonです。OIDCの登録情報を設定するとログインできます。"}
      </p>
      {store.auth.configured && (
        <button
          className="text-link"
          onClick={async () => {
            await request("POST", "/auth/logout", {});
            location.reload();
          }}
        >
          Tachyonからログアウト
        </button>
      )}
      <details>
        <summary>Fieldの営業タスク・成果指標</summary>
        <p className="empty-value">
          Fieldにあるデータを参照して、自分の計画とつなげます。元の営業タスクはFieldで管理します。
        </p>
        {!store.auth.field_configured ? (
          <p className="empty-value">
            Field APIのURLとテナントの接続設定が必要です。
          </p>
        ) : (
          <>
            <button
              className="secondary-button"
              onClick={() => void loadTenants()}
              disabled={store.pending}
            >
              アクセスできる組織を取得
            </button>
            {!!tenants.length && (
              <>
                <label>
                  Fieldの組織
                  <select
                    value={tenant}
                    onChange={(e) => {
                      setTenant(e.target.value);
                      setTasks([]);
                      setValues({});
                    }}
                  >
                    {tenants.map((t) => (
                      <option key={t.id} value={t.id}>
                        {t.name}（{t.environment}）
                      </option>
                    ))}
                  </select>
                </label>
                <div className="button-row">
                  <button
                    className="secondary-button"
                    onClick={() => void loadTasks()}
                    disabled={store.pending}
                  >
                    営業タスクを見る
                  </button>
                  <button
                    className="secondary-button"
                    disabled={store.pending}
                    onClick={() =>
                      void store.run(async () => {
                        const result = await request<{
                          values: Record<string, number | null>;
                        }>(
                          "GET",
                          `/v1/integrations/field/metrics?tenant_id=${encodeURIComponent(tenant)}`,
                        );
                        setValues(result.values);
                      })
                    }
                  >
                    成果指標を見る
                  </button>
                </div>
              </>
            )}
            {tasks.map((t) => (
              <div className="field-task" key={t.id}>
                <span>
                  <strong>{t.title}</strong>
                  <small>
                    Field · {t.status === "done" ? "完了" : "未完了"}
                  </small>
                </span>
                <button
                  disabled={store.pending}
                  className="text-link"
                  onClick={() =>
                    void store.run(
                      () =>
                        store.write<Item>(
                          "POST",
                          `/v1/workspaces/${workspaceId}/field/attach-task`,
                          { tenant_id: tenant, task_id: t.id },
                        ),
                      () =>
                        setMessage(
                          "Fieldへの参照を持つ行動を追加しました。元のデータは変更していません。",
                        ),
                    )
                  }
                >
                  行動として取り込む
                </button>
              </div>
            ))}
            {!!tasks.length && (
              <div className="button-row">
                <button
                  disabled={!offset || store.pending}
                  onClick={() => void loadTasks(Math.max(0, offset - 50))}
                >
                  前の50件
                </button>
                <button
                  disabled={tasks.length < 50 || store.pending}
                  onClick={() => void loadTasks(offset + 50)}
                >
                  次の50件
                </button>
              </div>
            )}
            {message && <p role="status">{message}</p>}
            {Object.entries(values)
              .filter(([k]) => metricKeys[k])
              .map(([k, v]) => (
                <div key={k} className="metric-value">
                  <strong>
                    {metricKeys[k]}：
                    {v == null
                      ? "未計測"
                      : v.toLocaleString("ja-JP") +
                        (k === "daysSalesOutstanding" ? "日" : "円")}
                  </strong>
                  {v != null && (
                    <form
                      className="button-row"
                      onSubmit={(e) => {
                        e.preventDefault();
                        const data = new FormData(e.currentTarget);
                        void store.run(
                          () =>
                            store.write(
                              "POST",
                              `/v1/workspaces/${workspaceId}/field/record-metric`,
                              {
                                tenant_id: tenant,
                                field_key: k,
                                metric_id: data.get("metric_id"),
                              },
                            ),
                          () =>
                            setMessage("Fieldの観測値を出典付きで記録しました"),
                        );
                      }}
                    >
                      <select
                        name="metric_id"
                        aria-label={`${metricKeys[k]}を記録する指標`}
                        required
                      >
                        <option value="">PathBaseの指標を選択</option>
                        {metrics
                          .filter(
                            (m) =>
                              m.unit ===
                              (k === "daysSalesOutstanding" ? "日" : "円"),
                          )
                          .map((m) => (
                            <option key={m.id} value={m.id}>
                              {m.name}
                            </option>
                          ))}
                      </select>
                      <button
                        className="secondary-button"
                        disabled={store.pending}
                      >
                        観測として記録
                      </button>
                    </form>
                  )}
                </div>
              ))}
          </>
        )}
      </details>
    </div>
  );
}
