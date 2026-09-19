/**
 * Which of the two the person is in: their own Basepath, or an organization's.
 *
 * These are not two views of one thing. They are separate stores with a
 * security boundary between them, and the interface has to say so in words a
 * person reads rather than in a colour they have to have been told about. A
 * context switcher that looks like a filter teaches people that the boundary
 * is a filter, and then one day they act as if it is.
 *
 * So: separate URLs, separate navigation trees, a label on every screen, and
 * nothing carried across a switch.
 */

export type ContextKind = "personal" | "organization";

export type AppContext = {
  kind: ContextKind;
  /** The workspace this context is, which is also what the API is asked for. */
  workspaceId: string;
  name: string;
};

/** A screen, by the route segment that names it. */
export type Screen =
  | "home"
  | "today"
  | "goals"
  | "breakdown"
  | "timeline"
  | "memory"
  | "reflection"
  | "cycles"
  | "alignment"
  | "dashboard"
  | "goal-review"
  | "templates"
  | "members";

export type NavItem = {
  screen: Screen;
  /**
   * What the person reads. It differs between contexts on purpose: an
   * organization's front page is an overview of other people's work, and a
   * person's is their own day. Calling both "ホーム" would say they are the
   * same page seen from two angles.
   */
  label: string;
  /** The screen component to render. Shared where the screen really is. */
  page: string;
  icon: string;
};

/**
 * The person's own Basepath.
 *
 * No alignment, no dashboard, no members: those are questions about an
 * organization, and offering them here would imply this plan rolls up into
 * one. It does not.
 */
export const PERSONAL_NAV: NavItem[] = [
  { screen: "home", label: "ホーム", page: "ホーム", icon: "home" },
  { screen: "today", label: "今日の行動", page: "今日の行動", icon: "tasks" },
  { screen: "goals", label: "自分の目標", page: "目標マップ", icon: "tree" },
  { screen: "breakdown", label: "分解", page: "分解", icon: "tree" },
  {
    screen: "timeline",
    label: "タイムライン",
    page: "タイムライン",
    icon: "calendar",
  },
  { screen: "memory", label: "記憶", page: "記憶", icon: "bulb" },
  { screen: "reflection", label: "振り返り", page: "振り返り", icon: "book" },
  { screen: "cycles", label: "計画期間", page: "計画期間", icon: "calendar" },
  {
    screen: "templates",
    label: "テンプレート",
    page: "テンプレート",
    icon: "stack",
  },
  /*
   * The same screen as an organization's "メンバー", asking a different
   * question: not "who is in this organization" but "which ones am I in".
   *
   * It is here because it is also where a person joins or creates one, and a
   * personal menu with no way out of itself strands anyone who has only ever
   * had their own Basepath.
   */
  {
    screen: "members",
    label: "ワークスペース",
    page: "メンバー",
    icon: "users",
  },
];

/**
 * An organization's Basepath.
 *
 * No memory. Personal memory exists only in a person's own workspace, and a
 * menu item leading to an empty one would suggest it could be here.
 */
export const ORGANIZATION_NAV: NavItem[] = [
  { screen: "home", label: "概要", page: "ホーム", icon: "home" },
  { screen: "goals", label: "組織の目標", page: "目標マップ", icon: "tree" },
  { screen: "breakdown", label: "分解", page: "分解", icon: "tree" },
  {
    screen: "alignment",
    label: "アラインメント",
    page: "アラインメント",
    icon: "tree",
  },
  {
    screen: "dashboard",
    label: "ダッシュボード",
    page: "ダッシュボード",
    icon: "chart",
  },
  {
    screen: "goal-review",
    label: "目標レビュー",
    page: "目標レビュー",
    icon: "check",
  },
  { screen: "cycles", label: "計画期間", page: "計画期間", icon: "calendar" },
  {
    screen: "timeline",
    label: "タイムライン",
    page: "タイムライン",
    icon: "calendar",
  },
  { screen: "today", label: "今日の行動", page: "今日の行動", icon: "tasks" },
  { screen: "reflection", label: "振り返り", page: "振り返り", icon: "book" },
  {
    screen: "templates",
    label: "テンプレート",
    page: "テンプレート",
    icon: "stack",
  },
  { screen: "members", label: "メンバー", page: "メンバー", icon: "users" },
];

