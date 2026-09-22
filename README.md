# PathBase

既存のパステル調のUIを維持した、React / TypeScript + Rust / Axum / SQLite + Tauri 2の目標・行動管理アプリです。目標と行動の共通モデルを使用し、認証はTachyon、営業データの参照はField APIへ接続します。

## 実装済み

- タイトルだけで項目を作成。目標・取り組み・行動・アイデア・節目を共通モデルで保存し、マップ・リスト・タイムライン・今日の行動で共有
- 5種類のテンプレート、任意の日付・時刻、週の実施回数、実施日ごとの完了・見送り・再開、アーカイブと復元
- 4種類の関連、循環検出、次の一歩、メモの下書き、学び・振り返りの追記履歴
- 自己評価と成果指標を分離。出典・日時・単位付き観測、訂正履歴、未計測表示。行動の完了で目標の達成率を変更しない
- Personal Memory（事実・好み・決定・学び・背景・出来事）。**個人ワークスペース専用**で、共有側へ移動も同期もされない。AIの候補は本人が確認するまでverifiedにならず、出典のない推測はfactにできない。有効期間・訂正履歴・AI非開示の指定・export/delete
- 目標のチェックイン・履歴・レビュー。訂正は追記で元の記録を残し、`as_of`でその時点に何が信じられていたかを再生できる。レビューは「沈黙」と「警告」を別リストにする
- 目標ダッシュボード。行動の実施・指標の進捗・自己評価・状況を混ぜずに並べる。集計方法に既定値を置かず、方法未設定の目標は導出進捗を出さない。未計測を0%にしない。各数値から観測へ辿れる
- 組織・チーム・個人を担当とするGoal Alignment。`part_of`（構造）と`contributes_to`（貢献）を分けたまま、上位未接続の目標も確認できる。1ワークスペース内のグラフで、個人ワークスペースの目標は含めない
- 四半期・月・週・任意期間の計画期間。現在/過去/次の期間の切り替え、次期間の作成、引き継ぎ（元項目は変更せず由来を残す）、期間の終了。期間を使わないワークスペースは従来どおり
- ワークスペースの現地週による週次レビュー。行動実績、自己評価、成果指標、担当者別集計、下書き・確定・訂正履歴、印刷用要約。MCPでは再利用可能な構造化データとtextを返し、ChatGPT側の埋め込みUIでは目標ツリーを表示します
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

`PATHBASE_MODE=tachyon`の場合、必要な認証設定がないと起動しません。コールバックは`PATHBASE_PUBLIC_URL/api/auth/callback`と完全一致させます。本番では`pathbase_session` HttpOnly Cookieは不透明なIDだけを持ち、access token・refresh token・選択テナントを`PATHBASE_SESSION_KEYS`でAES-256-GCM暗号化した共有TiDBのセッション行に保持します。ブラウザーJavaScriptからは読めず、access tokenの期限が近づくとサーバー側で自動更新します（セッション自体の上限は12時間）。同じDBと鍵を設定した`pathbase-api`の実行環境間でセッションを引き継げます。ログアウト時はCookieだけでなくセッション行も削除します。

実環境へ接続する前に`npm run preflight`を実行すると、設定形式、OIDC Discovery、Tachyonのトークン検証API、Fieldの権限付きテナント一覧APIへの到達性を確認できます。確認要求には意図的に無効な認証情報を使い、クライアントシークレット、トークン、テナント識別子は結果へ表示しません。成功後も、実ユーザーでログインしてFieldの許可・権限不足・期限切れを確認する必要があります。

本番では同一オリジンの`/api/*`をRust APIへ転送し、`/api`プレフィックスを除きます。CookieとOriginヘッダーを保持してください。セッション鍵はCloud Appのsecret/credentialとして設定し、`tachyon.yml`やソースへ書きません。

