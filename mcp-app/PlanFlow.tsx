import {
  Background,
  Controls,
  Handle,
  Position,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  type Edge,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import { useCallback, useEffect, useState } from "react";
import type { PlanNode } from "../src/shared/viewModel";
import type { TreeState } from "../src/shared/useTreeState";
import { countNodes } from "../src/shared/viewModel";
import "@xyflow/react/dist/style.css";

type FlowNodeData = {
  title: string;
  kind: PlanNode["kind"];
  state: string;
  conversationStatus: string | null;
  dueDate: string | null;
  selfAssessment: number | null;
  childCount: number;
  open: boolean;
  onToggle: () => void;
};

type FlowNode = Node<FlowNodeData, "mcp-plan">;

const NODE_WIDTH = 196;
const NODE_HEIGHT = 82;
const HORIZONTAL_GAP = 28;
const VERTICAL_GAP = 48;
const FIT_VIEW_OPTIONS = {
  padding: 0.1,
  minZoom: 0.05,
  maxZoom: 1.1,
} as const;

function kindLabel(kind: PlanNode["kind"]) {
  switch (kind) {
    case "action":
      return "行動";
    case "initiative":
      return "取り組み";
    case "milestone":
      return "節目";
    case "idea":
      return "アイデア";
    case "criterion":
      return "達成条件";
    case "constraint":
      return "制約";
    case "question":
      return "未解決の問い";
    default:
      return "目標";
  }
}

function conversationStatusLabel(status: string | null) {
  switch (status) {
    case "decided":
      return "決定";
    case "considering":
      return "検討中";
    case "hypothesis":
      return "仮説";
    case "suggested":
      return "AI提案";
    case "question":
      return "質問";
    default:
      return null;
  }
}

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

function FlowPlanNode({ data }: NodeProps<FlowNode>) {
  const hasChildren = data.childCount > 0;
  return (
    <div
      className="mcp-flow-node"
      data-kind={data.kind}
      data-state={data.state}
    >
      <Handle type="target" position={Position.Top} />
      <div className="mcp-flow-node-copy">
        <span className="mcp-flow-node-kind">{kindLabel(data.kind)}</span>
        <strong>{data.title}</strong>
        <span className="mcp-flow-node-meta">
          {conversationStatusLabel(data.conversationStatus) ??
            stateLabel(data.state)}
          {data.dueDate ? ` · 期限 ${data.dueDate}` : ""}
          {data.selfAssessment !== null
            ? ` · 自己評価 ${Math.round(data.selfAssessment)}%`
            : ""}
        </span>
      </div>
      {hasChildren && (
        <button
          type="button"
          className="mcp-flow-node-toggle nodrag nopan"
          aria-expanded={data.open}
          aria-label={`${data.title}の下位を${data.open ? "閉じる" : "開く"}`}
          onClick={(event) => {
            event.stopPropagation();
            data.onToggle();
          }}
        >
          {data.open ? "−" : `+${data.childCount}`}
        </button>
      )}
      <Handle type="source" position={Position.Bottom} />
    </div>
  );
}

const nodeTypes = { "mcp-plan": FlowPlanNode };

function buildFlowGraph(roots: PlanNode[], tree: TreeState) {
  const nodes: FlowNode[] = [];
  const edges: Edge[] = [];
  const widths = new Map<string, number>();

  const childrenOf = (node: PlanNode) =>
    tree.isOpen(node.id) ? node.children : [];

  const measure = (node: PlanNode): number => {
    const children = childrenOf(node);
    const childrenWidth = children.reduce(
      (total, child) => total + measure(child),
      0,
    );
    const gaps = Math.max(0, children.length - 1) * HORIZONTAL_GAP;
    const width = Math.max(NODE_WIDTH, childrenWidth + gaps);
    widths.set(node.id, width);
    return width;
  };

  roots.forEach(measure);

  const place = (
    node: PlanNode,
    left: number,
    depth: number,
    parentId?: string,
  ) => {
    const width = widths.get(node.id) ?? NODE_WIDTH;
    const children = childrenOf(node);
    const nodeLeft = left + (width - NODE_WIDTH) / 2;
    nodes.push({
      id: node.id,
      type: "mcp-plan",
      position: { x: nodeLeft, y: depth * (NODE_HEIGHT + VERTICAL_GAP) },
      style: { width: NODE_WIDTH, height: NODE_HEIGHT },
      data: {
        title: node.title,
        kind: node.kind,
        state: node.state,
        conversationStatus: node.conversationStatus ?? null,
        dueDate: node.dueDate,
        selfAssessment: node.selfAssessment,
        childCount: countNodes(node.children),
        open: tree.isOpen(node.id),
        onToggle: () => tree.toggle(node.id),
      },
    });
    if (parentId) {
      edges.push({
        id: `${parentId}-${node.id}`,
        source: parentId,
        target: node.id,
        type: "smoothstep",
      });
    }

    const childrenWidth = children.reduce(
      (total, child) => total + (widths.get(child.id) ?? NODE_WIDTH),
      0,
    );
    const gaps = Math.max(0, children.length - 1) * HORIZONTAL_GAP;
    let cursor = left + (width - (childrenWidth + gaps)) / 2;
    children.forEach((child) => {
      place(child, cursor, depth + 1, node.id);
      cursor += (widths.get(child.id) ?? NODE_WIDTH) + HORIZONTAL_GAP;
    });
  };

  let cursor = 0;
  roots.forEach((root) => {
    const width = widths.get(root.id) ?? NODE_WIDTH;
    place(root, cursor, 0);
    cursor += width + HORIZONTAL_GAP;
  });
  return { nodes, edges };
}

export function PlanFlow({
  roots,
  tree,
}: {
  roots: PlanNode[];
  tree: TreeState;
}) {
  const graph = buildFlowGraph(roots, tree);
  if (graph.nodes.length === 0) return null;
  return (
    <ReactFlowProvider>
      <PlanFlowCanvas graph={graph} tree={tree} />
    </ReactFlowProvider>
  );
}

function PlanFlowCanvas({
  graph,
  tree,
}: {
  graph: { nodes: FlowNode[]; edges: Edge[] };
  tree: TreeState;
}) {
  const flow = useReactFlow<FlowNode>();
  const [overviewRequest, setOverviewRequest] = useState(0);

  useEffect(() => {
    if (overviewRequest === 0) return;
    const frame = window.requestAnimationFrame(() => {
      void flow.fitView(FIT_VIEW_OPTIONS);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [flow, overviewRequest]);

  const showOverview = useCallback(() => {
    tree.openAll();
    setOverviewRequest((current) => current + 1);
  }, [tree]);

  return (
    <div className="mcp-plan-flow" aria-label="目標マップ">
      <ReactFlow<FlowNode>
        nodes={graph.nodes}
        edges={graph.edges}
        nodeTypes={nodeTypes}
        fitView
        fitViewOptions={FIT_VIEW_OPTIONS}
        // Keep custom node controls clickable without making the map selectable.
        onNodeClick={() => undefined}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable={false}
        zoomOnDoubleClick={false}
        zoomOnScroll={false}
        panOnDrag
        proOptions={{ hideAttribution: true }}
      >
        <Background gap={22} size={1} />
        <Controls showFitView={false} showInteractive={false} />
      </ReactFlow>
      <button
        type="button"
        className="mcp-flow-overview"
        onClick={showOverview}
      >
        全体表示
      </button>
    </div>
  );
}
