/**
 * The connection consent screen, on Basepath's own origin.
 *
 * An AI host sends the person here to authorize it. This is the one place that
 * can happen, for the same reason approving a change set is: the request
 * carries their own Basepath session and the same-origin CSRF header, so the
 * server can attribute the answer to them. A screen inside the host could not
 * — a call from there reaches this server indistinguishably from the model's.
 *
 * What is granted here is a delegation, never an approval. Even with every
 * scope, the client's writes are proposals the person reviews separately.
 */
import { useCallback, useEffect, useState } from "react";
import { ApiError, request } from "./api";

export type ConsentRequest = {
  requestId: string;
  clientName: string;
  scopes: string[];
  resource: string;
  redirectUri: string;
};

/** True when the current URL is the authorization endpoint. */
export function isConsentPath(pathname: string): boolean {
  return pathname.replace(/\/$/, "") === "/oauth/authorize";
}

const SCOPE_LABELS: Record<string, { title: string; detail: string }> = {
  "pathbase.read": {
    title: "目標と行動を読む",
    detail: "目標、今日の行動、週次レビューを会話の中で表示できます。",
  },
  "pathbase.propose": {
    title: "変更案を作る",
    detail:
      "項目の追加や変更を提案できます。提案のままで、あなたが承認するまで反映されません。",
  },
  "pathbase.apply": {
    title: "承認済みの変更を反映する",
    detail:
      "あなたがBasepathで承認した内容だけを反映できます。承認そのものはここでは行えません。",
  },
  "pathbase.context": {
    title: "会話を業務コンテキストにリンクする",
    detail: "会話の対象ワークスペースをリンクします。計画そのものは変更しません。",
  },
};

function describe(scope: string) {
  return SCOPE_LABELS[scope] ?? { title: scope, detail: "" };
}

export function OAuthConsent({
  search,
  onLeave,
}: {
  /** The authorization request's query string, captured before any redirect. */
  search: string;
  onLeave: () => void;
}) {
  const [ask, setAsk] = useState<ConsentRequest | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [failure, setFailure] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const value = await request<{
        ask?: {
          request_id: string;
          client_name: string;
          scopes: string[];
          resource: string;
          redirect_uri: string;
        };
        redirect_to?: string;
      }>("GET", `/oauth/authorize${search}`);
      // A request the specification says to refuse is reported to the client
      // rather than shown here: it is the client's mistake, not the person's.
      if (value.redirect_to) {
        window.location.replace(value.redirect_to);
        return;
      }
      if (!value.ask) {
        setFailure("この接続リクエストを読み取れません。");
        return;
      }
      setAsk({
        requestId: value.ask.request_id,
        clientName: value.ask.client_name,
        scopes: value.ask.scopes,
        resource: value.ask.resource,
        redirectUri: value.ask.redirect_uri,
      });
      // Everything asked for is pre-selected; the person narrows it, and the
      // server never widens beyond what was requested.
      setSelected(value.ask.scopes);
    } catch (error) {
      const api = error as ApiError;
      setFailure(
        api.message ||
          "この接続リクエストは期限切れかもしれません。AI側から接続し直してください。",
      );
    }
  }, [search]);

  useEffect(() => {
    void load();
  }, [load]);

  const decide = useCallback(
    async (allow: boolean) => {
      if (!ask || busy) return;
      setBusy(true);
      setFailure("");
      try {
        const value = await request<{ redirect_to: string }>(
          "POST",
          "/oauth/authorize",
          {
            request_id: ask.requestId,
            scopes: allow ? selected : [],
            allow,
          },
        );
        window.location.replace(value.redirect_to);
      } catch (error) {
        const api = error as ApiError;
        setFailure(api.message || "接続を処理できませんでした。");
        setBusy(false);
      }
    },
    [ask, selected, busy],
  );

  if (failure && !ask)
    return (
      <div className="page-content consent-screen">
        <section className="panel consent-panel">
          <h2>接続できません</h2>
          <p className="auth-error" role="alert">
            {failure}
          </p>
          <div className="consent-actions">
            <button className="secondary-button" onClick={onLeave}>
              Basepathを開く
            </button>
          </div>
        </section>
      </div>
    );

  if (!ask)
    return (
      <div className="page-content consent-screen">
        <section className="panel consent-panel" aria-busy="true">
          <p>接続リクエストを確認しています…</p>
        </section>
      </div>
    );

  const host = (() => {
    try {
      return new URL(ask.redirectUri).host;
    } catch {
      return ask.redirectUri;
    }
  })();

  return (
    <div className="page-content consent-screen">
      <section className="panel consent-panel">
        <span className="eyebrow">AIクライアントの接続</span>
        <h2>{ask.clientName} を許可しますか</h2>
        <p>
          許可すると、このクライアントはあなたとして下の操作を試せるようになります。
          接続先は <code>{host}</code> です。
        </p>

        <ul className="consent-scopes">
          {ask.scopes.map((scope) => {
            const described = describe(scope);
            const checked = selected.includes(scope);
            return (
              <li key={scope}>
                <label>
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={(event) =>
                      setSelected((current) =>
                        event.target.checked
                          ? [...current, scope]
                          : current.filter((held) => held !== scope),
                      )
                    }
                  />
                  <span>
                    <strong>{described.title}</strong>
                    <small>{described.detail}</small>
                  </span>
                </label>
              </li>
            );
          })}
        </ul>

        <p className="consent-note">
          どの権限を許可しても、AIの変更は「変更案」のままです。実際に反映されるのは、
          あなたがBasepathで差分を確認して承認したときだけです。接続はいつでも設定画面で解除できます。
        </p>

        {failure && (
          <p className="auth-error" role="alert">
            {failure}
          </p>
        )}

        <div className="consent-actions">
          <button
            className="primary-button"
            disabled={busy || selected.length === 0}
            onClick={() => void decide(true)}
          >
            許可する
          </button>
          <button
            className="secondary-button"
            disabled={busy}
            onClick={() => void decide(false)}
          >
            許可しない
          </button>
        </div>
      </section>
    </div>
  );
}
