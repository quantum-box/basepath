import { useEffect, useRef, useState, type FormEvent } from "react";
import { request, type Memberships, type Workspace } from "./api";
import type { WorkspaceStore } from "./useWorkspace";
import { Icon } from "./icons";

const roles = { owner: "オーナー", editor: "編集できる", viewer: "閲覧のみ" };
type Confirmation = {
  message: string;
  method: string;
  path: string;
  body: object;
};

export function WorkspaceMembers({
  store,
  workspaceId,
  onSelect,
}: {
  store: WorkspaceStore;
  workspaceId?: string;
  onSelect: (id: string, scope?: Workspace["scope"]) => void;
}) {
  const workspace = store.workspaces.find((w) => w.id === workspaceId);
  const [data, setData] = useState<Memberships | null>(null);
  const [loadError, setLoadError] = useState("");
  const [reload, setReload] = useState(0);
  const [copied, setCopied] = useState(false);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const confirmationRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    confirmationRef.current?.focus();
  }, [confirmation]);
  useEffect(() => {
    let cancelled = false;
    setData((previous) =>
      previous?.workspace.id === workspace?.id &&
      previous?.workspace.role === workspace?.role
        ? previous
        : null,
    );
    setLoadError("");
    if (workspace)
      void request<Memberships>(
        "GET",
        `/v1/workspaces/${workspace.id}/members`,
      ).then(
        (value) => {
          if (!cancelled) setData(value);
        },
        (error) => {
          if (!cancelled) setLoadError(error.message);
        },
      );
    return () => {
      cancelled = true;
    };
  }, [workspace?.id, workspace?.version, workspace?.role, reload, store.me.id]);
  useEffect(() => {
    setConfirmation(null);
  }, [workspace?.id]);
  const canManage =
    data?.workspace.role === "owner" && data.workspace.scope !== "個人";
  const base = `/v1/workspaces/${workspaceId}`;
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const values = new FormData(form);
    let created: Workspace | undefined;
    await store.run(
      async () => {
        created = await store.write<Workspace>("POST", "/v1/workspaces", {
          name: values.get("name"),
          scope: values.get("scope"),
          timezone: store.settings.timezone,
        });
      },
      () => {
        form.reset();
        if (created) onSelect(created.id, created.scope);
      },
    );
  }
  async function invite(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!data) return;
    const form = event.currentTarget;
    const values = new FormData(form);
    await store.run(
      () =>
        store.write("POST", `${base}/invitations`, {
          target_actor: String(values.get("target_actor") || "").trim(),
          role: values.get("role"),
          expected_version: data.workspace.version,
        }),
      () => form.reset(),
    );
  }
  return (
    <div className="feature-editor workspace-management">
      <div className="account-identity">
        <Icon name="users" size={24} />
        <div>
          <strong>{store.me.name}</strong>
          <small>
            {store.me.mode === "tachyon"
              ? "TachyonユーザーID"
              : "ローカル確認用アカウント"}
          </small>
          <code>{store.me.id}</code>
        </div>
        <button
          type="button"
          className="text-link"
          onClick={() => {
            void navigator.clipboard.writeText(store.me.id).then(
              () => setCopied(true),
              () => setLoadError("IDを選択してコピーしてください"),
            );
          }}
        >
          {copied ? "コピーしました" : "IDをコピー"}
        </button>
      </div>

      {store.invitations.length > 0 && (
        <section aria-label="届いた招待" className="invitation-inbox">
          <h3>
            届いた招待 <span>{store.invitations.length}</span>
          </h3>
          {store.invitations.map((invitation) => (
            <div className="member-row" key={invitation.id}>
              <div>
                <strong>{invitation.workspace_name}</strong>
                <small>
                  {roles[invitation.role]} ·{" "}
                  {new Date(invitation.expires_at).toLocaleDateString("ja-JP")}
                  まで
                </small>
                <small>招待者：{invitation.created_by}</small>
              </div>
              <div className="button-row">
                <button
                  className="primary-button"
                  disabled={store.pending}
                  onClick={() =>
                    void store.run(
                      () =>
                        store.write(
                          "POST",
                          `/v1/invitations/${invitation.id}/accept`,
                          { expected_version: invitation.version },
                        ),
                      () => onSelect(invitation.workspace_id),
                    )
                  }
                >
                  参加する
                </button>
                <button
                  className="secondary-button"
                  disabled={store.pending}
                  onClick={() =>
                    setConfirmation({
                      message: `「${invitation.workspace_name}」への招待を辞退します。`,
                      method: "POST",
                      path: `/v1/invitations/${invitation.id}/decline`,
                      body: { expected_version: invitation.version },
                    })
                  }
                >
                  辞退
                </button>
              </div>
            </div>
          ))}
        </section>
      )}

      <label>
        ワークスペース
        <select
          value={workspace?.id || ""}
          onChange={(event) => onSelect(event.target.value)}
          aria-label="管理するワークスペース"
        >
          {store.workspaces.map((w) => (
            <option value={w.id} key={w.id}>
              {w.name} · {roles[w.role]}
            </option>
          ))}
        </select>
      </label>
      {loadError && (
        <p role="alert" className="save-error">
          {loadError}
          <button onClick={() => setReload((n) => n + 1)}>再読み込み</button>
        </p>
      )}
      {workspace && !data && !loadError && (
        <p className="empty-value" role="status">
          メンバーを読み込んでいます…
        </p>
      )}
      {data && (
        <>
          <p className="modal-intro">
            {data.workspace.name} · {roles[data.workspace.role]}
          </p>
          {data.workspace.scope === "個人" ? (
            <p className="demo-note">
              この領域はあなた専用です。一緒に使うときは、別のワークスペースを作成できます。
            </p>
          ) : data.workspace.local ? (
            <p className="demo-note">
              この領域はこの端末内だけに保存されます。オンラインでの招待はTachyonでログインした環境で利用できます。
            </p>
          ) : (
            <p className="demo-note">
              メンバーはこの領域の目標・行動・記録を閲覧できます。個人のワークスペースは共有されません。
            </p>
          )}
          <section aria-label="参加メンバー">
            {data.members.map((member) => {
              const self = member.actor === store.me.id;
              const lastOwner =
                member.role === "owner" &&
                data.members.filter((m) => m.role === "owner").length === 1;
              return (
                <div className="member-row" key={member.actor}>
                  <div>
                    <strong>
                      {self ? `${store.me.name}（あなた）` : member.actor}
                    </strong>
                    <small>{self ? member.actor : roles[member.role]}</small>
                  </div>
                  {canManage ? (
                    <div className="member-controls">
                      <select
                        value={member.role}
                        disabled={store.pending || lastOwner}
                        aria-label={`${member.actor}の権限`}
                        onChange={(event) =>
                          setConfirmation({
                            message: `${self ? "あなた" : member.actor}の権限を「${roles[event.target.value as Workspace["role"]]}」に変更します。${event.target.value === "owner" ? "メンバーと招待を管理できるようになります。" : ""}`,
                            method: "PATCH",
                            path: `${base}/members/${encodeURIComponent(member.actor)}`,
                            body: {
                              role: event.target.value,
                              expected_version: data.workspace.version,
                            },
                          })
                        }
                      >
                        {Object.entries(roles).map(([role, label]) => (
                          <option key={role} value={role}>
                            {label}
                          </option>
                        ))}
                      </select>
                      <button
                        className="text-link"
                        disabled={store.pending || lastOwner}
                        onClick={() =>
                          setConfirmation({
                            message: `${self ? "あなた" : member.actor}を「${data.workspace.name}」から解除します。この領域にアクセスできなくなります。記録は残ります。`,
                            method: "DELETE",
                            path: `${base}/members/${encodeURIComponent(member.actor)}`,
                            body: { expected_version: data.workspace.version },
                          })
                        }
                      >
                        解除
                      </button>
                    </div>
                  ) : (
                    <small>{roles[member.role]}</small>
                  )}
                </div>
              );
            })}
          </section>
          {canManage && (
            <small className="empty-value">
              オーナーは最低1人必要です。権限の変更と解除は、内容を確認してから反映されます。
            </small>
          )}
          {canManage && !data.workspace.local && (
            <>
              <form className="editor-form" onSubmit={invite}>
                <fieldset disabled={store.pending}>
                  <h3>メンバーを招待</h3>
                  <label>
                    TachyonユーザーID
                    <input
                      name="target_actor"
                      required
                      maxLength={100}
                      pattern="us_[A-Za-z0-9_-]+"
                      placeholder="us_…"
                      autoComplete="off"
                    />
                  </label>
                  <label>
                    参加後の権限
                    <select name="role" defaultValue="editor">
                      <option value="editor">編集できる</option>
                      <option value="viewer">閲覧のみ</option>
                    </select>
                  </label>
                  <p className="empty-value">
                    相手の「メンバー」画面でコピーしたIDを入力してください。相手がPathBaseにログインすると招待が表示されます。有効期限は7日間です。
                  </p>
                  <button className="primary-button" type="submit">
                    招待を作成
                  </button>
                </fieldset>
              </form>
              {data.invitations.length > 0 && (
                <section aria-label="参加待ちの招待">
                  <h3>参加待ち</h3>
                  {data.invitations.map((invitation) => (
                    <div className="member-row" key={invitation.id}>
                      <div>
                        <strong>{invitation.target_actor}</strong>
                        <small>
                          {roles[invitation.role]} ·{" "}
                          {new Date(invitation.expires_at).toLocaleDateString(
                            "ja-JP",
                          )}
                          まで
                        </small>
                      </div>
                      <button
                        className="text-link"
                        disabled={store.pending}
                        onClick={() =>
                          setConfirmation({
                            message: `${invitation.target_actor}への招待を取り消します。`,
                            method: "DELETE",
                            path: `${base}/invitations/${invitation.id}`,
                            body: { expected_version: data.workspace.version },
                          })
                        }
                      >
                        取り消す
                      </button>
                    </div>
                  ))}
                </section>
              )}
            </>
          )}
          {canManage && (
            <WorkspaceName
              key={workspaceId}
              workspace={data.workspace}
              store={store}
            />
          )}
          {data.workspace.scope !== "個人" &&
            data.workspace.role !== "owner" && (
              <button
                className="text-link"
                disabled={store.pending}
                onClick={() =>
                  setConfirmation({
                    message: `「${data.workspace.name}」から退出します。再参加には招待が必要です。`,
                    method: "POST",
                    path: `${base}/leave`,
                    body: { expected_version: data.workspace.version },
                  })
                }
              >
                このワークスペースから退出
              </button>
            )}
        </>
      )}

      {confirmation && (
        <div
          className="conflict-note"
          role="region"
          aria-label="変更内容の確認"
          tabIndex={-1}
          ref={confirmationRef}
        >
          <p>{confirmation.message}</p>
          <div className="button-row">
            <button
              className="primary-button"
              disabled={store.pending}
              onClick={() =>
                void store.run(
                  () =>
                    store.write(
                      confirmation.method,
                      confirmation.path,
                      confirmation.body,
                    ),
                  () => setConfirmation(null),
                )
              }
            >
              確認して実行
            </button>
            <button
              className="secondary-button"
              disabled={store.pending}
              onClick={() => setConfirmation(null)}
            >
              キャンセル
            </button>
          </div>
        </div>
      )}

      <details>
        <summary>新しいワークスペースを作成</summary>
        <form className="editor-form" onSubmit={create}>
          <fieldset disabled={store.pending}>
            <label>
              名前
              <input
                name="name"
                placeholder="例：週末の読書会"
                required
                maxLength={100}
              />
            </label>
            <label>
              領域
              <select name="scope" defaultValue="チーム">
                <option>チーム</option>
                <option>組織</option>
              </select>
            </label>
            <p className="empty-value">
              空の領域から始めます。個人の目標や記録は移動されません。
            </p>
            <button className="primary-button" type="submit">
              作成する
            </button>
          </fieldset>
        </form>
      </details>
    </div>
  );
}

