import { useMemo, useState } from "react";
import { Icon } from "./icons";
import { dateLabel, localDate, uiId, type Item, type RecordEntry } from "./api";

type CalendarItem = { item: Item; label: string; date: string };
const dayMs = 86_400_000;
const parseDay = (value: string) => new Date(`${value}T12:00:00Z`);
const dayKey = (date: Date) => date.toISOString().slice(0, 10);
const addDays = (value: string, count: number) =>
  dayKey(new Date(parseDay(value).getTime() + count * dayMs));
const mondayIndex = (value: string) => (parseDay(value).getUTCDay() + 6) % 7;

function latestOccurrence(records: RecordEntry[], item: Item, date: string) {
  return records
    .filter((record) => record.occurrence_key === `${item.id}:${date}`)
    .sort((a, b) => b.created_at.localeCompare(a.created_at))[0];
}

function isHabitDue(item: Item, date: string) {
  const rule = item.fields.recurrence;
  if (!rule || item.kind !== "action") return false;
  if (item.start_date && date < item.start_date) return false;
  if (item.due_date && date > item.due_date) return false;
  return (
    rule.mode === "period_quota" || rule.weekdays.includes(mondayIndex(date))
  );
}

export function habitStats(item: Item, records: RecordEntry[], today: string) {
  const rule = item.fields.recurrence;
  if (!rule) return { streak: 0, weekDone: 0, weekTarget: 0 };
  const monday = addDays(today, -mondayIndex(today));
  const completedInRange = (start: string, end: string) =>
    Array.from(new Set(records.filter((record) => record.occurrence_key?.startsWith(`${item.id}:`)).map((record) => record.occurrence_key!.slice(-10))))
      .filter((date) => date >= start && date <= end && latestOccurrence(records, item, date)?.record_type === "completion").length;
  const weekDone = completedInRange(monday, addDays(monday, 6));
  let streak = 0;
  if (rule.mode === "fixed_schedule") {
    for (let offset = 0; offset < 370; offset++) {
      const date = addDays(today, -offset);
      if (!isHabitDue(item, date)) continue;
      if (latestOccurrence(records, item, date)?.record_type !== "completion")
        break;
      streak++;
    }
  } else {
    for (let offset = 0; offset < 53; offset++) {
      const start = addDays(monday, -offset * 7);
      const end = addDays(start, 6);
      const count = completedInRange(start, end);
      if (count < rule.times_per_week) break;
      streak++;
    }
  }
  return { streak, weekDone, weekTarget: rule.times_per_week };
}

