# PathBase API contract

Rustルーター内のパスを記載しています。本番ではRust APIをAWS Lambdaで実行し、Cloudflare Workerが同一オリジンの`/api/*`からLambdaの`/*`へ転送します。ブラウザからは先頭に`/api`を付けます。JSONの日時はRFC3339、日付は`YYYY-MM-DD`、時刻は`HH:MM`です。識別子は作成レスポンスから取得してください。`w`はアクセス可能なワークスペースIDです。

## 共通規則

- 本番はTachyon認証済みの`pathbase_session` Cookieを使います。Cookieは`PATHBASE_SESSION_KEYS`で認証付き暗号化され、サーバー再起動や同じ鍵を持つ別インスタンスへのルーティングでも有効です。`POST /auth/login`は同一オリジンのPathBaseログインフォームから資格情報を受け取り、Tachyonの`POST /oauth2/login`とサーバー間のAuthorization Code + PKCE交換でセッションを確立します。共有Tachyonプラットフォームのユーザーであれば所属オペレーターテナントを問わず認証でき、正規ユーザーと所属テナントはTachyonの`GET /v1/me`から取得します。ログイン後は`GET /v1/tenants`の一覧から利用テナントを`POST /v1/tenant-selection`で明示的に選ぶ必要があり、それまではワークスペースAPIが428 `TENANT_SELECTION_REQUIRED`を返します。選択したテナントはそのセッションが到達できる唯一のデータ境界です。Cognito Hosted UIと独自のパスワード保存は使いません。本番HTTPサーバーはTachyonの既定OAuth2エンドポイントを使って外部通信前にlistenを開始し、`--preflight`はセッション鍵、OIDC Discovery、各認証境界を検証します。`POST /auth/logout`でCookieを消去し、`GET /auth/status`と`GET /health`は公開の設定状況・ヘルス情報です。
- 変更には`Idempotency-Key`を指定します。同じ操作者・領域・キー・入力は同じ結果を返し、異なる入力は409。成功結果はDB内に保持し、履歴削除ポリシーはまだ設けていません。AI提案元と人の承認元でキーの名前空間を分けます。
- ブラウザの変更には`X-PathBase-Request: 1`が必要です。設定した公開オリジン以外からのリクエストは拒否します。CORSは許可しません。ローカル確認モードだけは開発プロキシ内のBearer資格情報でアクセスします。
- 更新は`expected_version`が必要です。競合は409 `VERSION_CONFLICT`、未指定は428。入力を保持して最新の内容を取得し、人が差分を確認してから再送してください。
- ワークスペースはいずれか一つのテナントに属し、移動しません。APIは操作ごとに「選択中テナントのワークスペースか」を先に確認し、そのうえでowner/editor/viewerを確認します。未認証は401、読み取りのみの人の更新は403、別テナント・別領域・存在しない項目はすべて同じ404です（403にすると存在自体を認めることになるため）。
- 同じ人でもテナントが違えば別のデータです。個人ワークスペースは（テナント, ユーザー）から導出するため、テナントを切り替えると別の個人ワークスペースになります。`GET /v1/workspaces`と`GET /v1/invitations`は選択中テナントの分だけを返し、メンバーシップはテナントをまたぎません。
- 招待は発行時点では相手のテナント所属を確認できない（PathBaseが知っているのは招待する側の所属だけ）ため、境界は受諾時に効きます。受諾者が対象ワークスペースのテナントで操作している場合にのみメンバーになり、それ以外では404です。
- MCPの委任（接続）はテナント単位です。人が承認した時点のテナントがその接続の活動範囲になり、同じAIクライアントを別テナントで使う場合は別の接続として承認し直します。
- Fieldのタスク・指標は、セッションで選択中のTachyonテナントと同じ`tenant_id`だけを受け付けます。Field側で別テナントの権限を持っていても、クライアント入力だけで現在のテナント境界を切り替えることはできません。
- エラーは`{status,code,message,details}`。本文上限8MB。書き込みは一つのDBトランザクション（本番はTiDB、明示local-previewはSQLite）で検証・変更・再送結果・監査を保存します。書き込みトランザクションは対象ワークスペースを排他ロックしてから読み取るため、複数インスタンスから更新しても`expected_version`・循環禁止・最後のオーナー・changesetの原子性が保たれます。

## 読み取り

