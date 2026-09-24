/**
 * Reviewing a proposed change.
 *
 * This component is used by Basepath's own-origin approval screen. The MCP
 * server returns change data and approval links as tool data; it does not
 * publish this component as an embedded host UI.
 *
 * Withdrawing a proposal changes no plan data, so the MCP reject tool can
 * expose that action independently of this approval screen.
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
  // Said differently from an approval, because they are different acts and the
  // person reading their own history needs to be able to tell which happened.
  if (change.status === "applied" && change.autoApplied) {
    return "自動反映済み";
  }
  switch (change.status) {
    case "approved":
      // Approving applies. Anything still sitting here was approved back when
      // that was not true, and it is not in the plan — which is what the
      // person needs told, rather than a word that sounds finished.
      return "承認済み・未反映";
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
      {row.matchRationale && (
        <p className="change-match-rationale">
          <strong>照合理由：</strong>{row.matchRationale}
        </p>
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
  /**
   * Present only where a real session can back an approval. Approving here
   * also applies: the person is looking at the diff and has decided.
   */
  onApprove?: () => void;
  onReject?: () => void;
  /**
   * For a proposal approved back when approving did not apply. Allowed
   * wherever the approver is; the server still checks.
   */
  onApply?: () => void;
  /**
   * Reflect a proposal that falls inside a range the person set in advance.
   *
   * Present only when the server said this one is covered. The server checks
   * again when it is called; this prop decides whether a control is shown, and
   * never whether it is allowed.
   */
  onAutoApply?: () => void;
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
  onAutoApply,
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

      {/* What actually happened, said where the person just pressed the
          button. "I clicked and I do not know what it did" is the failure this
          replaces, and it is the same failure whichever way it was applied. */}
      {change.status === "applied" && (
        <p className="change-applied" role="status">
          {change.autoApplied
            ? "この内容は、事前に決めた範囲としてBasepathに反映されました。差分はこのまま残るので、あとから確認できます。"
            : "この内容はBasepathに反映されました。"}
          {change.appliedAt ? ` ・ ${change.appliedAt}` : ""}
        </p>
      )}
      {change.status === "rejected" && (
        <p className="change-notice" role="status">
          この変更案は取り下げられました。反映されていません。
        </p>
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
            この内容で承認して反映する
          </button>
        )}
        {!onApprove &&
          change.status === "pending" &&
          !expired &&
          (change.autoApplyEligible && onAutoApply ? (
            <>
              <button type="button" disabled={busy} onClick={onAutoApply}>
                この内容を反映する
              </button>
              <p className="change-note">
                この変更案は、あなたがBasepathで事前に決めた範囲に入っています。
                ここでの操作は承認の代わりではなく、その範囲での反映を実行するだけです。
                範囲はBasepathの設定からいつでも解除できます。
              </p>
              {/* Still offered. A range is a decision about a kind of change,
                  not a reason to stop showing where the full history is. */}
              {approveHref && (
                <a href={approveHref} target="_blank" rel="noreferrer">
                  Basepathで確認する
                </a>
              )}
            </>
          ) : (
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
          ))}
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
