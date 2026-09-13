# PathBase

既存のパステル調のUIを維持した、React / TypeScript + Rust / Axum / SQLite + Tauri 2の目標・行動管理アプリです。目標と行動の共通モデルを使用し、認証はTachyon、営業データの参照はField APIへ接続します。

## 実装済み

- タイトルだけで項目を作成。目標・取り組み・行動・アイデア・節目を共通モデルで保存し、マップ・リスト・タイムライン・今日の行動で共有
- 5種類のテンプレート、任意の日付・時刻、週の実施回数、実施日ごとの完了・見送り・再開、アーカイブと復元
- 4種類の関連、循環検出、次の一歩、メモの下書き、学び・振り返りの追記履歴
- 自己評価と成果指標を分離。出典・日時・単位付き観測、訂正履歴、未計測表示。行動の完了で目標の達成率を変更しない
- SQLite永続化、トランザクション、楽観的ロック、再送の重複防止、領域ごとのアクセス制御、JSONバックアップと検証付き復元
- Tachyon OIDCログイン、PKCE・state・nonce・署名検証、サーバー側セッション。Tachyonの正規ユーザーIDから個人領域を解決
- 共有ワークスペースの作成・名前変更、TachyonユーザーID宛ての期限付き招待、参加・辞退・取り消し、オーナー／編集／閲覧の権限管理と退出。複数ワークスペースを名前で選択
- Fieldの権限付き組織一覧、営業タスクの参照、MRR / ARR / 受注残 / 売掛残 / DSOを成果指標へ記録
- Rustの共通処理を呼ぶHTTP API、Tauri IPC、ローカルstdio MCP。AIの変更案は差分表示・人の承認を経て原子的に適用
- 選択した目標・期限・直近記録に基づく30分以内の行動候補と、事実・推測・質問を分けた振り返り提案。編集・却下・差分承認に対応

## ローカル開発

Node.js 22.12以降とRustが必要です。ネイティブアプリにはOSのTauri開発環境も必要です。

```sh
npm ci
npm run dev
```

Rust APIを127.0.0.1:1431、画面をlocalhost:1420で一緒に起動します。API用のランダムな認証情報は開発プロセス内だけで扱います。標準では明示的な`local-preview`モードで、日付を現在に合わせたサンプル領域が作られます。`data/pathbase.sqlite3`に保存され、再起動後も残ります。サンプルの名前・写真・自己評価は実ユーザーの情報ではありません。

Tauriは`npm run tauri dev`で起動できます。debugでは同じRust処理をIPC経由で使用し、アプリデータディレクトリの`preview.sqlite3`に保存します。ブラウザ開発用DBとは別です。認証済みのデスクトップ利用は`PATHBASE_WEB_URL`で同じTachyon保護アプリを開きます。releaseはURL未設定時にローカル所有者へ切り替わりません。

## Tachyon / Field

[.env.example](.env.example)を参考に、PathBase用に登録されたOIDCクライアントと環境の接続情報を設定します。`npm run dev`は`.env` / `.env.local`の`PATHBASE_*`、`TACHYON_*`、`FIELD_*`を読みます。単独Rustプロセスには環境変数として渡してください。認証情報を`VITE_*`に置かないでください。

`PATHBASE_MODE=tachyon`の場合、必要な認証設定がないと起動しません。コールバックは`PATHBASE_PUBLIC_URL/api/auth/callback`と完全一致させます。ブラウザにはアクセストークンを渡さず、HttpOnlyのセッションCookieを使用します。セッションはプロセスのメモリに保持し、上限8時間・サーバー再起動後は再ログインです。ログアウトはPathBaseのセッションを破棄します。

実環境へ接続する前に`npm run preflight`を実行すると、設定形式、OIDC Discovery、Tachyonのトークン検証API、Fieldの権限付きテナント一覧APIへの到達性を確認できます。確認要求には意図的に無効な認証情報を使い、クライアントシークレット、トークン、テナント識別子は結果へ表示しません。成功後も、実ユーザーでログインしてFieldの許可・権限不足・期限切れを確認する必要があります。

本番では同一オリジンの`/api/*`をRust APIへ転送し、`/api`プレフィックスを除きます。APIはループバック待受なので、同じホストにリバースプロキシを置く構成です。CookieとOriginヘッダーを保持してください。複数インスタンス向けの共有セッションストアは未実装です。

