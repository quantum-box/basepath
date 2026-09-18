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
  loadPlanning,
  loadWeeklyReview,
  structuredResult,
} from "../src/shared/host";
import {
  buildPlanView,
  emptyPlanView,
  localDateIn,
  type PlanView,
} from "../src/shared/viewModel";
import {
  draftBody,
  draftDiffers,
  draftInputFrom,
  draftKey,
  emptyDraftInput,
  isWeeklyReview,
  mondayOf,
  weeklyReviewFrom,
  type WeeklyDraftInput,
  type WeeklyReviewView,
} from "../src/shared/weeklyView";
import { PlanningBar } from "../src/shared/PlanningBar";
import type { PlanningView } from "../src/shared/planningView";
import { PlanViewPanel, type ActionRequest } from "../src/shared/PlanView";
import { WeeklyReviewPanel } from "../src/shared/WeeklyReview";
import { ChangeReview } from "../src/shared/ChangeReview";
import {
  approvalUrl,
  changeSetsFrom,
  type ChangeSet,
} from "../src/shared/changeView";
import { useTreeState } from "../src/shared/useTreeState";
import "./document.css";
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
  const [changes, setChanges] = useState<ChangeSet[]>([]);
  const [basepathUrl, setBasepathUrl] = useState("");
  const [changeBusy, setChangeBusy] = useState(false);
  const [changeNotice, setChangeNotice] = useState<string | null>(null);
  // The plan is what is intended; the weekly review is what happened. They are
  // separate questions, so they are separate surfaces rather than one scroll.
  const [surface, setSurface] = useState<"plan" | "weekly">("plan");
  const [weekStart, setWeekStart] = useState("");
  const [weekly, setWeekly] = useState<WeeklyReviewView | null>(null);
  const [weeklyDraft, setWeeklyDraft] =
    useState<WeeklyDraftInput>(emptyDraftInput);
  const [weeklyLoading, setWeeklyLoading] = useState(false);
  const [weeklyBusy, setWeeklyBusy] = useState(false);
  const [weeklyNotice, setWeeklyNotice] = useState<string | null>(null);
  const [weeklyProblem, setWeeklyProblem] = useState<ReturnType<
    typeof problemFor
  > | null>(null);
  const weeklyGeneration = useRef(0);
  const lastWorkspace = useRef("");
  // Which planning period the plan is being looked at in. Most workspaces have
  // none, and then this stays null and nothing is shown.
  const [planning, setPlanning] = useState<PlanningView | null>(null);
  // Read by callbacks that must not re-run every keystroke.
  const weeklyRef = useRef<WeeklyReviewView | null>(null);
  const draftRef = useRef<WeeklyDraftInput>(emptyDraftInput);
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
          // The weekly summary opens this app too. Which tool the host ran is
          // what the person asked about, so it decides which surface is shown.
          if (isWeeklyReview(structured)) {
            const summary = weeklyReviewFrom(structured);
            if (summary) {
              setSurface("weekly");
              setWeekStart(summary.weekStart);
              adoptReview(summary);
              setWeeklyProblem(null);
              setWeeklyLoading(false);
              setStale(false);
              setLoading(false);
            }
            return;
          }
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

  /**
   * Takes a newer weekly summary without discarding unsent text.
   *
   * A tool result can arrive at any moment, including mid-sentence. The
   * numbers are always replaced — they are the server's — but text the person
   * typed and has not proposed yet is theirs, and retyping it is not free.
   */
  const adoptReview = useCallback((next: WeeklyReviewView) => {
    const current = weeklyRef.current;
    const unsent =
      current !== null &&
      current.weekStart === next.weekStart &&
      draftDiffers(draftRef.current, current.review);
    weeklyRef.current = next;
    setWeekly(next);
    if (!unsent) {
      draftRef.current = draftInputFrom(next.review);
      setWeeklyDraft(draftRef.current);
    }
  }, []);

  const changeDraft = useCallback((next: WeeklyDraftInput) => {
    draftRef.current = next;
    setWeeklyDraft(next);
  }, []);

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

    // Proposals awaiting the person, alongside the plan they would change.
    const workspace = next.workspace;
    if (!workspace) {
      setChanges([]);
      return;
    }
    try {
      const context = await host.call("pathbase_get_context", {});
      const url = (context as { basepath_url?: unknown })?.basepath_url;
      setBasepathUrl(typeof url === "string" ? url : "");
      const listed = await host.call("pathbase_list_changes", {
        workspace_id: workspace.id,
      });
      setChanges(
        changeSetsFrom(listed).filter(
          (change) =>
            change.status === "pending" || change.status === "approved",
        ),
      );
    } catch {
      // A plan that loads without its proposals is still worth showing.
      setChanges([]);
    }
  }, [hostFor, workspaceId, limit]);

  useEffect(() => {
    if (isConnected) void refresh();
  }, [isConnected, refresh]);

  // The week under review belongs to the workspace, not to the device showing
  // it, and a week from one workspace means nothing in another: switching
  // clears the summary and the unsent text rather than carrying them over.
  useEffect(() => {
    const workspace = view.workspace;
    if (!workspace) return;
    const thisWeek = () =>
      mondayOf(view.localDate || localDateIn(workspace.timezone));
    if (lastWorkspace.current && lastWorkspace.current !== workspace.id) {
      weeklyGeneration.current += 1;
      weeklyRef.current = null;
      draftRef.current = emptyDraftInput;
      setWeekly(null);
      setWeeklyDraft(emptyDraftInput);
      setWeeklyNotice(null);
      setWeeklyProblem(null);
      setWeekStart(thisWeek());
    } else {
      setWeekStart((current) => current || thisWeek());
    }
    lastWorkspace.current = workspace.id;
  }, [view.workspace?.id, view.workspace?.timezone, view.localDate]);

  const refreshWeekly = useCallback(async () => {
    const host = hostFor();
    const workspace = view.workspace;
    if (!host || !workspace || !weekStart) return;
    const ticket = ++weeklyGeneration.current;
    setWeeklyLoading(true);
    const { review, error: failure } = await loadWeeklyReview(
      host,
      workspace.id,
      weekStart,
    );
    if (ticket !== weeklyGeneration.current) return;
    setWeeklyLoading(false);
    if (failure) {
      setWeeklyProblem(problemFor(failure));
      return;
    }
    setWeeklyProblem(null);
    if (review) adoptReview(review);
  }, [hostFor, view.workspace?.id, weekStart, adoptReview]);

  useEffect(() => {
    if (isConnected && surface === "weekly") void refreshWeekly();
  }, [isConnected, surface, refreshWeekly]);

  // The period is context for the plan, so a workspace without one, or a
  // failure to read it, must not stop the plan from rendering.
  useEffect(() => {
    const host = hostFor();
    const workspace = view.workspace;
    if (!isConnected || !host || !workspace) return;
    let live = true;
    void loadPlanning(host, workspace.id).then(({ planning }) => {
      if (live) setPlanning(planning);
    });
    return () => {
      live = false;
    };
  }, [isConnected, hostFor, view.workspace?.id]);

  /**
   * Turns the person's review text into a change set.
   *
   * It is a proposal, not a save. A click here reaches the server as an
   * ordinary tool call that the server cannot tell apart from the model's, so
   * it cannot stand in for the person: they approve the diff in Basepath, and
   * only they can finalize the week there.
   */
  const proposeReview = useCallback(async () => {
    const host = hostFor();
    const workspace = view.workspace;
    const summary = weeklyRef.current;
    if (!host || !workspace || !summary || weeklyBusy) return;
    const input = draftRef.current;
    setWeeklyBusy(true);
    setWeeklyNotice(null);
    try {
      await host.call("pathbase_preview_changes", {
        workspace_id: workspace.id,
        title: `${summary.weekStart}の週次レビュー案`,
        operations: [
          {
            method: "POST",
            path: `/v1/workspaces/${workspace.id}/weekly-reviews/draft`,
            body: draftBody(summary.weekStart, input, summary.review),
          },
        ],
        // The version pins what this proposal was written against; the digest
        // keeps edited text from colliding with an earlier proposal.
        idempotency_key: `weekly:${summary.weekStart}:${summary.review?.version ?? 0}:${draftKey(input)}`,
      });
      setWeeklyNotice(
        "変更案を作成しました。Basepathで差分を確認して承認すると保存されます。この週はまだ確定していません。",
      );
    } catch (failure) {
      if (failure instanceof HostError) {
        const described = problemFor(failure);
        setWeeklyNotice(`${described.title}: ${described.detail}`);
      } else {
        setWeeklyNotice("変更案を作成できませんでした。");
      }
    } finally {
      setWeeklyBusy(false);
      await refresh();
      await refreshWeekly();
    }
  }, [hostFor, view.workspace?.id, weeklyBusy, refresh, refreshWeekly]);

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

  /**
   * Withdrawing a proposal, or applying one the person already approved.
   *
   * Approving is deliberately not here: a click in this app reaches the server
   * as an ordinary tool call, which the server cannot tell apart from the
   * model's, so it is not evidence of the person's intent.
   */
  const actOnChange = useCallback(
    async (change: ChangeSet, intent: "reject" | "apply") => {
      const host = hostFor();
      if (!host || changeBusy) return;
      setChangeBusy(true);
      setChangeNotice(null);
      try {
        await host.call(
          intent === "reject"
            ? "pathbase_reject_change"
            : "pathbase_apply_changes",
          {
            workspace_id: change.workspaceId,
            preview_id: change.id,
            idempotency_key: `${intent}:${change.id}:${change.hash}`,
          },
        );
        setChangeNotice(
          intent === "reject"
            ? "変更案を取り下げました。"
            : "承認済みの内容を適用しました。",
        );
      } catch (failure) {
        if (failure instanceof HostError) {
          const described = problemFor(failure);
          setChangeNotice(`${described.title}: ${described.detail}`);
        } else {
          setChangeNotice("操作できませんでした。");
        }
      } finally {
        setChangeBusy(false);
        await refresh();
      }
    },
    [hostFor, changeBusy, refresh],
  );

  const openApproval = useCallback(
    async (change: ChangeSet) => {
      if (!app || !basepathUrl) return;
      const url = approvalUrl(basepathUrl, change);
      if (app.getHostCapabilities()?.openLinks) {
        await app.openLink({ url });
        return;
      }
      setChangeNotice(
        `このホストはリンクを開けません。${url} を開いてください。`,
      );
    },
    [app, basepathUrl],
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
    <>
      <nav className="app-surfaces" aria-label="表示の切り替え">
        <button
          type="button"
          aria-pressed={surface === "plan"}
          onClick={() => setSurface("plan")}
        >
          計画
        </button>
        <button
          type="button"
          aria-pressed={surface === "weekly"}
          onClick={() => setSurface("weekly")}
        >
          週次レビュー
        </button>
      </nav>
      {surface === "plan" && planning && !planning.unused && (
        <PlanningBar compact planning={planning} />
      )}
      {surface === "plan" ? (
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
      ) : (
        <WeeklyReviewPanel
          compact
          review={weekly}
          workspaceName={view.workspace?.name}
          loading={weeklyLoading}
          problem={
            weeklyProblem
              ? { ...weeklyProblem, retry: () => void refreshWeekly() }
              : null
          }
          stale={stale}
          onSelectWeek={(next) => {
            setWeeklyNotice(null);
            setWeekStart(next);
          }}
          draft={weeklyDraft}
          onDraftChange={changeDraft}
          canEdit={view.workspace?.role !== "viewer"}
          // No save and no finalize here: this surface cannot prove who
          // clicked. It can only propose, and the person approves in Basepath.
          onProposeDraft={() => void proposeReview()}
          busy={weeklyBusy}
          notice={weeklyNotice}
        />
      )}
      {changes.map((change) => (
        <ChangeReview
          key={change.id}
          change={change}
          workspaceName={view.workspace?.name}
          busy={changeBusy}
          notice={changeNotice}
          approveHref={
            basepathUrl ? approvalUrl(basepathUrl, change) : undefined
          }
          onOpenApproval={
            basepathUrl ? () => void openApproval(change) : undefined
          }
          onReject={() => void actOnChange(change, "reject")}
          onApply={
            change.approvedBy
              ? () => void actOnChange(change, "apply")
              : undefined
          }
        />
      ))}
    </>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BasepathApp />
  </StrictMode>,
);
