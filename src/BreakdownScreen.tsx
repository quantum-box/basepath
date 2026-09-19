/**
 * Breaking a goal down until something is doable.
 *
 * The screen reads in both directions because people ask in both directions:
 * downward when planning ("what makes this happen"), upward when working
 * ("why am I doing this"). They are separate panels rather than one view,
 * since an answer that mixes them is an answer to neither.
 *
 * Depth is not a setting of the screen. It loads two levels, and a branch
 * with more says so and can be opened — which is how a plan with eight levels
 * renders without eight levels of markup and without fetching all of it.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { ApiError, request, type Workspace } from "./api";
import type { WorkspaceStore } from "./useWorkspace";
import {
  breakdownFrom,
  childrenOf,
  gapLabel,
  gapsFrom,
  kindLabel,
  rationaleFrom,
  type Breakdown,
  type BreakdownNode,
  type Gap,
  type Rationale,
} from "./shared/breakdownView";

function Branch({
  node,
  byParent,
  open,
  onToggle,
  onSelect,
  selected,
}: {
  node: BreakdownNode;
  byParent: Map<string, BreakdownNode[]>;
  open: Set<string>;
  onToggle: (node: BreakdownNode) => void;
  onSelect: (id: string) => void;
  selected: string;
}) {
  const children = byParent.get(node.id) ?? [];
  const expanded = open.has(node.id);
  // Two different reasons a branch is not showing its children: the person
  // closed it, and the response did not carry them. Only the second needs a
  // request, so they are not the same affordance.
  const collapsible = children.length > 0;
  return (
    <li className="breakdown-node" data-kind={node.kind}>
      <div className="breakdown-row" data-selected={node.id === selected}>
        {collapsible || node.hasMoreChildren ? (
          <button
            className="breakdown-toggle"
            aria-expanded={expanded}
            aria-label={`${node.title} の内訳`}
            onClick={() => onToggle(node)}
          >
            {expanded && children.length > 0 ? "−" : "+"}
          </button>
        ) : (
          <span className="breakdown-toggle is-leaf" aria-hidden="true" />
        )}
        <span className="breakdown-kind">{kindLabel(node.kind)}</span>
        <button className="breakdown-title" onClick={() => onSelect(node.id)}>
          {node.title}
        </button>
        {node.childCount > 0 && (
          <span className="breakdown-count">{node.childCount}</span>
        )}
        {node.contributesTo.length > 0 && (
          <span className="breakdown-contributes">
            ほかに貢献 {node.contributesTo.length}
          </span>
        )}
        {node.dependsOn.length > 0 && (
          <span className="breakdown-depends">
            待ち {node.dependsOn.length}
          </span>
        )}
      </div>
      {/* The reason is on the link, so it sits with the child it explains. */}
      {node.rationale && (
        <p className="breakdown-rationale">{node.rationale}</p>
      )}
      {expanded && children.length > 0 && (
        <ul>
          {children.map((child) => (
            <Branch
              key={child.id}
              node={child}
              byParent={byParent}
              open={open}
              onToggle={onToggle}
              onSelect={onSelect}
              selected={selected}
            />
          ))}
        </ul>
      )}
      {expanded && children.length === 0 && node.hasMoreChildren && (
        <p className="breakdown-more">読み込み中…</p>
      )}
    </li>
  );
}

