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
  hasActions?: boolean;
  open?: boolean;
  active?: boolean;
  onSelect?: () => void;
  onToggle?: () => void;
  onAddChild?: () => void;
  onAddSibling?: () => void;
  onEdit?: () => void;
  onMove?: () => void;
  onMoveUp?: () => void;
  onMoveDown?: () => void;
};
/** A node in the generic part_of tree.  The map intentionally does not
 * encode a Goal -> Initiative hierarchy: every item kind can have a parent. */
export type MapItem = {
  id: string;
  title: string;
  kind: string;
  workspaceId?: string;
  parentId?: string;
  scope: Scope;
  icon?: string;
  subtitle?: string;
  progress?: number | null;
  /** Persisted sibling order from the part_of relation. */
  position?: number | null;
};
type MapNode = Node<MapData, "goal">;
function GoalNode({ data }: NodeProps<MapNode>) {
  const color = data.scope ? scopeClass[data.scope] : "blue";
  return (
    <>
      <button
        className={`map-node nodrag nopan ${data.kind} ${color} ${data.active ? "is-selected" : ""} ${data.hasActions ? "has-actions" : ""}`}
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
      {data.kind !== "root" && (data.onAddChild || data.onAddSibling || data.onEdit || data.onMove) && (
        <span className="map-node-actions">
          {data.onEdit && <button type="button" className="map-node-action nodrag nopan" onClick={data.onEdit}>編集</button>}
          {data.onMove && <button type="button" className="map-node-action nodrag nopan" onClick={data.onMove}>親を変更</button>}
          {data.onAddChild && (
            <button
              type="button"
              className="map-node-action nodrag nopan"
              onClick={data.onAddChild}
              aria-label={`${data.title}に子項目を追加`}
            >
              子を追加
            </button>
          )}
          {data.onAddSibling && (
            <button
              type="button"
              className="map-node-action nodrag nopan"
              onClick={data.onAddSibling}
              aria-label={`${data.title}の兄弟項目を追加`}
            >
              兄弟を追加
            </button>
          )}
          {data.onMoveUp && <button type="button" className="map-node-action nodrag nopan" onClick={data.onMoveUp}>↑</button>}
          {data.onMoveDown && <button type="button" className="map-node-action nodrag nopan" onClick={data.onMoveDown}>↓</button>}
        </span>
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
  terminal?: boolean;
  position?: number | null;
  sourceIndex: number;
};
const GAP = 22;
const nodeSize = (item: TreeItem, hasActions: boolean) => {
  const compact = item.kind === "goal"
    ? { width: 214, height: 65 }
    : { width: 148, height: 70 };
  return hasActions
    ? { ...compact, height: compact.height + 51 }
    : compact;
};
const strokeColor = (scope: Scope) =>
  scope === "チーム" ? "#c4adf5" : scope === "組織" ? "#bdcce0" : "#abcaf6";
type Props = {
  goals: Goal[];
  initiatives: Initiative[];
  /** Optional complete graph projection. When supplied it is the source of
   * truth, including milestones and actions; the legacy props remain for
   * callers that only have the original two collections. */
  items?: MapItem[];
  selected: string;
  onSelect: (id: string) => void;
  scope: Scope | "すべて";
  setScope: (scope: Scope | "すべて") => void;
  onInitiative: (id: string) => void;
  onAddChild?: (id: string) => void;
  onAddSibling?: (id: string) => void;
  onEdit?: (id: string) => void;
  onMove?: (id: string) => void;
  onMoveUp?: (id: string) => void;
  onMoveDown?: (id: string) => void;
  canEdit?: (id: string) => boolean;
};
function MapCanvas({
  goals,
  initiatives,
  items,
  selected,
  onSelect,
  scope,
  setScope,
  onInitiative,
  onAddChild,
  onAddSibling,
  onEdit,
  onMove,
  onMoveUp,
  onMoveDown,
  canEdit,
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
    const source: MapItem[] = items ?? [
      ...goals.map((goal) => ({
        ...goal,
        kind: "goal",
        parentId: goal.parentId,
      })),
      ...initiatives.map((initiative) => ({
        ...initiative,
        kind: "initiative",
        scope:
          goals.find((goal) => goal.id === initiative.goalId)?.scope ?? "個人",
        parentId: initiative.parentId ?? initiative.goalId,
      })),
    ];
    const matched = source.filter(
      (item) => scope === "すべて" || item.scope === scope,
    );
    const byId = new Map(source.map((item) => [item.id, item]));
    const shown = new Set(matched.map((item) => item.id));
    const displayParent = (item: MapItem) => {
      const seen = new Set<string>();
      let current = item.parentId ? byId.get(item.parentId) : undefined;
      while (current && !shown.has(current.id) && !seen.has(current.id)) {
        seen.add(current.id);
        current = current.parentId ? byId.get(current.parentId) : undefined;
      }
      return current && shown.has(current.id) ? current.id : undefined;
    };
    const treeItems: TreeItem[] = matched.map((item, sourceIndex) => ({
      id: item.id,
      // The existing node treatment is deliberately retained for all
      // non-goals so the visual language and spacing do not change.
      kind:
        item.kind === "outcome" || item.kind === "idea" || item.kind === "goal" || item.kind === "milestone"
          ? "goal"
          : "initiative",
      workspaceId: item.workspaceId,
      parentId: displayParent(item),
      scope: item.scope,
      goal:
        item.kind === "outcome" || item.kind === "idea" || item.kind === "goal" || item.kind === "milestone"
          ? {
              id: item.id,
              title: item.title,
              parentId: item.parentId,
              subtitle: item.subtitle ?? "",
              scope: item.scope,
              icon: item.icon ?? "target",
              purpose: "",
              progress: item.progress ?? null,
              next: "",
              memo: "",
            }
          : undefined,
      initiative:
        item.kind === "outcome" || item.kind === "idea" || item.kind === "goal" || item.kind === "milestone"
          ? undefined
          : {
              id: item.id,
              goalId: item.parentId ?? "",
              parentId: item.parentId,
              title: item.title,
              icon: item.icon ?? (item.kind === "action" ? "rocket" : "flag"),
              progress: item.progress ?? null,
            },
      children: [],
      terminal: item.kind === "action",
      position: item.position,
      sourceIndex,
    }));
    const index = new Map(treeItems.map((item) => [item.id, item]));
    const roots: TreeItem[] = [];
    treeItems.forEach((item) => {
      const parent = item.parentId ? index.get(item.parentId) : undefined;
      if (parent && parent !== item) parent.children.push(item);
      else roots.push(item);
    });
    const compare = (a: TreeItem, b: TreeItem) => {
      const pa = a.position ?? Number.MAX_SAFE_INTEGER;
      const pb = b.position ?? Number.MAX_SAFE_INTEGER;
      return pa - pb || a.sourceIndex - b.sourceIndex;
    };
    for (const item of treeItems) item.children.sort(compare);
    roots.sort(compare);
    return roots;
  }, [goals, initiatives, items, scope]);
  const { nodes, edges } = useMemo(() => {
    const nodes: MapNode[] = [];
    const edges: Edge[] = [];
    // A single top-level goal is the root itself; a forest keeps the heading node.
    const heading = tree.length !== 1;
    const offset = heading ? 1 : 0;
    const isOpen = (item: TreeItem) => !closedIds.has(item.id);
    const childrenOf = (item: TreeItem) => (isOpen(item) ? item.children : []);
    const actionState = (item: TreeItem, siblingIndex: number, siblingCount: number) => {
      const editable = canEdit ? canEdit(item.id) : true;
      const parentEditable = item.parentId
        ? canEdit
          ? canEdit(item.parentId)
          : true
        : false;
      const canMove = editable && (!item.parentId || parentEditable);
      const canMoveUp = editable && parentEditable && siblingIndex > 0 && !!onMoveUp;
      const canMoveDown = editable && parentEditable && siblingIndex < siblingCount - 1 && !!onMoveDown;
      const hasActions = editable && Boolean(
          onEdit || (canMove && onMove) || onAddChild || onAddSibling || canMoveUp || canMoveDown,
      );
      return { editable, canMove, canMoveUp, canMoveDown, hasActions };
    };
    const depthHeights = new Map<number, number>();
    const measureDepth = (itemsAtDepth: TreeItem[], depth: number) => {
      itemsAtDepth.forEach((item, index) => {
        const state = actionState(item, index, itemsAtDepth.length);
        depthHeights.set(
          depth,
          Math.max(depthHeights.get(depth) ?? 0, nodeSize(item, state.hasActions).height),
        );
        measureDepth(childrenOf(item), depth + 1);
      });
    };
    measureDepth(tree, offset);
    const rowY = (depth: number) => {
      let y = 8;
      for (let current = 0; current < depth; current += 1) {
        const height = current === 0 && heading
          ? 62
          : depthHeights.get(current) ?? 70;
        y += height + (current === 0 ? (heading ? 48 : 45) : 38);
      }
      return y;
    };
    const measure = (item: TreeItem): number => {
      const own = nodeSize(item, false).width;
      const kids = childrenOf(item);
      if (!kids.length) return own;
      const width =
        kids.reduce((total, child) => total + measure(child), 0) +
        GAP * (kids.length - 1);
      return Math.max(own, width);
    };
    const place = (
      item: TreeItem,
      left: number,
      depth: number,
      siblingIndex = 0,
      siblingCount = 1,
    ) => {
      const width = measure(item);
      const kids = childrenOf(item);
      const shared = {
        hasChildren: kids.length > 0,
        childCount: item.children.length,
        open: isOpen(item),
        onToggle: () => toggleNode(item.id),
      };
      const { editable, canMove, canMoveUp, canMoveDown, hasActions } = actionState(
        item,
        siblingIndex,
        siblingCount,
      );
      const size = nodeSize(item, hasActions);
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
                hasActions,
                kind: "goal",
                subtitle: item.goal!.subtitle,
                progress: undefined,
                active: selected === item.id,
                onSelect: () => onSelect(item.id),
                onAddChild: editable && onAddChild ? () => onAddChild(item.id) : undefined,
                onAddSibling: editable && onAddSibling
                  ? () => onAddSibling(item.id)
                  : undefined,
                onEdit: editable && onEdit ? () => onEdit(item.id) : undefined,
                onMove: canMove && onMove ? () => onMove(item.id) : undefined,
                onMoveUp:
                  canMoveUp
                    ? () => onMoveUp!(item.id)
                    : undefined,
                onMoveDown:
                  canMoveDown
                    ? () => onMoveDown!(item.id)
                    : undefined,
              }
            : {
                ...item.initiative!,
                ...shared,
                hasActions,
                scope: item.scope,
                kind: "initiative",
                active: selected === item.id,
                onSelect: () => onInitiative(item.id),
                onAddChild: item.terminal || !editable
                  ? undefined
                  : onAddChild
                    ? () => onAddChild(item.id)
                    : undefined,
                onAddSibling: editable && onAddSibling
                  ? () => onAddSibling(item.id)
                  : undefined,
                onEdit: editable && onEdit ? () => onEdit(item.id) : undefined,
                onMove: canMove && onMove ? () => onMove(item.id) : undefined,
                onMoveUp:
                  canMoveUp
                    ? () => onMoveUp!(item.id)
                    : undefined,
                onMoveDown:
                  canMoveDown
                    ? () => onMoveDown!(item.id)
                    : undefined,
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
      kids.forEach((child, index) => {
        place(child, cursor, depth + 1, index, kids.length);
        cursor += measure(child) + GAP;
      });
    };
    let cursor = 0;
    tree.forEach((root, index) => {
      place(root, cursor, offset, index, tree.length);
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
  }, [
    tree,
    selected,
    closedIds,
    toggleNode,
    onSelect,
    onInitiative,
    onAddChild,
    onAddSibling,
    onEdit,
    onMove,
    onMoveUp,
    onMoveDown,
    canEdit,
  ]);
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
  // `tree` is the complete graph projection, including milestones and
  // actions. Counting the legacy collections here would leave a newly-added
  // deeper action outside the current viewport.
  }, [scope, tree, closedIds, expanded, resetView]);
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
