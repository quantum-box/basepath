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
import {
  changeSetFrom,
  summarize,
  type ChangeSet,
} from "../src/shared/changeView";
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
  // Today/week results also have an `items` array, but each entry is an
  // action wrapper rather than a plan node. A graph always carries either
  // its relations or the focused `nodes` shape, including an empty relation
  // list for a one-node preview.
  return Array.isArray(source.relations) || Array.isArray(source.nodes);
}

function isContextPayload(value: unknown): boolean {
  return Boolean(
    value &&
    typeof value === "object" &&
    Array.isArray((value as Record<string, unknown>).workspaces),
  );
}

type ProposalState = {
  title: string;
  status: string;
  approvalUrl: string | null;
  assumptions: string[];
  summary: ReturnType<typeof summarize>;
};

type PendingProposal = {
  proposal: ProposalState;
  payload: unknown;
  workspaceId?: string;
};

function changeFromPayload(value: unknown): ChangeSet | null {
  if (!value || typeof value !== "object") return null;
  const source = value as Record<string, unknown>;
  return changeSetFrom(value) ?? changeSetFrom(source.changeset);
}

function previewGraphFrom(value: unknown): unknown | null {
  if (!value || typeof value !== "object") return null;
  const graph = (value as Record<string, unknown>).preview_graph;
  return isGraphPayload(graph) ? graph : null;
}

function proposalFrom(value: unknown): ProposalState | null {
  if (!previewGraphFrom(value)) return null;
  const change = changeFromPayload(value);
  if (!change) return null;
  return {
    title: change.title,
    status: change.status,
    approvalUrl: change.approvalUrl,
    assumptions: change.assumptions,
    summary: summarize(change),
  };
}

function isFinishedChange(value: unknown): boolean {
  if (!value || typeof value !== "object") return false;
  const source = value as Record<string, unknown>;
  const change = changeFromPayload(value);
  return (
    source.already_applied === true ||
    source.auto_applied === true ||
    change?.status === "applied" ||
    change?.status === "rejected"
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
  const graphPayload =
    previewGraphFrom(payload) ?? (isGraphPayload(payload) ? payload : null);
  const graph = graphPayload
    ? buildPlanView({ graph: graphPayload, workspaceId })
    : null;
  return {
    ...current,
    workspaces: context?.workspaces ?? current.workspaces,
    workspace:
      context?.workspace ??
      (workspaceId
        ? (current.workspaces.find(
            (candidate) => candidate.id === workspaceId,
          ) ?? null)
        : null) ??
      current.workspace,
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
  const [proposal, setProposal] = useState<ProposalState | null>(null);
  const [viewMode, setViewMode] = useState<"list" | "map">("list");
  const [problem, setProblem] = useState<ReturnType<typeof problemFor> | null>(
    null,
  );
  const workspaceRef = useRef<string | undefined>(undefined);
  const viewRef = useRef(view);
  const generation = useRef(0);
  const refreshRef = useRef<
    (workspaceId?: string, limit?: number, preservedGraph?: unknown) => void
  >(() => undefined);
  const pendingWorkspaceRefresh = useRef<string | undefined>(undefined);
  const pendingProposal = useRef<PendingProposal | null>(null);
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
          const nextProposal = proposalFrom(payload);
          if (nextProposal) {
            pendingWorkspaceRefresh.current = undefined;
            const workspaceId =
              workspaceIdFrom(payload) ?? workspaceRef.current;
            if (!viewRef.current.workspace) {
              pendingProposal.current = {
                proposal: nextProposal,
                payload,
                workspaceId,
              };
              setLoading(true);
              setStale(true);
              void refreshRef.current(workspaceId);
              return;
            }
            generation.current += 1;
            pendingProposal.current = null;
            const next = mergeToolPayload(
              viewRef.current,
              payload,
              workspaceId,
            );
            viewRef.current = next;
            setView(next);
            setProposal(nextProposal);
            setLoading(false);
            setProblem(null);
            setStale(false);
            return;
          }
          if (isFinishedChange(payload)) {
            pendingWorkspaceRefresh.current = undefined;
            pendingProposal.current = null;
            setProposal(null);
            setStale(true);
            setLoading(true);
            void refreshRef.current(workspaceIdFrom(payload));
            return;
          }
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
          const workspaceIsKnown = workspaceId
            ? viewRef.current.workspaces.some(
                (workspace) => workspace.id === workspaceId,
              )
            : true;
          if (workspaceId && !workspaceIsKnown) {
            // The host may send a graph for a workspace that was just granted
            // while this iframe was open. Refresh context before rendering it,
            // then reapply this graph so a focused breakdown is not replaced
            // by the full graph fetched during the context refresh.
            setLoading(true);
            setStale(true);
            void refreshRef.current(workspaceId, undefined, payload);
            return;
          }
          const next = mergeToolPayload(viewRef.current, payload, workspaceId);
          viewRef.current = next;
          setView(next);
          // Context, Today, and Week results can arrive after a proposal was
          // rendered. They do not replace the saved graph, so keep the draft
          // marker until a committed graph or finished change arrives.
          if (isGraphPayload(payload)) setProposal(null);
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
    async (workspaceId?: string, limit?: number, preservedGraph?: unknown) => {
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
      const pending = pendingProposal.current;
      let next: PlanView;
      if (pending) {
        pendingProposal.current = null;
        next = mergeToolPayload(
          result.view,
          pending.payload,
          pending.workspaceId,
        );
        setProposal(pending.proposal);
      } else {
        next = preservedGraph
          ? mergeToolPayload(result.view, preservedGraph, workspaceId)
          : result.view;
        setProposal(null);
      }
      viewRef.current = next;
      setView(next);
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
    <main className="mcp-app-surface">
      <PlanViewPanel
        view={view}
        tree={tree}
        loading={loading}
        stale={stale}
        problem={problem ? { ...problem, retry: () => void refresh() } : null}
        proposal={proposal}
        onExpand={() => void refresh(workspaceRef.current, 200)}
        showHeader={false}
        showWorkspaceSwitcher={false}
        showDetails={false}
        showActions={false}
        showWeek={false}
        flowContent={<PlanFlow roots={view.nodes} tree={tree} />}
        showViewToggle
        viewMode={viewMode}
        onViewModeChange={setViewMode}
      />
    </main>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BasepathApp />
  </StrictMode>,
);