Tachyon Cloud Appは`pathbase-v2`（Cloudflare Worker、SPA配信と同一オリジンの`/api/*`転送）と`pathbase-api`（Lambda、Rust API）の2アプリで構成します。公開URLは`https://pathbase-v2.txcloud.app`です。旧Cloud Run版の`pathbase.txcloud.app`は廃止済みで、現在は経路層が`No route for: pathbase`を返します。`Dockerfile`は単一オリジンのコンテナ実行用に残してあり、`PATHBASE_WEB_ROOT`指定時だけRustサーバーがSPAと`/api/*`を同時に配信します。

業務データはSQLxでTachyon管理のTiDB（MySQLプロトコル）へ保存します。`tachyon.yml`の`pathbase-api`だけが`provisionedDatabase`（provider: tidb / engine: mysql / envVar: DATABASE_URL）を宣言し、専用DB・SQLユーザー・権限・DSN secretはTachyonが発行します。manifestにDSNもsecretパスも書きません。静的WorkerにはDBの秘密値を渡しません。`environments.preview.provisionedDatabase`によりPRごとに専用DBを払い出し、`previewSharesProductionDatabase`は宣言しないため、previewが本番DSNへfallbackすることはありません。

`PATHBASE_MODE`が`local-preview`以外のとき`DATABASE_URL`（または`PATHBASE_DATABASE_URL`）が必須で、未設定・接続不可はどちらも起動エラーです。ローカルSQLiteへのフォールバックはありません。migrationはAPIプロセスがDBを開くときに、DB全体のadvisory lockの下で適用します（PrivateLink専用のためビルドrunnerからは到達できません）。`pathbase-api --migrate`で適用だけ実行することもできます。

出荷の判定は`readinessProof: /health/ready`です。実際にDBへ到達し、適用済みスキーマ版と、このDBがどのdeploymentのものか（`PATHBASE_DB_ENVIRONMENT`のclaim）を検査します。migrationが失敗した候補や、別environmentのDSNを渡された候補は200を返せないため、稼働中のバージョンがそのまま残ります。`/api/health`は実際の保存先を返し、TiDB接続時は`storage: tidb` / `storage_durability: shared-durable`、明示local-preview時は`storage: sqlite` / `ephemeral-runtime`になります。

スキーマは`api/migrations/{sqlite,mysql}/`のversioned migrationsです。方言差・並行制御・接続プール・TLS・監視対象・バックアップの検証済み/未検証は`docs/production-durability.md`、移行と切替の手順は`docs/runbook-tidb-cutover.md`に記載しています。

データ移行は`pathbase-api --migrate-from <source>`（`--dry-run`あり）で行います。空のDBにだけ流し込み、件数・内容hash・オーナー/参照/版/冪等性の不変条件を両側で照合します。`pathbase-api --inventory`は対象DBの件数とhash、整合性チェック結果を出力します。

Fieldは現在のユーザーのTachyonトークンと正規のテナント文脈で呼び、操作ごとに権限を確認します。FieldのタスクをPathBaseで完了しても元タスクは更新しません。タスク参照の重複取り込みを防止し、観測できない値は0に変換しません。実装根拠と設定項目は[連携契約](docs/integration-contracts.md)を参照してください。

## API / MCP

[API契約と例](docs/api.md)。実行中の`/api/v1/openapi.json`は認証された利用者へOpenAPIを返します。

ローカルMCPは、ブラウザプレビューと同じ絶対DBパスを指定して起動します。stdioのためHTTP用トークンは不要です。

```sh
PATHBASE_MODE=local-preview PATHBASE_DB=/absolute/path/to/data/pathbase.sqlite3 npm run --silent api:mcp
```

hosted MCP は Streamable HTTP の `/mcp`（`PATHBASE_WEB_ROOT` 使用時は `/api/mcp`）で提供します。OAuth 2.1のprotected resourceとして動作し、固定トークン方式はありません。