| パス | 内容 |
| --- | --- |
| `GET /v1/me` | 正規ユーザーID、表示名（`name`/`display_name`）、実行モード、identity_source。MCPでは接続同意時のTachyon表示名を返す |
| `GET /v1/tenants` | ログインユーザーが所属するTachyonテナントと現在の選択 |
| `POST /v1/tenant-selection` | `{tenant_id}`で利用する所属テナントを選択 |
| `GET /v1/workspaces` | 利用できる領域とroleの配列 |
| `GET /v1/settings` | compact、notifications、timezone |
| `GET /v1/templates` | free / okr / project / learning / habit、バージョン、作成予定 |
| `GET /v1/workspaces/{w}/snapshot` | その領域のitems / relations / records / metrics / observations / views / changesets / weekly_reviews / cycles / checkins |
| `GET /v1/workspaces/{w}/weekly-review?week_start=2026-09-14` | 現地週の行動実績、自己評価、成果指標、担当者別集計、レビュー履歴 |
| `GET /v1/workspaces/{w}/planning` | 計画期間、今日が入る期間とその前後、期間なしの項目数、直近の確定レビュー |
| `GET /v1/workspaces/{w}/alignment` | 目標の担当・期間・`part_of`/`contributes_to`・上位未接続の一覧 |
| `GET /v1/workspaces/{w}/dashboard` | 行動の実施・指標の進捗・自己評価・状況を分けて返す |
| `GET /v1/workspaces/{w}/review` | 未チェックイン／停滞／要注意／最近更新の4リスト |
| `GET /v1/workspaces/{w}/memories` | 個人の記憶（個人ワークスペースのみ） |
| `GET /v1/workspaces/{w}/items/{id}/checkins` | チェックイン全履歴と現在有効なもの |
| `GET /v1/workspaces/{w}/items/{id}/timeline?as_of=` | 目標に起きたことの時系列と、その時点の状態 |
| `GET /v1/workspaces/{w}/cycles` | 計画期間一覧 |
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
| `GET /v1/workspaces/{w}/members` | workspace（versionと自分のrole）、members（actor/display_name/role）、invitations（ownerのみ） |
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

fieldsにはicon、subtitle、memo、next_action_id、self_assessment、recurrence、external_url、同じワークスペースのメンバーIDを指定するassigneeなどを保存します。自己評価の日時はサーバーが記録します。fieldsは部分更新で、nullは設定解除です。`archived_at`に日時を指定するとアーカイブ、nullで復元します。actionの完了は以下の専用操作を使います。

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

## Personal / Organization Navigation

個人と組織は、同じワークスペースの表示切替ではありません。境界を挟んだ別のストアで、UIもそう扱います。

* URLが分かれます。`/personal/{screen}` と `/org/{workspace_id}/{screen}`。deep link・reload・戻る/進むのどれでもURLが正で、画面に残っていたものではありません。
* navigation treeが別です。記憶は個人にだけあり、アラインメント／ダッシュボード／目標レビューは組織にだけあります。片方を絞り込んだものではなく、どちらももう片方の部分集合ではありません。
* 「ワークスペース」画面だけは両方にあります。名前が違うのは答える問いが違うからです（組織では「誰がいるか」、個人では「どこに所属しているか」）。ここは組織へ参加・作成する入口でもあるので、個人から外へ出る道のない menu は誰かを閉じ込めます。
* 文字で分かります。パンくず（`個人` / `組織・{名前}`）、タブのタイトル、空状態の文面。色やアイコンだけには依存しません。
* 切替時に何も持ち越しません。選択中の項目、検索語、開いていたパネルはすべて、去るcontextへのポインタです。
* 画面の中からワークスペースを選ぶ操作も同じ経路を通ります。境界を越える道が2本あって片方が古い選択を残すなら、2つはやがて分かれていません。
* deep linkの`tenant_id`が現在の選択と違う場合は、`/tenants?tenant_id=...&return_to=...`で明示選択を挟みます。return先は同一originのPathBase routeだけを許可し、refreshや共有後もworkspace/item/screenを復元します。
* 権限を失った組織は即座に使えなくなります。ワークスペース一覧が正で、そこに無いcontextはその人にとって存在しません。
* そのcontextに無い画面へのdeep linkは、空の画面ではなくそのcontextのホームに着きます。空の記憶画面は「ここに記憶はあるのか」に「たぶん」と答えてしまいます。

## Goal Breakdown

大きな目標を、実行できるところまで分解する構造。`part_of`は構造（親は1つ）、`contributes_to`は貢献（複数可）、`depends_on`は順序で、どれも別の質問です。

