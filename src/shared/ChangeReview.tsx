/**
 * Reviewing a proposed change.
 *
 * The same component is shown in the conversation and in Basepath itself, but
 * what a person can *do* differs, and deliberately so:
 *
 * - In Basepath they are signed in, on Basepath's own origin, with a session
 *   the server can verify. They can approve.
 * - In an AI host they are not. A click there reaches the server as an
 *   ordinary tool call, indistinguishable from the model's, so it is not
 *   evidence of anything. The app shows the diff and links out to approve.
 *
 * Withdrawing a proposal is available in both, because discarding a proposal
 * changes no plan data.
 */
import type { ChangeSet, ChangeRow } from "./changeView";
import { isExpired, summarize } from "./changeView";

function effectLabel(effect: ChangeRow["effect"]) {
  switch (effect) {
    case "created":
      return "追加";
    case "updated":
      return "更新";
    case "deleted":
      return "削除";
    default:
      return "変更";
  }
}

function statusLabel(change: ChangeSet) {
  switch (change.status) {
    case "approved":
      return "承認済み・適用待ち";
    case "applied":
      return "適用済み";
    case "rejected":
      return "却下済み";
    default:
      return isExpired(change) ? "期限切れ" : "承認待ち";
  }
}

function Row({ row }: { row: ChangeRow }) {
  return (
    <li className="change-row" data-effect={row.effect}>
      <div className="change-row-head">
        <span className="change-effect">{effectLabel(row.effect)}</span>
        <span className="change-title">{row.title}</span>
      </div>
      {row.fields.length > 0 && (
        <table>
          <thead>
            <tr>
              <th scope="col">項目</th>
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
      )}
      {/* A date, an owner or a target reads afterwards as something the
          person decided, so it is called out with where it came from rather
          than left as one row in a table of many. */}
      {row.guardedValues.length > 0 && (
        <p className="change-basis">
          <strong>{row.guardedValues.join("・")}</strong>
          {row.basis ? `の根拠: ${row.basis}` : "が設定されます"}
        </p>
      )}
      {row.effect === "deleted" && (
        <p className="change-warning">この項目は削除されます。</p>
      )}
    </li>
  );
}

export type ChangeReviewProps = {
  change: ChangeSet;
  workspaceName?: string;
  /** Present only where a real session can back an approval. */
  onApprove?: () => void;
  onReject?: () => void;
  /** Applying is allowed wherever the approver is; the server still checks. */
  onApply?: () => void;
  /** Where to go to approve, when approval is not possible here. */
  approveHref?: string;
  onOpenApproval?: () => void;
  busy?: boolean;
  notice?: string | null;
};

export function ChangeReview({
  change,
  workspaceName,
  onApprove,
  onReject,
  onApply,
  approveHref,
  onOpenApproval,
  busy,
  notice,
}: ChangeReviewProps) {
  const counts = summarize(change);
  const expired = isExpired(change);
  const open = change.status === "pending" || change.status === "approved";
  return (
    <section className="change-review" aria-label="変更案">
      <header>
        <h3>{change.title}</h3>
        <p className="change-meta">
          <span>{workspaceName ?? change.workspaceId}</span>
          <span>{statusLabel(change)}</span>
          {change.expiresAt && <span>期限 {change.expiresAt}</span>}
        </p>
        <p className="change-summary">
          追加{counts.created}・更新{counts.updated}・削除{counts.deleted}
        </p>
        {change.proposedBy && (
          <p className="change-origin">
            提案 {change.proposedBy}
            {change.proposedByConnection
              ? `（接続 ${change.proposedByConnection}）`
              : "（画面から）"}
          </p>
        )}
        {change.approvedBy && (
          <p className="change-origin">
            承認 {change.approvedBy}
            {change.approvedAt ? ` ・ ${change.approvedAt}` : ""}
          </p>
        )}
      </header>

      {change.assumptions.length > 0 && (
        <section className="change-assumptions">
          <h4>前提</h4>
          {/* Approving a breakdown is agreeing to the reasoning as much as to
              the rows, and reasoning that is not shown is not agreed to. */}
          <ul>
            {change.assumptions.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </section>
      )}

      {change.rows.length === 0 ? (
        <p className="change-empty">表示できる変更内容がありません。</p>
      ) : (
        <ul className="change-rows">
          {change.rows.map((row, index) => (
            <Row key={`${row.id}:${index}`} row={row} />
          ))}
        </ul>
      )}

      {expired && open && (
        <p className="change-warning" role="status">
          この変更案は期限切れです。もう一度作り直してください。
        </p>
      )}
      {notice && (
        <p className="change-notice" role="status">
          {notice}
        </p>
      )}

      <div className="change-actions">
        {onApprove && change.status === "pending" && !expired && (
          <button type="button" disabled={busy} onClick={onApprove}>
            この内容で承認する
          </button>
        )}
        {!onApprove && change.status === "pending" && !expired && (
          <>
            {onOpenApproval ? (
              <button type="button" disabled={busy} onClick={onOpenApproval}>
                Basepathで承認する
              </button>
            ) : null}
            {approveHref && (
              <a href={approveHref} target="_blank" rel="noreferrer">
                {approveHref}
              </a>
            )}
            <p className="change-note">
              承認はBasepathで行います。ここでの操作は本人確認の代わりになりません。
            </p>
          </>
        )}
        {onApply && change.status === "approved" && !expired && (
          <button type="button" disabled={busy} onClick={onApply}>
            承認済みの内容を適用する
          </button>
        )}
        {onReject && open && (
          <button
            type="button"
            className="secondary"
            disabled={busy}
            onClick={onReject}
          >
            この案を取り下げる
          </button>
        )}
      </div>
    </section>
  );
}
