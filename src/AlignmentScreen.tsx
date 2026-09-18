/**
 * The alignment map: whose goals these are, and what they roll up to.
 *
 * It reads downward from the goals that have nothing above them, because that
 * is how the question is asked — "what is the company trying to do, and who is
 * working on it". A goal with nothing above it is normal at the top and worth
 * noticing further down, so orphans are counted rather than flagged as errors.
 *
 * `part_of` and `contributes_to` are shown as different things, because they
 * are. Structure is the tree; contribution is listed beside it.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { ApiError, request, type Workspace } from "./api";
import {
  alignmentViewFrom,
  ownerKindLabel,
  ownerLabel,
  roots,
  supporters,
  type AlignedGoal,
  type AlignmentView,
  type OwnerKind,
} from "./shared/alignmentView";

type Lens = { kind: OwnerKind | "all"; id?: string };

function Goal({
  goal,
  view,
  depth,
  onOpen,
  seen,
}: {
  goal: AlignedGoal;
  view: AlignmentView;
  depth: number;
  onOpen: (id: string) => void;
  seen: Set<string>;
}) {
  // The graph refuses cycles, but a goal can be supported through two paths;
  // rendering it twice at the same depth would be noise.
  if (seen.has(goal.id)) return null;
  const next = new Set(seen).add(goal.id);
  const below = supporters(view, goal);
  return (
    <li className="alignment-node" data-state={goal.state}>
      <div className="alignment-row" style={{ paddingInlineStart: depth * 16 }}>
        <button className="alignment-title" onClick={() => onOpen(goal.id)}>
          <strong>{goal.title}</strong>
        </button>
        <span className="alignment-owner" data-kind={goal.owner?.kind}>
          {ownerLabel(goal.owner)}
        </span>
        {goal.cycleLabel && (
          <span className="alignment-cycle">{goal.cycleLabel}</span>
        )}
        <span className="alignment-assessment">
          {goal.selfAssessment === null
            ? "評価未設定"
            : `自己評価 ${goal.selfAssessment}%`}
        </span>
        {goal.descendantWork > 0 && (
          <span className="alignment-work">配下{goal.descendantWork}件</span>
        )}
        {goal.contributesTo.length > 0 && (
          <span className="alignment-contributes">
            貢献先{goal.contributesTo.length}件
          </span>
        )}
      </div>
      {below.length > 0 && (
        <ul>
          {below.map((child) => (
            <Goal
              key={child.id}
              goal={child}
              view={view}
              depth={depth + 1}
              onOpen={onOpen}
              seen={next}
            />
          ))}
        </ul>
      )}
    </li>
  );
}

export function AlignmentScreen({
  workspace,
  onOpenItem,
}: {
  workspace?: Workspace;
  onOpenItem: (id: string) => void;
}) {
  const [view, setView] = useState<AlignmentView | null>(null);
  const [lens, setLens] = useState<Lens>({ kind: "all" });
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    if (!workspace) return;
    setError("");
    try {
      const value = await request<unknown>(
        "GET",
        `/v1/workspaces/${workspace.id}/alignment`,
      );
      setView(alignmentViewFrom(value));
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "アラインメントを読み込めませんでした",
      );
    }
  }, [workspace]);

  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    setLens({ kind: "all" });
  }, [workspace?.id]);

  /**
   * The lens narrows *which goals are listed*, not what they are connected to.
   * Hiding an edge because its other end is filtered out would make the map
   * say something untrue about the graph.
   */
  const shown = useMemo(() => {
    if (!view) return [];
    if (lens.kind === "all") return roots(view);
    const matching = view.goals.filter(
      (goal) =>
        goal.owner?.kind === lens.kind &&
        (lens.id === undefined || goal.owner?.id === lens.id),
    );
    return matching;
  }, [view, lens]);

  if (!workspace) return null;

  return (
    <div className="page-content alignment-screen">
      <section className="panel alignment-intro">
        <div>
          <span className="eyebrow">{workspace.name}</span>
          <p>
            目標の担当（組織・チーム・個人）と、それが何に効いているかを1つのグラフで
            確認します。個人のワークスペースの目標はここには含まれません。
          </p>
        </div>
      </section>

      {error && (
        <p className="auth-error" role="alert">
          {error}
        </p>
      )}

      {view && (
        <>
          <section className="panel alignment-lenses">
            <div className="alignment-lens-row">
              <button
                aria-pressed={lens.kind === "all"}
                onClick={() => setLens({ kind: "all" })}
              >
                すべて
              </button>
              <button
                aria-pressed={lens.kind === "organization"}
                onClick={() => setLens({ kind: "organization" })}
              >
                {ownerKindLabel("organization")}
              </button>
              {view.teams.map((team) => (
                <button
                  key={team}
                  aria-pressed={lens.kind === "team" && lens.id === team}
                  onClick={() => setLens({ kind: "team", id: team })}
                >
                  {team}
                </button>
              ))}
              {view.people.map((who) => (
                <button
                  key={who}
                  aria-pressed={lens.kind === "person" && lens.id === who}
                  onClick={() => setLens({ kind: "person", id: who })}
                >
                  {who}
                </button>
              ))}
            </div>
            <p className="alignment-counts">
              目標{view.goals.length}件 ／ 上位に接続されていない目標
              {view.orphanGoals}件 ／ 担当なし{view.unownedGoals}件
            </p>
          </section>

          <section className="panel alignment-map">
            {shown.length === 0 ? (
              <p className="empty-value">
                {view.goals.length === 0
                  ? "この領域にはまだ目標がありません。"
                  : "この条件に当てはまる目標はありません。"}
              </p>
            ) : (
              <ul className="alignment-tree">
                {shown.map((goal) => (
                  <Goal
                    key={goal.id}
                    goal={goal}
                    view={view}
                    depth={0}
                    onOpen={onOpenItem}
                    seen={new Set()}
                  />
                ))}
              </ul>
            )}
          </section>
        </>
      )}
    </div>
  );
}