export function navFor(kind: ContextKind): NavItem[] {
  return kind === "personal" ? PERSONAL_NAV : ORGANIZATION_NAV;
}

/** The word for this context, for a heading, a breadcrumb or a screen reader. */
export function contextLabel(kind: ContextKind): string {
  return kind === "personal" ? "個人" : "組織";
}

/**
 * What this context is for, said in one sentence on an empty screen.
 *
 * The emptiness of a personal plan and the emptiness of an organization's mean
 * different things, and an empty state that reads the same in both is the one
 * place a person is most likely to be confused about where they are.
 */
export function emptyStateFor(kind: ContextKind): string {
  return kind === "personal"
    ? "ここはあなた個人のBasepathです。組織のメンバーには見えません。"
    : "ここは組織のBasepathです。あなた個人の目標や記憶はここにはありません。";
}

/** A workspace, as far as this module needs to know. */
export type WorkspaceLike = { id: string; name: string; scope: string };

export function contextOf(workspace: WorkspaceLike): AppContext {
  return {
    kind: workspace.scope === "個人" ? "personal" : "organization",
    workspaceId: workspace.id,
    name: workspace.name,
  };
}

/** Where a screen lives, in the URL. */
export function pathFor(context: AppContext, screen: Screen): string {
  return context.kind === "personal"
    ? `/personal/${screen}`
    : `/org/${context.workspaceId}/${screen}`;
}

export type ParsedRoute = {
  kind: ContextKind;
  /** Empty for a personal route: a person has exactly one of those. */
  orgId: string;
  screen: Screen;
};

const SCREENS = new Set<string>([
  ...PERSONAL_NAV.map((item) => item.screen),
  ...ORGANIZATION_NAV.map((item) => item.screen),
]);

/**
 * Reads a context route, or returns null when the path is not one.
 *
 * Null rather than a default, so a reload of an unknown path does not silently
 * land someone in a context they did not ask for.
 */
export function parsePath(pathname: string): ParsedRoute | null {
  const parts = pathname.split("/").filter(Boolean);
  if (parts[0] === "personal") {
    const screen = parts[1] ?? "home";
    return SCREENS.has(screen)
      ? { kind: "personal", orgId: "", screen: screen as Screen }
      : null;
  }
  if (parts[0] === "org" && parts[1]) {
    const screen = parts[2] ?? "home";
    if (!SCREENS.has(screen)) return null;
    return { kind: "organization", orgId: parts[1], screen: screen as Screen };
  }
  return null;
}

/**
 * Is this screen part of this context at all?
 *
 * Asked on every render rather than trusted from the URL: a deep link into
 * `/org/{id}/memory` is a link to something that does not exist there, and
 * showing an empty memory screen would answer the question wrongly.
 */
export function screenBelongs(kind: ContextKind, screen: Screen): boolean {
  return navFor(kind).some((item) => item.screen === screen);
}

/**
 * Where to land when a screen does not belong to the context being entered.
 *
 * Home, every time. Guessing at the nearest equivalent would carry the
 * person's place across a boundary the rest of this file exists to keep.
 */
export function landingFor(): Screen {
  return "home";
}

/** The nav entry for a screen in a context, or the home one as a fallback. */
export function navItemFor(kind: ContextKind, screen: Screen): NavItem {
  const items = navFor(kind);
  return items.find((item) => item.screen === screen) ?? items[0];
}

/**
 * What must not survive a context switch.
 *
 * The selected goal, the search, the open panel: each is a pointer into the
 * context being left. Carried across, a stale id either shows nothing or —
 * worse, if ids ever collide — shows the wrong thing.
 */
export type CarriedState = {
  selected: string;
  search: string;
  modal: string | null;
};

export function clearedOnSwitch(): CarriedState {
  return { selected: "", search: "", modal: null };
}

/**
 * Whether a context the person is in is still theirs to be in.
 *
 * An organization they were removed from stops being usable at once rather
 * than at the next reload: the workspace list is the authority, and a context
 * that is not in it no longer exists for them.
 */
export function stillAvailable(
  context: AppContext,
  workspaces: WorkspaceLike[],
): boolean {
  return workspaces.some((workspace) => workspace.id === context.workspaceId);
}
