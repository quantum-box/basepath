import { useEffect, useMemo, useState } from "react";
import { uiId, type Item, type Workspace } from "./api";
import { Icon } from "./icons";
import type { WorkspaceStore } from "./useWorkspace";

type Draft = {
  step: number;
  scope: Workspace["scope"];
  workspaceId: string;
  wish: string;
  title: string;
  purpose: string;
  dueDate: string;
  metricName: string;
  metricUnit: string;
  metricBaseline: string;
  metricTarget: string;
  initiativeTitle: string;
  actionTitle: string;
};

const blank = (workspace?: Workspace): Draft => ({
  step: 0,
  scope: workspace?.scope || "個人",
  workspaceId: workspace?.id || "",
  wish: "",
  title: "",
  purpose: "",
  dueDate: "",
  metricName: "",
  metricUnit: "",
  metricBaseline: "",
  metricTarget: "",
  initiativeTitle: "",
  actionTitle: "",
});

export function OnboardingWizard({
  store,
  workspaces,
  onComplete,
}: {
  store: WorkspaceStore;
  workspaces: Workspace[];
  onComplete: (goalId: string, actionId?: string) => void;
}) {
  const writable = workspaces.filter(
    (workspace) => workspace.role !== "viewer",
  );
  const storageKey = `pathbase:onboarding:${store.me.id || "anonymous"}`;
  const [open, setOpen] = useState(true);
  const [draft, setDraft] = useState<Draft>(() => blank(writable[0]));
  const [localError, setLocalError] = useState("");
  const selected = workspaces.find(
    (workspace) => workspace.id === draft.workspaceId,
  );
  const readOnly = !writable.length;

  useEffect(() => {
    const saved = localStorage.getItem(storageKey);
    if (saved) {
      try {
        const value = JSON.parse(saved) as Draft;
        setDraft({ ...blank(writable[0]), ...value });
        return;
      } catch {
        localStorage.removeItem(storageKey);
      }
    }
    setDraft(blank(writable[0]));
  }, [storageKey, workspaces.map((workspace) => workspace.id).join("|")]);

  useEffect(() => {
    if (draft.wish || draft.title || draft.step > 0)
      localStorage.setItem(storageKey, JSON.stringify(draft));
  }, [draft, storageKey]);

  const preview = useMemo(
    () => [
      { label: "目標", value: draft.title, required: true },
      { label: "成果指標", value: draft.metricName },
      { label: "期限", value: draft.dueDate },
      { label: "取り組み", value: draft.initiativeTitle },
      { label: "次の一歩", value: draft.actionTitle },
    ],
    [draft],
  );
  const update = (value: Partial<Draft>) =>
    setDraft((current) => ({ ...current, ...value }));
  const next = () => {
    setLocalError("");
    if (draft.step === 0 && !draft.workspaceId) {
      setLocalError("作成できるワークスペースがありません。");
      return;
    }
    if (draft.step === 1 && !draft.wish.trim()) {
      setLocalError("達成したいことを入力してください。");
      return;
    }
    if (draft.step === 1 && !draft.title.trim())
      update({ title: draft.wish.trim(), purpose: draft.wish.trim(), step: 2 });
    else update({ step: Math.min(2, draft.step + 1) });
  };
  async function save() {
    if (!selected || selected.role === "viewer" || !draft.title.trim()) return;
    setLocalError("");
    let result: { goal: Item; action: Item | null } | undefined;
    const metricRequested =
      draft.metricName.trim() ||
      draft.metricUnit.trim() ||
      draft.metricBaseline.trim() ||
      draft.metricTarget.trim();
    if (
      metricRequested &&
      (!draft.metricName.trim() ||
        !draft.metricUnit.trim() ||
        draft.metricBaseline === "" ||
        draft.metricTarget === "")
    ) {
      setLocalError(
        "成果指標は名前・単位・現在値・目標値をすべて入力してください。",
      );
      return;
    }
    if (
      metricRequested &&
      (!Number.isFinite(Number(draft.metricBaseline)) ||
        !Number.isFinite(Number(draft.metricTarget)) ||
        Number(draft.metricBaseline) === Number(draft.metricTarget))
    ) {
      setLocalError("成果指標の現在値と目標値は異なる数値で入力してください。");
      return;
    }
    const ok = await store.run(async () => {
      result = await store.write(
        "POST",
        `/v1/workspaces/${selected.id}/onboarding/complete`,
        {
          title: draft.title.trim(),
          purpose: draft.purpose.trim(),
          due_date: draft.dueDate || null,
          initiative_title: draft.initiativeTitle.trim(),
          action_title: draft.actionTitle.trim(),
          metric: metricRequested
            ? {
                name: draft.metricName.trim(),
                unit: draft.metricUnit.trim(),
                baseline: Number(draft.metricBaseline),
                target: Number(draft.metricTarget),
                direction:
                  Number(draft.metricTarget) >= Number(draft.metricBaseline)
                    ? "increase"
                    : "decrease",
              }
            : null,
        },
      );
    });
    if (ok && result) {
      localStorage.removeItem(storageKey);
      setOpen(false);
      onComplete(
        uiId(result.goal),
        result.action ? uiId(result.action) : undefined,
      );
    }
  }

  if (!open)
    return (
      <section className="panel onboarding-resume">
        <div>
          <strong>最初の目標を作りましょう</strong>
          <small>入力途中の内容はこの端末に保持されます。</small>
        </div>
        <button className="primary-button" onClick={() => setOpen(true)}>
          再開する
        </button>
      </section>
    );

  return (
    <section
      className="panel onboarding-wizard"
      aria-labelledby="onboarding-title"
    >
      <div className="onboarding-heading">
        <div>
          <small>はじめに · {draft.step + 1} / 3</small>
          <h2 id="onboarding-title">最初の目標を、動ける形に</h2>
          <p>
            タイトルだけでも完了できます。期限・数値・担当は、入力したものだけ保存します。
          </p>
        </div>
        <button className="text-link" onClick={() => setOpen(false)}>
          あとで
        </button>
      </div>
      <div
        className="onboarding-progress"
        aria-label={`3ステップ中${draft.step + 1}`}
      >
        {[0, 1, 2].map((step) => (
          <span key={step} className={step <= draft.step ? "active" : ""} />
        ))}
      </div>
      {readOnly ? (
        <div className="onboarding-readonly" role="note">
          <Icon name="shield" size={22} />
          <div>
            <strong>このワークスペースは閲覧のみです</strong>
            <p>owner または editor に作成を依頼してください。</p>
          </div>
        </div>
      ) : draft.step === 0 ? (
        <div className="onboarding-step">
          <h3>どこで使いますか？</h3>
          <div className="onboarding-scope-grid">
            {(["個人", "チーム", "組織"] as Workspace["scope"][]).map(
              (scope) => {
                const candidates = writable.filter(
                  (workspace) => workspace.scope === scope,
                );
                return (
                  <button
                    key={scope}
                    disabled={!candidates.length}
                    className={draft.scope === scope ? "active" : ""}
                    onClick={() =>
                      update({ scope, workspaceId: candidates[0]?.id || "" })
                    }
                  >
                    <Icon
                      name={
                        scope === "個人"
                          ? "user"
                          : scope === "チーム"
                            ? "users"
                            : "cube"
                      }
                      size={24}
                    />
                    <strong>{scope}</strong>
                    <small>
                      {candidates.length
                        ? `${candidates.length}件から選択`
                        : "利用できません"}
                    </small>
                  </button>
                );
              },
            )}
          </div>
          <label>
            ワークスペース
            <select
              value={draft.workspaceId}
              onChange={(event) => update({ workspaceId: event.target.value })}
            >
              {writable
                .filter((workspace) => workspace.scope === draft.scope)
                .map((workspace) => (
                  <option key={workspace.id} value={workspace.id}>
                    {workspace.name}
                  </option>
                ))}
            </select>
          </label>
        </div>
      ) : draft.step === 1 ? (
        <div className="onboarding-step">
          <label>
            達成したいこと
            <textarea
              autoFocus
              rows={5}
              value={draft.wish}
              onChange={(event) => update({ wish: event.target.value })}
              placeholder="例：毎週、本を読む時間をつくりたい"
            />
          </label>
          <p className="onboarding-hint">
            <Icon name="sparkle" size={18} />
            入力した文章をそのまま目標候補にします。日付や数値は推測しません。
          </p>
        </div>
      ) : (
        <div className="onboarding-review">
          <div className="onboarding-fields">
            <label>
              目標タイトル <span>必須</span>
              <input
                value={draft.title}
                onChange={(event) => update({ title: event.target.value })}
              />
            </label>
            <label>
              目的
              <textarea
                rows={3}
                value={draft.purpose}
                onChange={(event) => update({ purpose: event.target.value })}
              />
            </label>
            <div className="form-row">
              <label>
                期限 <small>任意</small>
                <input
                  type="date"
                  value={draft.dueDate}
                  onChange={(event) => update({ dueDate: event.target.value })}
                />
              </label>
              <label>
                取り組み <small>任意</small>
                <input
                  value={draft.initiativeTitle}
                  onChange={(event) =>
                    update({ initiativeTitle: event.target.value })
                  }
                  placeholder="例：読書時間を確保する"
                />
              </label>
            </div>
            <label>
              次の一歩 <small>任意</small>
              <input
                value={draft.actionTitle}
                onChange={(event) =>
                  update({ actionTitle: event.target.value })
                }
                placeholder="例：読みたい本を1冊選ぶ"
              />
            </label>
            <fieldset>
              <legend>
                成果指標 <small>任意</small>
              </legend>
              <div className="metric-fields">
                <input
                  aria-label="成果指標の名前"
                  placeholder="指標名"
                  value={draft.metricName}
                  onChange={(event) =>
                    update({ metricName: event.target.value })
                  }
                />
                <input
                  aria-label="成果指標の単位"
                  placeholder="単位"
                  value={draft.metricUnit}
                  onChange={(event) =>
                    update({ metricUnit: event.target.value })
                  }
                />
                <input
                  aria-label="成果指標の現在値"
                  type="number"
                  placeholder="現在値"
                  value={draft.metricBaseline}
                  onChange={(event) =>
                    update({ metricBaseline: event.target.value })
                  }
                />
                <input
                  aria-label="成果指標の目標値"
                  type="number"
                  placeholder="目標値"
                  value={draft.metricTarget}
                  onChange={(event) =>
                    update({ metricTarget: event.target.value })
                  }
                />
              </div>
            </fieldset>
          </div>
          <aside className="onboarding-preview">
            <small>保存前の確認</small>
            <h3>作成される内容</h3>
            {preview.map((item) => (
              <div key={item.label}>
                <span>{item.label}</span>
                <strong className={!item.value ? "unset" : ""}>
                  {item.value ||
                    (item.required ? "入力が必要です" : "設定しません")}
                </strong>
              </div>
            ))}
            <p>
              <Icon name="shield" size={16} />
              確認ボタンを押すまで保存されません。
            </p>
          </aside>
        </div>
      )}
      {(localError || store.error) && (
        <p className="save-error" role="alert">
          {localError || store.error?.message}
        </p>
      )}
      <div className="onboarding-actions">
        {draft.step > 0 && (
          <button
            className="secondary-button"
            disabled={store.pending}
            onClick={() => update({ step: draft.step - 1 })}
          >
            <Icon name="left" size={15} />
            戻る
          </button>
        )}
        <button className="text-link" onClick={() => setOpen(false)}>
          スキップ
        </button>
        {draft.step < 2 ? (
          <button className="primary-button" disabled={readOnly} onClick={next}>
            次へ
            <Icon name="right" size={15} />
          </button>
        ) : (
          <button
            className="primary-button"
            disabled={store.pending || !draft.title.trim()}
            onClick={() => void save()}
          >
            {store.pending ? "保存中…" : "この内容で作成"}
          </button>
        )}
      </div>
    </section>
  );
}
