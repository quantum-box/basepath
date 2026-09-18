# PathBase

既存のパステル調のUIを維持した、React / TypeScript + Rust / Axum / SQLite + Tauri 2の目標・行動管理アプリです。目標と行動の共通モデルを使用し、認証はTachyon、営業データの参照はField APIへ接続します。

## 実装済み

- タイトルだけで項目を作成。目標・取り組み・行動・アイデア・節目を共通モデルで保存し、マップ・リスト・タイムライン・今日の行動で共有
- 5種類のテンプレート、任意の日付・時刻、週の実施回数、実施日ごとの完了・見送り・再開、アーカイブと復元
- 4種類の関連、循環検出、次の一歩、メモの下書き、学び・振り返りの追記履歴
- 自己評価と成果指標を分離。出典・日時・単位付き観測、訂正履歴、未計測表示。行動の完了で目標の達成率を変更しない
- ワークスペースの現地週による週次レビュー。行動実績、自己評価、成果指標、担当者別集計、下書き・確定・訂正履歴、印刷用要約
- SQLite永続化、トランザクション、楽観的ロック、再送の重複防止、領域ごとのアクセス制御、JSONバックアップと検証付き復元
- Tachyon OIDCログイン、PKCE・state・nonce・署名検証、サーバー側セッション。Tachyonの正規ユーザーIDから個人領域を解決
- 共有ワークスペースの作成・名前変更、TachyonユーザーID宛ての期限付き招待、参加・辞退・取り消し、オーナー／編集／閲覧の権限管理と退出。複数ワークスペースを名前で選択
- Fieldの権限付き組織一覧、営業タスクの参照、MRR / ARR / 受注残 / 売掛残 / DSOを成果指標へ記録
- Rustの共通処理を呼ぶHTTP API、Tauri IPC、ローカルstdio MCP。AIの変更案は差分表示・人の承認を経て原子的に適用
- 選択した目標・期限・直近記録に基づく30分以内の行動候補と、事実・推測・質問を分けた振り返り提案。編集・却下・差分承認に対応

## ローカル開発