| ルート | 内容 |
| --- | --- |
| `GET /v1/workspaces/{w}/items/{id}/breakdown?depth=&limit=` | その項目の下。`depth`は既定2（最大20）、`limit`は既定200（最大500） |
| `GET /v1/workspaces/{w}/items/{id}/ancestry` | その項目の上。なぜ存在するのかを、記録された理由ごと |
| `GET /v1/workspaces/{w}/breakdown/gaps` | 降りきっていない場所。**報告するだけで、埋めません** |
| `POST /v1/workspaces/{w}/items/{id}/reparent` | 位置を変える。`parent_id`に`null`で「どこにも属さない」 |
| `POST /v1/workspaces/{w}/items/{id}/children` | 兄弟の並び。`order`に子のidを並べて一度に指定 |

**階層数は固定していません。** level列もtier enumもありません。10年計画の人と2週間計画の人の両方が正しく、どちらかを選ぶschemaはもう一方にとって間違いです。深さはedgeが決めます。`depth`と`limit`が縛るのは「一度に読む量」で、別の話です。

**深い計画は少しずつ読めます。** `has_more_children`が真のnodeは、そのidをrootにして呼び直すための取っ手です。`truncated`は「途中で止めた」を明示します。黙って短い結果を返しません。

**移動は移動です。** `reparent`はlinkのidを保ったまま向き先を変えます。下にあるものは親の親ではなくその項目に付いているので、一緒に動きます。自己参照・循環・他ワークスペースのparentは書き込む前に拒否し、失敗した移動は元の位置を1つも変えません。`parent_id: null`は外すだけで、消しません。

**親をarchiveしても子は消えません。** 親を整理することは子についての判断ではありません。子は残り、何の一部だったかも残ります。

**理由はlinkに載ります。** `ancestry`の各要素は上から順に並び、`rationale`とそれが説明する`child_id`を持ちます。理由が空なら「誰も書いていない」であって、こちらで作文はしません。

**変更は記録されます。** `reparent`は`breakdown_change`のrecordを残し（`from` / `to` / `rationale`）、リクエスト自体は他の変更と同じくauditに入ります。

## Goal Breakdown Copilot

Basepathはモデルを動かしません。AIはホスト（ChatGPT / Claude）側にいるので、ここでいうCopilotは**契約**です。提案の前に何を渡すか、提案に何を受け付けないか、決めるときに本人が何を見るか。

| ルート | 内容 |
| --- | --- |
| `GET /v1/workspaces/{w}/items/{id}/breakdown-brief` | 提案の前に読むもの。`questions`は**尋ねること**であって埋める欄ではありません |
| `POST /v1/workspaces/{w}/items/{id}/breakdown-comparison` | 候補と既存の子を並べる。keep / change / add / remove_candidate。**何も書きません** |

**穴は質問です。** 期限も測り方もない目標は、完成して見えて何も意味しない形に分解できます。「いつまでに」へのもっともらしい答えは、答えがないことより悪い — 承認された後は本人が決めたこととして読まれるからです。`context_kind`はワークスペースから決まり、呼び出し側は選べません。

**約束になる値には出どころが要ります。** agent接続からの提案で、`due_date` / `start_date` / `scheduled_date` / `assignee_id` / `self_assessment` / `target` / `baseline` のいずれかを設定する操作は、その操作に`basis`（値がどこから来たか）が必要です。ないと**拒否**します。黙って落とすのではなく拒否するのは、書けない値は本人に尋ねるべき値だからです。本人自身の提案にはこの規則は適用しません。自分の期限を自分で入れることは、出典の要る主張ではありません。

`basis`は操作に載せます。bodyの中ではありません。提案についての主張であって、提案される物の属性ではないからです。目標に`basis`という欄はなく、AIが提案したからといって生えるべきでもありません。

**前提も一緒に渡せます。** `changesets/preview`の`assumptions`（最大20行）は、差分の隣に表示されます。分解の承認は、行だけでなく考え方への同意でもあります。

**作り直しは消しません。** 既に子のある目標を再分解するとき、候補に含まれていない既存の子は`remove_candidate`として返ります。候補に入れ忘れたことは削除の理由ではありません。

## Personal Memory

**個人のワークスペースにだけ存在します。** 共有ワークスペースへ移動・継承・自動同期する機能はありません。個人目標が組織目標に貢献していても、組織側からPersonal Memoryへ辿る経路はありません。共有ワークスペースに置くこと自体が「共有する」という行為であり、製品が本人の代わりにその判断をしてはいけないからです。

将来Organization Memoryを作る場合も、このテーブル・ID・検索indexは共有しません。共有すると境界が「事実」ではなく「慣習」になります。

