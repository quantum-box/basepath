# PathBase API contract

Rustルーター内のパスを記載しています。ブラウザからは先頭に`/api`を付けます。JSONの日時はRFC3339、日付は`YYYY-MM-DD`、時刻は`HH:MM`です。識別子は作成レスポンスから取得してください。`w`はアクセス可能なワークスペースIDです。

## 共通規則

- 本番はTachyon認証済みの`pathbase_session` Cookieを使います。Cookieは`PATHBASE_SESSION_KEYS`で認証付き暗号化され、サーバー再起動や同じ鍵を持つ別インスタンスへのルーティングでも有効です。`POST /auth/login`は同一オリジンのPathBaseログインフォームから資格情報を受け取り、Tachyonの`POST /oauth2/login`とサーバー間のAuthorization Code + PKCE交換でセッションを確立します。共有Tachyonプラットフォームのユーザーであれば所属オペレーターテナントを問わず認証でき、正規ユーザーと所属テナントはTachyonの`GET /v1/me`から取得します。ログイン後は`GET /v1/tenants`の一覧から利用テナントを`POST /v1/tenant-selection`で明示的に選ぶ必要があり、それまではワークスペースAPIが428 `TENANT_SELECTION_REQUIRED`を返します。アクセス範囲は選択後もPathBaseワークスペースのメンバーシップで判定します。Cognito Hosted UIと独自のパスワード保存は使いません。本番HTTPサーバーはTachyonの既定OAuth2エンドポイントを使って外部通信前にlistenを開始し、`--preflight`はセッション鍵、OIDC Discovery、各認証境界を検証します。`POST /auth/logout`でCookieを消去し、`GET /auth/status`と`GET /health`は公開の設定状況・ヘルス情報です。
- 変更には`Idempotency-Key`を指定します。同じ操作者・領域・キー・入力は同じ結果を返し、異なる入力は409。成功結果はDB内に保持し、履歴削除ポリシーはまだ設けていません。AI提案元と人の承認元でキーの名前空間を分けます。
- ブラウザの変更には`X-PathBase-Request: 1`が必要です。設定した公開オリジン以外からのリクエストは拒否します。CORSは許可しません。ローカル確認モードだけは開発プロキシ内のBearer資格情報でアクセスします。
- 更新は`expected_version`が必要です。競合は409 `VERSION_CONFLICT`、未指定は428。入力を保持して最新の内容を取得し、人が差分を確認してから再送してください。
- APIは操作ごとにワークスペースのowner/editor/viewerを確認します。未認証は401、読み取りのみの人の更新は403、別領域や存在しない項目は同じ404です。
- エラーは`{status,code,message,details}`。本文上限8MB。書き込みは一つのSQLiteトランザクションで検証・変更・再送結果・監査を保存します。

## 読み取り

| パス | 内容 |
| --- | --- |
| `GET /v1/me` | 正規ユーザーID、表示名、実行モード |
| `GET /v1/tenants` | ログインユーザーが所属するTachyonテナントと現在の選択 |
| `POST /v1/tenant-selection` | `{tenant_id}`で利用する所属テナントを選択 |
| `GET /v1/workspaces` | 利用できる領域とroleの配列 |
| `GET /v1/settings` | compact、notifications、timezone |
| `GET /v1/templates` | free / okr / project / learning / habit、バージョン、作成予定 |
| `GET /v1/workspaces/{w}/snapshot` | その領域のitems / relations / records / metrics / observations / views / changesets |
| `GET /v1/workspaces/{w}/items` | query / kind / state / archived / cursor / limitによる検索 |
| `GET /v1/workspaces/{w}/items/{id}` | 版番号を含む項目 |
| `GET /v1/workspaces/{w}/relations` | 関連一覧 |
| `GET /v1/workspaces/{w}/records` | 記録一覧 |
| `GET /v1/workspaces/{w}/metrics` | 成果指標一覧 |
| `GET /v1/workspaces/{w}/observations` | 訂正前を含む観測一覧 |
| `GET /v1/workspaces/{w}/today?local_date=2026-09-12` | その日の行動と実施状態 |
| `GET /v1/workspaces/{w}/calendar?start=2026-09-01&end=2026-10-12&timezone=Asia/Tokyo` | 最大63日分の開始・期限・予定・習慣と未予定項目。習慣は訂正後の最新状態を返す |
| `GET /v1/workspaces/{w}/graph?limit=100` | グラフ投影。最大200ノード、truncatedを確認 |
| `GET /v1/workspaces/{w}/views` | 保存ビュー一覧 |
| `GET /v1/workspaces/{w}/changesets` | 変更案一覧 |
| `GET /v1/workspaces/{w}/audit` | 操作者、操作元、操作日時の監査一覧 |

