import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import { Icon } from "./icons";
import { GoalMap } from "./GoalMap";
import {
  initialGoals,
  initialTasks,
  initialInitiatives,
  templates,
  learnings,
  scopeClass,
  type Goal,
  type Scope,
  type Task,
  type Initiative,
} from "./data";

type ModalState =
  | { kind: "template"; template: string }
  | { kind: "task" }
  | { kind: "initiative" }
  | { kind: "members" }
  | { kind: "settings" }
  | { kind: "learnings" }
  | { kind: "activity" }
  | { kind: "editGoal" }
  | { kind: "initiativeDetail"; id: string }
  | null;
const navigation = [
  { label: "ホーム", icon: "home" },
  { label: "目標マップ", icon: "tree" },
  { label: "タイムライン", icon: "calendar" },
  { label: "今日の行動", icon: "tasks" },
  { label: "振り返り", icon: "book" },
  { label: "テンプレート", icon: "stack" },
  { label: "メンバー", icon: "users" },
];
function Badge({ scope }: { scope: Scope }) {
  return <span className={`scope-badge ${scopeClass[scope]}`}>{scope}</span>;
}
function Avatar({
  male = false,
  size = 32,
}: {
  male?: boolean;
  size?: number;
}) {
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
  value: number;
  color?: string;
}) {
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

export function App() {
  const [goals, setGoals] = useState(initialGoals);
  const [tasks, setTasks] = useState(initialTasks);
  const [initiatives, setInitiatives] = useState(initialInitiatives);
  const [selectedId, setSelectedId] = useState("event");
  const [scope, setScope] = useState<Scope | "すべて">("すべて");
  const [workspace, setWorkspace] = useState<Scope>("個人");
  const [activeNav, setActiveNav] = useState("ホーム");
  const [activeTab, setActiveTab] = useState("タイムライン");
  const [quarter, setQuarter] = useState(0);
  const [modal, setModal] = useState<ModalState>(null);
  const [toast, setToast] = useState("");
  const [notifications, setNotifications] = useState(false);
  const [unread, setUnread] = useState(true);
  const [menu, setMenu] = useState(false);
  const [sidebar, setSidebar] = useState(false);
  const [search, setSearch] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const [nextDone, setNextDone] = useState<Record<string, boolean>>({});
  const [compact, setCompact] = useState(false);
  const [activity, setActivity] = useState<string[]>([]);
  const [reflection, setReflection] = useState("");
  const [savedReflection, setSavedReflection] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const selected = goals.find((g) => g.id === selectedId) ?? goals[0];
  const notify = useCallback((message: string) => setToast(message), []);
  const selectGoal = useCallback((id: string) => setSelectedId(id), []);
  const openInitiative = useCallback(
    (id: string) => setModal({ kind: "initiativeDetail", id }),
    [],
  );
  useEffect(() => {
    if (!toast) return;
    const timeout = setTimeout(() => setToast(""), 3200);
    return () => clearTimeout(timeout);
  }, [toast]);
  useEffect(() => {
    const handle = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        searchRef.current?.focus();
        setSearchOpen(true);
      }
      if (e.key === "Escape") {
        setModal(null);
        setMenu(false);
        setNotifications(false);
        setSearchOpen(false);
        setSidebar(false);
      }
    };
    window.addEventListener("keydown", handle);
    return () => window.removeEventListener("keydown", handle);
  }, []);
  function toggleTask(id: string) {
    setTasks((items) =>
      items.map((item) =>
        item.id === id ? { ...item, done: !item.done } : item,
      ),
    );
  }
  function navigate(label: string) {
    setActiveNav(label);
    setSidebar(false);
    if (label === "メンバー") {
      setModal({ kind: "members" });
      return;
    }
    if (
      label === "今日の行動" ||
      label === "振り返り" ||
      label === "タイムライン"
    ) {
      setActiveTab(label);
      document
        .getElementById("workspace-panels")
        ?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    } else
      document
        .getElementById(
          label === "目標マップ"
            ? "goal-map"
            : label === "テンプレート"
              ? "templates"
              : "home",
        )
        ?.scrollIntoView({ behavior: "smooth", block: "start" });
  }
  function changeScope(value: Scope | "すべて") {
    setScope(value);
    if (value !== "すべて") {
      const first = goals.find((g) => g.scope === value);
      if (first) setSelectedId(first.id);
    }
  }
  function saveForm(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const data = new FormData(e.currentTarget);
    const title = String(data.get("title") ?? "").trim();
    if (!title) return;
    const formScope = String(data.get("scope") || workspace) as Scope;
    if (modal?.kind === "template") {
      const goal: Goal = {
        id: `goal-${Date.now()}`,
        title,
        scope: formScope,
        subtitle: String(
          data.get("description") || "やりたいことを、一歩ずつ形に。",
        ),
        icon:
          modal.template === "学習計画"
            ? "graduation"
            : modal.template === "プロジェクト"
              ? "folder"
              : "target",
        purpose: String(
          data.get("description") || "この目標を通じて、よりよい未来をつくる。",
        ),
        progress: 0,
        next: "最初の一歩を決める",
        memo: "",
      };
      setGoals([...goals, goal]);
      setSelectedId(goal.id);
      setScope("すべて");
      notify("新しい目標を追加しました");
    } else if (modal?.kind === "task") {
      setTasks([
        ...tasks,
        {
          id: `task-${Date.now()}`,
          title,
          scope: formScope,
          time: String(data.get("time") || "09:00"),
          done: false,
        },
      ]);
      notify("今日の行動に追加しました");
    } else if (modal?.kind === "initiative") {
      setInitiatives([
        ...initiatives,
        {
          id: `initiative-${Date.now()}`,
          title,
          goalId: selected.id,
          icon: "flag",
          progress: 0,
        },
      ]);
      notify("取り組みを追加しました");
    } else if (modal?.kind === "editGoal") {
      setGoals(
        goals.map((g) =>
          g.id === selected.id
            ? {
                ...g,
                title,
                purpose: String(data.get("description") || ""),
                progress: Number(data.get("progress")),
              }
            : g,
        ),
      );
      setActivity((items) => ["目標の内容を更新しました", ...items]);
      notify("目標を更新しました");
    }
    setModal(null);
  }
  const results = search.trim()
    ? [
        ...goals.map((g) => ({
          id: g.id,
          title: g.title,
          scope: g.scope,
          type: "目標",
        })),
        ...tasks.map((t) => ({
          id: t.id,
          title: t.title,
          scope: t.scope,
          type: "行動",
        })),
        ...["やまだ はるか", "佐藤 健太"].map((name, i) => ({
          id: `member-${i}`,
          title: name,
          scope: "チーム" as Scope,
          type: "メンバー",
        })),
      ].filter((item) =>
        item.title.toLowerCase().includes(search.toLowerCase()),
      )
    : [];

  return (
    <div className={`app-shell ${compact ? "compact" : ""}`}>
      {sidebar && (
        <button
          className="sidebar-scrim"
          aria-label="メニューを閉じる"
          onClick={() => setSidebar(false)}
        />
      )}
      <aside className={`sidebar ${sidebar ? "is-open" : ""}`}>
        <button
          className="brand"
          onClick={() => navigate("ホーム")}
          aria-label="PathBase ホーム"
        >
          <img className="brand-mark" src="/assets/pathbase-mark.png" alt="" />
          <span>PathBase</span>
        </button>
        <div className="workspace-label">現在のワークスペース</div>
        <div className="workspace-switch">
          {(["個人", "チーム", "組織"] as Scope[]).map((item) => (
            <button
              key={item}
              className={workspace === item ? "active" : ""}
              onClick={() => {
                setWorkspace(item);
                changeScope(item);
              }}
            >
              {item}
            </button>
          ))}
          <Icon name="down" size={13} />
        </div>
        <nav className="main-nav" aria-label="メインメニュー">
          {navigation.map((item) => (
            <button
              key={item.label}
              className={activeNav === item.label ? "active" : ""}
              aria-current={activeNav === item.label ? "page" : undefined}
              onClick={() => navigate(item.label)}
            >
              <Icon
                name={item.icon}
                size={23}
                weight={activeNav === item.label ? "fill" : "regular"}
              />
              <span>{item.label}</span>
            </button>
          ))}
        </nav>
        <nav className="utility-nav" aria-label="ユーティリティ">
          <button
            onClick={() => {
              setNotifications(!notifications);
              setUnread(false);
            }}
          >
            <Icon name="bell" size={22} />
            お知らせ
          </button>
          <button
            onClick={() => {
              searchRef.current?.focus();
              setSearchOpen(true);
            }}
          >
            <Icon name="search" size={22} />
            検索
          </button>
          <button onClick={() => setModal({ kind: "settings" })}>
            <Icon name="settings" size={22} />
            設定
          </button>
        </nav>
        <button
          className="profile"
          onClick={() => setModal({ kind: "members" })}
        >
          <Avatar size={46} />
          <span>
            <strong>やまだ はるか</strong>
            <small>haruka@pathbase.io</small>
          </span>
        </button>
      </aside>

      <main className="main-content" id="home">
        <header className="hero">
          <div className="topbar">
            <button
              className="icon-button mobile-menu"
              aria-label="メニューを開く"
              onClick={() => setSidebar(true)}
            >
              <Icon name="menu" />
            </button>
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
                          } else if (result.type === "メンバー")
                            setModal({ kind: "members" });
                          else navigate("今日の行動");
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
            <div className="notification-wrap">
              <button
                className="icon-button notification-button"
                aria-label="お知らせを表示"
                onClick={() => {
                  setNotifications(!notifications);
                  setUnread(false);
                }}
              >
                <Icon name="bell" size={23} />
                {unread && <span className="unread-dot" />}
              </button>
              {notifications && (
                <div className="notification-popover">
                  <h3>
                    お知らせ <span>今日</span>
                  </h3>
                  <p>
                    <Icon name="users" size={19} />
                    佐藤 健太さんが目標にコメントしました
                  </p>
                  <p>
                    <Icon name="calendar" size={19} />
                    会場の下見予約は4月25日です
                  </p>
                  <p>
                    <Icon name="check" size={19} />
                    今週の学習を1回達成しました
                  </p>
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
              <Avatar size={34} />
            </button>
          </div>
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
        </header>

        <div className="dashboard">
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
            <div className="left-column">
              <GoalMap
                goals={goals}
                initiatives={initiatives}
                selected={selectedId}
                onSelect={selectGoal}
                scope={scope}
                setScope={changeScope}
                onInitiative={openInitiative}
              />
              <section className="panel bottom-panel" id="workspace-panels">
                <div
                  className="bottom-tabs"
                  role="tablist"
                  aria-label="計画と振り返り"
                >
                  {["タイムライン", "今日の行動", "振り返り"].map((tab) => (
                    <button
                      role="tab"
                      aria-selected={activeTab === tab}
                      key={tab}
                      className={activeTab === tab ? "active" : ""}
                      onClick={() => setActiveTab(tab)}
                    >
                      {tab}
                    </button>
                  ))}
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
                          toggleTask={toggleTask}
                          expanded
                        />
                      </>
                    ) : (
                      <>
                        <div className="section-header">
                          <h3>今週の振り返り</h3>
                          <span className="week-label">4/21 – 4/27</span>
                        </div>
                        <div className="reflection-summary">
                          <span className="reflection-icon">
                            <Icon name="leaf" size={28} weight="duotone" />
                          </span>
                          <div>
                            <strong>小さな一歩を、積み重ねる。</strong>
                            <p>今週できたこと、気づいたことを残しましょう。</p>
                          </div>
                        </div>
                        <textarea
                          className="reflection-input"
                          value={reflection}
                          onChange={(e) => setReflection(e.target.value)}
                          aria-label="今週の振り返り"
                          placeholder="今週はどんな一歩を踏み出しましたか？"
                        />
                        <button
                          className="text-link"
                          disabled={
                            !reflection.trim() || reflection === savedReflection
                          }
                          onClick={() => {
                            setSavedReflection(reflection);
                            notify("振り返りを記録しました");
                          }}
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
                    <TaskList tasks={tasks} toggleTask={toggleTask} />
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
                      {learnings.map((learning) => (
                        <li key={learning}>{learning}</li>
                      ))}
                    </ul>
                  </div>
                </div>
              </section>
            </div>
            <aside
              className={`panel detail-panel ${scopeClass[selected.scope]}`}
              aria-label="選択した目標の詳細"
            >
              <div className="detail-heading">
                <span className={`detail-icon ${scopeClass[selected.scope]}`}>
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
                          setNextDone({ ...nextDone, [selected.id]: false });
                          setMenu(false);
                          notify("次の一歩を未完了に戻しました");
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
                <DetailRow icon="chart" label="進捗">
                  <Progress
                    value={selected.progress}
                    color={scopeClass[selected.scope]}
                  />
                </DetailRow>
                <DetailRow icon="rocket" label="次の一歩">
                  <label
                    className={`next-step ${nextDone[selected.id] ? "done" : ""}`}
                  >
                    <input
                      type="checkbox"
                      checked={!!nextDone[selected.id]}
                      onChange={(e) => {
                        setNextDone({
                          ...nextDone,
                          [selected.id]: e.target.checked,
                        });
                        if (e.target.checked) {
                          notify("一歩前進しました！");
                          setActivity((items) => [
                            "次の一歩を完了しました",
                            ...items,
                          ]);
                        }
                      }}
                    />
                    <span>
                      {selected.next}
                      <small className="date-chip">4月25日（金）</small>
                    </span>
                  </label>
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
                            <Icon name={i.icon} size={17} weight="duotone" />
                          </span>
                          {i.title.replace("\n", "")}
                        </button>
                      ))}
                    {selected.id === "event" && (
                      <button onClick={() => setModal({ kind: "initiative" })}>
                        <span className="related-icon blue">
                          <Icon name="link" size={17} />
                        </span>
                        地域の協力パートナーを探す
                      </button>
                    )}
                    <button
                      className="add-link"
                      onClick={() => setModal({ kind: "initiative" })}
                    >
                      <Icon name="plus" size={16} />
                      取り組みを追加
                    </button>
                  </div>
                </DetailRow>
                <DetailRow icon="note" label="メモ">
                  <textarea
                    key={selected.id}
                    className="field-box memo-input"
                    aria-label="目標のメモ"
                    value={selected.memo}
                    placeholder="この目標についてメモを残す"
                    onChange={(e) =>
                      setGoals(
                        goals.map((g) =>
                          g.id === selected.id
                            ? { ...g, memo: e.target.value }
                            : g,
                        ),
                      )
                    }
                  />
                </DetailRow>
              </div>
              <div className="activity-section">
                <div className="section-header">
                  <h3>
                    <Icon name="shield" size={19} />
                    アクティビティ
                  </h3>
                  <TextLink onClick={() => setModal({ kind: "activity" })} />
                </div>
                <ActivityList activity={activity} />
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
          </div>
        </div>
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
                : modal.kind === "initiative"
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
                            : "取り組みの詳細"
          }
        >
          {(modal.kind === "template" ||
            modal.kind === "task" ||
            modal.kind === "initiative" ||
            modal.kind === "editGoal") && (
            <form onSubmit={saveForm} className="editor-form">
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
              <label>
                {modal.kind === "task"
                  ? "どんな行動をしますか？"
                  : modal.kind === "initiative"
                    ? "取り組みの名前"
                    : "目標の名前"}
                <input
                  name="title"
                  autoFocus
                  required
                  maxLength={60}
                  defaultValue={modal.kind === "editGoal" ? selected.title : ""}
                  placeholder={
                    modal.kind === "task"
                      ? "例：英会話を30分学習する"
                      : "例：地域の人がつながる場所をつくる"
                  }
                />
              </label>
              {(modal.kind === "template" || modal.kind === "task") && (
                <label>
                  ワークスペース
                  <select name="scope" defaultValue={workspace}>
                    <option>個人</option>
                    <option>チーム</option>
                    <option>組織</option>
                  </select>
                </label>
              )}
              {(modal.kind === "template" || modal.kind === "editGoal") && (
                <label>
                  この目標の目的
                  <textarea
                    name="description"
                    rows={3}
                    defaultValue={
                      modal.kind === "editGoal" ? selected.purpose : ""
                    }
                    placeholder="どんな未来につなげたいですか？"
                  />
                </label>
              )}
              {modal.kind === "task" && (
                <label>
                  取り組む時間
                  <input
                    type="time"
                    name="time"
                    defaultValue="09:00"
                    required
                  />
                </label>
              )}
              {modal.kind === "editGoal" && (
                <label>
                  進捗（%）
                  <input
                    type="number"
                    name="progress"
                    min={0}
                    max={100}
                    required
                    defaultValue={selected.progress}
                  />
                </label>
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
            </form>
          )}
          {modal.kind === "members" && (
            <div className="members-list">
              <p className="modal-intro">一人ひとりの一歩を、チームの力に。</p>
              <div>
                <Avatar size={44} />
                <span>
                  <strong>
                    やまだ はるか <small>あなた</small>
                  </strong>
                  <p>haruka@pathbase.io</p>
                </span>
                <Badge scope="チーム" />
              </div>
              <div>
                <Avatar male size={44} />
                <span>
                  <strong>佐藤 健太</strong>
                  <p>kenta@pathbase.io</p>
                </span>
                <Badge scope="チーム" />
              </div>
              <p className="demo-note">サンプルワークスペース · メンバー2名</p>
            </div>
          )}
          {modal.kind === "settings" && (
            <div className="settings-list">
              <div>
                <span>
                  <strong>コンパクト表示</strong>
                  <p>タスクの行間を小さく表示します</p>
                </span>
                <input
                  type="checkbox"
                  checked={compact}
                  onChange={(e) => setCompact(e.target.checked)}
                  aria-label="コンパクト表示"
                />
              </div>
              <div>
                <span>
                  <strong>表示言語</strong>
                  <p>日本語</p>
                </span>
                <span>日本語</span>
              </div>
              <p className="demo-note">
                PathBase 0.1.0 · React + Tauri
                <br />
                変更内容はこのセッション中のみ保持されます。
              </p>
            </div>
          )}
          {modal.kind === "learnings" && (
            <ul className="learning-modal-list">
              {learnings.map((item, index) => (
                <li key={item}>
                  <span className="related-icon orange">
                    <Icon name="bulb" size={22} weight="duotone" />
                  </span>
                  <div>
                    {item}
                    <small>
                      4月{24 - index}日 · {index === 2 ? "個人" : "チーム"}
                      の学び
                    </small>
                  </div>
                </li>
              ))}
            </ul>
          )}
          {modal.kind === "activity" && (
            <ActivityList activity={activity} full />
          )}
          {modal.kind === "initiativeDetail" && (
            <InitiativeDetails
              initiative={initiatives.find((i) => i.id === modal.id)!}
              onUpdate={(value) => {
                setInitiatives(
                  initiatives.map((i) =>
                    i.id === modal.id ? { ...i, progress: value } : i,
                  ),
                );
                notify("取り組みの進捗を更新しました");
              }}
            />
          )}
        </Modal>
      )}
    </div>
  );
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
}: {
  tasks: Task[];
  toggleTask: (id: string) => void;
  expanded?: boolean;
}) {
  return (
    <div className={`task-list ${expanded ? "expanded-tasks" : ""}`}>
      {tasks.map((task) => (
        <label
          className={`task-row ${task.done ? "completed" : ""}`}
          key={task.id}
        >
          <input
            type="checkbox"
            checked={task.done}
            onChange={() => toggleTask(task.id)}
          />
          <span className="task-title">{task.title}</span>
          <Badge scope={task.scope} />
          <time>今日 {task.time.replace(/^0/, "")}</time>
        </label>
      ))}
    </div>
  );
}
function ActivityList({
  activity,
  full = false,
}: {
  activity: string[];
  full?: boolean;
}) {
  const entries = [
    ...activity.map((text) => ({
      name: "やまだ はるか",
      text,
      date: "たった今",
      male: false,
    })),
    {
      name: "やまだ はるか",
      text: "会場候補をリストアップしました",
      date: "2日前",
      male: false,
    },
    {
      name: "佐藤 健太",
      text: "この目標にコメントしました",
      date: "3日前",
      male: true,
    },
    {
      name: "やまだ はるか",
      text: "進捗を30% → 40%に更新しました",
      date: "4日前",
      male: false,
    },
  ];
  return (
    <div className={`activity-list ${full ? "full" : ""}`}>
      {entries.slice(0, full ? 20 : 3).map((item, index) => (
        <div className="activity-row" key={index}>
          <Avatar male={item.male} size={28} />
          <p>
            <strong>{item.name}</strong>
            <span>{item.text}</span>
          </p>
          <time>{item.date}</time>
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
  setQuarter: (value: number) => void;
  goals: Goal[];
  onSelect: (id: string) => void;
}) {
  const start = new Date(2025, 3 + quarter * 3, 1);
  const year = start.getFullYear();
  const month = start.getMonth() + 1;
  return (
    <>
      <div className="section-header timeline-header">
        <h3>
          {year}年 {month}月 - {month + 2}月
        </h3>
        <div className="date-controls">
          <button
            aria-label="前の3ヶ月"
            onClick={() => setQuarter(quarter - 1)}
          >
            <Icon name="left" size={14} />
          </button>
          <button onClick={() => setQuarter(0)}>今後3ヶ月</button>
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
          {goals.slice(0, 3).map((goal) => (
            <button key={goal.id} onClick={() => onSelect(goal.id)}>
              <span className={`timeline-dot ${scopeClass[goal.scope]}`} />
              <span>
                <strong>{goal.title}</strong>
                <Badge scope={goal.scope} />
              </span>
            </button>
          ))}
        </div>
        <div className="timeline-chart">
          <div className="months">
            {[month, month + 1, month + 2].map((m) => (
              <span key={m}>{m}月</span>
            ))}
          </div>
          <div className="timeline-lanes">
            {quarter === 0 ? (
              <>
                <div className="today-line">
                  <span>今週</span>
                </div>
                <div className="timeline-lane">
                  <button
                    className="timeline-bar blue"
                    style={{ left: "2%", width: "47%" }}
                    onClick={() => onSelect("english")}
                  >
                    英会話の基礎学習
                  </button>
                </div>
                <div className="timeline-lane">
                  <button
                    className="timeline-bar purple"
                    style={{ left: "14%", width: "39%" }}
                    onClick={() => onSelect("event")}
                  >
                    イベント準備
                  </button>
                  <button
                    className="timeline-bar purple event-bar"
                    style={{ left: "56%", width: "39%" }}
                    onClick={() => onSelect("event")}
                  >
                    イベント開催
                    <Icon name="sparkle" size={17} />
                  </button>
                </div>
                <div className="timeline-lane">
                  <button
                    className="timeline-bar green"
                    style={{ left: "7%", width: "50%" }}
                    onClick={() => onSelect("business")}
                  >
                    ユーザーインタビュー
                  </button>
                  <button
                    className="timeline-bar green"
                    style={{ left: "60%", width: "40%" }}
                    onClick={() => onSelect("business")}
                  >
                    MVP開発
                  </button>
                </div>
              </>
            ) : (
              <div className="timeline-empty">
                <Icon name="calendar" size={26} />
                <span>この期間の予定はありません</span>
                <button onClick={() => setQuarter(0)}>現在の計画に戻る</button>
              </div>
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
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
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
function InitiativeDetails({
  initiative,
  onUpdate,
}: {
  initiative: Initiative;
  onUpdate: (value: number) => void;
}) {
  const [progress, setProgress] = useState(initiative.progress);
  return (
    <div className="initiative-detail">
      <span className="detail-icon purple">
        <Icon name={initiative.icon} size={30} weight="duotone" />
      </span>
      <h3>{initiative.title.replace("\n", "")}</h3>
      <p>小さな取り組みを、目標につなげていきましょう。</p>
      <label htmlFor="initiative-progress">進捗を更新</label>
      <Progress value={progress} />
      <input
        id="initiative-progress"
        type="range"
        min={0}
        max={100}
        step={5}
        value={progress}
        onChange={(e) => setProgress(Number(e.target.value))}
      />
      <button className="primary-button" onClick={() => onUpdate(progress)}>
        <Icon name="check" size={18} />
        進捗を保存
      </button>
    </div>
  );
}
