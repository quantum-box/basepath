/**
 * Collapse and selection state for a goal tree.
 *
 * The web map and the MCP App draw the tree very differently — one with a
 * canvas, one with nested lists — but the behaviour a person relies on is the
 * same: every branch is open until they close it, and closing a branch does
 * not lose which node they had selected. That behaviour lives here so both
 * surfaces cannot drift apart.
 *
 * The state is keyed by scope (a workspace id). Switching workspaces resets
 * it, because a node id from one workspace means nothing in another and
 * carrying it over would show a stale selection as if it were current.
 */
import { useCallback, useEffect, useRef, useState } from "react";

export type TreeState = {
  /** True when a node's children are shown. Every node starts open. */
  isOpen: (id: string) => boolean;
  toggle: (id: string) => void;
  /** Currently selected node, or "" when nothing is selected. */
  selected: string;
  select: (id: string) => void;
  /** Number of branches the person has closed, for a "expand all" affordance. */
  closedCount: number;
  openAll: () => void;
};

export function useTreeState(scope: string): TreeState {
  const [closed, setClosed] = useState<Set<string>>(() => new Set());
  const [selected, setSelected] = useState("");
  const previousScope = useRef(scope);

  useEffect(() => {
    if (previousScope.current === scope) return;
    previousScope.current = scope;
    // A different workspace is a different plan. Nothing carries over.
    setClosed(new Set());
    setSelected("");
  }, [scope]);

  const toggle = useCallback((id: string) => {
    setClosed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const isOpen = useCallback((id: string) => !closed.has(id), [closed]);
  const openAll = useCallback(() => setClosed(new Set()), []);

  return {
    isOpen,
    toggle,
    selected,
    select: setSelected,
    closedCount: closed.size,
    openAll,
  };
}

/**
 * Drops a selection that is no longer in the tree.
 *
 * A node can disappear because the plan changed, because the person switched
 * workspaces, or because a truncated result no longer includes it. In every
 * case showing its detail would be showing something that is not there.
 */
export function pruneSelection(selected: string, ids: Set<string>): string {
  return selected && ids.has(selected) ? selected : "";
}