コレクション一覧は原則`{items,next_cursor}`、limitは標準50・最大200です。カーソルは最後のIDを返します。snapshotは画面向けの全件投影で、ページングAPIではありません。大規模データや複数サーバーへ拡張する際は差分同期が必要です。

## 共有ワークスペース

`POST /v1/workspaces`は`{name,scope:"チーム"|"組織",timezone?:"Asia/Tokyo"}`で空の領域を作ります。個人の項目は移動・複製されません。名前で複数領域を区別し、`workspace_id`で操作します。

| 操作 | 入力・結果 |
| --- | --- |
| `GET /v1/workspaces/{w}/members` | workspace（versionと自分のrole）、members（actor/role）、invitations（ownerのみ） |
| `PATCH /v1/workspaces/{w}` | name、timezone、expected_version |
| `POST /v1/workspaces/{w}/invitations` | target_actor（正規Tachyon ID）、role（editor/viewer）、expected_version |
| `DELETE /v1/workspaces/{w}/invitations/{id}` | expected_version。未使用の招待を取り消す |
| `GET /v1/invitations` | 自分宛ての有効な未処理招待。7日間有効 |
| `POST /v1/invitations/{id}/accept` | 招待のexpected_version。認証した本人だけが参加できる |
| `POST /v1/invitations/{id}/decline` | 招待のexpected_version。本人が辞退する |
| `PATCH /v1/workspaces/{w}/members/{actor}` | role（owner/editor/viewer）、expected_version |
| `DELETE /v1/workspaces/{w}/members/{actor}` | expected_version。アクセスを解除し、記録を残す |
| `POST /v1/workspaces/{w}/leave` | expected_version。自分の参加を解除する |

招待への応答以外のexpected_versionはworkspaceのversionです。共有設定・招待・参加・解除が変わると版が進み、古い入力は409で拒否します。ownerだけが共有設定と招待・他のメンバーを管理でき、最後のownerの解除・降格は409 LAST_OWNERです。個人領域は共有できません。ローカル確認用領域へのオンライン招待も拒否します。

招待はメールを送らず、対象のTachyonユーザーがログインしたPathBaseの「メンバー」と「お知らせ」に表示します。受諾前は領域を閲覧できません。取り消し・期限切れ・招待者のowner権限喪失後の受諾は拒否します。解除済みメンバーは以前の成功リクエストの再送を含め、領域にアクセスできません。人による共有管理をMCPの変更案から実行することはできません。

画面はフォーカス復帰と表示中30秒ごとに権限・招待を再取得します。取得に失敗した領域の403/404は画面の保持データから除きます。APIレスポンスは`Cache-Control: no-store`です。名前変更や招待で個人の項目・記録の共有範囲が変わることはありません。

## 項目と実行

`POST /v1/workspaces/{w}/items`の最小入力：

```json
{"title":"英語で話せるようになる"}
```

kindは`idea | outcome | initiative | action | milestone`（既定outcome）、stateは`draft | active | paused | done | abandoned`（既定active）。任意項目はdescription、start_date、due_date、scheduled_date、scheduled_time、fields、parent_idです。parent_idがあればpart_of関連と同時に作成します。日付・親・数値を自動補完しません。

`PATCH /v1/workspaces/{w}/items/{id}`：

```json
{"expected_version":1,"description":"海外の人と会話する","fields":{"memo":"週末に見直す","self_assessment":40}}
```

fieldsにはicon、subtitle、memo、next_action_id、self_assessment、recurrence、external_urlなどを保存します。自己評価の日時はサーバーが記録します。fieldsは部分更新で、nullは設定解除です。`archived_at`に日時を指定するとアーカイブ、nullで復元します。actionの完了は以下の専用操作を使います。

```text
POST /v1/workspaces/{w}/actions/{id}/complete
POST /v1/workspaces/{w}/actions/{id}/skip
POST /v1/workspaces/{w}/actions/{id}/reopen
```

```json
{"expected_version":1,"local_date":"2026-09-12","note":"30分取り組んだ"}
```

