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
  active?: boolean;
  onSelect?: () => void;
};
type MapNode = Node<MapData, "goal">;
function GoalNode({ data }: NodeProps<MapNode>) {
  const color = data.scope ? scopeClass[data.scope] : "blue";
  return (
    <button
      className={`map-node nodrag nopan ${data.kind} ${color} ${data.active ? "is-selected" : ""}`}
      onClick={data.onSelect}
      aria-pressed={data.active}
      tabIndex={data.kind === "root" ? -1 : 0}
    >
      {data.kind !== "root" && <Handle type="target" position={Position.Top} />}
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
      {data.kind !== "initiative" && (
        <Handle type="source" position={Position.Bottom} />
      )}
    </button>
  );
}
const nodeTypes = { goal: GoalNode };
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
  const container = useRef<HTMLDivElement>(null);
  const visible = useMemo(
    () => goals.filter((g) => scope === "すべて" || g.scope === scope),
    [goals, scope],
  );
  const { nodes, edges } = useMemo(() => {
    const count = visible.length;
    const fullWidth = count * 320;
    const nodes: MapNode[] = [
      {
        id: "root",
        type: "goal",
        position: { x: fullWidth / 2 - 110, y: 8 },
        data: {
          kind: "root",
          title: "自分の目標と行動",
          subtitle: "つながりを眺めて、次の一歩へ",
        },
        style: { width: 220, height: 62 },
      },
    ];
    const edges: Edge[] = [];
    visible.forEach((goal, i) => {
      nodes.push({
        id: goal.id,
        type: "goal",
        position: { x: i * 320 + 54, y: 118 },
        style: { width: 214, height: 65 },
        data: {
          ...goal,
          kind: "goal",
          subtitle: goal.subtitle,
          progress: undefined,
          active: selected === goal.id,
          onSelect: () => onSelect(goal.id),
        },
      });
      const stroke =
        goal.scope === "チーム"
          ? "#c4adf5"
          : goal.scope === "組織"
            ? "#bdcce0"
            : "#abcaf6";
      const children = initiatives.filter((it) => it.goalId === goal.id);
      children.forEach((item, childIndex) => {
        nodes.push({
          id: item.id,
          type: "goal",
          position: {
            x: i * 320 + (childIndex % 2) * 160 + 8,
            y: 221 + Math.floor(childIndex / 2) * 85,
          },
          style: { width: 148, height: 70 },
          data: {
            ...item,
            scope: goal.scope,
            kind: "initiative",
            onSelect: () => onInitiative(item.id),
          },
        });
        edges.push({
          id: `${goal.id}-${item.id}`,
          source: goal.id,
          target: item.id,
          type: "smoothstep",
          style: { stroke, strokeWidth: 1.4 },
        });
      });
    });
    return { nodes, edges };
  }, [visible, initiatives, selected, onSelect, onInitiative]);
  const resetView = useCallback(() => {
    // Establish the baseline after fitting async-loaded data, before animation
    // callbacks can report a percentage relative to an earlier empty graph.
    void flow.fitView({ padding: 0.035, duration: 0, minZoom: 0.1 }).then(() => {
      setBaseZoom(flow.getZoom());
      setZoom(100);
    });
  }, [flow]);
  useEffect(() => {
    const timer = setTimeout(resetView, 90);
    return () => clearTimeout(timer);
  }, [scope, goals.length, initiatives.length, expanded, resetView]);
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
          fitViewOptions={{ padding: 0.035 }}
          minZoom={0.1}
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
