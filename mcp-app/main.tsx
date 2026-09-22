/**
 * The single Basepath MCP App surface.
 *
 * The host owns credentials and proxies every tool call. This iframe only
 * renders the latest authorized result and keeps temporary tree state such as
 * folding and selection locally.
 */
import { StrictMode, useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  useApp,
  useHostStyleVariables,
} from "@modelcontextprotocol/ext-apps/react";
import {
  HostError,
  McpToolHost,
  loadPlanView,
  structuredResult,
} from "../src/shared/host";
import {
  buildPlanView,
  emptyPlanView,
  workspaceIdFrom,
  type PlanView,
} from "../src/shared/viewModel";
import { PlanViewPanel } from "../src/shared/PlanView";
import { useTreeState } from "../src/shared/useTreeState";
import { PlanFlow } from "./PlanFlow";
import "./document.css";
import "../src/shared/planView.css";

const APP_INFO = { name: "Basepath", version: "1.1.0" };

function problemFor(error: HostError | Error) {
  if (error instanceof HostError) {
    if (error.code === "INSUFFICIENT_SCOPE") {
      return {
        title: "権限が足りません",
        detail: "Basepathの接続設定で「目標と行動を読む」を許可してください。",
      };
    }
    if (error.code === "CONNECTION_REVOKED") {
      return {
        title: "接続が解除されています",
        detail: "Basepath側で接続し直してください。",
      };
    }
    if (error.code === "UNAUTHENTICATED" || error.code === "INVALID_TOKEN") {
      return {
        title: "サインインが必要です",
        detail: "ホスト側でBasepathへ接続し直してください。",
      };
    }
    if (error.code === "HOST_UNSUPPORTED") {
      return {
        title: "このホストでは表示できません",
        detail: "MCPツール呼び出しに対応したホストが必要です。",
      };
    }
    return { title: "表示できませんでした", detail: error.message };
  }
  return { title: "表示できませんでした", detail: error.message };
}

function isGraphPayload(value: unknown): boolean {
  if (!value || typeof value !== "object") return false;
  const source = value as Record<string, unknown>;
  return Array.isArray(source.relations) || Array.isArray(source.nodes);
}

function isContextPayload(value: unknown): boolean {
  return Boolean(
    value &&
    typeof value === "object" &&
    Array.isArray((value as Record<string, unknown>).workspaces),
  );
}

function mergeToolPayload(
  current: PlanView,
  payload: unknown,
  workspaceId?: string,
): PlanView {
  const context = isContextPayload(payload)
    ? buildPlanView({ context: payload, workspaceId })
    : null;
  const graph = isGraphPayload(payload)
    ? buildPlanView({ graph: payload, workspaceId })
    : null;
  return {
    ...current,
    workspaces: context?.workspaces ?? current.workspaces,
    workspace: context?.workspace ?? current.workspace,
    nodes: graph?.nodes ?? current.nodes,
    truncated: graph?.truncated ?? current.truncated,
    limit: graph?.limit || current.limit,
  };
}

function clearForWorkspace(current: PlanView, workspaceId: string): PlanView {
  return {
    ...current,
    workspace:
      current.workspaces.find((candidate) => candidate.id === workspaceId) ??
      null,
    nodes: [],
    truncated: false,
    limit: 0,
    localDate: "",
    actions: [],
    week: null,
  };
}

