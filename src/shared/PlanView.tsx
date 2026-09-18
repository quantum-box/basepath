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
import type { PlanAction, PlanNode, PlanView } from "./viewModel";
import { countNodes } from "./viewModel";

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

function Node({ node, depth }: { node: PlanNode; depth: number }) {
  return (
    <li className="plan-node" data-kind={node.kind} data-state={node.state}>
      <div className="plan-node-row" style={{ paddingInlineStart: depth * 14 }}>
        <span className="plan-node-kind">{kindLabel(node.kind)}</span>
        <span className="plan-node-title">{node.title}</span>
        <span className="plan-node-state">{stateLabel(node.state)}</span>
        {node.dueDate && (
          <span className="plan-node-due">〜{node.dueDate}</span>
        )}
        {node.selfAssessment !== null && (
          <span className="plan-node-assessment">
            自己評価 {Math.round(node.selfAssessment)}%
          </span>
        )}
      </div>
      {node.children.length > 0 && (
        <ul>
          {node.children.map((child) => (
            <Node key={child.id} node={child} depth={depth + 1} />
          ))}
        </ul>
      )}
    </li>
  );
}

function Actions({
  actions,
  localDate,
}: {
  actions: PlanAction[];
  localDate: string;
}) {
  return (
    <section className="plan-actions">
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
              <span>{action.title}</span>
              {action.scheduledTime && <small>{action.scheduledTime}</small>}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export type PlanViewProps = {
  view: PlanView;
  /** Shown instead of the plan while the first load is in flight. */
  loading?: boolean;
  /** A reason the plan could not be shown, already translated. */
  problem?: { title: string; detail: string; retry?: () => void } | null;
  /** Marked stale when a newer tool result has superseded this one. */
  stale?: boolean;
};

export function PlanViewPanel({
  view,
  loading,
  problem,
  stale,
}: PlanViewProps) {
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
  return (
    <div className="plan-panel" data-stale={stale ? "true" : undefined}>
      <header className="plan-header">
        <h2>{view.workspace.name}</h2>
        {view.workspace.scope && <span>{view.workspace.scope}</span>}
      </header>
      {stale && (
        <p className="plan-stale" role="status">
          新しい結果が届いています。表示は1つ前の内容です。
        </p>
      )}
      {view.nodes.length === 0 ? (
        <p className="plan-empty">まだ目標がありません。</p>
      ) : (
        <ul className="plan-tree">
          {view.nodes.map((node) => (
            <Node key={node.id} node={node} depth={0} />
          ))}
        </ul>
      )}
      {view.truncated && (
        <p className="plan-truncated" role="status">
          上位{view.limit}件までを表示しています（{shown}
          件）。これは計画の一部です。
        </p>
      )}
      <Actions actions={view.actions} localDate={view.localDate} />
    </div>
  );
}
