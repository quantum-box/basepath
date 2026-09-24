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
import { changeSetFrom, type ChangeSet } from "../src/shared/changeView";
import {
  PlanViewPanel,
  type ConversationDraftView,
  type PlanProposal,
} from "../src/shared/PlanView";
import { useTreeState } from "../src/shared/useTreeState";
import { PlanFlow } from "./PlanFlow";
import "./document.css";
import "../src/shared/planView.css";

const APP_INFO = { name: "Basepath", version: "1.1.0" };

type ChatGptRuntime = {
  openExternal?: (input: { href: string }) => Promise<unknown> | unknown;
};

function chatGptRuntime(): ChatGptRuntime | undefined {
  return (globalThis as typeof globalThis & { openai?: ChatGptRuntime }).openai;
}

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

type ProposalState = PlanProposal;

type PendingProposal = {
  proposal: ProposalState;
  payload: unknown;
  workspaceId?: string;
};

type ConversationDraftPayload = {
  view: ConversationDraftView;
  graph: unknown;
};

type PendingConversationDraft = {
  draft: ConversationDraftPayload;
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

function noChangeSetFrom(value: unknown): ChangeSet | null {
  if (!value || typeof value !== "object") return null;
  const source = value as Record<string, unknown>;
  if (
    source.status !== "no_change" ||
    typeof source.workspace_id !== "string"
  ) {
    return null;
  }
  const id =
    typeof source.proposal_version === "string"
      ? source.proposal_version
      : "no-change";
  return {
    id,
    workspaceId: source.workspace_id,
    title: typeof source.title === "string" ? source.title : "今回の変更",
    status: "no_change",
    hash: typeof source.hash === "string" ? source.hash : id,
    conversationId:
      typeof source.conversation_id === "string"
        ? source.conversation_id
        : null,
    approvedBy: null,
    approvedAt: null,
    rejectedBy: null,
    appliedAt: null,
    proposedBy: null,
    proposedByConnection: null,
    createdAt: "",
    expiresAt: "",
    autoApplyEligible: false,
    autoApplied: false,
    autoApplyRule: null,
    approvalUrl: null,
    rows: [],
    assumptions: Array.isArray(source.assumptions)
      ? source.assumptions.filter(
          (line): line is string => typeof line === "string",
        )
      : [],
  };
}

function proposalFrom(value: unknown): ProposalState | null {
  if (!value || typeof value !== "object") return null;
  const source = value as Record<string, unknown>;
  const noChange = source.status === "no_change";
  if (!noChange && !previewGraphFrom(value)) return null;
  const change = noChange ? noChangeSetFrom(value) : changeFromPayload(value);
  if (!change) return null;
  return {
    id: change.id,
    workspaceId: change.workspaceId,
    title: change.title,
    status: change.status,
    approvalUrl: change.approvalUrl,
    autoApplyEligible: change.autoApplyEligible,
    assumptions: change.assumptions,
    noChange,
    change,
  };
}

function conversationDraftFrom(
  value: unknown,
): ConversationDraftPayload | null {
  if (!value || typeof value !== "object") return null;
  const source = value as Record<string, unknown>;
  if (
    !Array.isArray(source.nodes) ||
    !Array.isArray(source.edges) ||
    !Array.isArray(source.assumptions) ||
    typeof source.title !== "string" ||
    typeof source.revision !== "number" ||
    (typeof source.id !== "string" && typeof source.draft_id !== "string")
  ) {
    return null;
  }
  const sourceNodes = source.nodes as Record<string, unknown>[];
  const items = sourceNodes.map((node) => {
    const fields =
      node.fields && typeof node.fields === "object"
        ? (node.fields as Record<string, unknown>)
        : {};
    return {
      id:
        typeof node.ref === "string"
          ? node.ref
          : typeof node.id === "string"
            ? node.id
            : "",
      title: typeof node.title === "string" ? node.title : "",
      kind: typeof node.kind === "string" ? node.kind : "outcome",
      state: "draft",
      due_date:
        typeof fields.due_date === "string" ? fields.due_date : undefined,
      fields,
      conversation_status: typeof node.status === "string" ? node.status : null,
      detail: typeof node.detail === "string" ? node.detail : null,
      basis: node.basis && typeof node.basis === "object" ? node.basis : null,
    };
  });
  const edges = source.edges as Record<string, unknown>[];
  const draftId =
    typeof source.draft_id === "string"
      ? source.draft_id
      : (source.id as string);
  const assumptions = (source.assumptions as unknown[]).filter(
    (assumption): assumption is string => typeof assumption === "string",
  );
  return {
    view: {
      title: source.title,
      status: source.status === "withdrawn" ? "withdrawn" : "open",
      revision:
        Number.isInteger(source.revision) && source.revision >= 1
          ? source.revision
          : 1,
      openQuestions: sourceNodes.filter((node) => node.status === "question")
        .length,
      assumptions,
    },
    graph: {
      workspace_id: source.workspace_id,
      items: items.filter((item) => item.id),
      relations: edges
        .filter(
          (edge) =>
            typeof edge.source === "string" &&
            typeof edge.target === "string" &&
            typeof edge.type === "string",
        )
        .map((edge) => ({
          source_id: edge.source,
          target_id: edge.target,
          type: edge.type,
          ...(typeof edge.position === "number"
            ? { position: edge.position }
            : {}),
          ...(typeof edge.rationale === "string"
            ? { rationale: edge.rationale }
            : {}),
          ...(edge.basis && typeof edge.basis === "object"
            ? { basis: edge.basis }
            : {}),
        })),
      truncated: false,
      limit: sourceNodes.length,
      draft_id: draftId,
    },
  };
}

function isFinishedChange(value: unknown): boolean {
  if (!value || typeof value !== "object") return false;
  const source = value as Record<string, unknown>;
  const change = changeFromPayload(value);
  return (
    source.status === "no_change" ||
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
  const conversationDraft = conversationDraftFrom(payload);
  const context = isContextPayload(payload)
    ? buildPlanView({ context: payload, workspaceId })
    : null;
  const graphPayload =
    conversationDraft?.graph ??
    previewGraphFrom(payload) ??
    (isGraphPayload(payload) ? payload : null);
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
  const [conversationDraft, setConversationDraft] =
    useState<ConversationDraftView | null>(null);
  const [proposalActionBusy, setProposalActionBusy] = useState(false);
  const [proposalActionNotice, setProposalActionNotice] = useState<
    string | null
  >(null);
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
  const pendingConversationDraft = useRef<PendingConversationDraft | null>(
    null,
  );
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
            pendingProposal.current = null;
            pendingConversationDraft.current = null;
            setProposal(null);
            setConversationDraft(null);
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
          const nextConversationDraft = conversationDraftFrom(payload);
          if (nextConversationDraft) {
            pendingWorkspaceRefresh.current = undefined;
            const workspaceId =
              workspaceIdFrom(payload) ?? workspaceRef.current;
            const workspaceIsKnown = workspaceId
              ? viewRef.current.workspaces.some(
                  (workspace) => workspace.id === workspaceId,
                )
              : false;
            if (
              !workspaceId ||
              !workspaceIsKnown ||
              viewRef.current.workspace?.id !== workspaceId
            ) {
              pendingConversationDraft.current = {
                draft: nextConversationDraft,
                payload,
                workspaceId,
              };
              setLoading(true);
              setStale(true);
              void refreshRef.current(workspaceId);
              return;
            }
            generation.current += 1;
            pendingConversationDraft.current = null;
            pendingProposal.current = null;
            const next = mergeToolPayload(
              viewRef.current,
              payload,
              workspaceId,
            );
            viewRef.current = next;
            setView(next);
            setConversationDraft(nextConversationDraft.view);
            setProposal(null);
            setLoading(false);
            setProblem(null);
            setStale(false);
            return;
          }
          const nextProposal = proposalFrom(payload);
          if (nextProposal) {
            pendingWorkspaceRefresh.current = undefined;
            pendingConversationDraft.current = null;
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
            setConversationDraft(null);
            setLoading(false);
            setProblem(null);
            setStale(false);
            return;
          }
          if (isFinishedChange(payload)) {
            pendingWorkspaceRefresh.current = undefined;
            pendingProposal.current = null;
            pendingConversationDraft.current = null;
            setProposal(null);
            setConversationDraft(null);
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
          if (isGraphPayload(payload)) setConversationDraft(null);
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
      const pendingDraft = pendingConversationDraft.current;
      const pending = pendingProposal.current;
      let next: PlanView;
      if (pendingDraft) {
        pendingConversationDraft.current = null;
        pendingProposal.current = null;
        next = mergeToolPayload(
          result.view,
          pendingDraft.payload,
          pendingDraft.workspaceId,
        );
        setConversationDraft(pendingDraft.draft.view);
        setProposal(null);
      } else if (pending) {
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
        setConversationDraft(null);
      }
      viewRef.current = next;
      setView(next);
      setLoading(false);
      setStale(false);
    },
    [hostFor],
  );
  refreshRef.current = refresh;

  const applyProposal = useCallback(async () => {
    const current = proposal;
    if (!current?.autoApplyEligible || proposalActionBusy) return;
    const host = hostFor();
    if (!host) return;

    setProposalActionBusy(true);
    setProposalActionNotice(null);
    setProblem(null);
    try {
      const result = await host.call("pathbase_apply_changes", {
        workspace_id: current.workspaceId,
        preview_id: current.id,
        idempotency_key: `mcp-app-auto-apply:${current.id}`,
      });
      if (!isFinishedChange(result)) {
        setProposalActionNotice(
          "反映できませんでした。この変更案は事前許可の範囲として反映されませんでした。Basepathで内容を確認してください。",
        );
        return;
      }
      pendingProposal.current = null;
      pendingWorkspaceRefresh.current = undefined;
      setProposal(null);
      setStale(true);
      await refresh(current.workspaceId);
    } catch (caught) {
      if (caught instanceof HostError) {
        setProposalActionNotice(`反映できませんでした。${caught.message}`);
      } else {
        setProposalActionNotice(
          `反映できませんでした。${caught instanceof Error ? caught.message : String(caught)}`,
        );
      }
    } finally {
      setProposalActionBusy(false);
    }
  }, [hostFor, proposal, proposalActionBusy, refresh]);

  const openApproval = useCallback((href: string) => {
    const openExternal = chatGptRuntime()?.openExternal;
    if (!openExternal) return;
    setProposalActionNotice(null);
    try {
      void Promise.resolve(openExternal({ href })).catch((caught) => {
        setProposalActionNotice(
          `承認画面を開けませんでした。${caught instanceof Error ? caught.message : String(caught)}`,
        );
      });
    } catch (caught) {
      setProposalActionNotice(
        `承認画面を開けませんでした。${caught instanceof Error ? caught.message : String(caught)}`,
      );
    }
  }, []);
  const canOpenApproval = typeof chatGptRuntime()?.openExternal === "function";

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
        notice={proposalActionNotice}
        proposal={proposal}
        conversationDraft={conversationDraft}
        onOpenApproval={canOpenApproval ? openApproval : undefined}
        onApplyProposal={applyProposal}
        proposalActionBusy={proposalActionBusy}
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
