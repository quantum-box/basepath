import { useEffect, useMemo, useState, useCallback, useRef } from "react";
import {
  ReactFlow,
  ReactFlowProvider,
  Handle,
  Position,
  useReactFlow,
  type Node,
  type NodeProps,
  type Edge,
} from "@xyflow/react";
import { Icon } from "./icons";
import { scopeClass, type Goal, type Initiative, type Scope } from "./data";

type MapData = {
  title: string;
  subtitle?: string;
  scope?: Scope;
  icon?: string;
  progress?: number | null;
  kind: "root" | "goal" | "initiative";
  hasChildren?: boolean;
  childCount?: number;
  open?: boolean;
  active?: boolean;
  onSelect?: () => void;
  onToggle?: () => void;
};
type MapNode = Node<MapData, "goal">;
function GoalNode({ data }: NodeProps<MapNode>) {
  const color = data.scope ? scopeClass[data.scope] : "blue";
  return (
    <>
      <button
        className={`map-node nodrag nopan ${data.kind} ${color} ${data.active ? "is-selected" : ""}`}
        onClick={data.onSelect}
        aria-pressed={data.active}
        tabIndex={data.kind === "root" ? -1 : 0}
      >
        {data.kind !== "root" && (
          <Handle type="target" position={Position.Top} />
        )}
        {data.kind === "root" ? (
          <>
            <span className="root-logo">
              <Icon name="mountains" size={20} weight="duotone" />
            </span>
            <strong>{data.title}</strong>
            <small>{data.subtitle}</small>
          </>
        ) : (
          <>
            {data.kind === "goal" && (
              <span className={`scope-badge ${color}`}>{data.scope}</span>
            )}
            <span
              className={`node-icon ${data.icon === "calendar" ? "pink" : data.icon === "bulb" ? "orange" : color}`}
            >
              <Icon
                name={data.icon!}
                size={data.kind === "goal" ? 23 : 18}
                weight="duotone"
              />
            </span>
            <span className="node-copy">
              <strong>{data.title}</strong>
              {data.subtitle && <small>{data.subtitle}</small>}
              {data.progress != null && (
                <span className="node-progress">
                  <span className="progress-track">
                    <span style={{ width: `${data.progress}%` }} />
                  </span>
                  <small>{data.progress}%</small>
                </span>
              )}
            </span>
          </>
        )}
        {data.hasChildren && (
          <Handle type="source" position={Position.Bottom} />
        )}
      </button>
      {!!data.childCount && (
        <button
          className="node-toggle nodrag nopan"
          onClick={data.onToggle}
          aria-expanded={data.open}
          aria-label={`${data.title.replace("\n", "")}の下の階層を${
            data.open ? "閉じる" : "開く"
          }`}
        >
          {data.open ? (
            <Icon name="minus" size={11} weight="bold" />
          ) : (
            <>
              <Icon name="plus" size={11} weight="bold" />
              {data.childCount}
            </>
          )}
        </button>
      )}
    </>
  );
}
const nodeTypes = { goal: GoalNode };
type TreeItem = {
  id: string;
  kind: "goal" | "initiative";
  parentId?: string;
  scope: Scope;
  goal?: Goal;
  initiative?: Initiative;
  children: TreeItem[];
};
const GAP = 22;
const nodeSize = (item: TreeItem) =>
  item.kind === "goal"
    ? { width: 214, height: 65 }
    : { width: 148, height: 70 };
const rowY = (depth: number) =>
  depth === 0 ? 8 : depth === 1 ? 118 : 221 + (depth - 2) * 95;
const strokeColor = (scope: Scope) =>
  scope === "チーム" ? "#c4adf5" : scope === "組織" ? "#bdcce0" : "#abcaf6";