function WorkspaceName({
  workspace,
  store,
}: {
  workspace: Workspace;
  store: WorkspaceStore;
}) {
  const [name, setName] = useState(workspace.name);
  const [original, setOriginal] = useState(workspace.name);
  const [base, setBase] = useState(workspace.version);
  const dirty = name !== original;
  const conflict = dirty && base !== workspace.version;
  useEffect(() => {
    if (!dirty) {
      setName(workspace.name);
      setOriginal(workspace.name);
      setBase(workspace.version);
    }
  }, [workspace.name, workspace.version, dirty]);
  return (
    <details>
      <summary>ワークスペースの名前</summary>
      <form
        className="editor-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (conflict) return;
          void store.run(async () => {
            const saved = await store.write<Workspace>(
              "PATCH",
              `/v1/workspaces/${workspace.id}`,
              { name, timezone: workspace.timezone, expected_version: base },
            );
            setName(saved.name);
            setOriginal(saved.name);
            setBase(saved.version);
          });
        }}
      >
        <label>
          名前
          <input
            name="name"
            required
            maxLength={100}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        {conflict && (
          <div className="conflict-note">
            <p>共有設定に更新があります。現在の名前：{workspace.name}</p>
            <button type="button" onClick={() => setBase(workspace.version)}>
              更新を確認し、入力中の名前を使う
            </button>
          </div>
        )}
        <button
          className="secondary-button"
          disabled={store.pending || conflict || !dirty}
        >
          保存する
        </button>
      </form>
    </details>
  );
}
