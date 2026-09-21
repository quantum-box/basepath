/**
 * The plan, rendered the same way wherever it is shown.
 *
 * Presentational only: it takes a view model and emits markup. It fetches
 * nothing, decides no permissions, and never renders a string as HTML —
 * everything here is user-authored text, and text it stays.
 *
 * Colours come from the host's CSS variables when it provides them, so the
 * same component follows a light or dark conversation without knowing which.
 */
import { useMemo, type ReactNode } from "react";
import type {
  PlanAction,
  PlanEntry,
  PlanNode,
  PlanView,
  PlanWorkspace,
} from "./viewModel";
import { countNodes, findNode, nodeIds } from "./viewModel";
import type { TreeState } from "./useTreeState";

function stateLabel(state: string) {
  switch (state) {
    case "done":
      return "完了";
    case "paused":
      return "保留";
    case "abandoned":
      return "見送り";
    case "draft":
      return "下書き";
    default:
      return "進行中";
  }
}

function kindLabel(kind: string) {
  switch (kind) {
    case "action":
      return "行動";
    case "initiative":
      return "取り組み";
    case "milestone":
      return "節目";
    case "idea":
      return "アイデア";
    default:
      return "目標";
  }
}

function entryLabel(label: PlanEntry["label"]) {
  switch (label) {
    case "start":
      return "開始";
    case "due":
      return "期限";
    case "habit":
      return "習慣";
    default:
      return "予定";
  }
}

function occurrenceLabel(status: string | null) {
  switch (status) {
    case "completion":
      return "実施";
    case "skip":
      return "見送り";
    case "missed":
      return "未実施";
    default:
      return null;
  }
}

function Node({
  node,
  depth,
  tree,
  selectable,
}: {
  node: PlanNode;
  depth: number;
  tree: TreeState;
  selectable: boolean;
}) {
  const open = tree.isOpen(node.id);
  const hasChildren = node.children.length > 0;
  return (
    <li className="plan-node" data-kind={node.kind} data-state={node.state}>
      <div className="plan-node-row" style={{ paddingInlineStart: depth * 14 }}>
        {hasChildren ? (
          <button
            type="button"
            className="plan-node-toggle"
            aria-expanded={open}
            aria-label={`${node.title}の下位を${open ? "閉じる" : "開く"}`}
            onClick={() => tree.toggle(node.id)}
          >
            {open ? "▾" : "▸"}
          </button>
        ) : (
          <span className="plan-node-toggle" aria-hidden="true" />
        )}
        {selectable ? (
          <button
            type="button"
            className="plan-node-select"
            aria-pressed={tree.selected === node.id}
            aria-label={`${node.title}の詳細`}
            onClick={() =>
              tree.select(tree.selected === node.id ? "" : node.id)
            }
          >
            <span className="plan-node-kind">{kindLabel(node.kind)}</span>
            <span className="plan-node-title">{node.title}</span>
          </button>
        ) : (
          <span className="plan-node-select plan-node-static">
            <span className="plan-node-kind">{kindLabel(node.kind)}</span>
            <span className="plan-node-title">{node.title}</span>
          </span>
        )}
        <span className="plan-node-state">{stateLabel(node.state)}</span>
        {node.dueDate && (
          <span className="plan-node-due">〜{node.dueDate}</span>
        )}
        {node.selfAssessment !== null && (
          <span className="plan-node-assessment">
            自己評価 {Math.round(node.selfAssessment)}%
          </span>
        )}
        {hasChildren && !open && (
          <span className="plan-node-folded">
            下位{countNodes(node.children)}件
          </span>
        )}
      </div>
      {hasChildren && open && (
        <ul>
          {node.children.map((child) => (
            <Node
              key={child.id}
              node={child}
              depth={depth + 1}
              tree={tree}
              selectable={selectable}
            />
          ))}
        </ul>
      )}
    </li>
  );
}

function Detail({ node }: { node: PlanNode }) {
  return (
    <section className="plan-detail" aria-label="選択中の項目">
      <h3>{node.title}</h3>
      <dl>
        <div>
          <dt>種類</dt>
          <dd>{kindLabel(node.kind)}</dd>
        </div>
        <div>
          <dt>状態</dt>
          <dd>{stateLabel(node.state)}</dd>
        </div>
        <div>
          <dt>期限</dt>
          <dd>{node.dueDate ?? "未設定"}</dd>
        </div>
        <div>
          <dt>自己評価</dt>
          <dd>
            {node.selfAssessment === null
              ? "未評価"
              : `${Math.round(node.selfAssessment)}%`}
          </dd>
        </div>
        <div>
          <dt>下位</dt>
          <dd>{countNodes(node.children)}件</dd>
        </div>
      </dl>
    </section>
  );
}

