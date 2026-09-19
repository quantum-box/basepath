/**
 * The approval screen, on Basepath's own origin.
 *
 * This is the only place a change set can be approved, and the reason is not
 * convenience. An approval has to be an act the server can attribute to the
 * person: here it is, because the request carries their own session, the
 * same-origin CSRF header, and a digest of exactly the content they were
 * shown. A button inside an AI host cannot supply any of that — a call from
 * there arrives as an ordinary tool call, indistinguishable from the model's.
 *
 * It is deep-linkable so the conversation can send someone straight here.
 */
import { useCallback, useEffect, useState } from "react";
import { ApiError, request } from "./api";
import { changeSetFrom, isExpired, type ChangeSet } from "./shared/changeView";
import { ChangeReview } from "./shared/ChangeReview";
import type { WorkspaceStore } from "./useWorkspace";

export type ChangeRoute = { workspaceId: string; changeId: string };

/** Reads `/changes/{workspace}/{id}` out of the current URL. */
export function changeRouteFromPath(pathname: string): ChangeRoute | null {
  const parts = pathname.split("/").filter(Boolean);
  if (parts.length !== 3 || parts[0] !== "changes") return null;
  return {
    workspaceId: decodeURIComponent(parts[1]),
    changeId: decodeURIComponent(parts[2]),
  };
}

export function ChangeApproval({
  route,
  store,
  onClose,
}: {
  route: ChangeRoute;
  store: WorkspaceStore;
  onClose: () => void;
}) {
  const [change, setChange] = useState<ChangeSet | null>(null);
  const [failure, setFailure] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const value = await request<unknown>(
        "GET",
        `/v1/workspaces/${encodeURIComponent(route.workspaceId)}/changesets/${encodeURIComponent(route.changeId)}`,
      );
      const parsed = changeSetFrom(value);
      setChange(parsed);
      setFailure(parsed ? "" : "この変更案を読み取れません。");
    } catch (error) {
      const api = error as ApiError;
      setFailure(
        api.status === 404
          ? "この変更案は見つかりません。すでに処理されたか、アクセスできません。"
          : api.message || "変更案を取得できません。",
      );
    }
  }, [route.workspaceId, route.changeId]);

  useEffect(() => {
    void load();
  }, [load]);

  const act = useCallback(
    async (intent: "approve" | "reject" | "apply") => {
      if (!change || busy) return;
      setBusy(true);
      setNotice(null);
      try {
        // The digest goes with the approval: if the content is not what was
        // rendered above, the server refuses rather than approving something
        // else.
        const body = intent === "approve" ? { hash: change.hash } : {};
        await store.write(
          "POST",
          `/v1/workspaces/${encodeURIComponent(change.workspaceId)}/changesets/${encodeURIComponent(change.id)}/${intent}`,
          body,
        );
        setNotice(
          intent === "approve"
            ? "承認しました。この内容を計画へ反映しました。"
            : intent === "apply"
              ? "適用しました。"
              : "取り下げました。",
        );
      } catch (error) {
        setNotice((error as ApiError).message || "操作できませんでした。");
      } finally {
        setBusy(false);
        await load();
        await store.refresh();
      }
    },
    [change, busy, store, load],
  );

  return (
    <div className="change-approval-screen">
      <header>
        <h1>変更案の確認</h1>
        <button type="button" onClick={onClose}>
          閉じる
        </button>
      </header>
      {failure && <p className="empty-value">{failure}</p>}
      {!change && !failure && <p className="empty-value">読み込んでいます…</p>}
      {change && (
        <ChangeReview
          change={change}
          workspaceName={
            store.workspaces.find(
              (workspace) => workspace.id === change.workspaceId,
            )?.name
          }
          busy={busy}
          notice={notice}
          onApprove={
            change.status === "pending" && !isExpired(change)
              ? () => void act("approve")
              : undefined
          }
          onApply={
            change.status === "approved" && !isExpired(change)
              ? () => void act("apply")
              : undefined
          }
          onReject={
            ["pending", "approved"].includes(change.status)
              ? () => void act("reject")
              : undefined
          }
        />
      )}
    </div>
  );
}