type Props = {
  goals: Goal[];
  initiatives: Initiative[];
  selected: string;
  onSelect: (id: string) => void;
  scope: Scope | "すべて";
  setScope: (scope: Scope | "すべて") => void;
  onInitiative: (id: string) => void;
};
function MapCanvas({
  goals,
  initiatives,
  selected,
  onSelect,
  scope,
  setScope,
  onInitiative,
}: Props) {
  const flow = useReactFlow<MapNode>();
  const [zoom, setZoom] = useState(100);
  const [expanded, setExpanded] = useState(false);
  const [baseZoom, setBaseZoom] = useState(1);
  const [closedIds, setClosedIds] = useState<Set<string>>(new Set());
  const toggleNode = useCallback((id: string) => {
    setClosedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);
  const container = useRef<HTMLDivElement>(null);
  const tree = useMemo(() => {
    const matched = goals.filter(
      (g) => scope === "すべて" || g.scope === scope,
    );
    const byId = new Map(goals.map((g) => [g.id, g]));
    const shown = new Set(matched.map((g) => g.id));
    const displayParent = (goal: Goal) => {
      const seen = new Set<string>();
      let current = goal.parentId ? byId.get(goal.parentId) : undefined;
      while (current && !shown.has(current.id) && !seen.has(current.id)) {
        seen.add(current.id);
        current = current.parentId ? byId.get(current.parentId) : undefined;
      }
      return current && shown.has(current.id) ? current.id : undefined;
    };
    const items: TreeItem[] = matched.map((goal) => ({
      id: goal.id,
      kind: "goal",
      parentId: displayParent(goal),
      scope: goal.scope,
      goal,
      children: [],
    }));
    initiatives.forEach((initiative) => {
      const goal = byId.get(initiative.goalId);
      if (!goal || !shown.has(goal.id)) return;
      items.push({
        id: initiative.id,
        kind: "initiative",
        parentId: initiative.parentId ?? initiative.goalId,
        scope: goal.scope,
        initiative,
        children: [],
      });
    });
    const index = new Map(items.map((item) => [item.id, item]));
    const roots: TreeItem[] = [];
    items.forEach((item) => {
      const parent = item.parentId ? index.get(item.parentId) : undefined;
      if (parent && parent !== item) parent.children.push(item);
      else roots.push(item);
    });
    return roots;
  }, [goals, initiatives, scope]);
  const { nodes, edges } = useMemo(() => {
    const nodes: MapNode[] = [];
    const edges: Edge[] = [];
    // A single top-level goal is the root itself; a forest keeps the heading node.
    const heading = tree.length !== 1;
    const offset = heading ? 1 : 0;
    const isOpen = (item: TreeItem) => !closedIds.has(item.id);
    const childrenOf = (item: TreeItem) => (isOpen(item) ? item.children : []);
    const measure = (item: TreeItem): number => {
      const own = nodeSize(item).width;
      const kids = childrenOf(item);
      if (!kids.length) return own;
      const width =
        kids.reduce((total, child) => total + measure(child), 0) +
        GAP * (kids.length - 1);
      return Math.max(own, width);
    };
    const place = (item: TreeItem, left: number, depth: number) => {
      const width = measure(item);
      const size = nodeSize(item);
      const kids = childrenOf(item);
      const shared = {
        hasChildren: kids.length > 0,
        childCount: item.children.length,
        open: isOpen(item),
        onToggle: () => toggleNode(item.id),
      };
      nodes.push({
        id: item.id,
        type: "goal",
        position: { x: left + (width - size.width) / 2, y: rowY(depth) },
        style: size,
        data:
          item.kind === "goal"
            ? {
                ...item.goal!,
                ...shared,
                kind: "goal",
                subtitle: item.goal!.subtitle,
                progress: undefined,
                active: selected === item.id,
                onSelect: () => onSelect(item.id),
              }
            : {
                ...item.initiative!,
                ...shared,
                scope: item.scope,
                kind: "initiative",
                onSelect: () => onInitiative(item.id),
              },
      });
      if (item.parentId && depth > offset) {
        edges.push({
          id: `${item.parentId}-${item.id}`,
          source: item.parentId,
          target: item.id,
          type: depth === offset + 1 ? "default" : "smoothstep",
          style: { stroke: strokeColor(item.scope), strokeWidth: 1.4 },
        });
      }
      const childrenWidth =
        kids.reduce((total, child) => total + measure(child), 0) +
        GAP * (kids.length - 1);
      let cursor = left + (width - childrenWidth) / 2;
      kids.forEach((child) => {
        place(child, cursor, depth + 1);
        cursor += measure(child) + GAP;
      });
    };
    let cursor = 0;
    tree.forEach((root) => {
      place(root, cursor, offset);
      cursor += measure(root) + GAP;
    });
    if (heading) {
      const fullWidth = Math.max(cursor - GAP, 220);
      nodes.unshift({
        id: "root",
        type: "goal",
        position: { x: fullWidth / 2 - 110, y: 8 },
        data: {
          kind: "root",
          title: "自分の目標と行動",
          subtitle: "つながりを眺めて、次の一歩へ",
        },
        style: { width: 220, height: 62 },
      });
    }
    return { nodes, edges };
  }, [tree, selected, closedIds, toggleNode, onSelect, onInitiative]);
  const resetView = useCallback(() => {
    // Establish the baseline after fitting async-loaded data, before animation
    // callbacks can report a percentage relative to an earlier empty graph.
    void flow
      .fitView({ padding: 0.035, duration: 0, minZoom: 0.05 })
      .then(() => {
        setBaseZoom(flow.getZoom());
        setZoom(100);
      });
  }, [flow]);
  useEffect(() => {
    const timer = setTimeout(resetView, 90);
    return () => clearTimeout(timer);
  }, [scope, goals.length, initiatives.length, closedIds, expanded, resetView]);
  useEffect(() => {
    const observer = new ResizeObserver(resetView);
    if (container.current) observer.observe(container.current);
    return () => observer.disconnect();
  }, [resetView]);
  useEffect(() => {
    if (!expanded) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setExpanded(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [expanded]);
  return (
    <section
      className={`panel map-panel ${expanded ? "map-expanded" : ""}`}
      id="goal-map"
      aria-label="目標マップ"
    >
      <div className="map-toolbar">
        <h2>
          <span className="heading-mark" />
          目標マップ
        </h2>
        <div className="map-tools">
          <div className="segmented map-filter">
            {(["すべて", "個人", "チーム", "組織"] as const).map((item) => (
              <button
                key={item}
                onClick={() => setScope(item)}
                className={scope === item ? "active" : ""}
                aria-pressed={scope === item}
              >
                {item}
              </button>
            ))}
          </div>
          <div className="zoom-control">
            <button
              aria-label="縮小"
              onClick={() => void flow.zoomOut({ duration: 160 })}
            >
              <Icon name="minus" size={15} />
            </button>
            <button
              className="zoom-value"
              aria-label="表示をリセット"
              onClick={resetView}
            >
              {zoom}%
            </button>
            <button
              aria-label="拡大"
              onClick={() => void flow.zoomIn({ duration: 160 })}
            >
              <Icon name="plus" size={15} />
            </button>
          </div>
          <button
            className="icon-button outlined"
            aria-label={expanded ? "拡大表示を閉じる" : "マップを全画面表示"}
            onClick={() => setExpanded(!expanded)}
          >
            <Icon name={expanded ? "close" : "expand"} size={19} />
          </button>
        </div>
      </div>
      <div className="flow-container" ref={container}>
        <ReactFlow<MapNode>
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          fitView
          fitViewOptions={{ padding: 0.035, minZoom: 0.05 }}
          minZoom={0.05}
          maxZoom={2}
          nodesDraggable={false}
          nodesConnectable={false}
          elementsSelectable={false}
          zoomOnDoubleClick={false}
          zoomOnScroll={false}
          preventScrolling={false}
          panOnDrag
          onMove={(_, view) =>
            setZoom(Math.round((view.zoom / baseZoom) * 100))
          }
        />
      </div>
    </section>
  );
}
export function GoalMap(props: Props) {
  return (
    <ReactFlowProvider>
      <MapCanvas {...props} />
    </ReactFlowProvider>
  );
}