種類は `fact` / `preference` / `decision` / `learning` / `context` / `episode`。意味が違い、古び方も違うので分けます。

| ルート | 内容 |
| --- | --- |
| `GET /v1/workspaces/{w}/memories?kind=&status=&current=&archived=` | 一覧。`current=true`は「いま有効なもの」（supersedeされておらず、有効期間内） |
| `GET /v1/workspaces/{w}/memories/duplicates` | 似ている記憶の報告。**統合も削除もしません** |
| `POST /v1/workspaces/{w}/memories` | 本人が書く。`status`は`verified` |
| `POST /v1/workspaces/{w}/memories/proposals` | AIの候補。`status`は`proposed` |
| `POST /v1/workspaces/{w}/memories/{id}/verify` | 本人が候補を確認する |
| `POST /v1/workspaces/{w}/memories/{id}/corrections` | 訂正の候補。`supersedes_id`が入り、`status`は`proposed` |
| `PATCH` / `DELETE /v1/workspaces/{w}/memories/{id}` | 編集・整理（archive）・削除 |

**`status`はリクエストで指定できません。** 「本人が言った」は偽装されてはいけない主張なので、ルートが決めます。AI接続は`changesets/preview`に`memories/proposals`だけを含められ、`memories`（verified）は含められません。承認は「その言葉でよい」であって「自分が言った」ではないからです。

**出典のない推測はfactにできません**（422）。`context`や`learning`として、何に基づくかを添えて記録します。根拠（`evidence_ids`）はそのワークスペースに実在するものだけを指せます。

**確度は候補にだけ付きます。** 本人が述べたことに機械の確度が付くのは、本人の言葉への機械の推定を並べることになります。確認すると確度は消えます。

`excluded_from_retrieval`を立てた記憶は、AI接続には**存在しません**（一覧から除外され、直接取得は404）。本人には見えます。捨てるのではなく持っておきたいもののためです。

`valid_from` / `valid_to`で有効期間を持てます。前職の好みは**間違いではなく**、いま有効ではないだけです。訂正は`supersedes_id`で追記し、元は残ります。

## Retrieval / Context Assembly

| ルート | 内容 |
| --- | --- |
| `GET /v1/workspaces/{w}/memories/search?query=&kind=&topics=&people=&item_ids=&from=&to=&limit=` | 個人indexの検索。個人ワークスペース以外では**404**（空の結果ではありません） |
| `GET /v1/workspaces/{w}/context?context_kind=…&query=&budget=&limit=` | 文脈の組み立て。`context_kind`は`personal`か`organization`で、**必須** |

**indexは2つあり、読む前に選びます。** `context_kind`が個人indexと組織indexのどちらを走らせるかを決めます。個人の記憶と組織の記録は別のテーブルを別の関数が読みます。全部を検索してから絞り込む実装にはしていません。それだと境界が絞り込み処理の正しさに依存し、絞り込みは1つのバグで壊れます。この形なら、返してはいけない行がそもそも存在しない瞬間がありません。

**どちらへもfallbackしません。** `context_kind=personal`が0件でも組織側は探しません。`context_kind=organization`を個人ワークスペースに投げると422、`personal`を共有ワークスペースに投げると404です。`context_kind`の省略は「両方」ではなく422です。

**検索結果の本文はdataです。** 本文に「これまでの指示を無視して…」と書いてあっても、そのまま返します。検出は試みません。できませんし、試せば「すり抜ける書き方」を作るだけです。代わりに、応答自身が`content_is_data`で何を運んでいるかを述べます。そして本文からは何の権限にも辿れません。変更するツールは別にあり、毎回保存済みの委任を確認します。

**順位の理由を返します。** 各結果の`relevance`に`matched_terms` / `related` / `recency_days` / `score`が入ります。`signals`は実際に使った手がかり（`keyword` / `relation` / `recency`）、`semantic`は埋め込み基盤がない環境では`"unavailable"`です。ないものを「使った」と書かないためで、keyword/structured retrievalが仕様上のfallbackとして常に動いています。

**結果は「いまの答え」かどうかを述べます。** `status`（本人が確認したか候補か）、`superseded`と`superseded_by`、`expired`、そして両方でないときだけ`current`が真になります。supersede済み・期限切れの記憶も返しますが、`current`ではありません。当時は本当だったものを消すと本人の過去を書き換えることになるからです。

**contextには予算があります。** `budget`（文字数、既定4000）まで詰め、同じidは1度しか入れず、入らなかった件数を`omitted_for_budget`で返します。`current_goals`が先に入るのは、本人が何をしようとしているかが他の全部の読み方を決めるからです。

