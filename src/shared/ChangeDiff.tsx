import type { CSSProperties } from "react";
import type {
  BranchImpact,
  ChangeInterpretation,
  ChangeRow,
  ChangeSet,
} from "./changeView";

type Meaning =
  | "addition"
  | "content"
  | "structure"
  | "relationship"
  | "hold"
  | "retire"
  | "state"
  | "deletion"
  | "question"
  | "conflict"
  | "unknown";

const meaningLabels: Record<Meaning, string> = {
  addition: "追加",
  content: "内容変更",
  structure: "構造変更",
  relationship: "関係変更",
  hold: "保留候補",
  retire: "終了候補",
  state: "状態変更",
  deletion: "削除",
  question: "確認待ち",
  conflict: "矛盾・確認待ち",
  unknown: "要確認",
};

const statusLabels: Record<ChangeInterpretation["status"], string> = {
  decided: "決定として記録",
  considering: "検討中",
  hypothesis: "仮説",
  suggested: "AI提案",
  question: "未回答の問い",
  conflict: "矛盾・確認待ち",
};

const originLabels: Record<ChangeInterpretation["origin"], string> = {
  person: "本人の発言として記録",
  assistant: "AIの提案",
  inference: "AIの推論",
};

function meaningFor(row: ChangeRow): Meaning {
  if (row.interpretations.some(({ status }) => status === "conflict")) {
    return "conflict";
  }
  if (row.interpretations.some(({ status }) => status === "question")) {
    return "question";
  }
  if (row.collection === "relations") return "relationship";
  if (row.effect === "created") return "addition";
  if (row.effect === "deleted") return "deletion";
  if (
    row.fields.some(({ field }) => field === "親項目" || field === "並び順")
  ) {
    return "structure";
  }
  if (row.fields.some(({ field }) => field === "関連")) return "relationship";

  const after = row.afterSnapshot as Record<string, unknown> | null;
  const state = typeof after?.state === "string" ? after.state : "";
  if (after?.archived_at) return "retire";
  if (["paused", "on_hold", "hold", "保留"].includes(state)) return "hold";
  if (["abandoned", "retired", "archived", "見送り", "終了"].includes(state)) {
    return "retire";
  }
  if (row.fields.some(({ field }) => field === "状態")) return "state";
  if (row.fields.length > 0) return "content";
  return "unknown";
}

function safeHttpUrl(value: string): string | null {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "https:" || parsed.protocol === "http:"
      ? parsed.toString()
      : null;
  } catch {
    return null;
  }
}

