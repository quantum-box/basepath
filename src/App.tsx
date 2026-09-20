import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import { Icon } from "./icons";
import { GoalMap, type MapItem } from "./GoalMap";
import {
  templates,
  scopeClass,
  type Goal,
  type Scope,
  type Task,
  type Initiative,
} from "./data";

import { useWorkspace } from "./useWorkspace";
import {
  ApiError,
  localDate,
  dateLabel,
  uiId,
  itemPath,
  request,
  type Item,
  type RecordEntry,
} from "./api";
import { FieldIntegration } from "./FieldIntegration";
import { McpConnections } from "./McpConnections";
import { ChangeApproval, changeRouteFromPath } from "./ChangeApproval";
import { OAuthConsent, isConsentPath } from "./OAuthConsent";
import { WorkspaceMembers } from "./WorkspaceMembers";
import { OnboardingWizard } from "./OnboardingWizard";
import { AiSuggestions } from "./AiSuggestions";
import { CalendarView } from "./CalendarView";
import { WeeklyReviewScreen } from "./WeeklyReview";
import { PlanningScreen } from "./PlanningScreen";
import { AlignmentScreen } from "./AlignmentScreen";
import { BreakdownScreen } from "./BreakdownScreen";
import type { BreakdownNode } from "./shared/breakdownView";
import {
  contextLabel,
  contextOf,
  emptyStateFor,
  landingFor,
  navFor,
  navItemFor,
  parsePath,
  pathFor,
  resolvePersonalWorkspace,
  screenBelongs,
  stillAvailable,
  tenantReturnWithSelection,
  type AppContext,
  type Screen,
} from "./shared/appContext";
import { DashboardScreen } from "./DashboardScreen";
import { ReviewScreen } from "./ReviewScreen";
import { MemoryScreen } from "./MemoryScreen";
import {
  MemoEditor,
  ItemEditor,
  MetricEditor,
  RelationsEditor,
  StorageSettings,
  ItemList,
  Evaluation,
} from "./Features";

type ModalState =
  | { kind: "template"; template: string }
  | { kind: "task" }
  | { kind: "initiative" }
  | {
      kind: "createItem";
      workspaceId: string;
      parentId: string | null;
      relation: "child" | "sibling";
      parentTitle: string;
      siblingTitle?: string;
      parentKind?: string;
    }
  | { kind: "members" }
  | { kind: "settings" }
  | { kind: "learnings" }
  | { kind: "activity" }
  | { kind: "editGoal" }
  | { kind: "initiativeDetail"; id: string }
  | { kind: "metrics" }
  | { kind: "relations" }
  | { kind: "aiSuggestions" }
  | null;
const navigation = [
  { label: "ホーム", icon: "home" },
  { label: "目標マップ", icon: "tree" },
  { label: "分解", icon: "tree" },
  { label: "タイムライン", icon: "calendar" },
  { label: "今日の行動", icon: "tasks" },
  { label: "振り返り", icon: "book" },
  { label: "計画期間", icon: "calendar" },
  { label: "アラインメント", icon: "tree" },
  { label: "ダッシュボード", icon: "chart" },
  { label: "目標レビュー", icon: "check" },
  { label: "記憶", icon: "bulb" },
  { label: "テンプレート", icon: "stack" },
  { label: "メンバー", icon: "users" },
] as const;
type NavigationLabel = (typeof navigation)[number]["label"];

const navigationRoutes: Record<NavigationLabel, string> = {
  ホーム: "home",
  目標マップ: "goals",
  分解: "breakdown",
  タイムライン: "timeline",
  今日の行動: "today",
  振り返り: "reflection",
  計画期間: "cycles",
  アラインメント: "alignment",
  ダッシュボード: "dashboard",
  目標レビュー: "goal-review",
  記憶: "memory",
  テンプレート: "templates",
  メンバー: "members",
};

const navigationDescriptions: Record<NavigationLabel, string> = {
  ホーム: "目標・計画・行動・学びを、ひとつの場所で見渡せます。",
  目標マップ: "目標と取り組みのつながりを見ながら、次の一歩を整えます。",
  分解: "大きな目標を、実行できるところまで何段階でも分解します。",
  タイムライン: "これからの予定と目標の進み方を、時間軸で確認します。",
  今日の行動: "今日やることに集中して、小さな前進を積み重ねます。",
  振り返り: "できたことや気づきを残し、次の行動につなげます。",
  計画期間: "四半期・月・週の区切りで計画を運用し、次の期間へ引き継ぎます。",
  アラインメント: "誰の目標が、どの目標に効いているかをたどります。",
  ダッシュボード: "実施・指標・自己評価・状況を混ぜずに並べて確認します。",
  目標レビュー: "チェックインの有無と内容から、話すべき目標を見つけます。",
  記憶: "あなた自身の記憶。共有ワークスペースへは移動も同期もされません。",
  テンプレート: "目的に合う型を選んで、新しい目標をすぐに始められます。",
  メンバー: "一緒に取り組むメンバーと、チームの状況を確認します。",
};

/**
 * Which screen the URL names, in either form.
 *
 * The context path is the real one. The old `#/goals` hash is still read so
 * that a link someone saved before this existed still opens the screen they
 * meant — it just opens it in whichever context they are in.
 */
function screenFromUrl(): Screen | null {
  const route = parsePath(window.location.pathname);
  if (route) return route.screen;
  const hash = window.location.hash.replace(/^#\/?/, "");
  const known = navigation.find(
    (item) => navigationRoutes[item.label] === hash,
  );
  return known ? (navigationRoutes[known.label] as Screen) : null;
}

const screenPaths = new Set(["/login", "/tenants"]);
function replaceScreenUrl(
  pathname: string,
  tenantId = "",
  clearContext = pathname !== "/",
) {
  const url = new URL(window.location.href);
  url.pathname = pathname;
  if (tenantId && pathname !== "/login")
    url.searchParams.set("tenant_id", tenantId);
  else url.searchParams.delete("tenant_id");
  // The unavailable-home action opts into clearing stale pointers. Normal
  // tenant selection briefly uses `/` before the authenticated route effect
  // canonicalizes it, so preserve its transition state until then.
  if (clearContext)
    for (const key of ["item", "scope", "view", "workspace"])
      url.searchParams.delete(key);
  window.history.replaceState({}, "", url);
}
function tenantReturnUrl(): string | null {
  const url = new URL(window.location.href);
  if (!parsePath(url.pathname)) return null;
  return `${url.pathname}${url.search}${url.hash}`;
}
function replaceTenantSelectionUrl(tenantId: string, returnTo?: string | null) {
  const url = new URL(window.location.href);
  url.pathname = "/tenants";
  url.search = "";
  if (tenantId) url.searchParams.set("tenant_id", tenantId);
  if (returnTo) url.searchParams.set("return_to", returnTo);
  url.hash = "";
  window.history.replaceState({}, "", url);
}
function replaceLoginUrl(returnTo?: string | null) {
  const url = new URL(window.location.href);
  url.pathname = "/login";
  url.search = "";
  if (returnTo) url.searchParams.set("return_to", returnTo);
  url.hash = "";
  window.history.replaceState({}, "", url);
}
function safeTenantReturn(value: string | null): string | null {
  if (!value) return null;
  try {
    const url = new URL(value, window.location.origin);
    if (url.origin !== window.location.origin || !parsePath(url.pathname))
      return null;
    return `${url.pathname}${url.search}${url.hash}`;
  } catch {
    return null;
  }
}
function restoreTenantReturn(value: string, tenantId: string): URL {
  return new URL(
    tenantReturnWithSelection(value, tenantId),
    window.location.origin,
  );
}
function Badge({ scope }: { scope: Scope }) {
  return <span className={`scope-badge ${scopeClass[scope]}`}>{scope}</span>;
}
function Avatar({
  male = false,
  size = 32,
  name,
}: {
  male?: boolean;
  size?: number;
  name?: string;
}) {
  if (name)
    return (
      <span
        className="avatar member-avatar"
        style={{ width: size, height: size }}
        aria-label={name}
      >
        {Array.from(name)[0]}
      </span>
    );
  return (
    <img
      className="avatar"
      src={male ? "/assets/kenta.png" : "/assets/haruka.png"}
      width={size}
      height={size}
      alt={male ? "佐藤 健太" : "やまだ はるか"}
    />
  );
}
function Progress({
  value,
  color = "purple",
}: {
  value: number | null;
  color?: string;
}) {
  if (value === null) return <span className="empty-value">評価未設定</span>;
  return (
    <div className={`progress ${color}`}>
      <span
        className="progress-track"
        role="progressbar"
        aria-valuenow={value}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="目標の進捗"
      >
        <span style={{ width: `${value}%` }} />
      </span>
      <strong>{value}%</strong>
    </div>
  );
}
function TextLink({
  children = "すべて見る",
  onClick,
}: {
  children?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button className="text-link" onClick={onClick}>
      {children}
      <Icon name="arrow" size={14} />
    </button>
  );
}
function TachyonLogin({ onSignedIn }: { onSignedIn: () => Promise<unknown> }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (pending) return;
    setPending(true);
    setError("");
    try {
      await request("POST", "/auth/login", { username, password });
      setPassword("");
      await onSignedIn();
    } catch (failure) {
      setPassword("");
      setError(
        failure instanceof ApiError
          ? failure.message
          : "ログインできませんでした。時間をおいて再試行してください。",
      );
    } finally {
      setPending(false);
    }
  }
  return (
    <main className="auth-page">
      <div className="panel auth-card">
        <img src="/assets/pathbase-mark.png" alt="" width="55" />
        <div>
          <h1>PathBase</h1>
          <p>やりたいことを、動ける形に。</p>
        </div>
        <form className="auth-form" onSubmit={submit}>
          <label>
            Tachyonユーザー名またはメールアドレス
            <input
              autoComplete="username"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              required
            />
          </label>
          <label>
            パスワード
            <input
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              required
            />
          </label>
          {error && (
            <p className="auth-error" role="alert">
              {error}
            </p>
          )}
          <button className="primary-button" disabled={pending} type="submit">
            {pending ? "ログイン中…" : "Tachyonでログイン"}
          </button>
        </form>
        <small>認証情報はPathBaseに保存されず、Tachyonで認証されます。</small>
      </div>
    </main>
  );
}
type TachyonTenant = { id: string; name: string };
function TachyonTenantSelection({
  onSelected,
  onBack,
  initialTenantId,
  onTenantChange,
  backLabel,
  backPendingLabel,
}: {
  onSelected: (tenantId: string) => Promise<unknown>;
  onBack: () => Promise<unknown> | void;
  initialTenantId: string;
  onTenantChange: (tenantId: string) => void;
  backLabel: string;
  backPendingLabel: string;
}) {
  const [tenants, setTenants] = useState<TachyonTenant[]>([]);
  const [tenantId, setTenantId] = useState("");
  const [pending, setPending] = useState(true);
  const [goingBack, setGoingBack] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    void request<{
      tenants: TachyonTenant[];
      selected_tenant_id: string | null;
    }>("GET", "/v1/tenants")
      .then((result) => {
        if (!active) return;
        const nextTenantId = result.tenants.some(
          (tenant) => tenant.id === initialTenantId,
        )
          ? initialTenantId
          : result.selected_tenant_id || result.tenants[0]?.id || "";
        setTenants(result.tenants);
        setTenantId(nextTenantId);
        onTenantChange(nextTenantId);
        if (!result.tenants.length)
          setError("利用できるTachyonテナントがありません。");
      })
      .catch((failure) => {
        if (!active) return;
        setError(
          failure instanceof ApiError
            ? failure.message
            : "Tachyonテナントを取得できませんでした。",
        );
      })
      .finally(() => {
        if (active) setPending(false);
      });
    return () => {
      active = false;
    };
  }, []);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (pending || !tenantId) return;
    setPending(true);
    setError("");
    try {
      await request("POST", "/v1/tenant-selection", {
        tenant_id: tenantId,
      });
      await onSelected(tenantId);
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "テナントを選択できませんでした。",
      );
    } finally {
      setPending(false);
    }
  }
  async function goBack() {
    if (pending) return;
    setPending(true);
    setGoingBack(true);
    setError("");
    try {
      await onBack();
    } catch (failure) {
      setError(
        failure instanceof ApiError
          ? failure.message
          : "前の画面に戻れませんでした。時間をおいて再試行してください。",
      );
    } finally {
      setGoingBack(false);
      setPending(false);
    }
  }
  return (
    <main className="auth-page">
      <div className="panel auth-card">
        <img src="/assets/pathbase-mark.png" alt="" width="55" />
        <div>
          <h1>利用するテナント</h1>
          <p>PathBaseで操作するTachyonテナントを選択してください。</p>
        </div>
        <form className="auth-form" onSubmit={submit}>
          <label>
            Tachyonテナント
            <select
              value={tenantId}
              onChange={(event) => {
                setTenantId(event.target.value);
                onTenantChange(event.target.value);
              }}
              disabled={pending || !tenants.length}
              required
            >
              {tenants.map((tenant) => (
                <option key={tenant.id} value={tenant.id}>
                  {tenant.name}
                </option>
              ))}
            </select>
          </label>
          {error && (
            <p className="auth-error" role="alert">
              {error}
            </p>
          )}
          <button
            className="primary-button"
            disabled={pending || !tenantId}
            type="submit"
          >
            {pending ? "読み込み中…" : "このテナントで始める"}
          </button>
          <button
            className="secondary-button"
            disabled={pending}
            type="button"
            onClick={() => void goBack()}
          >
            <Icon name="left" size={14} />
            {goingBack ? backPendingLabel : backLabel}
          </button>
        </form>
        <small>選択後も、表示できる内容はPathBaseの権限で制御されます。</small>
      </div>
    </main>
  );
}