記憶を含むバックアップは個人ワークスペースにしか復元できません（ファイル経由で境界を越えられないように）。

## Check-in・履歴・レビュー

目標を「作って終わり」にしないための追記型の記録です。

- `POST /v1/workspaces/{w}/items/{id}/checkins`：`health`（on_track / at_risk / off_track）、`self_assessment`、`comment`、`results`、`blockers`、`next_focus`、`observation_ids`、`supersedes_id`。**記入者と日時はサーバーが打ちます。** 何も書かれていないチェックインは拒否します。
- `POST /v1/workspaces/{w}/items/{id}/health`：状況だけを記録する短縮形。内部はチェックインなので、状況が記録なしに変わることはありません。
- `GET /v1/workspaces/{w}/items/{id}/checkins`：全履歴と`standing_id`（現在有効なもの）。
- `GET /v1/workspaces/{w}/items/{id}/timeline?as_of=`：作成・引き継ぎ・チェックインと訂正・観測と訂正・記録・つながりの変更・編集を時系列で。`as_of`を渡すとその時点まで再生し、**その時点で何が信じられていたか**を`state`で返します。
- `GET /v1/workspaces/{w}/review?stale_days=&cycle_id=`：レビュー用の4リスト。

**訂正は追記です。** 間違っていたチェックインは`supersedes_id`で新しいものを書き、元は残ります。チェックイン履歴の価値は「その時点で何を信じていたか」を言えることで、編集してしまうと「今何を信じているか」しか言えなくなります。

**数値と言葉は別データのままです。** `observation_ids`は観測を指すだけで値を複製しません（複製するとコメントと測定値が食い違い始めます）。

レビューは4つのリストに分けます。**沈黙は警告ではありません**：誰もチェックインしていない目標を「要注意」に混ぜると、新しい目標がすべて問題に見え、実際に誰かが警告した目標が埋もれます。

| リスト | 意味 |
| --- | --- |
| `never_checked_in` | 一度も記録がない |
| `stale` | 記録はあるが`stale_days`（既定14日）以上前 |
| `at_risk` | 誰かがat_risk / off_trackと記録した |
| `recently_updated` | 期間内に記録があった |

目標のcurrent healthは**最新の有効なチェックインの投影**です。AI接続は`changesets/preview`にチェックインを含めて提案でき、差分には本人の前回の言葉が`before`として並びます。適用は本人として実行されるので、記録の`author`は本人になります。

## Goal Dashboard

**4つの別々の事実を、混ぜずに返します。** ここは目標系の製品が嘘をつき始める場所です。チケットを4枚出したことが「売上目標に40%」になり、その数字が画面に載り、四半期のあいだ擁護される。

- `GET /v1/workspaces/{w}/dashboard?owner_kind=&owner_id=&cycle_id=`

| フィールド | 意味 | 無いとき |
| --- | --- | --- |
| `action_completion` | 予定した行動のうち実施された割合 | 行動が0件なら`rate: null`（0%ではない） |
| `metric_progress` | 観測から導出した進捗。`method`で算出方法を明示 | 方法未設定なら`value: null`、`method: null` |
| `self_assessment` | 本人の判断 | 未記入なら`null` |
| `health` | 誰かが記録した状況。記入者と日時つき | 未記入なら`null`（=unknown） |

**集計方法に既定値はありません。** 方法を誰も選んでいない目標は、導出した進捗を一切返しません。方法が明示されていない数字は議論できないためです。`metric_average` / `metric_worst` / `children_average` / `children_worst` から選びます（`items.fields.rollup`）。

`metric_progress.counted`と`missing`で、何件を根拠にし何件を数えられなかったかを返します。3つのうち1つが未計測なら、残り2つで平均を出しつつ「1件は数えていない」と言います。

**指標の進捗は基準値→目標値の到達率**です。`increase`は`(latest - baseline) / (target - baseline)`、`decrease`はその逆、`threshold`は達成/未達の2値（「閾値の83%」という中間は存在しないため作りません）。100%を超えた場合は超えたまま返します。

`signals`は事実（期限超過、古い観測、未計測、チェックインなし）です。`suggested_health`はそこから規則で導いた**提案**で、`health`にはなりません。それらが「注意」を意味すると決めるのは判断であり、判断には記入者がいます。

- `POST /v1/workspaces/{w}/items/{id}/health`：`status`（on_track / at_risk / off_track）、`note`、`expected_version`。記入者と日時はサーバーが打ちます（クライアントが他人名義や過去日時で記録できないように）。