- 認可サーバー: このリソースの認可サーバーはBasepath自身です（`/.well-known/oauth-authorization-server`）。Cognitoプールのdiscoveryは`code_challenge_methods_supported`もregistration endpointも公開しておらず、redirect URIもデプロイ時固定のため、接続ごとにcallbackを発行するホストからは使えません。実測の根拠は[docs/chatgpt-plugin.md](docs/chatgpt-plugin.md)にあります。
- 認証: サインインは従来どおりTachyon管理のCognitoユーザープールです。Basepathが発行するのは「どのAIクライアントに何を許すか」という委譲だけで、これは元々Basepathが持っていた状態です。
- 登録: RFC 7591のdynamic client registrationに対応します。登録しただけでは何も得られません。本人がBasepathの許可画面でサインインした状態で権限を選んではじめて、トークンが発行されます。
- audience: トークンは1つのリソースに紐づきます。preview用のトークンは本番では通りませんし、その逆も同様です。
- 同意: どのAIクライアントに何を許すかは`mcp_connections`の記録です。許可画面で選んだ権限がそのまま記録になり、設定画面でいつでも狭められます。接続を解除すると、有効期限の残っているトークンも同じトランザクションで無効になります。
- 権限: `pathbase.read` / `pathbase.propose` / `pathbase.apply`。scopeがあっても変更は案のままで、本人が差分を確認して承認するまで反映されません。承認した時点で反映されるので、`apply`が実際に書くのはそれ以前に承認された案だけです。
- 認可: 操作ごとにワークスペース権限を再検証します。引数で別のworkspaceを指定しても権限は得られません。

transportはstateless Streamable HTTP（JSON応答）です。Lambdaでは連続したリクエストが別の実行環境に届くため、sessionを持ちません。`initialize`は`Mcp-Session-Id`を返さず、応答は`application/json`、SSE用の`GET`は拒否します。`Host`（と設定時は`Origin`）を検証し、応答は常に`Cache-Control: no-store`です。

変更案の確認と承認は[docs/change-approval.md](docs/change-approval.md)にまとめています。要点は「AIホスト内のクリックは承認の証拠にならない」ことです。アプリからのtool呼び出しはモデルからの呼び出しと同じ接続・同じトークン・同じ形でサーバーへ届き、区別できる情報がありません。そのため承認はBasepath自身のオリジンで本人のセッションを使って行い（`/changes/{workspace}/{id}`）、会話内では差分の提示と取り下げを行います。

**承認がそのまま適用です。**同じトランザクションで操作を実行し、承認だけされて反映されていない状態は作られません。2段階の検証（`approved_hash` / `approved_by`）が守っているのはAIが勝手に適用しないことであって、人に二度押させることではありませんでした。AIが適用できるのは本人が承認した内容だけ、という保証は変わりません。

**事前に決めた範囲。**本人は設定→AIクライアントの接続で、「この接続の、このワークスペースの、この種類の変更は確認なしで反映してよい」と先に決められます。境界は動いていません。その行はBasepathのオリジンで本人のセッションからしか書けず（承認とまったく同じ証拠）、AI接続は読むことも書くこともできません。変わったのは*いつ*決めるかだけで、承認が1件ずつから範囲ごとになりました。範囲に入るかどうかは、HTTPメソッドではなく**実際に何をしたか**で決めます。`POST /actions/{id}/complete` は何も作らず既存の行動を書き換えるので、メソッドで見ると「追加」に化けます。判定にはSAVEPOINT内で記録した差分——本人が読むのと同じ行——を使います。

範囲に入らないもの: 削除（列自体がありません）、アーカイブ（行は残りますが本人の視界からは消えます）、`due_date` / `start_date` / `scheduled_date` / `assignee_id` / `self_assessment` / `target` / `baseline` を**触る**操作（設定するときも消すときも。明示しない限り）、別のワークスペース、別の接続、1件でも範囲外を含む変更案、`pathbase.apply` を持たない接続、解除後に再接続した接続、無期限。自動反映された案は`approved_by`がnullのまま`auto_applied`を持ち、一覧にも差分ごと残るので「1件ずつ承認した」と「範囲で自動反映した」は後から区別できます。