export type ActionRequest = {
  action: PlanAction;
  intent: "complete" | "skip";
};

function Actions({
  actions,
  localDate,
  onPropose,
  busy,
}: {
  actions: PlanAction[];
  localDate: string;
  onPropose?: (request: ActionRequest) => void;
  busy?: string | null;
}) {
  return (
    <section className="plan-actions" aria-label="今日の行動">
      <h3>{localDate || "今日"}の行動</h3>
      {actions.length === 0 ? (
        <p className="plan-empty">この日に予定された行動はありません。</p>
      ) : (
        <ul>
          {actions.map((action) => (
            <li
              key={action.occurrenceKey || action.id}
              data-done={action.completed}
            >
              <span aria-hidden="true">{action.completed ? "☑" : "☐"}</span>
              <span className="plan-action-title">{action.title}</span>
              {action.scheduledTime && <small>{action.scheduledTime}</small>}
              {action.dueDate && <small>期限 {action.dueDate}</small>}
              {action.assignee && <small>担当 {action.assignee}</small>}
              {onPropose && !action.completed && (
                <button
                  type="button"
                  disabled={busy === action.occurrenceKey}
                  onClick={() => onPropose({ action, intent: "complete" })}
                >
                  {busy === action.occurrenceKey ? "送信中…" : "完了を提案"}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function Week({ view }: { view: PlanView }) {
  const week = view.week;
  if (!week) return null;
  return (
    <section className="plan-week" aria-label="今週の行動">
      <h3>
        今週（{week.start} 〜 {week.end}
        {week.timezone ? ` / ${week.timezone}` : ""}）
      </h3>
      <ul className="plan-week-days">
        {week.days.map((day) => (
          <li key={day.date} data-today={day.date === view.localDate}>
            <h4>{day.date}</h4>
            {day.entries.length === 0 ? (
              <p className="plan-empty">予定なし</p>
            ) : (
              <ul>
                {day.entries.map((entry, index) => (
                  <li key={`${entry.id}:${entry.label}:${index}`}>
                    <span className="plan-entry-label">
                      {entryLabel(entry.label)}
                    </span>
                    <span>{entry.title}</span>
                    {entry.scheduledTime && (
                      <small>{entry.scheduledTime}</small>
                    )}
                    {entry.assignee && <small>担当 {entry.assignee}</small>}
                    {occurrenceLabel(entry.status) && (
                      <small>{occurrenceLabel(entry.status)}</small>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </li>
        ))}
      </ul>
      {week.unscheduled.length > 0 && (
        <p className="plan-unscheduled">
          日付のない項目が{week.unscheduled.length}件あります。
        </p>
      )}
    </section>
  );
}

function WorkspaceSwitcher({
  workspaces,
  current,
  onSelect,
}: {
  workspaces: PlanWorkspace[];
  current: PlanWorkspace;
  onSelect?: (id: string) => void;
}) {
  if (!onSelect || workspaces.length <= 1) return null;
  return (
    <label className="plan-workspace-switch">
      <span className="plan-workspace-switch-label">表示対象</span>
      <select
        value={current.id}
        onChange={(event) => onSelect(event.target.value)}
      >
        {workspaces.map((workspace) => (
          <option key={workspace.id} value={workspace.id}>
            {workspace.name}（{workspace.scope || "領域"}）
          </option>
        ))}
      </select>
    </label>
  );
}

export type PlanViewProps = {
  view: PlanView;
  tree: TreeState;
  /** Shown instead of the plan while the first load is in flight. */
  loading?: boolean;
  /** A reason the plan could not be shown, already translated. */
  problem?: { title: string; detail: string; retry?: () => void } | null;
  /** Marked stale when a newer tool result has superseded this one. */
  stale?: boolean;
  onSelectWorkspace?: (id: string) => void;
  onPropose?: (request: ActionRequest) => void;
  /** Occurrence key of an action whose proposal is in flight. */
  busyAction?: string | null;
  /** Result of the last proposal, shown verbatim rather than assumed. */
  notice?: string | null;
  onExpand?: () => void;
  /** MCP Apps uses the tree as its only content and hides the other panels. */
  showWorkspaceSwitcher?: boolean;
  showDetails?: boolean;
  showActions?: boolean;
  showWeek?: boolean;
  /** Optional alternate rendering of the same tree, such as React Flow. */
  flowContent?: ReactNode;
  showViewToggle?: boolean;
  viewMode?: "list" | "map";
  onViewModeChange?: (mode: "list" | "map") => void;
};

export function PlanViewPanel({
  view,
  tree,
  loading,
  problem,
  stale,
  onSelectWorkspace,
  onPropose,
  busyAction,
  notice,
  onExpand,
  showWorkspaceSwitcher = true,
  showDetails = true,
  showActions = true,
  showWeek = true,
  flowContent,
  showViewToggle = false,
  viewMode = "list",
  onViewModeChange,
}: PlanViewProps) {
  // A selection that is no longer in the tree must not show a detail panel.
  const selectedNode = useMemo(() => {
    const ids = nodeIds(view.nodes);
    return showDetails && ids.has(tree.selected)
      ? findNode(view.nodes, tree.selected)
      : null;
  }, [showDetails, view.nodes, tree.selected]);

  if (problem) {
    return (
      <div className="plan-panel plan-problem" role="alert">
        <h3>{problem.title}</h3>
        <p>{problem.detail}</p>
        {problem.retry && (
          <button type="button" onClick={problem.retry}>
            再試行
          </button>
        )}
      </div>
    );
  }
  if (loading) {
    return (
      <div className="plan-panel plan-loading" aria-busy="true">
        <p>目標を読み込んでいます…</p>
      </div>
    );
  }
  if (!view.workspace) {
    return (
      <div className="plan-panel plan-empty-state">
        <p>利用できるワークスペースがありません。</p>
      </div>
    );
  }
  const shown = countNodes(view.nodes);
  const isPersonal =
    view.workspace.id === "personal" || view.workspace.scope === "個人";
  const contextLabel = isPersonal ? "個人の計画" : "組織の計画";
  return (
    <div className="plan-panel" data-stale={stale ? "true" : undefined}>
      <header className="plan-header">
        <div className="plan-heading">
          <span className="plan-eyebrow">いま見ている場所</span>
          <div className="plan-title-row">
            <span
              className={`plan-context-mark ${isPersonal ? "personal" : "organization"}`}
              aria-hidden="true"
            >
              {isPersonal ? "●" : "◆"}
            </span>
            <h2>{view.workspace.name}</h2>
          </div>
          <p>{contextLabel} · 目標から行動までをひとつのツリーで表示</p>
        </div>
        <div className="plan-header-meta">
          <span className="plan-scope-chip">
            {view.workspace.scope || (isPersonal ? "個人" : "組織")}
          </span>
          {view.workspace.role && (
            <span className="plan-role">{view.workspace.role}</span>
          )}
        </div>
        {showWorkspaceSwitcher && (
          <WorkspaceSwitcher
            workspaces={view.workspaces}
            current={view.workspace}
            onSelect={onSelectWorkspace}
          />
        )}
      </header>
      {stale && (
        <p className="plan-stale" role="status">
          新しい結果が届いています。表示は1つ前の内容です。
        </p>
      )}
      {notice && (
        <p className="plan-notice" role="status">
          {notice}
        </p>
      )}
      <section
        className="plan-tree-section"
        aria-labelledby="plan-tree-heading"
      >
        <div className="plan-section-heading">
          <div>
            <span className="plan-eyebrow">PLAN MAP</span>
            <h3 id="plan-tree-heading">目標ツリー</h3>
          </div>
          <div className="plan-section-tools">
            {showViewToggle && onViewModeChange && (
              <div
                className="plan-view-toggle"
                role="group"
                aria-label="表示形式"
              >
                <button
                  type="button"
                  aria-pressed={viewMode === "list"}
                  onClick={() => onViewModeChange("list")}
                >
                  リスト表示
                </button>
                <button
                  type="button"
                  aria-pressed={viewMode === "map"}
                  onClick={() => onViewModeChange("map")}
                >
                  マップ表示
                </button>
              </div>
            )}
            <span className="plan-tree-count">{shown}項目</span>
          </div>
        </div>
        {view.nodes.length === 0 ? (
          <p className="plan-empty">
            このワークスペースにはまだ目標がありません。
          </p>
        ) : (
          <>
            {tree.closedCount > 0 && (
              <button
                type="button"
                className="plan-expand-all"
                onClick={tree.openAll}
              >
                すべて開く
              </button>
            )}
            {viewMode === "map" && flowContent ? (
              flowContent
            ) : (
              <ul className="plan-tree">
                {view.nodes.map((node) => (
                  <Node
                    key={node.id}
                    node={node}
                    depth={0}
                    tree={tree}
                    selectable={showDetails}
                  />
                ))}
              </ul>
            )}
          </>
        )}
      </section>
      {showDetails && selectedNode && <Detail node={selectedNode} />}
      {view.truncated && (
        <p className="plan-truncated" role="status">
          上位{view.limit}件までを表示しています（{shown}
          件）。これは計画の一部です。
          {onExpand && (
            <button type="button" onClick={onExpand}>
              さらに読み込む
            </button>
          )}
        </p>
      )}
      {showActions && (
        <Actions
          actions={view.actions}
          localDate={view.localDate}
          onPropose={onPropose}
          busy={busyAction}
        />
      )}
      {showWeek && <Week view={view} />}
    </div>
  );
}