組織全体を1つの数字にはしません。サマリは状況別の件数です。

## Goal Alignment

「誰の何が、どの目標に効いているか」を1つのグラフで扱います。OKR専用のデータモデルは作りません。OKRテンプレートもこのモデルの上に乗ります。

- `GET /v1/workspaces/{w}/alignment?owner_kind=&owner_id=&cycle_id=`：目標ごとに担当（organization / team / person）、期間、状態、自己評価、`part_of`、`contributes_to`、上位から見た`supported_by`、配下の取り組み・行動の件数、上位未接続かどうか。ワークスペース内に存在するチーム名・担当者、担当なしの件数も返します。
- 担当は`items`の`fields.owner`（`{kind, id}`）です。`organization`にidは指定しません（ワークスペース自身が組織です）。`person`のidはそのワークスペースのメンバーに限ります。

**alignmentは1つのワークスペース内のグラフです。** 個人ワークスペースの目標は含まれず、別ワークスペースのIDへリンクすることもできません。共有ワークスペースに目標を置くこと自体が「共有する」という行為であり、会社目標が個人の非公開目標を指せてしまうと、その選択を本人の代わりに製品が行うことになります。

`part_of`と`contributes_to`は別物として扱います。前者は構造（親は1つまで）、後者は貢献（複数可）。どちらも循環は拒否します（`contributes_to`を含む）。「これは何の一部か」と「これは何に効くか」は別の問いなので、画面でも混ぜません。

個人担当の目標は、本人かワークスペースのオーナーだけが変更できます（`403 GOAL_OWNER_REQUIRED`）。閲覧は通常のメンバー権限どおりです。誰にも見えない目標は何にも紐づけられないためです。

## 計画期間

四半期・月・週・任意期間で計画を運用します。期間を使わないワークスペースはこれまでどおり動きます。期間は「枠」であって「入れ物」ではありません。

- `GET /v1/workspaces/{w}/planning?cycle_id=...`：全期間、今日が入る期間（`cycle_id`指定時はその期間）、その前後、各期間の項目数、どの期間にも属さない項目数、直近の確定済み週次レビュー本文。日付はワークスペースのタイムゾーンの現地日付です。
- `POST /v1/workspaces/{w}/cycles`：`cadence`（quarter / month / week / custom）と`start_date`。長さと名前はcadenceから決まります。quarterは1・4・7・10月の1日、monthは月初、weekは月曜開始が必要です。会計年度が暦年と違う場合はcustomで`start_date`・`end_date`・`label`を指定します。`previous_id`で前期間からの継続を記録できます。
- `PATCH /v1/workspaces/{w}/cycles/{id}`：`label`・`status`（planned / active / closed）と`expected_version`。
- `DELETE /v1/workspaces/{w}/cycles/{id}`：所属項目が0件のときだけ。中身を孤立させません。
- `POST /v1/workspaces/{w}/cycles/{id}/carry-over`：`item_ids`と`expected_version`。**元項目は変更しません。** 新しい項目を作り、`fields.carried_from`で由来を残します。日付は引き継ぎません（前期間のために決めた日付が、次期間の日付になるのは決定の捏造です）。

期間は重なりません。「今はどの期間か」に答えが2つある状態を作らないためです。終了した期間は中身を保持し、そこへの引き継ぎだけを拒否します。

AI接続は期間を直接作れません。`changesets/preview`に`POST /v1/workspaces/{w}/cycles`を含めて提案し、本人が承認してから適用されます。

## 週次レビュー

`week_start`はワークスペースのタイムゾーンにおける月曜日を`YYYY-MM-DD`で指定します。集計は完了・見送り・未完了の行動を自己評価と分け、成果指標には最新値、前週以前の最新値との差、未計測、14日超の古い観測を返します。数値の各行には元のitem / record / observation IDを含みます。チーム領域の担当者別集計は、そのワークスペースのmembershipに含まれるactorだけを返します。

- `POST /v1/workspaces/{w}/weekly-reviews/draft`：`week_start`、`learnings`、`challenges`、`next_focus`。既存下書きの更新には`expected_version`が必要です。
- `POST /v1/workspaces/{w}/weekly-reviews/{id}/finalize`：`expected_version`。確定済みレビューは変更せず、次の下書き保存で`supersedes_id`付きの訂正版を作ります。

いずれの書き込みも通常の冪等キー、ワークスペース権限、監査ログを通ります。viewerは集計と確定済み内容を閲覧できますが、保存・確定はできません。