function BasepathApp() {
  const [view, setView] = useState<PlanView>(emptyPlanView);
  const [loading, setLoading] = useState(true);
  const [stale, setStale] = useState(false);
  const [viewMode, setViewMode] = useState<"list" | "map">("list");
  const [problem, setProblem] = useState<ReturnType<typeof problemFor> | null>(
    null,
  );
  const workspaceRef = useRef<string | undefined>(undefined);
  const viewRef = useRef(view);
  const generation = useRef(0);
  const refreshRef = useRef<(workspaceId?: string, limit?: number) => void>(
    () => undefined,
  );
  const pendingWorkspaceRefresh = useRef<string | undefined>(undefined);
  const tree = useTreeState(view.workspace?.id ?? "");

  useEffect(() => {
    viewRef.current = view;
  }, [view]);

  const { app, isConnected, error } = useApp({
    appInfo: APP_INFO,
    capabilities: {},
    onAppCreated: (created) => {
      created.ontoolinput = (input) => {
        const workspaceId = workspaceIdFrom(input);
        if (workspaceId) {
          const changed = workspaceRef.current !== workspaceId;
          workspaceRef.current = workspaceId;
          if (changed) {
            // A non-graph tool can arrive after a workspace switch. Do not
            // let its result clear the loading state while old nodes remain
            // under the new workspace name.
            generation.current += 1;
            pendingWorkspaceRefresh.current = workspaceId;
            const next = clearForWorkspace(viewRef.current, workspaceId);
            viewRef.current = next;
            setView(next);
            setLoading(true);
          }
        }
        setStale(true);
      };
      created.ontoolresult = (params) => {
        try {
          const payload = structuredResult(params);
          const refreshWorkspace = pendingWorkspaceRefresh.current;
          if (refreshWorkspace && !isGraphPayload(payload)) {
            pendingWorkspaceRefresh.current = undefined;
            setLoading(true);
            setStale(true);
            void refreshRef.current(refreshWorkspace);
            return;
          }
          pendingWorkspaceRefresh.current = undefined;
          const workspaceId = workspaceIdFrom(payload) ?? workspaceRef.current;
          const next = mergeToolPayload(viewRef.current, payload, workspaceId);
          viewRef.current = next;
          setView(next);
          setLoading(false);
          setProblem(null);
          setStale(false);
        } catch (caught) {
          if (caught instanceof HostError) setProblem(problemFor(caught));
        }
      };
    },
  });

  useHostStyleVariables(app);

  const hostFor = useCallback(() => {
    if (!app) return null;
    const capabilities = app.getHostCapabilities();
    return new McpToolHost((params) => app.callServerTool(params), {
      serverTools: Boolean(capabilities?.serverTools),
    });
  }, [app]);

  const refresh = useCallback(
    async (workspaceId?: string, limit?: number) => {
      const host = hostFor();
      if (!host) return;
      const currentGeneration = ++generation.current;
      setLoading(true);
      setProblem(null);
      const result = await loadPlanView(host, {
        workspaceId: workspaceId ?? workspaceRef.current,
        limit,
        includeToday: false,
        includeWeek: false,
      });
      if (currentGeneration !== generation.current) return;
      if (result.error) {
        setProblem(problemFor(result.error));
        setLoading(false);
        return;
      }
      if (result.view.workspace)
        workspaceRef.current = result.view.workspace.id;
      setView(result.view);
      setLoading(false);
      setStale(false);
    },
    [hostFor],
  );
  refreshRef.current = refresh;

  useEffect(() => {
    if (!isConnected || !app) return;
    const capabilities = app.getHostCapabilities();
    if (capabilities?.serverTools) void refresh();
    else if (!viewRef.current.workspace) {
      setProblem({
        title: "このホストでは表示できません",
        detail: "MCPツール呼び出しに対応したホストが必要です。",
      });
      setLoading(false);
    }
  }, [app, isConnected, refresh]);

  useEffect(() => {
    if (error) {
      setProblem({
        title: "ホストに接続できませんでした",
        detail: error.message,
      });
      setLoading(false);
    }
  }, [error]);

  return (
    <PlanViewPanel
      view={view}
      tree={tree}
      loading={loading}
      stale={stale}
      problem={problem ? { ...problem, retry: () => void refresh() } : null}
      onExpand={() => void refresh(workspaceRef.current, 200)}
      showWorkspaceSwitcher={false}
      showDetails={false}
      showActions={false}
      showWeek={false}
      flowContent={<PlanFlow roots={view.nodes} tree={tree} />}
      showViewToggle
      viewMode={viewMode}
      onViewModeChange={setViewMode}
    />
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BasepathApp />
  </StrictMode>,
);
