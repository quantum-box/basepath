import { useCallback, useEffect, useRef, useState } from "react";
import {
  ApiError,
  request,
  type Snapshot,
  type Workspace,
  type Settings,
  type Invitation,
} from "./api";
export function useWorkspace() {
  const [me, setMe] = useState<{ id: string; name: string; mode: string }>({
    id: "",
    name: "あなた",
    mode: "local-preview",
  });
  const [auth, setAuth] = useState<{
    mode: string;
    configured: boolean;
    field_configured: boolean;
  }>({ mode: "local-preview", configured: false, field_configured: false });
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [invitations, setInvitations] = useState<Invitation[]>([]);
  const [settings, setSettings] = useState<Settings>({
    compact: false,
    notifications: true,
    timezone: "Asia/Tokyo",
  });
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const [savedAt, setSavedAt] = useState<Date | null>(null);
  const busy = useRef(false);
  const retryKeys = useRef(new Map<string, string>());
  const sequence = useRef(0);
  const identityId = useRef("");
  const refresh = useCallback(async () => {
    const seq = ++sequence.current;
    const [ws, prefs, identity, authentication, invites] = await Promise.all([
      request<Workspace[]>("GET", "/v1/workspaces"),
      request<Settings>("GET", "/v1/settings"),
      request<typeof me>("GET", "/v1/me"),
      request<typeof auth>("GET", "/auth/status"),
      request<Invitation[]>("GET", "/v1/invitations"),
    ]);
    if (seq !== sequence.current) return;
    const sameIdentity = identityId.current === identity.id;
    identityId.current = identity.id;
    setMe(identity);
    setWorkspaces(ws);
    setInvitations(invites);
    setSnapshots((previous) =>
      sameIdentity
        ? previous.filter((s) => ws.some((w) => w.id === s.workspace_id))
        : [],
    );
    const result = await Promise.allSettled(
      ws.map((w) =>
        request<Snapshot>("GET", `/v1/workspaces/${w.id}/snapshot`),
      ),
    );
    if (seq !== sequence.current) return;
    const accessible: Workspace[] = [];
    const snapshots: Snapshot[] = [];
    for (let index = 0; index < result.length; index++) {
      const entry = result[index];
      if (entry.status === "fulfilled") {
        accessible.push(ws[index]);
        snapshots.push(entry.value);
      } else if (!(
        entry.reason instanceof ApiError &&
        [403, 404].includes(entry.reason.status)
      )) {
        if (
          entry.reason instanceof ApiError &&
          entry.reason.code === "UNAUTHENTICATED"
        )
          throw entry.reason;
        // Retain no snapshot for a workspace whose access could not be refreshed.
        setError(
          entry.reason instanceof ApiError
            ? entry.reason
            : new ApiError(
                "CONNECTION_ERROR",
                "一部のワークスペースを取得できませんでした",
              ),
        );
      }
    }
    setMe(identity);
    setAuth(authentication);
    setWorkspaces(accessible);
    setSnapshots(snapshots);
    setInvitations(invites);
    setSettings(prefs);
    setLoading(false);
    setError((current) =>
      current?.code === "UNAUTHENTICATED" ? null : current,
    );
  }, []);
  const handleError = useCallback((e: unknown) => {
    const error =
      e instanceof ApiError
        ? e
        : new ApiError(
            "UNKNOWN_ERROR",
            e instanceof Error ? e.message : "読み込みに失敗しました",
          );
    if (error.code === "UNAUTHENTICATED") {
      sequence.current++;
      setSnapshots([]);
      setWorkspaces([]);
      setInvitations([]);
      setMe({ id: "", name: "あなた", mode: "tachyon" });
      retryKeys.current.clear();
      identityId.current = "";
    }
    setError(error);
  }, []);
  useEffect(() => {
    void refresh().catch((e) => {
      handleError(e);
      setLoading(false);
    });
    const sync = () => {
      if (!busy.current && document.visibilityState === "visible")
        void refresh().catch(handleError);
    };
    window.addEventListener("focus", sync);
    document.addEventListener("visibilitychange", sync);
    const timer = window.setInterval(sync, 30_000);
    return () => {
      window.removeEventListener("focus", sync);
      document.removeEventListener("visibilitychange", sync);
      window.clearInterval(timer);
      sequence.current++;
    };
  }, [refresh, handleError]);
  const write = useCallback(
    <T>(method: string, path: string, body: unknown = {}) => {
      const fingerprint = JSON.stringify([method, path, body]);
      let key = retryKeys.current.get(fingerprint);
      if (!key) {
        key = crypto.randomUUID();
        retryKeys.current.set(fingerprint, key);
      }
      return request<T>(method, path, body, key);
    },
    [],
  );
  const run = useCallback(
    async (operation: () => Promise<unknown>, onSuccess?: () => void) => {
      if (busy.current) return false;
      busy.current = true;
      setPending(true);
      setError(null);
      try {
        await operation();
        await refresh();
        retryKeys.current.clear();
        setSavedAt(new Date());
        onSuccess?.();
        return true;
      } catch (e) {
        let failure = e;
        if (e instanceof ApiError && [403, 404].includes(e.status)) {
          try {
            await refresh();
          } catch (refreshError) {
            failure = refreshError;
          }
        }
        handleError(failure);
        return false;
      } finally {
        busy.current = false;
        setPending(false);
      }
    },
    [refresh, handleError],
  );
  const reload = useCallback(() => run(async () => {}), [run]);
  return {
    me,
    auth,
    snapshots,
    workspaces,
    invitations,
    settings,
    loading,
    pending,
    error,
    savedAt,
    refresh: reload,
    run,
    write,
    clearError: () => setError(null),
  };
}
export type WorkspaceStore = ReturnType<typeof useWorkspace>;