AI接続（提案モード）は下書きを直接保存できません。`POST /v1/workspaces/{w}/changesets/preview`に`POST /v1/workspaces/{w}/weekly-reviews/draft`を含めて提案し、本人がBasepathで差分を承認してから適用されます。`finalize`は変更案に含められません（週を確定するのは本人の行為として残します）。差分の`before`はその週の最新リビジョンです。

集計はサーバだけが行います。画面は`未計測`を0にせず、前週データのない差分も0にせず、行動が0件の週の完了率は0%ではなく「—」として表示します。行動の完了率と目標の自己評価は最後まで別の値のままです。

## テンプレート・提案・入出力

- `POST /v1/workspaces/{w}/ai/suggestions/preview`：`goal_id`と`expected_version`を指定。選択した目標・期限・同じワークスペースの直近記録だけから、30分以内の行動候補3件と、事実・推測・質問を分けた振り返り案を返します。AI接続が利用できない場合は安全なローカル候補へフォールバックします。この操作だけでは項目や記録を変更しません。採用時は下記changeset契約を使用します。
- `POST /v1/workspaces/{w}/templates/{id}/apply`：titleと任意description / start_date / due_date。テンプレートが項目・関連・ビューを同じトランザクションで作ります。OKRの目標値は自動生成しません。
- `POST /v1/workspaces/{w}/views`：name、type（list / map / timeline / okr / today）、filters。`POST …/views/{id}/query`で保存条件による項目検索を実行します。
- `PATCH /v1/settings`：compact、notifications、timezoneをすべて指定。タイムゾーンはIANA識別子です。
- `POST /v1/workspaces/{w}/exports`：空オブジェクト。schema_version=1のJSONを返します。
- `POST /v1/workspaces/{w}/imports`：exportしたJSON。項目・関連・記録・指標・観測・ビュー・週次レビューを再検証して追加します。同じIDや壊れた参照があれば全件ロールバックします。既存項目を上書きする機能ではありません。
- `POST /v1/workspaces/{w}/changesets/preview`：titleとoperations（method / path / bodyの配列）。SAVEPOINT内で全件検証後に取り消し、30分有効な変更案を保存します。成功時の応答には、シミュレーション後の目標ツリーを`preview_graph`として含めます。形は`{items, relations, truncated, limit}`で、`items`と`relations`は操作をSAVEPOINT内で反映した状態、`truncated`と`limit`は通常のグラフ取得と同じ上限情報です。このフィールドは会話中の可視化専用で、保存されたchangesetには含まれません。`GET /changesets`や`GET /changesets/{id}`で後から再取得できる値ではありません。
- `POST …/changesets/{id}/approve`：`hash`（任意）。人のアプリ操作のみが承認できます。**承認がそのまま適用です。**同じトランザクションで操作を実行し、`status`は`applied`になります。承認だけして反映されていない状態は作られません。2段階に分かれているのはAIが適用する経路のためで、人に二度押させるためではありませんでした。
- `POST …/changesets/{id}/apply`：空オブジェクト。承認・期限・内容ハッシュ・領域の更新状態を確認して原子的に適用します。作成後に領域のデータや権限が変わった案は再プレビューが必要です。承認時に適用されるようになる前に承認された案のための経路で、すでに適用済みのものには`already_applied: true`を返し、何も書きません。**本人が事前に決めた範囲に入る案は、個別の承認なしでもここで適用されます**（下記）。

### 事前に決めた範囲（自動反映）