function FieldTable({ row }: { row: ChangeRow }) {
  if (
    !row.fields.length ||
    (row.effect === "deleted" && row.collection !== "relations")
  ) {
    return null;
  }
  return (
    <table className="change-diff-fields">
      <thead>
        <tr>
          <th scope="col">変わる点</th>
          <th scope="col">変更前</th>
          <th scope="col">変更後</th>
        </tr>
      </thead>
      <tbody>
        {row.fields.map((field) => (
          <tr key={field.field}>
            <th scope="row">{field.field}</th>
            <td>{field.before}</td>
            <td>{field.after}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function ImpactSide({
  label,
  impact,
}: {
  label: string;
  impact: BranchImpact;
}) {
  return (
    <div className="change-impact-side">
      <strong>{label}</strong>
      <span>配下 {impact.descendantCount}項目</span>
      <span>行動 {impact.actionCount}件</span>
      <span>依存 {impact.dependencyCount}件</span>
      <span>寄与 {impact.contributionCount}件</span>
      {impact.items.length > 0 && (
        <ul aria-label={`${label}の下位項目`}>
          {impact.items.map((item) => (
            <li
              key={item.id}
              style={
                {
                  paddingInlineStart: `${8 + Math.min(item.depth, 5) * 8}px`,
                } as CSSProperties
              }
            >
              {item.title}
              {item.kind === "action" ? "（行動）" : ""}
            </li>
          ))}
        </ul>
      )}
      {impact.truncated && (
        <small>一覧は先頭12件です。項目数は全件を数えています。</small>
      )}
    </div>
  );
}

function Impact({ row }: { row: ChangeRow }) {
  if (!row.impact) return null;
  const { before, after } = row.impact;
  const hasContext =
    before.descendantCount > 0 ||
    after.descendantCount > 0 ||
    before.dependencyCount > 0 ||
    after.dependencyCount > 0 ||
    before.contributionCount > 0 ||
    after.contributionCount > 0;
  if (!hasContext) return null;
  return (
    <section className="change-impact" aria-label="構造上の影響範囲">
      <h5>構造上の影響範囲</h5>
      <p>
        この項目につながる下位項目・関係の実数です。日付や状態は自動変更しません。
      </p>
      <div className="change-impact-sides">
        <ImpactSide label="変更前" impact={before} />
        <ImpactSide label="変更後" impact={after} />
      </div>
    </section>
  );
}

function Interpretation({ value }: { value: ChangeInterpretation }) {
  const href = value.sourceUrl ? safeHttpUrl(value.sourceUrl) : null;
  return (
    <li className="change-interpretation">
      <p>
        <strong>{statusLabels[value.status]}</strong>
        <span>{originLabels[value.origin]}</span>
        {value.speaker && <span>話者: {value.speaker}</span>}
        {value.at && <time>{value.at}</time>}
      </p>
      {value.sourceRef && (
        <p>
          <strong>会話の範囲:</strong> {value.sourceRef}
        </p>
      )}
      {href && (
        <p>
          <strong>出典:</strong>{" "}
          <a href={href} target="_blank" rel="noreferrer">
            {value.sourceUrl}
          </a>
        </p>
      )}
      {value.quote && <blockquote>{value.quote}</blockquote>}
      {value.reason && (
        <p>
          <strong>理由:</strong> {value.reason}
        </p>
      )}
      {(value.sourceRef || value.sourceUrl || value.quote) && (
        <small>
          提案者が添付した参照情報です。会話本文との一致は検証していません。
        </small>
      )}
    </li>
  );
}

function ChangeCard({
  row,
  conversationLinked,
}: {
  row: ChangeRow;
  conversationLinked: boolean;
}) {
  const meaning = meaningFor(row);
  return (
    <li className="change-diff-card" data-meaning={meaning}>
      <header>
        <span className="change-meaning-label">{meaningLabels[meaning]}</span>
        <strong>{row.title}</strong>
        {row.steps > 1 && <small>{row.steps}操作を集約</small>}
      </header>
      <FieldTable row={row} />
      {row.effect === "deleted" && row.collection !== "relations" && (
        <p className="change-warning">この項目は削除されます。</p>
      )}
      {row.matchRationale.map((rationale, index) => (
        <p className="change-match-rationale" key={`${rationale}:${index}`}>
          <strong>照合理由:</strong> {rationale}
        </p>
      ))}
      {row.guardedValues.length > 0 && (
        <p className="change-basis">
          <strong>{row.guardedValues.join("・")}</strong>
          {row.basis.length > 0
            ? `の根拠: ${row.basis.join(" / ")}`
            : "が設定されます"}
        </p>
      )}
      {row.basis.length > 0 && row.guardedValues.length === 0 && (
        <p className="change-basis">
          <strong>提案の根拠:</strong> {row.basis.join(" / ")}
        </p>
      )}
      {row.interpretations.length > 0 ? (
        <ul className="change-interpretations" aria-label="会話上の位置づけ">
          {row.interpretations.map((value, index) => (
            <Interpretation
              key={`${value.status}:${value.sourceRef}:${index}`}
              value={value}
            />
          ))}
        </ul>
      ) : conversationLinked ? (
        <p className="change-evidence-missing">
          会話範囲と「決定・仮説・AI提案」の分類は添付されていません。
        </p>
      ) : null}
      <Impact row={row} />
    </li>
  );
}

export function ChangeDiff({ change }: { change: ChangeSet }) {
  const isNoChange = change.status === "no_change";
  const counts = new Map<Meaning, number>();
  for (const row of change.rows) {
    const meaning = meaningFor(row);
    counts.set(meaning, (counts.get(meaning) ?? 0) + 1);
  }
  return (
    <section className="change-diff" aria-label="今回の変更">
      <header className="change-diff-heading">
        <h4>今回の変更</h4>
        <p>
          {isNoChange
            ? "この会話では計画を変更しません。"
            : `${change.rows.length}件の対象をまとめて表示しています。`}
        </p>
      </header>
      {counts.size > 0 && (
        <ul className="change-diff-summary" aria-label="変更の種類と件数">
          {[...counts.entries()].map(([meaning, count]) => (
            <li key={meaning}>
              <strong>{count}</strong> {meaningLabels[meaning]}
            </li>
          ))}
        </ul>
      )}
      {change.rows.length === 0 ? (
        <p className="change-empty">
          変更はありません。現在の計画はそのままです。
        </p>
      ) : (
        <ul className="change-diff-list">
          {change.rows.map((row, index) => (
            <ChangeCard
              key={`${row.collection}:${row.id}:${index}`}
              row={row}
              conversationLinked={Boolean(change.conversationId)}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