Tachyon Cloud Appでは`Dockerfile`がWebとRust APIを一つのCloud Runコンテナへまとめ、`PATHBASE_WEB_ROOT`指定時だけRustサーバーがSPAと同一オリジンの`/api/*`を配信します。現在のSQLiteとセッションはコンテナローカルのため、このデプロイはプロトタイプ用途です。再起動・再デプロイでデータやログイン状態が失われる可能性があり、永続ストレージと共有セッションを導入するまでは本番データを保存しないでください。

Fieldは現在のユーザーのTachyonトークンと正規のテナント文脈で呼び、操作ごとに権限を確認します。FieldのタスクをPathBaseで完了しても元タスクは更新しません。タスク参照の重複取り込みを防止し、観測できない値は0に変換しません。実装根拠と設定項目は[連携契約](docs/integration-contracts.md)を参照してください。

## API / MCP

[API契約と例](docs/api.md)。実行中の`/api/v1/openapi.json`は認証された利用者へOpenAPIを返します。

ローカルMCPは、ブラウザプレビューと同じ絶対DBパスを指定して起動します。stdioのためHTTP用トークンは不要です。

```sh
PATHBASE_MODE=local-preview PATHBASE_DB=/absolute/path/to/data/pathbase.sqlite3 npm run --silent api:mcp
```

13個のツール、項目のResource Template、3個のPromptを提供します。書き込みツールは提案を作り、設定画面の「AIからの変更案」で人が承認するまで反映しません。承認はAIが渡すフラグでは代用できません。rmcpのロック済みバージョンが提供するプロトコルを使用します。ホスト型MCP、OAuth委譲、将来のMCP仕様用アダプタは含めていません。

## 検証

```sh
npm run check
npm run test:api
npm run build
npm run test:sites
cargo clippy --manifest-path api/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml
```

APIテストは一時DBとローカルの模擬OIDC / Tachyon / Fieldサーバーを使用します。`api/tests/fixtures/oidc-test-key.pem`はテスト専用に生成した公開fixtureです。実アカウントの認証情報ではありません。

GitHub ActionsではPRと`main`へのpushで、次の4ジョブを実行します。外部サービスの認証情報は不要です。

- Web：TypeScript、製品ビルド、Sites配信テスト、配信ファイルとフォントライセンスの存在確認
- Rust API：fmt、ドメイン・権限・OIDC / Field連携・MCPのテスト、clippy（警告をエラーとして扱う）
- Browser：Chromiumで目標とメモの永続化、行動完了、同じ領域の複数ワークスペース、モバイルナビゲーションを検証
- Desktop：macOSでRust fmtとTauriのコンパイル確認

ブラウザテストはAPIジョブでビルド済みの実際のRustバイナリを再利用し、毎回空の一時DBと専用ポート（画面1425 / API1435）で実行します。クラウド認証情報や開発用DB、`.env`は引き継ぎません。失敗時は画面・トレース・HTMLレポートを7日間保存します。GitHub ActionsはコミットSHAで固定しています。

ローカルのRust検証は変更に必要な範囲に絞り、重い全体検証はCIで行います。ブラウザテストを明示的にローカル実行する場合は、既存のAPIバイナリを`PATHBASE_E2E_API_BIN`で指定し、Chromiumを用意して`npm run test:e2e`を実行できます。テストがRustを自動ビルドすることはありません。

実際のTachyon / Field環境との疎通、TauriのGUI実行・配布用パッケージ・Windows / Linuxでの動作は、このCIの対象外です。

## 現在の範囲

PathBase用のTachyonクライアント登録、issuer、コールバック登録、Field接続環境は未提供のため、実環境でのログイン・複数アカウントでの招待・Fieldデータ取得は未検証です。招待は相手がPathBaseへログインすると画面内に届き、メールは送信しません。個人領域とローカル確認用領域は招待できません。外部通知、担当者指定、自動双方向同期、組織ポリシーの詳細設定、分散DB / 共有セッション、ホスト型MCPは別途実装が必要です。

`.openai/hosting.json`、`worker/index.js`、`scripts/prepare-sites-build.mjs`、`tests/sites-worker.test.mjs`は既存構成を維持しています。`npm run build`は`dist/client/index.html`、`dist/server/index.js`、`dist/.openai/hosting.json`を生成します。Sites用workerは静的配信であり、それだけではRust APIは公開されません。外部へのデプロイは行っていません。

## ライセンス

独自コードは[MIT License](LICENSE)です。同梱フォントなどの第三者著作物は元のライセンスを維持します。[第三者ライセンス表記](THIRD_PARTY_NOTICES.md)を参照してください。
