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
import { PlanViewPanel } from "../src/shared/PlanView";
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
  /** Only the newest load may write to the view. */
  const generation = useRef(0);

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

  const refresh = useCallback(async () => {
    if (!app) return;
    const capabilities = app.getHostCapabilities();
    const host = new McpAppHost((params) => app.callServerTool(params), {
      serverTools: Boolean(capabilities?.serverTools),
    });
    const ticket = ++generation.current;
    setLoading(true);
    const { view: next, error: failure } = await loadPlanView(host);
    if (ticket !== generation.current) return;
    setLoading(false);
    setStale(false);
    if (failure) {
      setProblem(problemFor(failure));
      return;
    }
    setProblem(null);
    setView(next);
  }, [app]);

  useEffect(() => {
    if (isConnected) void refresh();
  }, [isConnected, refresh]);

  if (error) {
    return (
      <PlanViewPanel
        view={emptyPlanView}
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
      loading={loading && !problem}
      problem={problem ? { ...problem, retry: () => void refresh() } : null}
      stale={stale}
    />
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BasepathApp />
  </StrictMode>,
);