export function App() {
  const store = useWorkspace();
  const route = new URLSearchParams(window.location.search);
  const [tenantId, setTenantId] = useState(route.get("tenant_id") || "");
  const [selectedId, setSelectedId] = useState(
    route.get("item") || "team~event",
  );
  const [scope, setScope] = useState<Scope | "すべて">(
    (route.get("scope") as Scope) || "すべて",
  );
  // A context deep link names the workspace in the path, and that is the
  // authority on load: `/org/{id}/alignment` has to open that organization,
  // not whichever workspace happened to come back first.
  const [workspaceId, setWorkspaceId] = useState(
    () =>
      parsePath(window.location.pathname)?.orgId ||
      route.get("workspace") ||
      "",
  );
  // The screen, by route segment. The context it belongs to comes from the
  // workspace, so this is only half of "where am I" — which is the point: a
  // screen name alone never decides which side of the boundary it is on.
  const [screen, setScreen] = useState<Screen>(
    () => screenFromUrl() ?? landingFor(),
  );
  const [activeTab, setActiveTab] = useState(
    route.get("view") || "タイムライン",
  );
  const [quarter, setQuarter] = useState(0);
  const [modal, setModal] = useState<ModalState>(null);
  const [toast, setToast] = useState("");
  const [notifications, setNotifications] = useState(false);
  const [editBase, setEditBase] = useState<number | null>(null);
  const [menu, setMenu] = useState(false);
  const [sidebar, setSidebar] = useState(false);
  const [selectingTenant, setSelectingTenant] = useState(
    window.location.pathname === "/tenants",
  );
  const tenantReturnRef = useRef<string | null>(
    safeTenantReturn(new URLSearchParams(window.location.search).get("return_to")),
  );
  // A deep link from the conversation lands here. Approval needs this origin
  // and this session, so the link goes to a real screen rather than back into
  // the AI host.
  const [changeRoute, setChangeRoute] = useState(() =>
    changeRouteFromPath(window.location.pathname),
  );
  // An AI host sends the person here to authorize a connection. Like approval,
  // it needs this origin and this session, so it is a screen rather than
  // something the host could do on its own.
  // Captured on the first render, before any effect can rewrite the URL: the
  // sign-in and tenant screens replace the location, and the authorization
  // request lives in its query string. Someone who has to sign in first must
  // still end up back at the connection they were asked about.
  const [consenting] = useState(() =>
    isConsentPath(window.location.pathname) ? window.location.search : null,
  );
  const [search, setSearch] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const [reflection, setReflection] = useState("");
  const [savedReflection, setSavedReflection] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const mainRef = useRef<HTMLElement>(null);
  const compact = store.settings.compact;
  const allItems = store.snapshots.flatMap((s) => s.items);
  const allRelations = store.snapshots.flatMap((s) => s.relations);
  const allRecords = store.snapshots.flatMap((s) => s.records);
  const allNotifications = store.snapshots
    .flatMap((snapshot) => snapshot.notifications || [])
    .sort((a, b) => b.created_at.localeCompare(a.created_at));
  const raw = (id: string) => allItems.find((i) => uiId(i) === id);
  const scopeOf = (w: string): Scope =>
    store.workspaces.find((s) => s.id === w)?.scope || "個人";
  const requestedRoute = parsePath(window.location.pathname);
  const requestedOrganizationId =
    requestedRoute?.kind === "organization" ? requestedRoute.orgId : "";
  const requestedPersonalWorkspaceId =
    requestedRoute?.kind === "personal"
      ? new URLSearchParams(window.location.search).get("workspace") || ""
      : "";
  const requestedPersonalRoute = requestedRoute?.kind === "personal";
  const currentWorkspace =
    requestedOrganizationId
      ? store.workspaces.find((w) => w.id === requestedOrganizationId)
      : requestedPersonalRoute
        ? requestedPersonalWorkspaceId
          ? store.workspaces.find(
              (w) =>
                w.id === requestedPersonalWorkspaceId && w.scope === "個人",
            )
          : store.workspaces.find((w) => w.scope === "個人")
        : store.workspaces.find((w) => w.id === workspaceId) ||
          store.workspaces[0];
  const contextUnavailable =
    !store.loading &&
    ((!!requestedOrganizationId && !currentWorkspace) ||
      (!!requestedPersonalWorkspaceId && !currentWorkspace));
  /**
   * Where the person is: their own Basepath, or an organization's.
   *
   * Derived from the workspace rather than stored beside it, so the two can
   * never disagree. A context that says "personal" over an organization's data
   * is the exact failure the separation exists to prevent.
   */
  const context: AppContext | null = currentWorkspace
    ? contextOf(currentWorkspace)
    : null;
  const contextKind = context?.kind ?? "personal";
  /** The person's own workspace, which they always have exactly one of. */
  const personalWorkspaceId = () =>
    resolvePersonalWorkspace("", store.workspaces);
  const navItems = navFor(contextKind);
  // The screen that is actually shown. A deep link into a screen this context
  // does not have lands on its home instead of rendering an empty one, because
  // an empty memory screen in an organization answers "is my memory here?"
  // with a maybe.
  const shownScreen: Screen = screenBelongs(contextKind, screen)
    ? screen
    : landingFor();
  const navItem = navItemFor(contextKind, shownScreen);
  const activeNav = navItem.page as NavigationLabel;
  const currentSnapshot = store.snapshots.find(
    (snapshot) => snapshot.workspace_id === currentWorkspace?.id,
  );
  const workspace = currentWorkspace?.name || "個人";
  const canWrite = (w?: string) =>
    store.workspaces.some((entry) => entry.id === w && entry.role !== "viewer");
  const canEditItem = (item?: Item) => {
    if (!item || !canWrite(item.workspace_id)) return false;
    const owner = item.fields.owner;
    if (owner?.kind !== "person" || !owner.id || owner.id === store.me.id) {
      return true;
    }
    return store.workspaces.some(
      (entry) => entry.id === item.workspace_id && entry.role === "owner",
    );
  };
  const canEditMapItem = (id: string) => canEditItem(raw(id));
  const workspaceMatches = (id: string) =>
    scope === "すべて" ||
    (scopeOf(id) === scope &&
      (currentWorkspace?.scope !== scope || currentWorkspace.id === id));
  const reviewDraftKey = `pathbase:review:${store.me.id}:${currentWorkspace?.id || ""}`;
  useEffect(() => {
    setReflection(
      store.me.id && currentWorkspace
        ? localStorage.getItem(reviewDraftKey) || ""
        : "",
    );
    setSavedReflection("");
  }, [reviewDraftKey]);
  const today = localDate(store.settings.timezone);
  const sevenDaysFromToday = localDate(
    store.settings.timezone,
    new Date(Date.now() + 7 * 86400000),
  );
  const visibleItems = allItems.filter(
    (i) => !i.archived_at && workspaceMatches(i.workspace_id),
  );
  const linkedItem = raw(selectedId);
  const autoOpenedActionRef = useRef<string | null>(null);
  const autoOpenRouteKey = `${window.location.pathname}${window.location.search}`;
  useEffect(() => {
    if (shownScreen !== "today") {
      autoOpenedActionRef.current = null;
      return;
    }
    if (
      linkedItem?.kind === "action" &&
      linkedItem.workspace_id === currentWorkspace?.id &&
      !linkedItem.archived_at &&
      !modal &&
      autoOpenedActionRef.current !== autoOpenRouteKey
    ) {
      autoOpenedActionRef.current = autoOpenRouteKey;
      setModal({ kind: "initiativeDetail", id: selectedId });
    }
  }, [autoOpenRouteKey, linkedItem?.kind, modal, selectedId, shownScreen]);
  const isGoalKind = (item: Item) =>
    ["outcome", "idea", "milestone"].includes(item.kind);
  const partOfTarget = (item: Item) => {
    const relation = allRelations.find(
      (r) =>
        r.workspace_id === item.workspace_id &&
        r.source_id === item.id &&
        r.type === "part_of",
    );
    return relation
      ? allItems.find(
          (candidate) =>
            candidate.workspace_id === item.workspace_id &&
            candidate.id === relation.target_id,
        )
      : undefined;
  };
  const goals: Goal[] = visibleItems.filter(isGoalKind).map((i) => ({
    id: uiId(i),
    parentId: (() => {
      const parent = partOfTarget(i);
      return parent && isGoalKind(parent) ? uiId(parent) : undefined;
    })(),
    title: i.title,
    subtitle: i.fields.subtitle || "",
    scope: scopeOf(i.workspace_id),
    icon: i.fields.icon || "target",
    purpose: i.description,
    progress: i.fields.self_assessment ?? null,
    next:
      allItems.find(
        (a) =>
          a.id === i.fields.next_action_id && a.workspace_id === i.workspace_id,
      )?.title || "",
    memo: i.fields.memo || "",
    startDate: i.start_date,
    dueDate: i.due_date,
    state: i.state,
  }));
  const initiatives: Initiative[] = visibleItems
    .filter((i) => i.kind === "initiative")
    .map((i) => {
      const parent = partOfTarget(i);
      let owner = parent;
      const seen = new Set<string>();
      while (owner && owner.kind === "initiative" && !seen.has(owner.id)) {
        seen.add(owner.id);
        owner = partOfTarget(owner);
      }
      return {
        id: uiId(i),
        goalId: owner && isGoalKind(owner) ? uiId(owner) : "",
        parentId: parent?.kind === "initiative" ? uiId(parent) : undefined,
        title: i.title,
        icon: i.fields.icon || "flag",
        progress: i.fields.self_assessment ?? null,
      };
    });
  // Goal Map is a projection of the complete `part_of` graph.  Do not infer
  // hierarchy from item kinds: milestones and actions can be children of any
  // item, and their direct parent is the only structural authority.
  const mapItems: MapItem[] = visibleItems
    .filter((item) =>
      ["outcome", "idea", "initiative", "milestone", "action"].includes(
        item.kind,
      ),
    )
    .map((item) => {
      const parent = partOfTarget(item);
      return {
        id: uiId(item),
        workspaceId: item.workspace_id,
        title: item.title,
        kind: item.kind,
        parentId: parent ? uiId(parent) : undefined,
        position:
          allRelations.find(
            (relation) =>
              relation.workspace_id === item.workspace_id &&
              relation.source_id === item.id &&
              relation.target_id === parent?.id &&
              relation.type === "part_of",
          )?.position ?? null,
        scope: scopeOf(item.workspace_id),
        icon:
          item.fields.icon ||
          (item.kind === "action"
            ? "rocket"
            : item.kind === "milestone"
              ? "flag"
              : item.kind === "initiative"
                ? "flag"
                : "target"),
        subtitle: item.fields.subtitle || undefined,
        progress: item.fields.self_assessment ?? null,
      };
    });
  const doneFor = (item: Item, date = today) =>
    item.fields.recurrence
      ? [...allRecords]
          .reverse()
          .find(
            (r) =>
              r.workspace_id === item.workspace_id &&
              r.occurrence_key === `${item.id}:${date}`,
          )?.record_type === "completion"
      : item.state === "done";
  const tasks: Task[] = visibleItems
    .filter(
      (i) =>
        i.kind === "action" &&
        !["paused", "abandoned", "draft"].includes(i.state) &&
        ((linkedItem?.kind === "action" &&
          linkedItem.workspace_id === currentWorkspace?.id &&
          i.id === linkedItem.id) ||
          (i.fields.recurrence
            ? (!i.start_date || i.start_date <= today) &&
              (!i.due_date || i.due_date >= today) &&
              (i.fields.recurrence.mode === "period_quota" ||
                i.fields.recurrence.weekdays.includes(
                  (new Date(today + "T12:00:00").getDay() + 6) % 7,
                ))
            : !i.scheduled_date ||
              i.scheduled_date === today ||
              (!!i.due_date && i.due_date <= sevenDaysFromToday))),
    )
    .map((i) => ({
      id: uiId(i),
      title: i.title,
      scope: scopeOf(i.workspace_id),
      time: i.scheduled_time || "",
      done: doneFor(i),
      date: i.scheduled_date,
      recurring: !!i.fields.recurrence,
      state: i.state,
      dueDate: i.due_date,
      assigneeId: i.fields.assignee_id,
      priority: i.fields.priority,
      dueStatus: i.due_date
        ? i.due_date < today
          ? "overdue"
          : i.due_date === today
            ? "today"
            : i.due_date <= sevenDaysFromToday
              ? "soon"
              : null
        : null,
    }));
  // An item deep link must not silently select the first goal when the target
  // is an action or initiative. Those links are routed to their own screen by
  // the MCP contract; keeping the detail empty here also protects old links.
  const selected =
    linkedItem && !isGoalKind(linkedItem)
      ? undefined
      : goals.find((g) => g.id === selectedId) ?? goals[0];
  const selectedRaw = selected ? raw(selected.id) : undefined;
  const nextAction =
    selectedRaw &&
    allItems.find(
      (i) =>
        i.workspace_id === selectedRaw.workspace_id &&
        i.id === selectedRaw.fields.next_action_id &&
        !i.archived_at,
    );
  const records = allRecords
    .filter((r) =>
      scope === "すべて"
        ? r.workspace_id === currentWorkspace?.id
        : workspaceMatches(r.workspace_id),
    )
    .sort((a, b) => b.created_at.localeCompare(a.created_at));
  const learnings = records
    .filter(
      (r) =>
        ["learning", "review", "checkin"].includes(r.record_type) && r.body,
    )
    .slice(0, 4)
    .map((r) => r.body);
  const activity = allRecords
    .filter(
      (r) =>
        selectedRaw &&
        r.workspace_id === selectedRaw.workspace_id &&
        r.item_ids.some(
          (id) =>
            id === selectedRaw.id || id === selectedRaw.fields.next_action_id,
        ),
    )
    .sort((a, b) => b.created_at.localeCompare(a.created_at));
  const unreadNotifications = allNotifications.filter(
    (entry) => !entry.read_at,
  );
  const unread = store.invitations.length > 0 || unreadNotifications.length > 0;
  function openNotifications() {
    setNotifications(!notifications);
    if (notifications || !unreadNotifications.length) return;
    void store.run(() =>
      Promise.all(
        unreadNotifications.map((entry) =>
          store.write(
            "PATCH",
            `/v1/workspaces/${entry.workspace_id}/notifications/${entry.id}/read`,
            { read: true },
          ),
        ),
      ),
    );
  }
  const notify = useCallback((message: string) => setToast(message), []);
  function routeTo(changes: Record<string, string>) {
    const url = new URL(window.location.href);
    url.pathname = "/";
    if (tenantId) url.searchParams.set("tenant_id", tenantId);
    for (const [k, v] of Object.entries(changes)) url.searchParams.set(k, v);
    window.history.pushState({}, "", url);
  }
  const selectGoal = useCallback((id: string) => {
    setSelectedId(id);
    routeTo({ item: id });
  }, []);
  const openInitiative = useCallback(
    (id: string) => setModal({ kind: "initiativeDetail", id }),
    [],
  );
  const openCreateRelative = useCallback(
    (id: string, relation: "child" | "sibling") => {
      const item = raw(id);
      if (!item) return;
      const parent = relation === "child" ? item : partOfTarget(item);
      setModal({
        kind: "createItem",
        workspaceId: item.workspace_id,
        parentId: parent?.id ?? null,
        relation,
        parentTitle: parent?.title ?? "ルート",
        siblingTitle: relation === "sibling" ? item.title : undefined,
        parentKind: parent?.kind,
      });
    },
    [allItems],
  );
  const editTreeItem = useCallback((id: string) => {
    const item = raw(id);
    if (!item) return;
    setSelectedId(id);
    if (item.kind === "action" || item.kind === "initiative" || item.kind === "milestone") {
      setModal({ kind: "initiativeDetail", id });
    } else {
      setModal({ kind: "editGoal" });
    }
  }, [allItems]);
  const moveTreeItem = useCallback((id: string) => {
    const item = raw(id);
    if (!item || !canWrite(item.workspace_id)) return;
    const current = partOfTarget(item);
    const parentId = window.prompt(
      "新しい親（表示ID workspace~item または項目ID。ルートに戻す場合は空欄）",
      current ? uiId(current) : "",
    );
    if (parentId === null) return;
    const target = parentId
      ? allItems.find(
          (entry) =>
            entry.workspace_id === item.workspace_id &&
            (entry.id === parentId || uiId(entry) === parentId),
        )
      : undefined;
    if (parentId && !target) { notify("同じワークスペースの項目IDを指定してください"); return; }
    void store.run(() => store.write("POST", `/v1/workspaces/${item.workspace_id}/items/${item.id}/reparent`, { parent_id: target?.id ?? null, expected_version: item.version }), () => notify("親を変更しました"));
  }, [allItems, canWrite, notify, raw, store]);
  const reorderTreeItem = useCallback((id: string, delta: -1 | 1) => {
    const item = raw(id);
    if (!item || !canWrite(item.workspace_id)) return;
    const parent = partOfTarget(item);
    if (!parent || !canEditItem(parent)) return;
    const siblings = allItems.filter((entry) => entry.workspace_id === item.workspace_id && partOfTarget(entry)?.id === parent.id && ["outcome", "idea", "initiative", "milestone", "action"].includes(entry.kind));
    siblings.sort((a, b) => {
      const pa = allRelations.find((relation) => relation.workspace_id === item.workspace_id && relation.source_id === a.id && relation.target_id === parent.id && relation.type === "part_of")?.position ?? Number.MAX_SAFE_INTEGER;
      const pb = allRelations.find((relation) => relation.workspace_id === item.workspace_id && relation.source_id === b.id && relation.target_id === parent.id && relation.type === "part_of")?.position ?? Number.MAX_SAFE_INTEGER;
      const ai = allItems.findIndex(
        (entry) => entry.workspace_id === item.workspace_id && entry.id === a.id,
      );
      const bi = allItems.findIndex(
        (entry) => entry.workspace_id === item.workspace_id && entry.id === b.id,
      );
      return pa - pb || ai - bi;
    });
    const index = siblings.findIndex((entry) => entry.id === item.id);
    if (index < 0) return;
    // Archived children remain part of the persisted order, but are not
    // visible siblings in the map. Move to the nearest visible sibling and
    // swap those two slots so hidden entries keep their relative position.
    const visible = siblings.filter((entry) => !entry.archived_at);
    const visibleIndex = visible.findIndex((entry) => entry.id === item.id);
    const adjacent = visible[visibleIndex + delta];
    if (!adjacent) return;
    const next = siblings.findIndex((entry) => entry.id === adjacent.id);
    [siblings[index], siblings[next]] = [siblings[next], siblings[index]];
    void store.run(() => store.write("POST", `/v1/workspaces/${item.workspace_id}/items/${parent.id}/children`, { order: siblings.map((entry) => entry.id) }), () => notify("並び順を変更しました"));
  }, [allItems, allRelations, canEditItem, canWrite, notify, raw, store]);
  useEffect(() => {
    if (!toast) return;
    const timer = setTimeout(() => setToast(""), 3200);
    return () => clearTimeout(timer);
  }, [toast]);
  useEffect(() => {
    function handle(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        searchRef.current?.focus();
        setSearchOpen(true);
      }
      if (e.key === "Escape") {
        setMenu(false);
        setNotifications(false);
        setSearchOpen(false);
        setSidebar(false);
      }
    }
    function back() {
      const p = new URLSearchParams(window.location.search);
      setSelectedId(p.get("item") || "");
      setScope((p.get("scope") as Scope) || "すべて");
      setActiveTab(p.get("view") || "タイムライン");
      setScreen(screenFromUrl() ?? landingFor());
      // Going back across a context boundary changes context. The URL is the
      // authority here, not what was on screen a moment ago — otherwise the
      // back button leaves someone in one context reading another's menu.
      const route = parsePath(window.location.pathname);
      if (route?.kind === "organization") setWorkspaceId(route.orgId);
      if (route?.kind === "personal") setWorkspaceId(personalWorkspaceId());
      setSidebar(false);
      window.scrollTo({ top: 0, behavior: "smooth" });
    }
    window.addEventListener("keydown", handle);
    window.addEventListener("popstate", back);
    return () => {
      window.removeEventListener("keydown", handle);
      window.removeEventListener("popstate", back);
    };
  }, []);
  useEffect(() => {
    if (store.error?.code === "UNAUTHENTICATED") {
      // `/login` is intentionally not a context route, so tenantReturnUrl()
      // is null there. Preserve a return target captured before the auth gate
      // (and recover one already present in the login URL) instead of dropping
      // the deep link on a second render.
      tenantReturnRef.current =
        tenantReturnRef.current ??
        safeTenantReturn(
          new URLSearchParams(window.location.search).get("return_to"),
        ) ??
        tenantReturnUrl();
      replaceLoginUrl(tenantReturnRef.current);
      return;
    }
    if (store.error?.code === "TENANT_SELECTION_REQUIRED") {
      const requestedTenantId =
        tenantId ||
        new URLSearchParams(window.location.search).get("tenant_id") ||
        "";
      tenantReturnRef.current = tenantReturnRef.current ?? tenantReturnUrl();
      setSelectingTenant(true);
      replaceTenantSelectionUrl(requestedTenantId, tenantReturnRef.current);
      return;
    }
    if (
      !store.loading &&
      store.me.mode !== "tachyon" &&
      screenPaths.has(window.location.pathname)
    ) {
      setSelectingTenant(false);
      replaceScreenUrl("/");
    }
  }, [store.error?.code, store.loading, store.me.mode, tenantId]);
  useEffect(() => {
    if (
      store.loading ||
      store.me.mode !== "tachyon" ||
      selectingTenant ||
      store.error?.code === "UNAUTHENTICATED" ||
      store.error?.code === "TENANT_SELECTION_REQUIRED"
    )
      return;
    let active = true;
    void request<{
      tenants: TachyonTenant[];
      selected_tenant_id: string | null;
    }>("GET", "/v1/tenants").then(
      (result) => {
        if (!active) return;
        const returnTo = tenantReturnRef.current;
        const returnTenantId = returnTo
          ? new URL(returnTo, window.location.origin).searchParams.get(
              "tenant_id",
            )
          : null;
        const requestedTenantId = new URLSearchParams(
          window.location.search,
        ).get("tenant_id") || returnTenantId;
        // A conversation deep link may name a tenant other than the one in
        // this browser session. Do not silently replace that target with the
        // current tenant: move through the explicit selector, whose URL is
        // the safe boundary and whose membership check is authoritative.
        if (
          requestedTenantId &&
          requestedTenantId !== result.selected_tenant_id &&
          result.tenants.some((tenant) => tenant.id === requestedTenantId)
        ) {
          tenantReturnRef.current = tenantReturnUrl();
          setTenantId(requestedTenantId);
          setSelectingTenant(true);
          replaceTenantSelectionUrl(
            requestedTenantId,
            tenantReturnRef.current,
          );
          return;
        }
        if (!result.selected_tenant_id) return;
        setTenantId(result.selected_tenant_id);
        if (returnTo && returnTenantId === result.selected_tenant_id) {
          const restored = restoreTenantReturn(returnTo, result.selected_tenant_id);
          const restoredRoute = parsePath(restored.pathname);
          tenantReturnRef.current = null;
          setScreen(restoredRoute?.screen ?? "home");
          setWorkspaceId(restoredRoute?.orgId ?? "");
          setSelectedId(restored.searchParams.get("item") || "");
          window.history.replaceState({}, "", restored);
          return;
        }
        const route = parsePath(window.location.pathname);
        if (route) {
          setWorkspaceId(route.kind === "organization" ? route.orgId : "");
          setScreen(route.screen);
          setSelectedId(
            new URLSearchParams(window.location.search).get("item") || "",
          );
          return;
        }
        replaceScreenUrl("/personal/home", result.selected_tenant_id);
      },
      () => {},
    );
    return () => {
      active = false;
    };
  }, [
    selectingTenant,
    store.error?.code,
    store.loading,
    store.me.id,
    store.me.mode,
  ]);

  useEffect(() => {
    // The tab title says the context too, so a person with both open in two
    // windows can tell them apart without switching to either.
    const where = contextLabel(contextKind);
    document.title =
      shownScreen === "home"
        ? `${where} | PathBase`
        : `${navItem.label}（${where}） | PathBase`;
  }, [contextKind, navItem.label, shownScreen]);

  /**
   * A context that stopped being the person's.
   *
   * Removed from an organization, the workspace disappears from the list and
   * this drops them into their own Basepath at once, rather than leaving an
   * organization's screen up until something happens to reload.
   */
  useEffect(() => {
    if (
      store.loading ||
      store.workspaces.length === 0 ||
      !context ||
      requestedOrganizationId ||
      (requestedPersonalRoute && requestedPersonalWorkspaceId)
    )
      return;
    if (stillAvailable(context, store.workspaces)) return;
    const own = personalWorkspaceId();
    if (!own) return;
    notify("このワークスペースを利用できなくなりました");
    switchContext(own);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [store.loading, store.workspaces, context?.workspaceId, requestedOrganizationId]);

  /**
   * A personal deep link, once the workspace list has arrived.
   *
   * `/personal/...` cannot name an id — a person has exactly one — so the
   * workspace is resolved here rather than guessed at the first render.
   */
  useEffect(() => {
    if (store.workspaces.length === 0) return;
    const route = parsePath(window.location.pathname);
    if (route?.kind !== "personal") return;
    const own = personalWorkspaceId();
    const requested = new URLSearchParams(window.location.search).get(
      "workspace",
    );
    const target = requested
      ? store.workspaces.find(
          (workspace) =>
            workspace.id === requested && workspace.scope === "個人",
        )?.id
      : own;
    if (target && target !== workspaceId) setWorkspaceId(target);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [store.workspaces.length]);

  /**
   * Keeps the URL honest about which context is being shown.
   *
   * The workspace can change from places that do not go through `goTo` — the
   * first load, a member screen, a deep link that named a workspace in the
   * query string. Whatever the reason, the address bar has to end up saying
   * which side of the boundary this is.
   */
  useEffect(() => {
    if (!context) return;
    const route = parsePath(window.location.pathname);
    const expected = pathFor(context, shownScreen);
    const tenantMatches =
      !tenantId ||
      store.me.mode !== "tachyon" ||
      new URL(window.location.href).searchParams.get("tenant_id") === tenantId;
    if (
      route &&
      window.location.pathname === expected &&
      !window.location.hash &&
      tenantMatches
    )
      return;
    const url = new URL(window.location.href);
    url.pathname = expected;
    if (tenantId && store.me.mode === "tachyon")
      url.searchParams.set("tenant_id", tenantId);
    url.hash = "";
    window.history.replaceState(null, "", url);
  }, [context?.kind, context?.workspaceId, shownScreen, tenantId, store.me.mode]);

  function toggleTask(id: string) {
    const item = raw(id);
    if (!item || !canWrite(item.workspace_id)) return;
    void store.run(
      () =>
        store.write(
          "POST",
          `/v1/workspaces/${item.workspace_id}/actions/${item.id}/${doneFor(item) ? "reopen" : "complete"}`,
          { expected_version: item.version, local_date: today },
        ),
      () => notify(doneFor(item) ? "未完了に戻しました" : "行動を記録しました"),
    );
  }
  /** Moves to a screen inside the context the person is already in. */
  function goTo(target: Screen, into: AppContext | null = context) {
    if (into) {
      const url = new URL(window.location.href);
      url.pathname = pathFor(into, target);
      // The hash was the old route. Leaving it behind would let a stale
      // `#/goals` win on the next read of the URL.
      url.hash = "";
      if (url.pathname !== window.location.pathname || window.location.hash) {
        window.history.pushState(null, "", url);
      }
    }
    setScreen(target);
    setSidebar(false);
    window.requestAnimationFrame(() => mainRef.current?.focus());
    setModal(null);
    setMenu(false);
    setNotifications(false);
    setSearchOpen(false);
    window.scrollTo({ top: 0, behavior: "smooth" });
  }

  /**
   * The old way of naming a screen, kept for the many call sites that say
   * "go to 目標マップ". It resolves to whatever that screen is called in the
   * context the person is in.
   */
  function navigate(label: NavigationLabel) {
    const item =
      navItems.find((entry) => entry.page === label) ??
      navFor(contextKind === "personal" ? "organization" : "personal").find(
        (entry) => entry.page === label,
      );
    if (item) goTo(item.screen);
  }

  /**
   * Crossing between a person's own Basepath and an organization's.
   *
   * Deliberately not `selectWorkspace` with a different argument: nothing is
   * carried over. The selected goal, the search and any open panel are all
   * pointers into the context being left, and a stale one either shows nothing
   * or, worse, looks like it belongs here.
   */
  function switchContext(id: string, keepScreen = false, scopeHint?: Scope) {
    // A workspace created a moment ago is not in the list yet. Its scope comes
    // back with it, which is enough to know which side of the boundary it is
    // on — and refusing to move there until a refresh catches up would leave
    // the person looking at the wrong context.
    const target =
      store.workspaces.find((w) => w.id === id) ??
      (scopeHint ? { id, name: "", scope: scopeHint } : undefined);
    if (!target) return;
    const next = contextOf(target);
    setWorkspaceId(id);
    setScope(target.scope);
    setSelectedId("");
    setSearch("");
    setSearchOpen(false);
    setModal(null);
    notify(
      next.kind === "personal"
        ? "個人のBasepathに切り替えました"
        : `${target.name || "組織"}に切り替えました`,
    );
    // Crossing from the switcher lands on the new context's front page, so
    // the change is unmissable. Crossing from inside a screen that exists on
    // both sides — choosing a workspace while managing workspaces — stays
    // there, because the person is still doing the same thing. Either way the
    // selection, the search and any open panel are gone.
    const landing =
      keepScreen && screenBelongs(next.kind, shownScreen)
        ? shownScreen
        : landingFor();
    goTo(landing, next);
  }
  function changeScope(value: Scope | "すべて") {
    setScope(value);
    routeTo({ scope: value });
    if (value !== "すべて") {
      const target =
        currentWorkspace?.scope === value
          ? currentWorkspace
          : store.workspaces.find((w) => w.scope === value);
      if (target) setWorkspaceId(target.id);
      const first = allItems.find(
        (i) =>
          i.kind === "outcome" &&
          !i.archived_at &&
          scopeOf(i.workspace_id) === value,
      );
      if (first) setSelectedId(uiId(first));
    }
  }
  /**
   * Choosing a workspace from anywhere else in the app.
   *
   * Every workspace is a context, so this is the same act as using the
   * switcher and goes through the same door: the URL is rewritten and nothing
   * from the previous context is carried over. Two ways to cross a boundary,
   * one of which quietly kept the old selection, is how the two stop being
   * separate in practice.
   */
  function selectWorkspace(id: string, selectedScope?: Scope) {
    switchContext(id, true, selectedScope);
  }
  async function saveForm(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const data = new FormData(e.currentTarget);
    const title = String(data.get("title") || "").trim();
    if (!title) return;
    const w = String(data.get("workspace_id") || currentWorkspace?.id || "");
    if (!w) return;
    const base = `/v1/workspaces/${w}`;
    await store.run(
      async () => {
        if (modal?.kind === "template") {
          const ids: Record<string, string> = {
            自由形式: "free",
            OKR: "okr",
            プロジェクト: "project",
            学習計画: "learning",
            習慣づくり: "habit",
          };
          const item = await store.write<Item>(
            "POST",
            `${base}/templates/${ids[modal.template]}/apply`,
            {
              title,
              description: String(data.get("description") || ""),
              start_date: data.get("start_date") || null,
              due_date: data.get("due_date") || null,
            },
          );
          setSelectedId(uiId(item));
          setScope("すべて");
          routeTo({ item: uiId(item), scope: "すべて" });
        } else if (modal?.kind === "task") {
          const frequency = Number(data.get("frequency") || 0);
          const weekdays = data.getAll("weekdays").map(Number);
          const recurrenceMode = String(
            data.get("recurrence_mode") || "period_quota",
          );
          await store.write("POST", `${base}/items`, {
            title,
            kind: "action",
            start_date: data.get("start_date") || null,
            due_date: data.get("due_date") || null,
            scheduled_date: data.get("date") || null,
            scheduled_time: data.get("time") || null,
            fields: {
              assignee_id: data.get("assignee_id") || null,
              priority: data.get("priority") || null,
              recurrence:
                frequency || weekdays.length
                  ? {
                      mode: recurrenceMode,
                      times_per_week:
                        recurrenceMode === "fixed_schedule"
                          ? weekdays.length
                          : frequency,
                      timezone: store.settings.timezone,
                      weekdays,
                    }
                  : null,
            },
          });
        } else if (modal?.kind === "initiative" && selectedRaw) {
          await store.write(
            "POST",
            `/v1/workspaces/${selectedRaw.workspace_id}/items`,
            {
              title,
              kind: "initiative",
              parent_id: selectedRaw.id,
              fields: { icon: "flag" },
            },
          );
        } else if (modal?.kind === "createItem") {
          const kind = String(data.get("kind") || "initiative");
          if (kind === "action" && modal.parentId) {
            // Actions are terminal in the UI; this guard also protects callers
            // that keep a stale create dialog open while changing selection.
            const parent = allItems.find(
              (item) =>
                item.workspace_id === modal.workspaceId &&
                item.id === modal.parentId,
            );
            if (parent?.kind === "action") return;
          }
          const created = await store.write<Item>(
            "POST",
            `/v1/workspaces/${modal.workspaceId}/items`,
            {
              title,
              kind,
              parent_id: modal.parentId,
              fields: kind === "action" ? {} : { icon: "flag" },
            },
          );
          setSelectedId(uiId(created));
          setScope("すべて");
          routeTo({ item: uiId(created), scope: "すべて" });
        } else if (modal?.kind === "editGoal" && selectedRaw) {
          const assessment = String(data.get("progress") || "");
          await store.write("PATCH", itemPath(selectedRaw), {
            expected_version: editBase,
            title,
            description: String(data.get("description") || ""),
            state: data.get("state"),
            start_date: data.get("start_date") || null,
            due_date: data.get("due_date") || null,
            fields: {
              self_assessment: assessment === "" ? null : Number(assessment),
              external_url: String(data.get("external_url") || ""),
            },
          });
        }
      },
      () => {
        setModal(null);
        notify("保存しました");
      },
    );
  }
  useEffect(() => {
    if (modal?.kind === "editGoal") setEditBase(selectedRaw?.version ?? null);
  }, [modal?.kind]);
  const results = search.trim()
    ? visibleItems
        .filter((i) => i.title.toLowerCase().includes(search.toLowerCase()))
        .slice(0, 30)
        .map((i) => ({
          id: uiId(i),
          title: i.title,
          scope: scopeOf(i.workspace_id),
          type:
            i.kind === "action"
              ? "行動"
              : i.kind === "initiative"
                ? "取り組み"
                : "目標",
        }))
    : [];

  if (store.error?.code === "UNAUTHENTICATED")
    return <TachyonLogin onSignedIn={store.refresh} />;
  if (store.error?.code === "TENANT_SELECTION_REQUIRED")
    return (
      <TachyonTenantSelection
        initialTenantId={tenantId}
        onTenantChange={(id) => {
          setTenantId(id);
          replaceTenantSelectionUrl(id, tenantReturnRef.current);
        }}
        onSelected={async (id) => {
          setTenantId(id);
          await store.refresh();
          setSelectingTenant(false);
          const returnTo = tenantReturnRef.current;
          tenantReturnRef.current = null;
          if (returnTo) {
            const restored = restoreTenantReturn(returnTo, id);
            const route = parsePath(restored.pathname);
            setScreen(route?.screen ?? "home");
            setWorkspaceId(route?.orgId ?? "");
            setSelectedId(restored.searchParams.get("item") || "");
            window.history.replaceState({}, "", restored);
          } else {
            replaceScreenUrl("/personal/home", id);
          }
        }}
        onBack={async () => {
          await request("POST", "/auth/logout", {});
          await store.refresh();
          setTenantId("");
          tenantReturnRef.current = null;
          replaceScreenUrl("/login");
        }}
        backLabel="ログイン画面に戻る"
        backPendingLabel="ログアウト中…"
      />
    );
  // The connection consent screen, once the person is signed in, for the same
  // reason: a delegation is theirs to grant, so it needs their session.
  if (consenting !== null)
    return (
      <OAuthConsent
        search={consenting}
        onLeave={() => {
          window.location.replace("/");
        }}
      />
    );
  // The approval deep link, once the person is signed in. It comes after the
  // sign-in and tenant screens on purpose: approving needs a real session.
  if (changeRoute)
    return (
      <ChangeApproval
        route={changeRoute}
        store={store}
        onClose={() => {
          setChangeRoute(null);
          window.history.replaceState({}, "", "/");
        }}
      />
    );
  if (selectingTenant && store.me.mode === "tachyon")
    return (
      <TachyonTenantSelection
        initialTenantId={tenantId}
        onTenantChange={(id) => {
          setTenantId(id);
          replaceTenantSelectionUrl(id, tenantReturnRef.current);
        }}
        onSelected={async (id) => {
          setTenantId(id);
          await store.refresh();
          setSelectingTenant(false);
          const returnTo = tenantReturnRef.current;
          tenantReturnRef.current = null;
          if (returnTo) {
            const restored = restoreTenantReturn(returnTo, id);
            const route = parsePath(restored.pathname);
            setScreen(route?.screen ?? "home");
            setWorkspaceId(route?.orgId ?? "");
            setSelectedId(restored.searchParams.get("item") || "");
            window.history.replaceState({}, "", restored);
          } else {
            replaceScreenUrl("/personal/home", id);
          }
        }}
        onBack={() => {
          tenantReturnRef.current = null;
          setSelectingTenant(false);
          // The URL may still carry a candidate tenant from a deep link. Do
          // not send that unselected candidate back into the tenant gate;
          // omitting it lets the server-selected session tenant be restored.
          setTenantId("");
          replaceScreenUrl("/");
        }}
        backLabel="ホームに戻る"
        backPendingLabel="戻っています…"
      />
    );
  if (contextUnavailable)
    return (
      <main className="auth-page">
        <div className="panel auth-card">
          <h1>ワークスペースを開けません</h1>
          <p>このリンクのワークスペースは、現在のテナントでは利用できません。</p>
          <button
            className="primary-button"
            onClick={() => {
              const own = personalWorkspaceId();
              if (own) setWorkspaceId(own);
              setSelectedId("");
              setScreen("home");
              replaceScreenUrl("/", tenantId, true);
            }}
          >
            ホームへ戻る
          </button>
        </div>
      </main>
    );
  return (
    <div className={`app-shell ${compact ? "compact" : ""}`}>
      {sidebar && (
        <button
          className="sidebar-scrim"
          aria-label="メニューを閉じる"
          onClick={() => setSidebar(false)}
        />
      )}
      <aside
        className={`sidebar ${sidebar ? "is-open" : ""}`}
        id="app-navigation"
      >
        <button
          className="brand"
          onClick={() => navigate("ホーム")}
          aria-label="PathBase ホーム"
        >
          <img className="brand-mark" src="/assets/pathbase-mark.png" alt="" />
          <span>PathBase</span>
        </button>
        <div className="workspace-label">
          <span>現在のワークスペース</span>
          {store.me.mode === "tachyon" && (
            <button
              className="text-link"
              onClick={() => {
                setSidebar(false);
                setNotifications(false);
                setModal(null);
                setSelectingTenant(true);
                tenantReturnRef.current = null;
                replaceTenantSelectionUrl(tenantId);
              }}
            >
              テナント切替
              <Icon name="right" size={12} />
            </button>
          )}
        </div>
        {/* The places and the screens scroll as one. They are one column of
            choices to a person reading it; two scroll areas stacked in a
            sidebar cut each other off and hide the bottom of both. */}
        <div className="sidebar-nav">
          {/* The top-level choice: this person's own Basepath, or an
              organization's. Not a filter over one list — two separate places,
              and the control says which one it is in words. */}
          <div className="context-switch" role="group" aria-label="現在の場所">
            <button
              className={contextKind === "personal" ? "active" : ""}
              aria-pressed={contextKind === "personal"}
              disabled={!personalWorkspaceId()}
              onClick={() => switchContext(personalWorkspaceId())}
            >
              <Icon name="home" size={18} weight="duotone" />
              <span>
                個人
                <small>あなただけのBasepath</small>
              </span>
            </button>
            {store.workspaces
              .filter((w) => w.scope !== "個人")
              .map((w) => (
                <button
                  key={w.id}
                  className={currentWorkspace?.id === w.id ? "active" : ""}
                  aria-pressed={currentWorkspace?.id === w.id}
                  onClick={() => switchContext(w.id)}
                >
                  <Icon name="users" size={18} weight="duotone" />
                  <span>
                    {w.name}
                    <small>組織{w.role === "viewer" ? "・閲覧のみ" : ""}</small>
                  </span>
                </button>
              ))}
          </div>
          <nav className="main-nav" aria-label="メインメニュー">
            {navItems.map((item) => (
              <button
                key={item.screen}
                className={shownScreen === item.screen ? "active" : ""}
                aria-current={shownScreen === item.screen ? "page" : undefined}
                onClick={() => goTo(item.screen)}
              >
                <Icon
                  name={item.icon}
                  size={23}
                  weight={shownScreen === item.screen ? "fill" : "regular"}
                />
                <span>{item.label}</span>
                {item.screen === "members" && store.invitations.length > 0 && (
                  <span className="invitation-count">
                    {store.invitations.length}
                  </span>
                )}
              </button>
            ))}
          </nav>
        </div>
        <nav className="utility-nav" aria-label="ユーティリティ">
          <button
            onClick={() => {
              setSidebar(false);
              openNotifications();
            }}
          >
            <Icon name="bell" size={22} />
            お知らせ
          </button>
          <button
            onClick={() => {
              setSidebar(false);
              searchRef.current?.focus();
              setSearchOpen(true);
            }}
          >
            <Icon name="search" size={22} />
            検索
          </button>
          <button
            onClick={() => {
              setSidebar(false);
              setModal({ kind: "settings" });
            }}
          >
            <Icon name="settings" size={22} />
            設定
          </button>
        </nav>
        <button
          className="profile"
          onClick={() => setModal({ kind: "members" })}
        >
          <Avatar
            size={46}
            name={store.me.mode === "tachyon" ? store.me.name : undefined}
          />
          <span>
            <strong>{store.me.name}</strong>
            <small>
              {store.me.mode === "tachyon"
                ? "Tachyonでログイン中"
                : "ローカル確認用"}
            </small>
          </span>
        </button>
      </aside>

      <main className="main-content" id="home" ref={mainRef} tabIndex={-1}>
        <header className={activeNav === "ホーム" ? "hero" : "subpage-header"}>
          <div className="topbar">
            <button
              className="icon-button mobile-menu"
              aria-label="メニューを開く"
              aria-controls="app-navigation"
              aria-expanded={sidebar}
              onClick={() => setSidebar(true)}
            >
              <Icon name="menu" />
            </button>
            {/* Where you are, in words. Not a colour and not an icon: a person
                who has not been told what the colour means cannot read it,
                and this is the one thing they must never misread. */}
            <p className="context-breadcrumb" data-context={contextKind}>
              <span className="context-chip">
                {contextLabel(contextKind)}
                {contextKind === "organization" && currentWorkspace
                  ? `・${currentWorkspace.name}`
                  : ""}
              </span>
              <span aria-hidden="true">/</span>
              <span className="context-screen">{navItem.label}</span>
            </p>
            <div className="search-wrap">
              <Icon name="search" size={17} />
              <input
                ref={searchRef}
                value={search}
                onChange={(e) => {
                  setSearch(e.target.value);
                  setSearchOpen(true);
                }}
                onFocus={() => setSearchOpen(true)}
                onBlur={() => setTimeout(() => setSearchOpen(false), 180)}
                placeholder="目標・タスク・メンバーを検索..."
                aria-label="目標やタスクを検索"
              />
              <kbd>⌘ K</kbd>
              {searchOpen && (
                <div className="search-results">
                  {!search.trim() ? (
                    <p>目標や行動の名前を入力して検索</p>
                  ) : !results.length ? (
                    <p>「{search}」に一致する項目はありません</p>
                  ) : (
                    results.map((result) => (
                      <button
                        key={result.id}
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => {
                          if (result.type === "目標") {
                            setSelectedId(result.id);
                            setScope("すべて");
                            navigate("目標マップ");
                          } else if (result.type === "メンバー") {
                            navigate("メンバー");
                          } else {
                            setModal({
                              kind: "initiativeDetail",
                              id: result.id,
                            });
                          }
                          setSearch("");
                          setSearchOpen(false);
                        }}
                      >
                        <Icon
                          name={result.type === "目標" ? "target" : "tasks"}
                          size={18}
                        />
                        <span>
                          {result.title}
                          <small>{result.type}</small>
                        </span>
                        <Badge scope={result.scope} />
                      </button>
                    ))
                  )}
                </div>
              )}
            </div>
            <span
              className={`save-status ${store.error ? "has-error" : ""}`}
              role="status"
            >
              {store.pending
                ? "保存中…"
                : store.error
                  ? "未保存・確認が必要"
                  : store.loading
                    ? "読み込み中…"
                    : store.me.mode === "tachyon"
                      ? "保存済み"
                      : "この端末に保存済み"}
            </span>
            <div className="notification-wrap">
              <button
                className="icon-button notification-button"
                aria-label="お知らせを表示"
                onClick={openNotifications}
              >
                <Icon name="bell" size={23} />
                {unread && <span className="unread-dot" />}
              </button>
              {notifications && (
                <div className="notification-popover">
                  <h3>
                    お知らせ <span>今日</span>
                  </h3>
                  {store.invitations.length > 0 && (
                    <button
                      className="text-link"
                      onClick={() => {
                        setModal({ kind: "members" });
                        setNotifications(false);
                      }}
                    >
                      ワークスペースへの招待が{store.invitations.length}
                      件届いています
                    </button>
                  )}
                  {allNotifications.slice(0, 5).map((entry) => (
                    <p
                      key={entry.id}
                      className={!entry.read_at ? "unread-notification" : ""}
                    >
                      <Icon name="calendar" size={19} />
                      {entry.title}
                    </p>
                  ))}
                  {!allNotifications.length && (
                    <p>新しいお知らせはありません</p>
                  )}
                  <button
                    className="text-link"
                    onClick={() => setNotifications(false)}
                  >
                    確認しました
                    <Icon name="check" size={15} />
                  </button>
                </div>
              )}
            </div>
            <button
              className="avatar-button"
              aria-label="プロフィールを表示"
              onClick={() => setModal({ kind: "members" })}
            >
              <Avatar
                size={34}
                name={store.me.mode === "tachyon" ? store.me.name : undefined}
              />
            </button>
          </div>
          {activeNav === "ホーム" ? (
            <>
              <div className="hero-copy">
                <h1>やりたいことを、動ける形に。</h1>
                <p>
                  目標・計画・行動・学びをつなげて、人生も仕事も、前に進めるプラットフォームです。
                </p>
              </div>
              <p className="hero-note">
                今日の一歩が、
                <br />
                <span>未来をつくる。</span>
              </p>
            </>
          ) : (
            <div className="subpage-heading">
              <span className="subpage-heading-icon">
                <Icon
                  name={
                    navigation.find((item) => item.label === activeNav)?.icon ??
                    "home"
                  }
                  size={26}
                  weight="duotone"
                />
              </span>
              <div>
                <small>PathBase / {workspace}</small>
                <h1>{activeNav}</h1>
                <p>{navigationDescriptions[activeNav]}</p>
              </div>
            </div>
          )}
        </header>

        {store.error && (
          <div className="save-error global-save-error" role="alert">
            <span>{store.error.message}</span>
            <button
              onClick={() => void store.refresh()}
              disabled={store.pending}
            >
              最新を読み込む
            </button>
            <button aria-label="エラーを閉じる" onClick={store.clearError}>
              ×
            </button>
          </div>
        )}
        {activeNav === "ホーム" ? (
          <div className="dashboard">
            {store.loading && (
              <p className="empty-value">保存した目標を読み込んでいます…</p>
            )}
            {!store.loading &&
              currentWorkspace &&
              currentSnapshot &&
              currentSnapshot.items.every((item) => item.archived_at) && (
                <OnboardingWizard
                  store={store}
                  workspaces={
                    currentWorkspace.role === "viewer"
                      ? [currentWorkspace]
                      : store.workspaces
                  }
                  onComplete={(goalId, actionId) => {
                    setSelectedId(goalId);
                    setScope("すべて");
                    routeTo({ item: goalId, scope: "すべて" });
                    navigate(actionId ? "今日の行動" : "目標マップ");
                    notify("最初の目標を作成しました");
                  }}
                />
              )}
            <section className="panel templates-panel" id="templates">
              <div className="section-header">
                <h2>テンプレートからはじめる</h2>
                <TextLink
                  onClick={() =>
                    setModal({ kind: "template", template: "自由形式" })
                  }
                >
                  すべてのテンプレート
                </TextLink>
              </div>
              <div className="template-grid">
                {templates.map((template) => (
                  <button
                    key={template.title}
                    className={`template-card ${template.color}`}
                    onClick={() =>
                      setModal({ kind: "template", template: template.title })
                    }
                  >
                    <Icon name={template.icon} size={36} />
                    <span>
                      <strong>{template.title}</strong>
                      <small>{template.description}</small>
                    </span>
                    <Icon name="arrow" size={16} className="template-arrow" />
                  </button>
                ))}
              </div>
            </section>
            <div className="dashboard-grid">
              <GoalMap
                goals={goals}
                initiatives={initiatives}
                items={mapItems}
                selected={selectedId}
                onSelect={selectGoal}
                scope={scope}
                setScope={changeScope}
                onInitiative={openInitiative}
                canEdit={canEditMapItem}
                onAddChild={(id) => openCreateRelative(id, "child")}
                onAddSibling={(id) => openCreateRelative(id, "sibling")}
                onEdit={editTreeItem}
                onMove={moveTreeItem}
                onMoveUp={(id) => reorderTreeItem(id, -1)}
                onMoveDown={(id) => reorderTreeItem(id, 1)}
              />
              <div className="left-column">
                <section className="panel bottom-panel" id="workspace-panels">
                  <div
                    className="bottom-tabs"
                    role="tablist"
                    aria-label="計画と振り返り"
                  >
                    {["タイムライン", "今日の行動", "振り返り", "リスト"].map(
                      (tab) => (
                        <button
                          role="tab"
                          aria-selected={activeTab === tab}
                          key={tab}
                          className={activeTab === tab ? "active" : ""}
                          onClick={() => {
                            setActiveTab(tab);
                            routeTo({ view: tab });
                          }}
                        >
                          {tab}
                        </button>
                      ),
                    )}
                  </div>
                  <div className="bottom-grid">
                    <div className="timeline-card">
                      {activeTab === "タイムライン" ? (
                        <Timeline
                          quarter={quarter}
                          setQuarter={setQuarter}
                          goals={goals}
                          onSelect={selectGoal}
                        />
                      ) : activeTab === "今日の行動" ? (
                        <>
                          <div className="section-header">
                            <h3>
                              今日の行動{" "}
                              <small>
                                {tasks.filter((t) => t.done).length}/
                                {tasks.length} 完了
                              </small>
                            </h3>
                            <button
                              className="icon-button"
                              aria-label="行動を追加"
                              onClick={() => setModal({ kind: "task" })}
                            >
                              <Icon name="plus" />
                            </button>
                          </div>
                          <TaskList
                            tasks={tasks}
                            canEdit={(id) => canWrite(raw(id)?.workspace_id)}
                            toggleTask={toggleTask}
                            expanded
                            disabled={store.pending}
                            onEdit={(id) =>
                              setModal({ kind: "initiativeDetail", id })
                            }
                          />
                        </>
                      ) : activeTab === "リスト" ? (
                        <ItemList
                          items={visibleItems}
                          onSelect={(id) =>
                            setModal({ kind: "initiativeDetail", id })
                          }
                        />
                      ) : (
                        <>
                          <div className="section-header">
                            <h3>今週の振り返り</h3>
                            <span className="week-label">
                              {workspace} · {dateLabel(today)}
                            </span>
                          </div>
                          <div className="reflection-summary">
                            <span className="reflection-icon">
                              <Icon name="leaf" size={28} weight="duotone" />
                            </span>
                            <div>
                              <strong>小さな一歩を、積み重ねる。</strong>
                              <p>
                                今週できたこと、気づいたことを残しましょう。
                              </p>
                            </div>
                          </div>
                          <textarea
                            className="reflection-input"
                            value={reflection}
                            onChange={(e) => {
                              setReflection(e.target.value);
                              localStorage.setItem(
                                reviewDraftKey,
                                e.target.value,
                              );
                            }}
                            aria-label="今週の振り返り"
                            placeholder="今週はどんな一歩を踏み出しましたか？"
                          />
                          <button
                            className="text-link"
                            disabled={
                              store.pending ||
                              !canWrite(currentWorkspace?.id) ||
                              !currentWorkspace ||
                              !reflection.trim() ||
                              reflection === savedReflection
                            }
                            onClick={() =>
                              void store.run(
                                () =>
                                  store.write(
                                    "POST",
                                    `/v1/workspaces/${currentWorkspace?.id}/records`,
                                    {
                                      record_type: "review",
                                      body: reflection,
                                      item_ids: [],
                                    },
                                  ),
                                () => {
                                  setSavedReflection(reflection);
                                  localStorage.removeItem(reviewDraftKey);
                                  notify("振り返りを記録しました");
                                },
                              )
                            }
                          >
                            {savedReflection && reflection === savedReflection
                              ? "記録しました"
                              : "振り返りを記録"}
                            <Icon name="check" size={15} />
                          </button>
                        </>
                      )}
                    </div>
                    <div className="today-card">
                      <div className="section-header">
                        <h3>
                          <Icon
                            name="shield"
                            size={18}
                            className="green-text"
                            weight="duotone"
                          />
                          今日の行動
                        </h3>
                        <TextLink onClick={() => setActiveTab("今日の行動")} />
                      </div>
                      <TaskList
                        tasks={tasks.slice(0, 5)}
                        canEdit={(id) => canWrite(raw(id)?.workspace_id)}
                        toggleTask={toggleTask}
                        disabled={store.pending}
                        onEdit={(id) =>
                          setModal({ kind: "initiativeDetail", id })
                        }
                      />
                      <button
                        className="add-link"
                        onClick={() => setModal({ kind: "task" })}
                      >
                        <Icon name="plus" size={16} />
                        行動を追加
                      </button>
                      <div className="quote-card">
                        <Icon name="quote" size={25} weight="fill" />
                        <p>
                          小さな行動の積み重ねが、
                          <br />
                          大きな変化をつくる。
                        </p>
                      </div>
                    </div>
                    <div className="learning-card">
                      <div className="section-header">
                        <h3>
                          <Icon
                            name="bulb"
                            className="orange-text"
                            size={18}
                            weight="duotone"
                          />
                          最近の学び
                        </h3>
                        <TextLink
                          onClick={() => setModal({ kind: "learnings" })}
                        />
                      </div>
                      <ul>
                        {!learnings.length && (
                          <li>振り返りを記録すると、ここに学びが届きます。</li>
                        )}
                        {learnings.map((learning) => (
                          <li key={learning}>{learning}</li>
                        ))}
                      </ul>
                    </div>
                  </div>
                </section>
              </div>
              {selected && selectedRaw ? (
                <aside
                  className={`panel detail-panel ${scopeClass[selected.scope]}`}
                  aria-label="選択した目標の詳細"
                >
                  <div className="detail-heading">
                    <span
                      className={`detail-icon ${scopeClass[selected.scope]}`}
                    >
                      <Icon name={selected.icon} size={30} weight="duotone" />
                    </span>
                    <div>
                      <Badge scope={selected.scope} />
                      <h2>{selected.title}</h2>
                      <p>{selected.subtitle}</p>
                    </div>
                    <div className="goal-menu">
                      <button
                        className="icon-button"
                        aria-label="目標のメニュー"
                        onClick={() => setMenu(!menu)}
                      >
                        <Icon name="more" size={25} weight="bold" />
                      </button>
                      {menu && (
                        <div className="context-menu">
                          <button
                            onClick={() => {
                              setModal({ kind: "editGoal" });
                              setMenu(false);
                            }}
                          >
                            <Icon name="note" size={17} />
                            目標を編集
                          </button>
                          <button
                            onClick={() => {
                              if (nextAction && doneFor(nextAction))
                                toggleTask(uiId(nextAction));
                              setMenu(false);
                            }}
                          >
                            <Icon name="repeat" size={17} />
                            次の一歩をリセット
                          </button>
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="detail-fields">
                    <DetailRow icon="flag" label="目的">
                      <div className="field-box">{selected.purpose}</div>
                    </DetailRow>
                    <DetailRow icon="chart" label="成果・手応え">
                      <Evaluation
                        item={selectedRaw}
                        snapshots={store.snapshots}
                      />
                      <button
                        className="text-link"
                        onClick={() => setModal({ kind: "metrics" })}
                      >
                        成果を記録 <Icon name="plus" size={13} />
                      </button>
                    </DetailRow>
                    <DetailRow icon="rocket" label="次の一歩">
                      {nextAction ? (
                        <label
                          className={`next-step ${doneFor(nextAction) ? "done" : ""}`}
                        >
                          <input
                            type="checkbox"
                            disabled={store.pending}
                            checked={doneFor(nextAction)}
                            onChange={() => toggleTask(uiId(nextAction))}
                          />
                          <span>
                            {nextAction.title}
                            <small className="date-chip">
                              {dateLabel(nextAction.scheduled_date)}
                            </small>
                          </span>
                        </label>
                      ) : (
                        <button
                          className="add-link"
                          onClick={() => setModal({ kind: "relations" })}
                        >
                          次の行動を関連付ける
                        </button>
                      )}
                      <button
                        className="ai-suggest-link"
                        disabled={!canWrite(selectedRaw.workspace_id)}
                        onClick={() => setModal({ kind: "aiSuggestions" })}
                      >
                        <Icon name="sparkle" size={15} weight="duotone" />
                        AIと次の一歩を考える
                      </button>
                    </DetailRow>
                    <DetailRow icon="graduation" label="関連する取り組み">
                      <div className="related-list">
                        {initiatives
                          .filter((i) => i.goalId === selected.id)
                          .map((i) => (
                            <button
                              key={i.id}
                              onClick={() =>
                                setModal({ kind: "initiativeDetail", id: i.id })
                              }
                            >
                              <span
                                className={`related-icon ${i.icon === "calendar" ? "pink" : "blue"}`}
                              >
                                <Icon
                                  name={i.icon}
                                  size={17}
                                  weight="duotone"
                                />
                              </span>
                              {i.title.replace("\n", "")}
                            </button>
                          ))}
                        <button
                          className="add-link"
                          onClick={() => setModal({ kind: "initiative" })}
                        >
                          <Icon name="plus" size={16} />
                          取り組みを追加
                        </button>
                      </div>
                    </DetailRow>
                    <DetailRow icon="link" label="つながり">
                      <button
                        className="text-link"
                        onClick={() => setModal({ kind: "relations" })}
                      >
                        関連・依存関係を編集 <Icon name="arrow" size={14} />
                      </button>
                      {selectedRaw.fields.external_url && (
                        <a
                          className="text-link"
                          href={selectedRaw.fields.external_url}
                          target="_blank"
                          rel="noreferrer"
                        >
                          参考リンク
                        </a>
                      )}
                    </DetailRow>
                    <DetailRow icon="note" label="メモ">
                      <MemoEditor
                        key={`${store.me.id}:${selected.id}`}
                        item={selectedRaw}
                        store={store}
                      />
                    </DetailRow>
                  </div>
                  <div className="activity-section">
                    <div className="section-header">
                      <h3>
                        <Icon name="shield" size={19} />
                        アクティビティ
                      </h3>
                      <TextLink
                        onClick={() => setModal({ kind: "activity" })}
                      />
                    </div>
                    <ActivityList
                      activity={activity}
                      currentActor={store.me.id}
                    />
                  </div>
                  <div className="detail-quote">
                    <Icon
                      name="leaf"
                      size={25}
                      className="green-text"
                      weight="duotone"
                    />
                    <p>
                      やってみたからこそ、わかったことがある。
                      <br />
                      それが、次の一歩につながる。
                    </p>
                  </div>
                </aside>
              ) : (
                <aside className="panel empty-detail">
                  <Icon name="target" size={38} />
                  <h2>やりたいことを一つ、ここから。</h2>
                  <p>タイトルだけで保存できます。</p>
                  <button
                    className="primary-button"
                    onClick={() =>
                      setModal({ kind: "template", template: "自由形式" })
                    }
                  >
                    目標を追加
                  </button>
                </aside>
              )}
            </div>
          </div>
        ) : activeNav === "記憶" ? (
          <MemoryScreen store={store} workspace={currentWorkspace} />
        ) : activeNav === "目標レビュー" ? (
          <ReviewScreen store={store} workspace={currentWorkspace} />
        ) : activeNav === "ダッシュボード" ? (
          <DashboardScreen
            store={store}
            workspace={currentWorkspace}
            onOpenItem={(id) => {
              const item = allItems.find(
                (entry) =>
                  entry.workspace_id === currentWorkspace?.id &&
                  entry.id === id,
              );
              if (!item) return;
              selectGoal(uiId(item));
              navigate("目標マップ");
            }}
          />
        ) : activeNav === "分解" ? (
          <BreakdownScreen
            store={store}
            workspace={currentWorkspace}
            refreshKey={`${allItems.length}:${allRelations.length}`}
            selectedId={
              raw(selectedId)?.workspace_id === currentWorkspace?.id
                ? raw(selectedId)?.id
                : ""
            }
            onAddChild={(node: BreakdownNode) => {
              const item = allItems.find(
                (entry) =>
                  entry.workspace_id === currentWorkspace?.id &&
                  entry.id === node.id,
              );
              if (!item || item.kind === "action") return;
              setModal({
                kind: "createItem",
                workspaceId: item.workspace_id,
                parentId: item.id,
                relation: "child",
                parentTitle: item.title,
                parentKind: item.kind,
              });
            }}
            onAddSibling={(node: BreakdownNode) => {
              const item = allItems.find(
                (entry) =>
                  entry.workspace_id === currentWorkspace?.id &&
                  entry.id === node.id,
              );
              if (!item) return;
              const parent = partOfTarget(item);
              setModal({
                kind: "createItem",
                workspaceId: item.workspace_id,
                parentId: parent?.id ?? null,
                relation: "sibling",
                parentTitle: parent?.title ?? "ルート",
                siblingTitle: item.title,
                parentKind: parent?.kind,
              });
            }}
          />
        ) : activeNav === "アラインメント" ? (
          <AlignmentScreen
            workspace={currentWorkspace}
            onOpenItem={(id) => {
              const item = allItems.find(
                (entry) =>
                  entry.workspace_id === currentWorkspace?.id &&
                  entry.id === id,
              );
              if (!item) return;
              selectGoal(uiId(item));
              navigate("目標マップ");
            }}
          />
        ) : activeNav === "計画期間" ? (
          <PlanningScreen
            store={store}
            workspace={currentWorkspace}
            items={allItems.filter(
              (item) => item.workspace_id === currentWorkspace?.id,
            )}
            onOpenItem={(id) => {
              const item = allItems.find(
                (entry) =>
                  entry.workspace_id === currentWorkspace?.id &&
                  entry.id === id,
              );
              if (!item) return;
              if (item.kind === "action" || item.kind === "initiative") {
                setModal({ kind: "initiativeDetail", id: uiId(item) });
              } else {
                selectGoal(uiId(item));
                navigate("目標マップ");
              }
            }}
          />
        ) : activeNav === "振り返り" ? (
          <WeeklyReviewScreen
            store={store}
            workspace={currentWorkspace}
            onOpenItem={(id) => {
              const item = currentWorkspace
                ? allItems.find(
                    (entry) =>
                      entry.workspace_id === currentWorkspace.id &&
                      entry.id === id,
                  )
                : undefined;
              if (!item) return;
              if (item.kind === "action" || item.kind === "initiative") {
                setModal({ kind: "initiativeDetail", id: uiId(item) });
              } else {
                selectGoal(uiId(item));
                navigate("目標マップ");
              }
            }}
          />
        ) : (
          <DedicatedScreen
            page={activeNav}
            goals={goals}
            mapItems={mapItems}
            tasks={tasks}
            initiatives={initiatives}
            selected={selected}
            selectedId={selectedId}
            scope={scope}
            workspace={workspace}
            calendarItems={visibleItems}
            calendarRecords={allRecords.filter((record) =>
              workspaceMatches(record.workspace_id),
            )}
            timezone={store.settings.timezone}
            reflection={reflection}
            savedReflection={savedReflection}
            learnings={learnings}
            pending={store.pending}
            canSaveReflection={
              !!currentWorkspace && canWrite(currentWorkspace.id)
            }
            membersContent={
              <WorkspaceMembers
                store={store}
                workspaceId={currentWorkspace?.id}
                onSelect={selectWorkspace}
              />
            }
            onSelectGoal={(id) => {
              selectGoal(id);
              if (activeNav === "タイムライン") navigate("目標マップ");
            }}
            onChangeScope={changeScope}
            onOpenInitiative={openInitiative}
            onCalendarToday={() => navigate("今日の行動")}
            onToggleTask={toggleTask}
            onCanEditTask={(id) => canWrite(raw(id)?.workspace_id)}
            onEditTask={(id) => setModal({ kind: "initiativeDetail", id })}
            onAddTask={() => setModal({ kind: "task" })}
            onChooseTemplate={(template) =>
              setModal({ kind: "template", template })
            }
            onEditGoal={() => setModal({ kind: "editGoal" })}
            onAddInitiative={() => setModal({ kind: "initiative" })}
            canEditMapItem={canEditMapItem}
            onAddChild={(id) => openCreateRelative(id, "child")}
            onAddSibling={(id) => openCreateRelative(id, "sibling")}
            onEditTreeItem={editTreeItem}
            onMoveTreeItem={moveTreeItem}
            onMoveUpTreeItem={(id) => reorderTreeItem(id, -1)}
            onMoveDownTreeItem={(id) => reorderTreeItem(id, 1)}
            onReflectionChange={(value) => {
              setReflection(value);
              localStorage.setItem(reviewDraftKey, value);
            }}
            onSaveReflection={() => {
              if (!currentWorkspace) return;
              void store.run(
                () =>
                  store.write(
                    "POST",
                    `/v1/workspaces/${currentWorkspace.id}/records`,
                    {
                      record_type: "review",
                      body: reflection,
                      item_ids: [],
                    },
                  ),
                () => {
                  setSavedReflection(reflection);
                  localStorage.removeItem(reviewDraftKey);
                  notify("振り返りを記録しました");
                },
              );
            }}
          />
        )}
      </main>
      {toast && (
        <div className="toast" role="status">
          <span>
            <Icon name="check" size={18} />
          </span>
          {toast}
          <button aria-label="通知を閉じる" onClick={() => setToast("")}>
            <Icon name="close" size={16} />
          </button>
        </div>
      )}
      {modal && (
        <Modal
          onClose={() => setModal(null)}
          title={
            modal.kind === "template"
              ? "新しい目標をつくる"
              : modal.kind === "task"
                ? "今日の行動を追加"
                : modal.kind === "initiative" || modal.kind === "createItem"
                  ? "取り組みを追加"
                  : modal.kind === "members"
                    ? "ワークスペースのメンバー"
                    : modal.kind === "settings"
                      ? "設定"
                      : modal.kind === "learnings"
                        ? "最近の学び"
                        : modal.kind === "activity"
                          ? "アクティビティ"
                          : modal.kind === "editGoal"
                            ? "目標を編集"
                            : modal.kind === "metrics"
                              ? "成果と手応えを記録"
                              : modal.kind === "relations"
                                ? "項目のつながり"
                                : modal.kind === "aiSuggestions"
                                  ? "AIによる次の行動・振り返り提案"
                                  : "項目の詳細"
          }
        >
          {store.error && (
            <p className="save-error" role="alert">
              {store.error.message}
              <button onClick={() => void store.refresh()}>最新を確認</button>
            </p>
          )}
          {modal.kind === "editGoal" &&
            editBase !== null &&
            editBase !== selectedRaw?.version && (
              <div className="conflict-note">
                <p>
                  最新の目標：{selectedRaw?.title} · {selectedRaw?.description}
                </p>
                <button
                  onClick={() => setEditBase(selectedRaw?.version ?? null)}
                >
                  最新を確認し、自分の入力で更新する
                </button>
              </div>
            )}
          {(modal.kind === "template" ||
            modal.kind === "task" ||
            modal.kind === "initiative" ||
            modal.kind === "createItem" ||
            modal.kind === "editGoal") && (
            <form onSubmit={saveForm} className="editor-form">
              <fieldset
                disabled={
                  store.pending ||
                  ((modal.kind === "editGoal" || modal.kind === "initiative") &&
                    !canWrite(selectedRaw?.workspace_id)) ||
                  (modal.kind === "createItem" &&
                    !canWrite(modal.workspaceId)) ||
                  (modal.kind === "editGoal" &&
                    editBase !== null &&
                    editBase !== selectedRaw?.version)
                }
              >
                {modal.kind === "template" && (
                  <>
                    <p className="modal-intro">
                      やってみたいことを、ここから始めましょう。
                    </p>
                    <div className="template-picker">
                      {templates.map((t) => (
                        <button
                          type="button"
                          key={t.title}
                          aria-pressed={modal.template === t.title}
                          className={`${t.color} ${modal.template === t.title ? "selected" : ""}`}
                          onClick={() =>
                            setModal({ kind: "template", template: t.title })
                          }
                        >
                          <Icon name={t.icon} size={23} />
                          <span>{t.title}</span>
                        </button>
                      ))}
                    </div>
                  </>
                )}
                {modal.kind === "createItem" && (
                  <>
                    <p className="modal-intro">
                      {modal.relation === "child" ? "子項目" : "兄弟項目"}として
                      「{modal.parentTitle}」に追加します。
                      {modal.siblingTitle && `（基準: ${modal.siblingTitle}）`}
                    </p>
                    <label>
                      種類
                      <select
                        name="kind"
                        defaultValue={
                          modal.parentKind === "action"
                            ? "action"
                            : "initiative"
                        }
                      >
                        {modal.parentKind !== "action" && (
                          <>
                            <option value="outcome">目標</option>
                            <option value="initiative">取り組み</option>
                            <option value="milestone">節目</option>
                            <option value="action">行動（最後の階層）</option>
                          </>
                        )}
                        {modal.parentKind === "action" && (
                          <option value="action">行動</option>
                        )}
                      </select>
                    </label>
                  </>
                )}
                <label>
                  {modal.kind === "task"
                    ? "どんな行動をしますか？"
                    : modal.kind === "initiative" || modal.kind === "createItem"
                      ? "取り組みの名前"
                      : "目標の名前"}
                  <input
                    name="title"
                    autoFocus
                    required
                    maxLength={60}
                    defaultValue={
                      modal.kind === "editGoal" ? selected?.title : ""
                    }
                    placeholder={
                      modal.kind === "task"
                        ? "例：英会話を30分学習する"
                        : "例：地域の人がつながる場所をつくる"
                    }
                  />
                </label>
                {store.error && (
                  <p className="save-error" role="alert">
                    {store.error.message}
                    <button onClick={() => void store.refresh()}>
                      最新を確認
                    </button>
                  </p>
                )}
                {modal.kind === "editGoal" &&
                  editBase !== null &&
                  editBase !== selectedRaw?.version && (
                    <div className="conflict-note">
                      <p>
                        最新の目標：{selectedRaw?.title} ·{" "}
                        {selectedRaw?.description}
                      </p>
                      <button
                        onClick={() =>
                          setEditBase(selectedRaw?.version ?? null)
                        }
                      >
                        最新を確認し、自分の入力で更新する
                      </button>
                    </div>
                  )}
                {(modal.kind === "template" || modal.kind === "task") && (
                  <label>
                    ワークスペース
                    <select
                      name="workspace_id"
                      defaultValue={
                        canWrite(currentWorkspace?.id)
                          ? currentWorkspace?.id
                          : store.workspaces.find((w) => w.role !== "viewer")
                              ?.id
                      }
                    >
                      {store.workspaces
                        .filter((w) => w.role !== "viewer")
                        .map((w) => (
                          <option key={w.id} value={w.id}>
                            {w.name} · {w.scope}
                          </option>
                        ))}
                    </select>
                  </label>
                )}
                {store.error && (
                  <p className="save-error" role="alert">
                    {store.error.message}
                    <button onClick={() => void store.refresh()}>
                      最新を確認
                    </button>
                  </p>
                )}
                {modal.kind === "editGoal" &&
                  editBase !== null &&
                  editBase !== selectedRaw?.version && (
                    <div className="conflict-note">
                      <p>
                        最新の目標：{selectedRaw?.title} ·{" "}
                        {selectedRaw?.description}
                      </p>
                      <button
                        onClick={() =>
                          setEditBase(selectedRaw?.version ?? null)
                        }
                      >
                        最新を確認し、自分の入力で更新する
                      </button>
                    </div>
                  )}
                {(modal.kind === "template" || modal.kind === "editGoal") && (
                  <label>
                    この目標の目的
                    <textarea
                      name="description"
                      rows={3}
                      defaultValue={
                        modal.kind === "editGoal" ? selected?.purpose : ""
                      }
                      placeholder="どんな未来につなげたいですか？"
                    />
                  </label>
                )}
                {modal.kind === "template" && (
                  <p className="template-preview">
                    作成内容：
                    {modal.template === "自由形式"
                      ? "目標を1つ。数値・期限・親は任意です。"
                      : modal.template === "OKR"
                        ? "目標とOKRビュー。指標は後から自分で設定できます。"
                        : modal.template === "プロジェクト"
                          ? "目標・取り組み・最初の行動を作成します。"
                          : "目標・取り組み・週3回の習慣を作成します。"}
                  </p>
                )}
                {(modal.kind === "template" || modal.kind === "editGoal") && (
                  <div className="form-columns">
                    <label>
                      開始日（任意）
                      <input
                        type="date"
                        name="start_date"
                        defaultValue={
                          modal.kind === "editGoal"
                            ? selectedRaw?.start_date || ""
                            : ""
                        }
                      />
                    </label>
                    <label>
                      終了日・期限（任意）
                      <input
                        type="date"
                        name="due_date"
                        defaultValue={
                          modal.kind === "editGoal"
                            ? selectedRaw?.due_date || ""
                            : ""
                        }
                      />
                    </label>
                  </div>
                )}
                {modal.kind === "task" && (
                  <>
                    <div className="form-columns">
                      <label>
                        担当者
                        <select name="assignee_id" defaultValue={store.me.id}>
                          <option value="">担当者なし</option>
                          <option value={store.me.id}>
                            {store.me.name}（あなた）
                          </option>
                        </select>
                      </label>
                      <label>
                        優先度
                        <select name="priority" defaultValue="">
                          <option value="">未設定</option>
                          <option value="low">低</option>
                          <option value="medium">中</option>
                          <option value="high">高</option>
                          <option value="urgent">緊急</option>
                        </select>
                      </label>
                    </div>
                    <div className="form-columns">
                      <label>
                        開始日（任意）
                        <input type="date" name="start_date" />
                      </label>
                      <label>
                        期限（任意）
                        <input type="date" name="due_date" />
                      </label>
                    </div>
                    <label>
                      日付（任意）
                      <input type="date" name="date" defaultValue={today} />
                    </label>
                    <label>
                      習慣ルール
                      <select
                        name="recurrence_mode"
                        defaultValue="period_quota"
                      >
                        <option value="period_quota">週の回数で決める</option>
                        <option value="fixed_schedule">曜日を固定する</option>
                      </select>
                    </label>
                    <label>
                      週の目標回数
                      <select name="frequency" defaultValue="0">
                        <option value="0">繰り返さない</option>
                        {[1, 2, 3, 4, 5, 6, 7].map((n) => (
                          <option key={n} value={n}>
                            週{n}回
                          </option>
                        ))}
                      </select>
                    </label>
                    <fieldset className="weekday-picker">
                      <legend>固定する曜日（曜日固定を選んだ場合）</legend>
                      {["月", "火", "水", "木", "金", "土", "日"].map(
                        (label, index) => (
                          <label key={label}>
                            <input
                              type="checkbox"
                              name="weekdays"
                              value={index}
                            />
                            {label}
                          </label>
                        ),
                      )}
                    </fieldset>
                  </>
                )}
                {modal.kind === "task" && (
                  <label>
                    取り組む時間
                    <input type="time" name="time" defaultValue="" />
                  </label>
                )}
                {modal.kind === "editGoal" && (
                  <label>
                    自己評価（%・任意）
                    <input
                      type="number"
                      name="progress"
                      min={0}
                      max={100}
                      defaultValue={selected?.progress ?? ""}
                    />
                  </label>
                )}
                {modal.kind === "editGoal" && (
                  <>
                    <label>
                      状態
                      <select name="state" defaultValue={selectedRaw?.state}>
                        {Object.entries({
                          draft: "下書き",
                          active: "進行中",
                          paused: "休止中",
                          done: "達成",
                          abandoned: "見送り",
                        }).map(([v, l]) => (
                          <option key={v} value={v}>
                            {l}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label>
                      参考リンク（任意）
                      <input
                        type="url"
                        name="external_url"
                        defaultValue={selectedRaw?.fields.external_url || ""}
                        placeholder="https://…"
                      />
                    </label>
                  </>
                )}
                <div className="modal-actions">
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => setModal(null)}
                  >
                    キャンセル
                  </button>
                  <button className="primary-button" type="submit">
                    <Icon
                      name={modal.kind === "editGoal" ? "check" : "plus"}
                      size={18}
                    />
                    {modal.kind === "editGoal"
                      ? "変更を保存"
                      : modal.kind === "template"
                        ? "目標を作成"
                        : "追加する"}
                  </button>
                </div>
              </fieldset>
            </form>
          )}
          {modal.kind === "members" && (
            <WorkspaceMembers
              store={store}
              workspaceId={currentWorkspace?.id}
              onSelect={selectWorkspace}
            />
          )}
          {modal.kind === "settings" && (
            <>
              <FieldIntegration
                store={store}
                workspaceId={currentWorkspace?.id || "personal"}
              />
              <McpConnections store={store} />
              <StorageSettings
                store={store}
                workspaceId={currentWorkspace?.id || "personal"}
                onSelect={(id) => {
                  setModal({ kind: "initiativeDetail", id });
                }}
              />
            </>
          )}
          {modal.kind === "metrics" && selectedRaw && (
            <fieldset disabled={!canWrite(selectedRaw.workspace_id)}>
              <MetricEditor item={selectedRaw} store={store} />
            </fieldset>
          )}
          {modal.kind === "relations" && selectedRaw && (
            <fieldset disabled={!canWrite(selectedRaw.workspace_id)}>
              <RelationsEditor item={selectedRaw} store={store} />
            </fieldset>
          )}
          {modal.kind === "aiSuggestions" && selectedRaw && (
            <AiSuggestions
              goal={selectedRaw}
              store={store}
              onApplied={() => {
                setModal(null);
                notify("AI提案を確認して採用しました");
              }}
            />
          )}
          {modal.kind === "learnings" && (
            <ul className="learning-modal-list">
              {learnings.map((item) => (
                <li key={item}>
                  <span className="related-icon orange">
                    <Icon name="bulb" size={22} weight="duotone" />
                  </span>
                  <div>
                    {item}
                    <small>{workspace}の学び</small>
                  </div>
                </li>
              ))}
            </ul>
          )}
          {modal.kind === "activity" && (
            <ActivityList activity={activity} currentActor={store.me.id} full />
          )}
          {modal.kind === "initiativeDetail" && raw(modal.id) && (
            <fieldset disabled={!canWrite(raw(modal.id)?.workspace_id)}>
              <ItemEditor
                key={`${store.me.id}:${modal.id}`}
                item={raw(modal.id)!}
                store={store}
                onClose={() => setModal(null)}
                onAddChild={() => {
                  const item = raw(modal.id);
                  if (!item || item.kind === "action") return;
                  setModal({
                    kind: "createItem",
                    workspaceId: item.workspace_id,
                    parentId: item.id,
                    relation: "child",
                    parentTitle: item.title,
                    parentKind: item.kind,
                  });
                }}
                onAddSibling={() => {
                  const item = raw(modal.id);
                  if (!item) return;
                  const parent = partOfTarget(item);
                  setModal({
                    kind: "createItem",
                    workspaceId: item.workspace_id,
                    parentId: parent?.id ?? null,
                    relation: "sibling",
                    parentTitle: parent?.title ?? "ルート",
                    siblingTitle: item.title,
                    parentKind: parent?.kind,
                  });
                }}
              />
            </fieldset>
          )}
        </Modal>
      )}
    </div>
  );
}

type DedicatedScreenProps = {
  page: NavigationLabel;
  goals: Goal[];
  mapItems: MapItem[];
  tasks: Task[];
  initiatives: Initiative[];
  selected?: Goal;
  selectedId: string;
  scope: Scope | "すべて";
  workspace: string;
  calendarItems: Item[];
  calendarRecords: RecordEntry[];
  timezone: string;
  reflection: string;
  savedReflection: string;
  learnings: string[];
  pending: boolean;
  canSaveReflection: boolean;
  membersContent: ReactNode;
  onSelectGoal: (id: string) => void;
  onChangeScope: (scope: Scope | "すべて") => void;
  onOpenInitiative: (id: string) => void;
  onCalendarToday: () => void;
  onToggleTask: (id: string) => void;
  onCanEditTask: (id: string) => boolean;
  onEditTask: (id: string) => void;
  onAddTask: () => void;
  onChooseTemplate: (template: string) => void;
  onEditGoal: () => void;
  onAddInitiative: () => void;
  canEditMapItem: (id: string) => boolean;
  onAddChild?: (id: string) => void;
  onAddSibling?: (id: string) => void;
  onEditTreeItem?: (id: string) => void;
  onMoveTreeItem?: (id: string) => void;
  onMoveUpTreeItem?: (id: string) => void;
  onMoveDownTreeItem?: (id: string) => void;
  onReflectionChange: (value: string) => void;
  onSaveReflection: () => void;
};

function DedicatedScreen({
  page,
  goals,
  mapItems,
  tasks,
  initiatives,
  selected,
  selectedId,
  scope,
  workspace,
  calendarItems,
  calendarRecords,
  timezone,
  reflection,
  savedReflection,
  learnings,
  pending,
  canSaveReflection,
  membersContent,
  onSelectGoal,
  onChangeScope,
  onOpenInitiative,
  onCalendarToday,
  onToggleTask,
  onCanEditTask,
  onEditTask,
  onAddTask,
  onChooseTemplate,
  onEditGoal,
  onAddInitiative,
  canEditMapItem,
  onAddChild,
  onAddSibling,
  onEditTreeItem,
  onMoveTreeItem,
  onMoveUpTreeItem,
  onMoveDownTreeItem,
  onReflectionChange,
  onSaveReflection,
}: DedicatedScreenProps) {
  const doneCount = tasks.filter((task) => task.done).length;
  const completion = tasks.length
    ? Math.round((doneCount / tasks.length) * 100)
    : 0;

  if (page === "目標マップ") {
    const selectedInitiatives = selected
      ? initiatives.filter((item) => item.goalId === selected.id)
      : [];
    return (
      <div className="page-content goal-map-screen">
        <div className="goal-map-screen-grid">
          <GoalMap
            goals={goals}
            initiatives={initiatives}
            items={mapItems}
            selected={selectedId}
            onSelect={onSelectGoal}
            scope={scope}
            setScope={onChangeScope}
            onInitiative={onOpenInitiative}
            canEdit={canEditMapItem}
            onAddChild={onAddChild}
            onAddSibling={onAddSibling}
            onEdit={onEditTreeItem}
            onMove={onMoveTreeItem}
            onMoveUp={onMoveUpTreeItem}
            onMoveDown={onMoveDownTreeItem}
          />
          {selected ? (
            <section
              className={`panel goal-overview ${scopeClass[selected.scope]}`}
            >
              <div className="goal-overview-heading">
                <span className={`detail-icon ${scopeClass[selected.scope]}`}>
                  <Icon name={selected.icon} size={28} weight="duotone" />
                </span>
                <div>
                  <Badge scope={selected.scope} />
                  <h2>{selected.title}</h2>
                  <p>{selected.subtitle}</p>
                </div>
              </div>
              <div className="goal-overview-progress">
                <span>現在の進捗</span>
                <Progress
                  value={selected.progress}
                  color={scopeClass[selected.scope]}
                />
              </div>
              <div className="goal-overview-block">
                <small>この目標の目的</small>
                <p>{selected.purpose || "目的はまだ設定されていません。"}</p>
              </div>
              <div className="goal-overview-block">
                <small>次の一歩</small>
                <strong>
                  <Icon name="rocket" size={18} weight="duotone" />
                  {selected.next || "次の一歩を設定しましょう"}
                </strong>
              </div>
              <div className="goal-overview-block">
                <div className="section-header">
                  <small>関連する取り組み</small>
                  <button className="text-link" onClick={onAddInitiative}>
                    <Icon name="plus" size={14} />
                    追加
                  </button>
                </div>
                <div className="goal-initiative-list">
                  {!selectedInitiatives.length && (
                    <p className="empty-value">
                      関連する取り組みはまだありません。
                    </p>
                  )}
                  {selectedInitiatives.map((item) => (
                    <button
                      key={item.id}
                      onClick={() => onOpenInitiative(item.id)}
                    >
                      <span
                        className={`related-icon ${scopeClass[selected.scope]}`}
                      >
                        <Icon name={item.icon} size={18} weight="duotone" />
                      </span>
                      <span>
                        <strong>{item.title.replace("\n", "")}</strong>
                        <small>
                          {item.progress === null
                            ? "評価未設定"
                            : `${item.progress}% 完了`}
                        </small>
                      </span>
                      <Icon name="right" size={15} />
                    </button>
                  ))}
                </div>
              </div>
              <button
                className="secondary-button goal-edit-button"
                onClick={onEditGoal}
              >
                <Icon name="note" size={17} />
                この目標を編集
              </button>
            </section>
          ) : (
            <aside className="panel empty-detail">
              <Icon name="target" size={38} />
              <h2>目標はまだありません</h2>
              <p>テンプレート画面から最初の目標を作成できます。</p>
            </aside>
          )}
        </div>
      </div>
    );
  }

  if (page === "タイムライン") {
    return (
      <div className="page-content timeline-screen">
        <section className="panel screen-primary-card calendar-card">
          <CalendarView
            items={calendarItems}
            records={calendarRecords}
            timezone={timezone}
            onOpen={onEditTask}
            onToday={onCalendarToday}
          />
        </section>
      </div>
    );
  }

  if (page === "今日の行動") {
    const orderedTasks = [...tasks].sort(compareTasksByDone);
    return (
      <div className="page-content today-screen">
        <div className="metric-grid" aria-label="今日の進捗">
          <section className="panel metric-card blue">
            <span className="metric-icon">
              <Icon name="tasks" size={23} weight="duotone" />
            </span>
            <span>
              <small>今日の行動</small>
              <strong>{tasks.length}件</strong>
            </span>
          </section>
          <section className="panel metric-card green">
            <span className="metric-icon">
              <Icon name="check" size={23} weight="bold" />
            </span>
            <span>
              <small>完了</small>
              <strong>{doneCount}件</strong>
            </span>
          </section>
          <section className="panel metric-card purple">
            <span className="metric-icon">
              <Icon name="chart" size={23} weight="duotone" />
            </span>
            <span>
              <small>達成率</small>
              <strong>{completion}%</strong>
            </span>
          </section>
        </div>
        <section className="panel screen-primary-card action-list-panel">
          <div className="screen-card-header">
            <div>
              <span className="eyebrow">{workspace}</span>
              <h2>今日やること</h2>
              <p>未完了の行動から順に表示しています。</p>
            </div>
            <button className="primary-button" onClick={onAddTask}>
              <Icon name="plus" size={18} />
              行動を追加
            </button>
          </div>
          <TaskList
            tasks={orderedTasks}
            toggleTask={onToggleTask}
            expanded
            disabled={pending}
            canEdit={onCanEditTask}
            onEdit={onEditTask}
          />
          <div className="action-progress-footer">
            <span>
              {doneCount} / {tasks.length}件を完了
            </span>
            <Progress value={completion} color="green" />
          </div>
        </section>
      </div>
    );
  }

  if (page === "振り返り") {
    return (
      <div className="page-content reflection-screen">
        <div className="screen-main-grid reflection-grid">
          <section className="panel screen-primary-card reflection-editor-card">
            <div className="reflection-lead">
              <span className="reflection-icon">
                <Icon name="leaf" size={30} weight="duotone" />
              </span>
              <div>
                <span className="eyebrow">{workspace} · 今週の振り返り</span>
                <h2>小さな一歩を、次の力に。</h2>
                <p>
                  できたこと、気づいたこと、次に試したいことを自由に残しましょう。
                </p>
              </div>
            </div>
            <label
              className="reflection-editor-label"
              htmlFor="weekly-reflection"
            >
              今週はどんな一歩を踏み出しましたか？
            </label>
            <textarea
              id="weekly-reflection"
              className="reflection-page-input"
              value={reflection}
              onChange={(event) => onReflectionChange(event.target.value)}
              placeholder="できたことや、次に試したいことを記録しましょう。"
            />
            <div className="reflection-actions">
              <span>
                {savedReflection && reflection === savedReflection
                  ? "保存済みです"
                  : "入力内容は下書きとして保存されます"}
              </span>
              <button
                className="primary-button"
                disabled={
                  pending ||
                  !canSaveReflection ||
                  !reflection.trim() ||
                  reflection === savedReflection
                }
                onClick={onSaveReflection}
              >
                <Icon name="check" size={18} />
                振り返りを記録
              </button>
            </div>
          </section>
          <aside className="panel screen-side-card learning-history-panel">
            <div className="section-header">
              <h2>最近の学び</h2>
              <Icon name="bulb" size={21} className="orange-text" />
            </div>
            <ul>
              {!learnings.length && (
                <li>
                  <p>振り返りを記録すると、ここに学びが届きます。</p>
                </li>
              )}
              {learnings.map((learning, index) => (
                <li key={`${learning}-${index}`}>
                  <span>{index + 1}</span>
                  <p>{learning}</p>
                </li>
              ))}
            </ul>
          </aside>
        </div>
      </div>
    );
  }

  if (page === "テンプレート") {
    return (
      <div className="page-content template-screen">
        <div className="screen-card-header template-screen-heading">
          <div>
            <span className="eyebrow">5つのスタート地点</span>
            <h2>どんな形で始めますか？</h2>
            <p>
              あとから自由に編集できるので、今の目的に近いものを選んでください。
            </p>
          </div>
        </div>
        <div className="template-screen-grid">
          {templates.map((template, index) => (
            <button
              key={template.title}
              className={`panel template-screen-card ${template.color}`}
              onClick={() => onChooseTemplate(template.title)}
            >
              <span className="template-number">0{index + 1}</span>
              <span className="template-screen-icon">
                <Icon name={template.icon} size={31} weight="duotone" />
              </span>
              <span className="template-screen-copy">
                <strong>{template.title}</strong>
                <small>{template.description}</small>
              </span>
              <span className="template-start">
                このテンプレートで始める
                <Icon name="arrow" size={16} />
              </span>
            </button>
          ))}
        </div>
      </div>
    );
  }

  if (page === "メンバー") {
    return (
      <div className="page-content members-screen">
        <section className="panel screen-primary-card members-panel">
          <div className="screen-card-header">
            <div>
              <span className="eyebrow">{workspace}</span>
              <h2>一緒に進めるメンバー</h2>
              <p>メンバー、招待、権限をこの画面で管理できます。</p>
            </div>
          </div>
          <div className="members-page-content">{membersContent}</div>
        </section>
      </div>
    );
  }

  return null;
}

function compareTasksByDone(a: Task, b: Task) {
  return Number(a.done) - Number(b.done);
}

function DetailRow({
  icon,
  label,
  children,
}: {
  icon: string;
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="detail-row">
      <div className="detail-label">
        <Icon name={icon} size={18} />
        <strong>{label}</strong>
      </div>
      <div className="detail-value">{children}</div>
    </div>
  );
}
function TaskList({
  tasks,
  toggleTask,
  expanded = false,
  disabled = false,
  onEdit,
  canEdit,
}: {
  tasks: Task[];
  toggleTask: (id: string) => void;
  expanded?: boolean;
  disabled?: boolean;
  onEdit: (id: string) => void;
  canEdit: (id: string) => boolean;
}) {
  return (
    <div className={`task-list ${expanded ? "expanded-tasks" : ""}`}>
      {!tasks.length && (
        <p className="empty-value">今日の行動はまだありません。</p>
      )}
      {tasks.map((task) => (
        <div
          className={`task-row ${task.done ? "completed" : ""}`}
          key={task.id}
        >
          <input
            type="checkbox"
            aria-label={`${task.title}を完了`}
            checked={task.done}
            disabled={disabled || !canEdit(task.id)}
            onChange={() => toggleTask(task.id)}
          />
          <button className="task-title" onClick={() => onEdit(task.id)}>
            {task.title}
            {task.recurring && <small> ↻ 習慣</small>}
            <span className="task-meta">
              {task.assigneeId && <small>担当: {task.assigneeId}</small>}
              {task.priority && (
                <small className={`priority-${task.priority}`}>
                  優先度:{" "}
                  {
                    { low: "低", medium: "中", high: "高", urgent: "緊急" }[
                      task.priority
                    ]
                  }
                </small>
              )}
              {task.dueStatus && (
                <small className={`due-${task.dueStatus}`}>
                  {task.dueStatus === "overdue"
                    ? "期限超過"
                    : task.dueStatus === "today"
                      ? "今日が期限"
                      : "7日以内"}
                </small>
              )}
            </span>
          </button>
          <Badge scope={task.scope} />
          <time>
            {task.dueDate
              ? `期限 ${dateLabel(task.dueDate)}`
              : task.time || "期限なし"}
          </time>
        </div>
      ))}
    </div>
  );
}
function ActivityList({
  activity,
  currentActor,
  full = false,
}: {
  activity: RecordEntry[];
  currentActor: string;
  full?: boolean;
}) {
  return (
    <div className={`activity-list ${full ? "full" : ""}`}>
      {!activity.length && <p className="empty-value">まだ記録はありません</p>}
      {activity.slice(0, full ? 100 : 3).map((item) => (
        <div className="activity-row" key={item.id}>
          <Avatar
            size={28}
            name={item.author === currentActor ? "あなた" : item.author}
          />
          <p>
            <strong>
              {item.author === currentActor ? "あなた" : item.author}
            </strong>
            <span>
              {item.body ||
                {
                  completion: "行動を完了しました",
                  reopen: "行動を未完了に戻しました",
                  skip: "今回は見送りました",
                  recurrence_change: "習慣の設定を変更しました",
                }[item.record_type] ||
                "記録を追加しました"}
            </span>
          </p>
          <time>{new Date(item.created_at).toLocaleDateString("ja-JP")}</time>
        </div>
      ))}
    </div>
  );
}
function Timeline({
  quarter,
  setQuarter,
  goals,
  onSelect,
}: {
  quarter: number;
  setQuarter: (n: number) => void;
  goals: Goal[];
  onSelect: (id: string) => void;
}) {
  const now = new Date();
  const start = new Date(
    now.getFullYear(),
    Math.floor(now.getMonth() / 3) * 3 + quarter * 3,
    1,
  );
  const end = new Date(start.getFullYear(), start.getMonth() + 3, 1);
  const span = end.getTime() - start.getTime();
  const rows = goals.filter(
    (g) =>
      !g.startDate ||
      !g.dueDate ||
      (new Date(g.startDate + "T00:00:00") < end &&
        new Date(g.dueDate + "T23:59:59") >= start),
  );
  const todayPosition = (100 * (now.getTime() - start.getTime())) / span;
  return (
    <>
      <div className="section-header timeline-header">
        <h3>
          {start.getFullYear()}年 {start.getMonth() + 1}月 -{" "}
          {new Date(end.getTime() - 1).getMonth() + 1}月
        </h3>
        <div className="date-controls">
          <button
            aria-label="前の3ヶ月"
            onClick={() => setQuarter(quarter - 1)}
          >
            <Icon name="left" size={14} />
          </button>
          <button onClick={() => setQuarter(0)}>今の四半期</button>
          <button
            aria-label="次の3ヶ月"
            onClick={() => setQuarter(quarter + 1)}
          >
            <Icon name="right" size={14} />
          </button>
        </div>
      </div>
      <div className="timeline">
        <div className="timeline-labels">
          <div className="month-spacer" />
          {rows.map((g) => (
            <button key={g.id} onClick={() => onSelect(g.id)}>
              <span className={`timeline-dot ${scopeClass[g.scope]}`} />
              <span>
                <strong>{g.title}</strong>
                <Badge scope={g.scope} />
              </span>
            </button>
          ))}
        </div>
        <div className="timeline-chart">
          <div className="months">
            {[0, 1, 2].map((n) => (
              <span key={n}>
                {new Date(
                  start.getFullYear(),
                  start.getMonth() + n,
                  1,
                ).getMonth() + 1}
                月
              </span>
            ))}
          </div>
          <div className="timeline-lanes">
            {todayPosition >= 0 && todayPosition <= 100 && (
              <div className="today-line" style={{ left: `${todayPosition}%` }}>
                <span>今日</span>
              </div>
            )}
            {rows.map((g) => {
              const left = g.startDate
                ? Math.max(
                    0,
                    (100 *
                      (new Date(g.startDate + "T00:00:00").getTime() -
                        start.getTime())) /
                      span,
                  )
                : 0;
              const right = g.dueDate
                ? Math.min(
                    100,
                    (100 *
                      (new Date(g.dueDate + "T23:59:59").getTime() -
                        start.getTime())) /
                      span,
                  )
                : 100;
              return (
                <div className="timeline-lane" key={g.id}>
                  <button
                    className={`timeline-bar ${scopeClass[g.scope]} ${!g.startDate || !g.dueDate ? "undated" : ""}`}
                    style={{
                      left: `${left}%`,
                      width: `${Math.max(2, right - left)}%`,
                    }}
                    onClick={() => onSelect(g.id)}
                  >
                    {!g.startDate || !g.dueDate ? "日程未定 · " : ""}
                    {g.title}
                  </button>
                </div>
              );
            })}
            {!rows.length && (
              <p className="timeline-empty">この期間の予定はありません</p>
            )}
          </div>
        </div>
      </div>
    </>
  );
}
function Modal({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  useEffect(() => {
    const element = dialog.current;
    returnFocus.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    element?.showModal();
    return () => {
      element?.close();
      window.requestAnimationFrame(() => {
        const target = returnFocus.current;
        if (target && window.getComputedStyle(target).visibility !== "hidden")
          target.focus();
        else document.querySelector<HTMLButtonElement>(".mobile-menu")?.focus();
      });
    };
  }, []);
  return (
    <dialog
      className="modal"
      ref={dialog}
      onCancel={onClose}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="modal-content">
        <div className="modal-header">
          <h2>{title}</h2>
          <button className="icon-button" aria-label="閉じる" onClick={onClose}>
            <Icon name="close" size={22} />
          </button>
        </div>
        {children}
      </div>
    </dialog>
  );
}