toolのannotationsは実際の副作用に合わせています。変更案はDELETEを含みうるので、preview / propose / applyは`destructiveHint: true`です。`pathbase_get_graph`は最大`limit`件（既定・上限とも200）を返し、`truncated`を明示します。

このMCPサーバーは**1つの埋め込みMCP Apps UI**を使います。`tools/call` は引き続き
`structuredContent` と text を返し、目標を読むツールは共通の
`ui://basepath/plan-v3.html` を開きます。更新前の `plan-v2.html` もlegacy aliasとして読み取れます。UIはツール入力・結果の `workspace_id` を使い、指定がない場合だけ個人を安定したフォールバックとして表示します。組織を指定した読取や `part_of` のツリー結果では、個人画面を残さず組織の目標ツリーへ切り替えます。目標ツリーは、コンパクトなリスト表示とReact Flow風のマップ表示を切り替えられます。

変更案には引き続き`approval_url`（Basepathの絶対URL）と`where_to_approve`が付き、ChatGPTは構造化データとtextから差分と承認先を説明できます。認可はRust側で毎回行われ、表示や要約は権限の代替ではありません。
Basepathで事前許可した範囲に入る変更案だけは、本人が会話で承認・反映を明示した場合にMCP Appsのボタンまたは`pathbase_apply_changes`から反映できます。サーバーは接続・ワークスペース・期限・全操作の範囲を毎回再確認します。

discovery用に`/.well-known/oauth-protected-resource/...`（RFC 9728）と`/.well-known/oauth-authorization-server`（RFC 8414）を公開し、未認証時は`WWW-Authenticate: Bearer ... resource_metadata="..."`を返します。API GatewayがこのヘッダーをリネームするのでWorkerが元に戻します。脅威モデルと拒否する操作の一覧は[docs/mcp-authorization.md](docs/mcp-authorization.md)にあります。ChatGPT Workからの接続・提案・承認後の再読取・事前許可範囲の自動適用は2026-09-20に確認済みです。MCP Appsの目標ツリー表示はUI更新後の実host再確認が必要です。

ワークフロー（目標分解・週の計画・記録・週次振り返り）は`skills/`に1か所だけ書き、3つの経路で届きます。(1) MCPの`io.modelcontextprotocol/skills`拡張でサーバー自身が配信（`skills/list` / `skills/get` / `skill://`のresources/read。各ファイルのSHA-256とサイズをhostが検証できます）。(2) 拡張非対応のhost向けに通常のresourceとしても列挙。(3) diskから読むhost向けにパッケージへ同梱。同じバイト列であることはCIで検証しています。

配布パッケージは`plugin/<host>/`（manifest・接続先・アイコン）と`skills/`から`npm run build:plugin`で組み立てます。hostごとの差分は`scripts/build-plugin.mjs`の`LAYOUTS`の1行だけで、ワークフローにhost名が出てきたらbuildが失敗します。手順・接続導線・検証済み/未検証の切り分けは[docs/chatgpt-plugin.md](docs/chatgpt-plugin.md)と[docs/claude-connector.md](docs/claude-connector.md)を参照してください。claude.ai / Claude Desktopはカスタムコネクタ（URLのみ、インストール不要）で、会話内UIに対応します。Claude CodeはCLIなので会話内UIは約束しません。

34個のツール、項目のResource Template、3個のPromptを提供します。stdio と remote のどちらでも、MCP actor はAI agentとして扱われます。書き込みツールは提案を作り、設定画面の「AIからの変更案」で人が承認するまで反映しません。承認はAIが渡すフラグでは代用できません。rmcpのロック済みバージョンが提供するプロトコルを使用します。

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
