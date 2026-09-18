/**
 * The Basepath MCP App.
 *
 * It runs inside the host's sandboxed iframe and talks to the host over
 * postMessage. It holds no credentials: the host proxies every tool call to the
 * MCP server, which authorizes it against the person's delegation and their
 * workspace membership. Showing something here is never permission to do it.
 */
import { StrictMode, useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  useApp,
  useHostStyleVariables,
} from "@modelcontextprotocol/ext-apps/react";
import {
  HostError,
  McpAppHost,
  loadPlanView,
  structuredResult,
} from "../src/shared/host";
import {
  buildPlanView,
  emptyPlanView,
  type PlanView,
} from "../src/shared/viewModel";
import { PlanViewPanel, type ActionRequest } from "../src/shared/PlanView";
import { useTreeState } from "../src/shared/useTreeState";
import "../src/shared/planView.css";

const APP_INFO = { name: "Basepath", version: "1.0.0" };

function problemFor(error: HostError) {
  switch (error.code) {
    case "CONNECTION_APPROVAL_REQUIRED":
      return {
        title: "接続の許可が必要です",
        detail:
          "Basepathの設定画面でこのAIクライアントを許可すると、目標を表示できます。",
      };
    case "CONNECTION_REVOKED":
      return {
        title: "接続が解除されています",
        detail: "Basepath側で接続し直してください。",
      };
    case "INSUFFICIENT_SCOPE":
      return {
        title: "権限が足りません",
        detail: "Basepathの接続設定で「目標と行動を読む」を許可してください。",
      };
    case "UNAUTHENTICATED":
    case "INVALID_TOKEN":
      return {
        title: "サインインが必要です",
        detail: "ホスト側でBasepathへ接続し直してください。",
      };
    case "HOST_UNSUPPORTED":
      return {
        title: "このホストでは表示できません",
        detail:
          "会話内の表示に対応していないホストです。通常のMCPツールの結果をご覧ください。",
      };
    case "NOT_FOUND":
      return {
        title: "見つかりません",
        detail: "対象がないか、アクセスできません。",
      };
    default:
      return { title: "表示できませんでした", detail: error.message };
  }
}

function BasepathApp() {
  const [view, setView] = useState<PlanView>(emptyPlanView);
  const [loading, setLoading] = useState(true);
  const [problem, setProblem] = useState<ReturnType<typeof problemFor> | null>(
    null,
  );
  const [stale, setStale] = useState(false);
  const [workspaceId, setWorkspaceId] = useState("");
  const [limit, setLimit] = useState<number | undefined>(undefined);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** Only the newest load may write to the view. */
  const generation = useRef(0);
  // Folding and selection reset when the workspace does: a node id from one
  // plan means nothing in another.
  const tree = useTreeState(view.workspace?.id ?? "");

  const { app, isConnected, error } = useApp({
    appInfo: APP_INFO,
    // Declared features, not wishes: the host may refuse any of them, and the
    // app checks what it actually got before using it.
    capabilities: {},
    onAppCreated: (created) => {
      // The host pushes the tool result that opened this app. Render it
      // immediately instead of making a round trip for data we already have.
      created.ontoolresult = (params) => {
        try {
          const structured = structuredResult(params);
          const next = buildPlanView({ graph: structured, today: structured });
          if (next.nodes.length > 0 || next.actions.length > 0) {
            setView((current) => ({
              ...current,
              nodes: next.nodes.length ? next.nodes : current.nodes,
              truncated: next.truncated,
              limit: next.limit || current.limit,
              actions: next.actions.length ? next.actions : current.actions,
              localDate: next.localDate || current.localDate,
            }));
            setStale(false);
            setLoading(false);
          }
        } catch (failure) {
          if (failure instanceof HostError) setProblem(problemFor(failure));
        }
      };
      // A newer call is on its way: say so rather than showing old data as if
      // it were current.
      created.ontoolinput = () => setStale(true);
    },
  });

  useHostStyleVariables(app);

  const hostFor = useCallback(() => {
    if (!app) return null;
    const capabilities = app.getHostCapabilities();
    return new McpAppHost((params) => app.callServerTool(params), {
      serverTools: Boolean(capabilities?.serverTools),
    });
  }, [app]);

  const refresh = useCallback(async () => {
    const host = hostFor();
    if (!host) return;
    const ticket = ++generation.current;
    setLoading(true);
    const { view: next, error: failure } = await loadPlanView(host, {
      workspaceId: workspaceId || undefined,
      includeWeek: true,
      limit,
    });
    if (ticket !== generation.current) return;
    setLoading(false);
    setStale(false);
    if (failure) {
      setProblem(problemFor(failure));
      return;
    }
    setProblem(null);
    setView(next);
  }, [hostFor, workspaceId, limit]);

  useEffect(() => {
    if (isConnected) void refresh();
  }, [isConnected, refresh]);

  /**
   * Proposes an action completion.
   *
   * The MCP tool creates a change set; nothing is applied here. The result is
   * reported as what it is — a proposal awaiting the person's approval — and
   * the view is reloaded so the next render shows the server's current state
   * rather than an optimistic guess.
   */
  const propose = useCallback(
    async ({ action, intent }: ActionRequest) => {
      const host = hostFor();
      if (!host || !view.workspace) return;
      const key = action.occurrenceKey || action.id;
      if (busyAction) return; // a second click must not send a second proposal
      setBusyAction(key);
      setNotice(null);
      try {
        await host.call("pathbase_complete_action", {
          workspace_id: view.workspace.id,
          item_id: action.id,
          expected_version: action.version,
          local_date: view.localDate,
          idempotency_key: `${intent}:${key}:${action.version}`,
        });
        setNotice(
          "変更案を作成しました。Basepathで内容を確認して承認すると反映されます。",
        );
      } catch (failure) {
        if (failure instanceof HostError) {
          const described = problemFor(failure);
          setNotice(`${described.title}: ${described.detail}`);
        } else {
          setNotice("提案を作成できませんでした。");
        }
      } finally {
        setBusyAction(null);
        await refresh();
      }
    },
    [hostFor, view.workspace, view.localDate, busyAction, refresh],
  );

  if (error) {
    return (
      <PlanViewPanel
        view={emptyPlanView}
        tree={tree}
        problem={{
          title: "ホストに接続できませんでした",
          detail: error.message,
        }}
      />
    );
  }

  return (
    <PlanViewPanel
      view={view}
      tree={tree}
      loading={loading && !problem}
      problem={problem ? { ...problem, retry: () => void refresh() } : null}
      stale={stale}
      onSelectWorkspace={(id) => {
        setNotice(null);
        setLimit(undefined);
        setWorkspaceId(id);
      }}
      onPropose={(request) => void propose(request)}
      busyAction={busyAction}
      notice={notice}
      onExpand={() => setLimit(200)}
    />
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BasepathApp />
  </StrictMode>,
);