Node.js 22.12以降とRustが必要です。`mise.toml`でNode 24とRust 1.95.0（CIと同じ）を固定しているため、[mise](https://mise.jdx.dev/)を使う場合は`mise install`だけで揃います。ネイティブアプリにはOSのTauri開発環境も必要です。

```sh
npm ci
npm run dev
```

Rust APIを127.0.0.1:1431、画面をlocalhost:1420で一緒に起動します。APIクレートは`pathbase-api`（通常のHTTPサーバー）と`lambda-pathbase-api`（Lambda専用）の2バイナリを持つため、開発用スクリプトは常に`--bin pathbase-api`を明示します（[scripts/api-binary.mjs](scripts/api-binary.mjs)）。ポートが既に使われている場合や、APIが起動に失敗した場合は画面だけが動く中途半端な状態にせず終了します。API用のランダムな認証情報は開発プロセス内だけで扱います。標準では明示的な`local-preview`モードで、日付を現在に合わせたサンプル領域が作られます。サンプルはゴルフ場運営のシナリオで、組織ワークスペースに「償却前利益3億円」を最上位とする目標・取り組みの多階層ツリーが入ります（`api/src/seed.json`）。`data/pathbase.sqlite3`に保存され、再起動後も残ります。サンプルの名前・写真・自己評価は実ユーザーの情報ではありません。

Tauriは`npm run tauri dev`で起動できます。debugでは同じRust処理をIPC経由で使用し、アプリデータディレクトリの`preview.sqlite3`に保存します。ブラウザ開発用DBとは別です。認証済みのデスクトップ利用は`PATHBASE_WEB_URL`で同じTachyon保護アプリを開きます。releaseはURL未設定時にローカル所有者へ切り替わりません。

## Tachyon / Field

[.env.example](.env.example)を参考に、PathBase用に登録されたOIDCクライアントと環境の接続情報を設定します。`npm run dev`は`.env` / `.env.local`の`PATHBASE_*`、`TACHYON_*`、`FIELD_*`を読みます。単独Rustプロセスには環境変数として渡してください。認証情報を`VITE_*`に置かないでください。

ログインフォームの認証先は設定で決まります。`PATHBASE_COGNITO_ISSUER`を設定すると、Cognitoの`InitiateAuth`（`USER_PASSWORD_AUTH`）を直接呼び、その access token をセッションのbearerにします。Hosted UIとAmplifyは使いません。専用のApp Clientを別途作る必要はありません。`useTachyonUserPool`付きのOAuth2Clientを登録するとTachyonが共有ユーザープールにsecretなしApp Clientを作り、`USER_PASSWORD_AUTH`も既定で有効なため、`TACHYON_OIDC_CLIENT_ID`がそのApp Client IDです。別のApp Clientを指す場合だけ`PATHBASE_COGNITO_CLIENT_ID`を設定します。issuer未設定時はTachyon OAuth2のPKCEフローになりますが、FieldはCognito発行token以外を受け付けないため、Fieldの呼び出しは401になります。`npm run preflight`が未設定を警告します。

`PATHBASE_MODE=tachyon`の場合、必要な認証設定がないと起動しません。コールバックは`PATHBASE_PUBLIC_URL/api/auth/callback`と完全一致させます。アクセストークンと選択テナントは`PATHBASE_SESSION_KEYS`でAES-256-GCM暗号化したHttpOnly Cookieに保持し、ブラウザーJavaScriptからは読めません。同じ鍵を設定した`pathbase-api`の実行環境間でセッションを引き継げます。Cookieは上流アクセストークンと同時（最大8時間）に失効し、ログアウト時に消去します。

実環境へ接続する前に`npm run preflight`を実行すると、設定形式、OIDC Discovery、Tachyonのトークン検証API、Fieldの権限付きテナント一覧APIへの到達性を確認できます。確認要求には意図的に無効な認証情報を使い、クライアントシークレット、トークン、テナント識別子は結果へ表示しません。成功後も、実ユーザーでログインしてFieldの許可・権限不足・期限切れを確認する必要があります。

本番では同一オリジンの`/api/*`をRust APIへ転送し、`/api`プレフィックスを除きます。CookieとOriginヘッダーを保持してください。セッション鍵はCloud Appのsecret/credentialとして設定し、`tachyon.yml`やソースへ書きません。

Tachyon Cloud Appは`pathbase-v2`（Cloudflare Worker、SPA配信と同一オリジンの`/api/*`転送）と`pathbase-api`（Lambda、Rust API）の2アプリで構成します。公開URLは`https://pathbase-v2.txcloud.app`です。旧Cloud Run版の`pathbase.txcloud.app`は廃止済みで、現在は経路層が`No route for: pathbase`を返します。`Dockerfile`は単一オリジンのコンテナ実行用に残してあり、`PATHBASE_WEB_ROOT`指定時だけRustサーバーがSPAと`/api/*`を同時に配信します。

業務データはSQLxでTachyon管理のTiDB（MySQLプロトコル）へ保存します。`PATHBASE_MODE`が`local-preview`以外のとき`DATABASE_URL`（または`PATHBASE_DATABASE_URL`）が必須で、未設定・接続不可はどちらも起動エラーです。ローカルSQLiteへのフォールバックはありません。`/api/health`は実際の保存先を返し、TiDB接続時は`storage: tidb` / `storage_durability: shared-durable`、明示local-preview時は`storage: sqlite` / `ephemeral-runtime`になります。スキーマは`api/migrations/{sqlite,mysql}/`のversioned migrationsで、起動時に適用されます。方言差・並行制御・移行手順は`docs/production-durability.md`に記載しています。

Fieldは現在のユーザーのTachyonトークンと正規のテナント文脈で呼び、操作ごとに権限を確認します。FieldのタスクをPathBaseで完了しても元タスクは更新しません。タスク参照の重複取り込みを防止し、観測できない値は0に変換しません。実装根拠と設定項目は[連携契約](docs/integration-contracts.md)を参照してください。

## API / MCP

[API契約と例](docs/api.md)。実行中の`/api/v1/openapi.json`は認証された利用者へOpenAPIを返します。

ローカルMCPは、ブラウザプレビューと同じ絶対DBパスを指定して起動します。stdioのためHTTP用トークンは不要です。

```sh
PATHBASE_MODE=local-preview PATHBASE_DB=/absolute/path/to/data/pathbase.sqlite3 npm run --silent api:mcp
```

remote MCP は Streamable HTTP の `/mcp`（`PATHBASE_WEB_ROOT` 使用時は `/api/mcp`）で提供できます。`PATHBASE_MCP_TOKEN`（32文字以上）、`PATHBASE_MCP_ACTOR_ID`、`PATHBASE_MCP_ALLOWED_HOSTS` をサーバー環境に設定した場合だけ有効になります。MCPクライアントは `Authorization: Bearer ...` を送ります。actorは既存のPathBaseメンバーである必要があり、各操作でもワークスペース権限が再検証されます。この固定token方式は信頼できる単一クライアント向けで、ユーザーごとのOAuth委譲ではありません。

13個のツール、項目のResource Template、3個のPromptを提供します。stdio と remote のどちらでも、MCP actor はAI agentとして扱われます。書き込みツールは提案を作り、設定画面の「AIからの変更案」で人が承認するまで反映しません。承認はAIが渡すフラグでは代用できません。rmcpのロック済みバージョンが提供するプロトコルを使用します。

## 検証

```sh
npm run check
npm run test:api
npm run build
npm run test:sites
npm run test:smoke
cargo clippy --manifest-path api/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml
```

`npm run test:smoke`はバイナリ選択（通常API / Lambda / MCPの取り違え）を静的に確認し、ビルド済みの`pathbase-api`があれば
起動・`/health`・認証済み読取・SIGTERMでの終了・設定不備での異常終了までを確認します。バイナリが無い環境では起動確認だけ
skipします。`PATHBASE_SMOKE_API_BIN`で既存バイナリを指定できます。Rustの自動ビルドは行いません。

共有DBの並行動作（別インスタンス同士のversion競合、冪等性の再送、changesetの二重適用、ロールバック、最後のオーナー、
権限剥奪後の再送）は実TiDBでのみ検証できるため、`PATHBASE_TEST_DATABASE_URL`を設定したときだけ`api/tests/tidb.rs`が動きます。
未設定ならskipします。ローカルでは`tiup playground`で起動できます。

```sh
tiup playground v8.5.8 --db 1 --kv 1 --pd 1 --without-monitor
PATHBASE_TEST_DATABASE_URL=mysql://root@127.0.0.1:4000/test \
  cargo test --manifest-path api/Cargo.toml --test tidb
```

テストはTiDB上に使い捨てのデータベースを作り、`SELECT tidb_version()`が通ることを確認します。MySQL単体では実行できません。

APIテストは一時DBとローカルの模擬OIDC / Tachyon / Fieldサーバーを使用します。`api/tests/fixtures/oidc-test-key.pem`はテスト専用に生成した公開fixtureです。実アカウントの認証情報ではありません。

GitHub ActionsではPRと`main`へのpushで、次の4ジョブを実行します。外部サービスの認証情報は不要です。

- Web：TypeScript、製品ビルド、Sites配信テスト、起動スクリプトのバイナリ選択、配信ファイルとフォントライセンスの存在確認
- Rust API：fmt、ドメイン・権限・OIDC / Field連携・MCPのテスト、clippy（警告をエラーとして扱う）
- TiDB：実TiDBコンテナに対する共有DBの並行動作とmigration、DB未設定時の起動拒否
- Browser：ビルド済みAPIの起動smoke test、Chromiumで目標とメモの永続化、行動完了、同じ領域の複数ワークスペース、モバイルナビゲーションを検証
- Desktop：macOSでRust fmtとTauriのコンパイル確認

ブラウザテストはAPIジョブでビルド済みの実際のRustバイナリを再利用し、毎回空の一時DBと専用ポート（画面1425 / API1435）で実行します。クラウド認証情報や開発用DB、`.env`は引き継ぎません。失敗時は画面・トレース・HTMLレポートを7日間保存します。GitHub ActionsはコミットSHAで固定しています。

ローカルのRust検証は変更に必要な範囲に絞り、重い全体検証はCIで行います。ブラウザテストを明示的にローカル実行する場合は、既存のAPIバイナリを`PATHBASE_E2E_API_BIN`で指定し、Chromiumを用意して`npm run test:e2e`を実行できます。テストがRustを自動ビルドすることはありません。

実際のTachyon / Field環境との疎通、TauriのGUI実行・配布用パッケージ・Windows / Linuxでの動作は、このCIの対象外です。

## 現在の範囲

実環境（`https://pathbase-v2.txcloud.app`）でのTachyonログインとテナント選択は2026-09-18に確認済みです。Fieldデータの取得は未確認です。FieldはTachyonの`/auth/v1beta/verify`へ委譲し、Cognitoユーザープールが発行したtokenしか受け付けないため、Tachyon OAuth2発行のtokenでは必ず401になります。`PATHBASE_COGNITO_ISSUER`を設定するとCognito直接認証に切り替わります（詳細は`docs/implementation-qa.md`）。複数アカウントでの招待も未検証です。招待は相手がPathBaseへログインすると画面内に届き、メールは送信しません。個人領域とローカル確認用領域は招待できません。外部通知、担当者指定、自動双方向同期、組織ポリシーの詳細設定、分散DB、ホスト型MCPは別途実装が必要です。

`.openai/hosting.json`、`worker/index.js`、`scripts/prepare-sites-build.mjs`、`tests/sites-worker.test.mjs`は既存構成を維持しています。`npm run build`は`dist/client/index.html`、`dist/server/index.js`、`dist/.openai/hosting.json`を生成します。Sites用workerは静的配信であり、それだけではRust APIは公開されません。外部へのデプロイは行っていません。

## ライセンス

独自コードは[MIT License](LICENSE)です。同梱フォントなどの第三者著作物は元のライセンスを維持します。[第三者ライセンス表記](THIRD_PARTY_NOTICES.md)を参照してください。