export function CalendarView({
  items,
  records,
  timezone,
  onOpen,
  onToday,
}: {
  items: Item[];
  records: RecordEntry[];
  timezone: string;
  onOpen: (id: string) => void;
  onToday: () => void;
}) {
  const today = localDate(timezone);
  const [cursor, setCursor] = useState(today);
  const [mode, setMode] = useState<"month" | "week">(
    window.matchMedia("(max-width: 540px)").matches ? "week" : "month",
  );
  const start = useMemo(() => {
    if (mode === "week") return addDays(cursor, -mondayIndex(cursor));
    const first = `${cursor.slice(0, 7)}-01`;
    return addDays(first, -mondayIndex(first));
  }, [cursor, mode]);
  const days = Array.from({ length: mode === "week" ? 7 : 42 }, (_, index) =>
    addDays(start, index),
  );
  const scheduled = useMemo(() => {
    const map = new Map<string, CalendarItem[]>();
    const push = (date: string, item: Item, label: string) => {
      if (!days.includes(date)) return;
      map.set(date, [...(map.get(date) || []), { date, item, label }]);
    };
    for (const item of items.filter((entry) => !entry.archived_at)) {
      if (item.start_date) push(item.start_date, item, "開始");
      if (item.due_date) push(item.due_date, item, "期限");
      if (item.scheduled_date) push(item.scheduled_date, item, "予定");
      if (item.fields.recurrence) {
        for (const date of days)
          if (isHabitDue(item, date)) push(date, item, "習慣");
      }
    }
    return map;
  }, [days.join("|"), items]);
  const undated = items.filter(
    (item) =>
      !item.archived_at &&
      !item.start_date &&
      !item.due_date &&
      !item.scheduled_date &&
      !item.fields.recurrence,
  );
  const habits = items.filter(
    (item) => item.fields.recurrence && !item.archived_at,
  );
  const move = (amount: number) =>
    setCursor(
      mode === "week"
        ? addDays(cursor, amount * 7)
        : dayKey(
            new Date(
              Date.UTC(
                Number(cursor.slice(0, 4)),
                Number(cursor.slice(5, 7)) - 1 + amount,
                1,
              ),
            ),
          ),
    );
  const title =
    mode === "week"
      ? `${dateLabel(start)}〜${dateLabel(addDays(start, 6))}`
      : `${Number(cursor.slice(0, 4))}年${Number(cursor.slice(5, 7))}月`;
  return (
    <div className="calendar-layout">
      <section className="calendar-main">
        <div className="calendar-toolbar">
          <div>
            <span className="eyebrow">{timezone}</span>
            <h2>{title}</h2>
          </div>
          <div className="calendar-actions">
            <div
              className="calendar-mode"
              role="tablist"
              aria-label="カレンダー表示"
            >
              <button
                role="tab"
                aria-selected={mode === "month"}
                onClick={() => setMode("month")}
              >
                月
              </button>
              <button
                role="tab"
                aria-selected={mode === "week"}
                onClick={() => setMode("week")}
              >
                週
              </button>
            </div>
            <button aria-label="前の期間" onClick={() => move(-1)}>
              <Icon name="left" size={15} />
            </button>
            <button onClick={() => setCursor(today)}>今日</button>
            <button aria-label="次の期間" onClick={() => move(1)}>
              <Icon name="right" size={15} />
            </button>
          </div>
        </div>
        <div className={`calendar-grid ${mode}`}>
          {["月", "火", "水", "木", "金", "土", "日"].map((label) => (
            <strong className="calendar-weekday" key={label}>
              {label}
            </strong>
          ))}
          {days.map((date) => {
            const entries = scheduled.get(date) || [];
            return (
              <div
                className={`calendar-day ${date === today ? "today" : ""} ${mode === "month" && date.slice(0, 7) !== cursor.slice(0, 7) ? "outside" : ""}`}
                key={date}
              >
                <button
                  className="calendar-date"
                  onClick={date === today ? onToday : undefined}
                  aria-label={date === today ? `${date} 今日の行動へ` : date}
                >
                  {Number(date.slice(-2))}
                </button>
                <div className="calendar-events">
                  {entries
                    .slice(0, mode === "month" ? 4 : 12)
                    .map(({ item, label }, index) => {
                      const record = item.fields.recurrence
                        ? latestOccurrence(records, item, date)
                        : undefined;
                      return (
                        <button
                          className={`calendar-event ${item.kind} ${record?.record_type || ""}`}
                          key={`${item.id}:${label}:${index}`}
                          onClick={() => onOpen(uiId(item))}
                          title={item.title}
                        >
                          <small>{label}</small>
                          <span>{item.title}</span>
                        </button>
                      );
                    })}
                  {entries.length > (mode === "month" ? 4 : 12) && (
                    <span className="calendar-more">
                      ほか{entries.length - 4}件
                    </span>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </section>
      <aside className="calendar-side">
        <div className="calendar-side-section">
          <div className="section-header">
            <h3>習慣トラッカー</h3>
            <span className="eyebrow">今週</span>
          </div>
          {!habits.length && (
            <p className="empty-value">習慣はまだありません。</p>
          )}
          {habits.map((item) => {
            const stats = habitStats(item, records, today);
            return (
              <button
                className="habit-row"
                key={item.id}
                onClick={() => onOpen(uiId(item))}
              >
                <span>
                  <strong>{item.title}</strong>
                  <small>
                    {item.fields.recurrence?.mode === "fixed_schedule"
                      ? "曜日固定"
                      : `週${stats.weekTarget}回`}
                  </small>
                </span>
                <span className="habit-score">
                  <strong>
                    {stats.weekDone}/{stats.weekTarget}
                  </strong>
                  <small>
                    {stats.streak}
                    {item.fields.recurrence?.mode === "fixed_schedule"
                      ? "回"
                      : "週"}
                    継続
                  </small>
                </span>
              </button>
            );
          })}
        </div>
        <div className="calendar-side-section">
          <div className="section-header">
            <h3>未予定</h3>
            <span className="eyebrow">{undated.length}件</span>
          </div>
          {!undated.length && (
            <p className="empty-value">日付未設定の項目はありません。</p>
          )}
          {undated.slice(0, 20).map((item) => (
            <button
              className="unscheduled-row"
              key={item.id}
              onClick={() => onOpen(uiId(item))}
            >
              <span>{item.title}</span>
              <small>
                {item.kind === "action"
                  ? "行動"
                  : item.kind === "milestone"
                    ? "節目"
                    : "目標・計画"}
              </small>
            </button>
          ))}
        </div>
      </aside>
    </div>
  );
}
