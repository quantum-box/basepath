# リリースチェックリスト

出荷判定の基準を1か所に置きます。「コードがある」「CIが通った」「本番で確認した」は
別のことなので、この文書では最後まで分けて書きます。

`npm run gate` がこの文書と `.github/workflows/ci.yml` の整合を検査します。必須チェックの
名前が片方から消えれば、もう片方が落ちます。

## 出荷前に通るもの

`main` のbranch protectionが要求するチェックです。この一覧はリポジトリ設定側にあり、
pull requestから狭められません。

| チェック | 何を保証するか |
| --- | --- |
| `Web build and Sites tests` | 型、ビルド、Worker、Tachyon manifest、ビューモデル、配布パッケージ、MCP Appバンドルの同一性、出荷ゲート自身 |
| `Rust API tests and lint` | APIの契約と拒否条件、承認、OAuth認可サーバー、指示注入、Skills配信。`clippy -D warnings` と `fmt --check` |
| `Shared TiDB behaviour` | **出荷する保存先での**受入フロー、移行、耐久性、readiness、環境claim |
| `Browser integration tests` | 実ブラウザでのWeb画面、変更承認、接続許可、MCP Appの実host bridge |
| `Tauri compile check` | ネイティブ側が同じRust処理でビルドできること |

`Shared TiDB behaviour` を必須に含めるのは、これが**本番と同じストレージ**で走る唯一の
ジョブだからです。外すと、再デプロイをまたぐ契約がSQLiteに対してしか検証されません。

### 受入フロー

`api/tests/acceptance.rs` が、実TiDB・実HTTP・実MCPで次を1本の流れとして通します。

1. 本人の個人ワークスペースを解決する
2. AIクライアントがOAuthで認可され、目標マップ・今日・今週・週次レビューを読む
3. AIが変更案を作る。**この時点で計画は変わらない**。未承認のapplyは拒否される
4. 本人が表示された内容のdigestで承認する。違うdigestは拒否される
5. 承認と同じトランザクションで原子的に適用され、別のexecution environmentから同じ項目・版・監査が見える。承認だけして反映されていない状態は残らない
6. 行動完了はAI経由でも提案のまま。週次レビューは本人が確定する
7. 接続解除で、期限の残ったトークンが次のリクエストから失効する。データは残る

権限（owner/editor/viewer）、本人A/B、個人/共有、membership削除、scope不足、
別ワークスペース参照は同じファイルの2本目のテストが扱います。プロセスを落として別
プロセスから続けられること、後から起動したプロセスが同じ委譲で動くことは3本目です。

### 拒否されること

| 試み | 結果 | どこで |
| --- | --- | --- |
| 未承認の適用 | `403 APPROVAL_REQUIRED` | `approval.rs`, `acceptance.rs` |
| 表示された内容と違うものの承認 | `409 CHANGESET_SUPERSEDED` | `approval.rs`, `acceptance.rs` |
| 期限切れ・再送・二重クリック | 保存済みの結果、適用は1回 | `approval.rs`, `acceptance.rs` |
| 期待版の競合 | `409 VERSION_CONFLICT` | `tidb.rs`, `weekly_review.rs` |
| 記録本文による指示注入 | 本文はデータのまま。権限は記録から読まない | `injection.rs` |
| 権限フィールドを書く操作の提案 | 提案時に拒否 | `injection.rs` |
| 別ワークスペースのchangeset/項目 | `404` / `VALIDATION_ERROR` | `injection.rs`, `acceptance.rs` |
| 単回使用のcode/refreshの再提示 | 一族ごと失効 | `oauth.rs` |
| 解除済み接続のトークン | `401` | `oauth.rs`, `acceptance.rs` |
| DB未設定・接続不可 | 起動しない。SQLiteへ落ちない | `database_configuration.rs` |
| pool枯渇 | 待たずに失敗し、DSNを出さない | `durability.rs` |
| migration失敗・別環境のDSN | readinessが200を返さず、旧versionが残る | `deployment_gate.rs` |

## 未検証

ここに書いてあることは「まだ確かめていない」であって「動かない」ではありません。
確かめるまでは動くとも書きません。

| 項目 | 状態 | 必要なもの |
| --- | --- | --- |
| 実ChatGPTからの接続 | 確認済み（2026-09-20） | ChatGPT Work（GPT-5.6 Sol / Basepath plugin 1.0.0）で本番OAuth再接続、個人ワークスペース読取、変更案の差分・Basepath導線・承認後の再読取まで確認。詳細は `docs/chatgpt-plugin.md` |
| 実claude.ai / Claude Desktopからの接続 | 未実施 | Claudeアカウント |
| 両hostでのMCP Apps描画 | 一部確認済み | ChatGPT は実機で確認済み。Claude は未実施。harnessの成功は実hostの成功ではない |
| 対応クライアント・プラン・バージョン | 一部測定済み | ChatGPT Work（GPT-5.6 Sol / Basepath plugin 1.0.0）は実接続で確認済み。その他は未測定 |
| TiDB Cloud Serverlessのretention / PITR | 未検証 | 実際の復元試行。PathBase自身のexport/importはCI検証済み |
| `mcp_connections` の移行 | 対象外 | `migrate.rs` の `TABLES` に含まれない。SQLite→TiDBの一度きりの移行は完了済み |
| 公開ディレクトリへの掲載 | 未申請 | 明示承認。提出可能と公開済みは別 |

実hostでの確認を行ったときは、host名・バージョン・検証日・画面またはtrace・結果を
`docs/chatgpt-plugin.md` と `docs/claude-connector.md` の表に追記します。harnessの
成功と混ぜないでください。

## 停止と切り戻し

**出荷しない条件**

- 必須チェックのいずれかが落ちている
- `Shared TiDB behaviour` がスキップで通っている（`PATHBASE_TEST_DATABASE_URL` 未設定）
- readiness proof `/health/ready` が `"schema":"current"` を返さない
- 上の「未検証」に書いていない未確認事項がある

**切り戻し**

| 変更 | 戻し方 |
| --- | --- |
| アプリケーションコード | Tachyonで直前のdeploymentへ戻す。候補がreadinessを通らなければ、そもそも有効化されない |
| スキーマ | migrationは前進のみ。破壊的変更は入れない。戻す必要が出たら、戻すためのmigrationを書く |
| MCP接続 | 設定画面で接続解除。委譲と発行済みトークンが同じトランザクションで失効する |
| MCPエンドポイント全体 | `PATHBASE_MCP_ENABLED` を外す。エンドポイントもOAuthのエンドポイントも消える |
| 配布パッケージ | hostから削除。サーバー側は無関係に動き続ける |

**止める判断をする事象**

- 他人の計画が見えた、または書けた
- 本人が承認していない変更が適用された
- 解除した接続が動き続けた
- 未計測の値が0や推測値として保存された

いずれも上のテストが拒否として固定しているものです。実環境で起きたなら、テストが
現実を写していないということなので、まず再現するテストを書きます。

## CI artifactに出さないもの

- DSN、セッション鍵、アクセストークン、OAuthのclient secret（発行していません）
- 本番データ。CIのTiDBはジョブごとに作られる使い捨てで、本番へは到達しません
- 失敗時のtrace・スクリーンショットはPlaywrightのものだけで、実データを含みません