`{item,record,outcome_updated:false}`を返します。繰り返す行動はその日の履歴だけを変更し、親目標の達成率は変更しません。recurrenceは`{mode:"period_quota",times_per_week:3,timezone:"Asia/Tokyo",weekdays:[]}`、曜日固定はmode=`fixed_schedule`とweekdays（月曜日0〜日曜日6）で表します。現在の作成UIは週の回数を提供します。曜日固定はAPIから利用できます。変更前の繰り返し設定も履歴に残します。

## 関連・記録・成果

| 操作 | 入力 |
| --- | --- |
| `POST …/{w}/relations` | source_id、target_id、type、任意rationale |
| `DELETE …/{w}/relations/{id}` | expected_version |
| `POST …/{w}/records` | body、任意item_ids / record_type / happened_at / decision / supersedes_id |
| `POST …/{w}/metrics` | item_id、name、unit、baseline、target、direction、任意period_start / period_end |
| `POST …/{w}/observations` | metric_id、value、unit、source、任意observed_at / supersedes_id |

関連のtypeはpart_of / contributes_to / depends_on / relates_to。part_ofは親が最大1つ。part_ofとdepends_onは種類ごとに循環を検出し、relates_toは逆向きの重複も拒否します。別領域のIDは利用できません。

通常の記録はnote / review / learning / checkin。実行履歴を偽装できないようcompletionなどは専用操作だけが生成します。記録・観測の訂正はsupersedes_idで追記し、元データは保持します。

指標のdirectionはincrease / decrease / threshold。指標の単位と観測の単位は一致が必須です。最新の有効な観測を評価し、観測がない場合は未計測です。実績の比率は100%超も残し、バーだけ0〜100%へ収めます。30日より古い観測は画面で更新が必要と表示します。

## テンプレート・提案・入出力

- `POST /v1/workspaces/{w}/ai/suggestions/preview`：`goal_id`と`expected_version`を指定。選択した目標・期限・同じワークスペースの直近記録だけから、30分以内の行動候補3件と、事実・推測・質問を分けた振り返り案を返します。AI接続が利用できない場合は安全なローカル候補へフォールバックします。この操作だけでは項目や記録を変更しません。採用時は下記changeset契約を使用します。
- `POST /v1/workspaces/{w}/templates/{id}/apply`：titleと任意description / start_date / due_date。テンプレートが項目・関連・ビューを同じトランザクションで作ります。OKRの目標値は自動生成しません。
- `POST /v1/workspaces/{w}/views`：name、type（list / map / timeline / okr / today）、filters。`POST …/views/{id}/query`で保存条件による項目検索を実行します。
- `PATCH /v1/settings`：compact、notifications、timezoneをすべて指定。タイムゾーンはIANA識別子です。
- `POST /v1/workspaces/{w}/exports`：空オブジェクト。schema_version=1のJSONを返します。
- `POST /v1/workspaces/{w}/imports`：exportしたJSON。項目・関連・記録・指標・観測・ビューを再検証して追加します。同じIDや壊れた参照があれば全件ロールバックします。既存項目を上書きする機能ではありません。
- `POST /v1/workspaces/{w}/changesets/preview`：titleとoperations（method / path / bodyの配列）。SAVEPOINT内で全件検証後に取り消し、30分有効な変更案を保存します。
- `POST …/changesets/{id}/approve`：空オブジェクト。人のアプリ操作のみが承認できます。
- `POST …/changesets/{id}/apply`：空オブジェクト。承認・期限・内容ハッシュ・領域の更新状態を確認して原子的に適用します。作成後に領域のデータや権限が変わった案は再プレビューが必要です。

## Field

Tachyonセッションを使用し、Field側の権限を毎回確認します。

- `GET /v1/integrations/field/tenants`
- `GET /v1/integrations/field/tasks?tenant_id=…&offset=0`（50件）
- `GET /v1/integrations/field/metrics?tenant_id=…`
- `POST /v1/workspaces/{w}/field/attach-task`：tenant_id、task_id。Field参照情報付きのローカル行動を作成。同じ外部タスクは重複作成しません。
- `POST /v1/workspaces/{w}/field/record-metric`：tenant_id、metric_id、field_key。mrr / arr / backlogAmount / receivableOutstandingは円、daysSalesOutstandingは日。単位不一致は422、欠測は422 UNOBSERVEDです。

Fieldへの変更操作・バックグラウンド同期はありません。詳しい上流契約と権限の分離は[integration-contracts.md](integration-contracts.md)を参照してください。