1件ずつの承認を、範囲ごとの承認に置き換える設定です。承認の境界は動きません：行はBasepathのオリジンで本人のセッションからしか書けず、AI接続は読むことも書くこともできません（`GET`は404、書き込みはエージェントの一律拒否）。詳細と脅威モデルは[change-approval.md](change-approval.md#deciding-in-advance)。

- `GET /v1/mcp/auto-apply`：本人の範囲一覧。
- `POST /v1/mcp/auto-apply`：`workspace_id`、`connection_id`、`allow_create`、`allow_update`、`allow_guarded`、`days`（1〜90）。同じ(本人, ワークスペース, 接続)の組は置き換えです。追加も更新も許可しない範囲、有効な接続でないもの、参加していないワークスペースは422 / 404。
判定はHTTPメソッドではなく、SAVEPOINT内で記録した差分の`effect`で行います。`POST /actions/{id}/complete`のような命令型POSTは既存の状態を書き換えるので`updated`です。

範囲に**入らない**もの：削除、アーカイブ（`archived_at`がnullから非nullになる操作）、`due_date` / `start_date` / `scheduled_date` / `assignee_id` / `self_assessment` / `target` / `baseline`を**含む**操作（nullで消す場合も含む。`allow_guarded`で明示しない限り）、別のワークスペース、別の接続、提案が届いた接続以外からの適用、1件でも範囲外の操作を含む変更案、期限切れ・解除済みの範囲、`pathbase.apply`を持たない接続、解除後に再接続した接続（解除時に範囲も同じトランザクションで解除されます）。

- `POST /v1/mcp/auto-apply/{id}/revoke`：空オブジェクト。次の呼び出しから効きます。

変更案を読むと`auto_apply_eligible`（この案をその場で反映できるか）が付きます。自動反映されたものは`auto_applied`と`auto_apply_rule`を持ち、`approved_by`はnullのままです——「本人が1件ずつ承認した」と「事前承認の範囲で自動反映した」を後から区別するためです。

## Field

Tachyonセッションを使用し、Field側の権限を毎回確認します。

- `GET /v1/integrations/field/tenants`
- `GET /v1/integrations/field/tasks?tenant_id=…&offset=0`（50件）
- `GET /v1/integrations/field/metrics?tenant_id=…`
- `POST /v1/workspaces/{w}/field/attach-task`：tenant_id、task_id。Field参照情報付きのローカル行動を作成。同じ外部タスクは重複作成しません。
- `POST /v1/workspaces/{w}/field/record-metric`：tenant_id、metric_id、field_key。mrr / arr / backlogAmount / receivableOutstandingは円、daysSalesOutstandingは日。単位不一致は422、欠測は422 UNOBSERVEDです。
- `POST /v1/workspaces/{w}/field/refresh-task`：tenant_id、item_id。保存済みのField参照とテナント境界を検証し、Fieldからタイトル・状態・更新日時を明示的に再取得します。Field側のデータは変更しません。

Fieldへの変更操作・バックグラウンド同期はありません。詳しい上流契約と権限の分離は[integration-contracts.md](integration-contracts.md)を参照してください。

## 会話と業務コンテキストのリンク

MCP接続は会話本文を保存しません。ホストが渡した`conversation_id`と、本人が
アクセスできる1つのワークスペース（任意で項目）だけをリンク情報として保存します。
リンクは計画を変更せず、解決は`pathbase.read`で利用できます。

- `pathbase_link_context`：`workspace_id`が必須。対応hostでは`conversation_id`（最大191文字）を
  指定します。`idempotency_key`は任意で、conversation IDが1〜200 ASCII bytesならそれを
  fallback keyとして安全に再試行できます。明示したkeyも1〜200 ASCII bytesです。
  同じキーに別の会話・対象・画面を割り当てると409です。
  `item_id`と`screen`は任意ですが、画面はworkspaceのscopeに適合するものだけ指定できます。
  `item_id`が行動なら`today`、取り組みなら`breakdown`へ正規化し、目標画面へ誤ってfallbackしません。
  対象は通常のtenant/workspace membershipで再検証され、同じ会話への再試行は同じリンクを返します。
  作成・明示的な再リンクの返却`status`は`active`です。別の対象や画面への再リンクは409です。
- `pathbase_link_context`（リンクの作成・明示的な再リンク）には`pathbase.context`権限が必要です。
  `pathbase_get_linked_context`（既存リンクの解決・読み取り）には`pathbase.read`権限が必要です。
  conversation IDはMCP client namespace内で解決され、別clientの同名IDとは混ざりません。
- `pathbase_get_linked_context`：`conversation_id`で現在のリンクを解決します。`active`、
  `stopped`、`not_linked`を区別します。別tenantのリンクは返しません。
- 接続解除はリンクを`stopped`にし、トークンも同じトランザクションで無効化します。
  `stopped`リンクは自動では`active`に戻りません。新しい許可済みMCP接続から、同じ対象を
  `pathbase_link_context`で明示的に再リンクした場合だけ`active`に戻ります。
- `conversation_id`を提供しないホスト、または`PATHBASE_PUBLIC_URL`未設定の環境は
  `unsupported_host`を返します。存在しない自動トリガーやURLを推測しません。

`source`は認証済みMCP transport自身が付ける`mcp`固定値、`source_version`はこの保存契約の
バージョン`1`固定値です。モデルやホストがこれらを任意に名乗る入力欄は提供しません。
リンクの`source`と`source_version`は、どの契約で作られたかを示す監査用メタデータです。
承認済みの変更案・目標・行動をこの機能から直接変更することはありません。