export function BreakdownScreen({
  store,
  workspace,
}: {
  store: WorkspaceStore;
  workspace?: Workspace;
}) {
  const [roots, setRoots] = useState<BreakdownNode[]>([]);
  const [rootId, setRootId] = useState("");
  const [tree, setTree] = useState<Breakdown | null>(null);
  const [extra, setExtra] = useState<BreakdownNode[]>([]);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState("");
  const [why, setWhy] = useState<Rationale | null>(null);
  const [gaps, setGaps] = useState<Gap[]>([]);
  const [error, setError] = useState("");

  const say = (failure: unknown, fallback: string) =>
    setError(failure instanceof ApiError ? failure.message : fallback);

  const loadRoots = useCallback(async () => {
    if (!workspace) return;
    try {
      const [graph, found] = await Promise.all([
        request<unknown>("GET", `/v1/workspaces/${workspace.id}/graph`),
        request<unknown>(
          "GET",
          `/v1/workspaces/${workspace.id}/breakdown/gaps`,
        ),
      ]);
      setGaps(gapsFrom(found));
      const source = graph as {
        items?: { id: string; kind: string; title: string }[];
        relations?: { source_id: string; target_id: string; type: string }[];
      };
      const placed = new Set(
        (source.relations ?? [])
          .filter((edge) => edge.type === "part_of")
          .map((edge) => edge.source_id),
      );
      // Everything with nothing above it. That is where a plan is read from,
      // and a top-level item is not the same thing as a "goal kind".
      const tops = (source.items ?? [])
        .filter((item) => !placed.has(item.id))
        .map((item) => ({
          id: item.id,
          kind: item.kind,
          title: item.title,
        })) as BreakdownNode[];
      setRoots(tops);
      setRootId((current) =>
        current && tops.some((item) => item.id === current)
          ? current
          : (tops[0]?.id ?? ""),
      );
      setError("");
    } catch (failure) {
      say(failure, "内訳を読み込めませんでした");
    }
  }, [workspace]);

  const loadTree = useCallback(async () => {
    if (!workspace || !rootId) {
      setTree(null);
      return;
    }
    try {
      const found = await request<unknown>(
        "GET",
        `/v1/workspaces/${workspace.id}/items/${rootId}/breakdown?depth=2`,
      );
      const parsed = breakdownFrom(found);
      setTree(parsed);
      setExtra([]);
      setOpen(new Set(parsed ? parsed.nodes.map((node) => node.id) : []));
      setError("");
    } catch (failure) {
      say(failure, "内訳を読み込めませんでした");
    }
  }, [workspace, rootId]);

  useEffect(() => {
    void loadRoots();
  }, [loadRoots]);
  useEffect(() => {
    void loadTree();
  }, [loadTree]);

  /** Fetches one more level from the node the person opened. */
  const expand = async (node: BreakdownNode) => {
    if (!workspace) return;
    try {
      const found = await request<unknown>(
        "GET",
        `/v1/workspaces/${workspace.id}/items/${node.id}/breakdown?depth=2`,
      );
      const parsed = breakdownFrom(found);
      if (!parsed) return;
      const known = new Set([
        ...(tree?.nodes ?? []).map((entry) => entry.id),
        ...extra.map((entry) => entry.id),
      ]);
      setExtra((current) => [
        ...current,
        ...parsed.nodes.filter((entry) => !known.has(entry.id)),
      ]);
      setOpen((current) => {
        const next = new Set(current);
        for (const entry of parsed.nodes) next.add(entry.id);
        return next;
      });
    } catch (failure) {
      say(failure, "続きを読み込めませんでした");
    }
  };

  const toggle = (node: BreakdownNode) => {
    const loaded = (tree?.nodes ?? [])
      .concat(extra)
      .some((entry) => entry.parentId === node.id);
    // A node at the edge of what was fetched is open in state but has nothing
    // under it to show. Clicking it means "get the rest", never "close" — a
    // branch the person has not seen yet cannot be one they are folding away.
    if (!loaded && node.childCount > 0) {
      setOpen((current) => new Set(current).add(node.id));
      void expand(node);
      return;
    }
    setOpen((current) => {
      const next = new Set(current);
      if (current.has(node.id)) next.delete(node.id);
      else next.add(node.id);
      return next;
    });
  };

  const select = async (id: string) => {
    setSelected(id);
    if (!workspace) return;
    try {
      const found = await request<unknown>(
        "GET",
        `/v1/workspaces/${workspace.id}/items/${id}/ancestry`,
      );
      setWhy(rationaleFrom(found));
    } catch (failure) {
      say(failure, "理由をたどれませんでした");
    }
  };

  const move = async (parentId: string | null) => {
    if (!workspace || !selected) return;
    setError("");
    try {
      await store.run(async () => {
        const item = await request<{ version: number }>(
          "GET",
          `/v1/workspaces/${workspace.id}/items/${selected}`,
        );
        await store.write(
          "POST",
          `/v1/workspaces/${workspace.id}/items/${selected}/reparent`,
          { parent_id: parentId, expected_version: item.version },
        );
      });
      await loadRoots();
      await loadTree();
      await select(selected);
    } catch (failure) {
      say(failure, "移動できませんでした");
    }
  };

  const merged = useMemo(() => {
    if (!tree) return null;
    const seen = new Map<string, BreakdownNode>();
    for (const node of tree.nodes.concat(extra)) seen.set(node.id, node);
    return { ...tree, nodes: [...seen.values()] };
  }, [tree, extra]);

  const byParent = useMemo(
    () => (merged ? childrenOf(merged) : new Map<string, BreakdownNode[]>()),
    [merged],
  );
  const root = merged?.nodes.find((node) => node.id === merged.rootId);

  /**
   * Where this item could be moved to.
   *
   * Everything loaded under the current root, plus every other top-level
   * item — otherwise a branch could only ever move within the tree it is
   * already in, which is the move people most often want to make.
   */
  const targets = useMemo(() => {
    const seen = new Map<string, BreakdownNode>();
    for (const node of (merged?.nodes ?? []).concat(roots)) {
      if (!seen.has(node.id)) seen.set(node.id, node);
    }
    return [...seen.values()];
  }, [merged, roots]);

  if (!workspace) return null;

  return (
    <div className="page-content breakdown-screen">
      <section className="panel breakdown-intro">
        <div>
          <span className="eyebrow">{workspace.name}</span>
          <p>
            大きな目標を、実行できるところまで分解します。階層の数は決まっていません。
            必要なだけ深くでき、途中を飛ばしても構いません。
          </p>
        </div>
        {roots.length > 0 && (
          <label className="breakdown-root-picker">
            <span>起点</span>
            <select
              aria-label="起点"
              value={rootId}
              onChange={(event) => setRootId(event.target.value)}
            >
              {roots.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.title}
                </option>
              ))}
            </select>
          </label>
        )}
      </section>

      {error && (
        <p className="auth-error" role="alert">
          {error}
        </p>
      )}

      <div className="breakdown-columns">
        <section className="panel breakdown-tree">
          <div className="section-header">
            <h3>何によって達成されるか</h3>
            {merged?.truncated && <span>一部のみ表示</span>}
          </div>
          {!root ? (
            <p className="empty-value">
              まだ何もありません。目標をひとつ作るところからです。
            </p>
          ) : (
            <ul>
              <Branch
                node={root}
                byParent={byParent}
                open={open}
                onToggle={toggle}
                onSelect={(id) => void select(id)}
                selected={selected}
              />
            </ul>
          )}
          {merged?.truncated && (
            <p className="breakdown-more">
              大きいため一部だけ読み込んでいます。枝を開くと続きを取得します。
            </p>
          )}
        </section>

        <section className="panel breakdown-why">
          <div className="section-header">
            <h3>なぜ必要か</h3>
          </div>
          {!why ? (
            <p className="empty-value">
              左で項目を選ぶと、上位の目標までたどります。
            </p>
          ) : (
            <>
              <p className="breakdown-selected">{why.itemTitle}</p>
              {why.topLevel ? (
                <p className="empty-value">
                  これより上はありません。いちばん上の目標です。
                </p>
              ) : (
                <ol className="breakdown-chain">
                  {why.ancestors.map((step) => (
                    <li key={step.id}>
                      <strong>{step.title}</strong>
                      {/* Empty means nobody wrote one. Saying so beats
                          inventing a reason that reads like a record. */}
                      <small>
                        {step.rationale || "理由は記録されていません"}
                      </small>
                    </li>
                  ))}
                </ol>
              )}
              {why.contributesTo.length > 0 && (
                <div className="breakdown-links">
                  <h4>ほかに貢献している目標</h4>
                  <ul>
                    {why.contributesTo.map((link) => (
                      <li key={link.id}>
                        {link.title}
                        {link.rationale && <small>{link.rationale}</small>}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              {why.dependsOn.length > 0 && (
                <div className="breakdown-links">
                  <h4>待っていること</h4>
                  <ul>
                    {why.dependsOn.map((link) => (
                      <li key={link.id}>
                        {link.title}
                        {link.rationale && <small>{link.rationale}</small>}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              <div className="breakdown-move">
                <label>
                  <span>この項目の位置</span>
                  <select
                    aria-label="この項目の位置"
                    value={
                      why.ancestors.length > 0
                        ? why.ancestors[why.ancestors.length - 1].id
                        : ""
                    }
                    onChange={(event) => void move(event.target.value || null)}
                  >
                    <option value="">どこにも属さない</option>
                    {targets
                      .filter((node) => node.id !== why.itemId)
                      .map((node) => (
                        <option key={node.id} value={node.id}>
                          {node.title}
                        </option>
                      ))}
                  </select>
                </label>
                <p className="breakdown-hint">
                  移動しても下にあるものは一緒に動きます。外しても消えません。
                </p>
              </div>
            </>
          )}
        </section>
      </div>

      <section className="panel breakdown-gaps">
        <div className="section-header">
          <h3>降りきっていないところ</h3>
          <span>{gaps.length}件</span>
        </div>
        {gaps.length === 0 ? (
          <p className="empty-value">
            いまのところ、実行できるところまで降りています。
          </p>
        ) : (
          <ul>
            {gaps.map((gap) => (
              <li key={`${gap.itemId}-${gap.gap}`} data-gap={gap.gap}>
                <div className="breakdown-row">
                  <span className="breakdown-kind">{kindLabel(gap.kind)}</span>
                  <strong>{gap.title}</strong>
                  <span className="breakdown-gap-label">
                    {gapLabel(gap.gap)}
                  </span>
                </div>
                <small>{gap.detail}</small>
              </li>
            ))}
          </ul>
        )}
        <p className="breakdown-hint">
          見つけた場所を並べているだけです。どう分解するかはあなたが決めます。
          こちらで勝手に埋めることはしません。
        </p>
      </section>
    </div>
  );
}
