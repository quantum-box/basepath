# PathBase

参考画像をもとに実装した、React + TypeScript + Tauri 2 の目標管理デスクトップUIです。

## 起動

Node.js 22.12以降とRust、OSごとの[Tauri開発環境](https://v2.tauri.app/start/prerequisites/)を使用します。

```sh
npm install
npm run tauri dev
```

ブラウザだけで確認する場合は `npm run dev` で http://localhost:1420 を開きます。開発サーバーが既に起動している場合は、そのプロセスを終了してから通常の `tauri dev` を実行してください。

```sh
npm run check                        # TypeScript
npm run build                        # フロントエンド
npm run tauri -- build --debug --bundles app   # macOSのローカル検証用.app
npm run tauri build                  # リリースビルド
npm run format                       # ソース整形
```

この環境で生成したアプリ: `src-tauri/target/debug/bundle/macos/PathBase.app`

## 実装した操作

- 個人・チーム・組織の目標マップ、絞り込み、拡大・縮小、全画面表示
- 目標の選択と詳細パネルの連動、目標編集、メモ編集、次の一歩の完了
- 5種類のテンプレート選択と目標作成
- 取り組みの追加、進捗更新
- 今日の行動の追加と完了チェック
- タイムラインの期間切り替えと目標選択
- 目標・行動・メンバー検索（⌘K / Ctrl+K）
- 振り返り入力、メンバー・通知・設定パネル
- 狭いウィンドウでは縦レイアウトと折りたたみメニュー

UI確認用のサンプルデータです。画像に合わせて2025年4月の予定を表示し、編集内容はReactのメモリ上で保持します。アプリの再起動・ページの再読み込みで初期状態に戻ります。アカウント認証、クラウド同期、ファイル保存、実通知の送信は未実装です。

## 構成

- `src/App.tsx`: ダッシュボードと操作フロー
- `src/GoalMap.tsx`: React Flowのノード・接続線・表示制御
- `src/data.ts`: 型定義とサンプルデータ
- `src/styles.css`: 配色・余白・レスポンシブレイアウト
- `src/fonts.css`: オフライン利用できる日本語フォント
- `src-tauri/`: Rustエントリポイント、ウィンドウ設定、アプリアイコン
- `ASSETS.md`: 生成素材と生成プロンプト

画像とフォントはアプリに同梱しています。UIアイコンは[Phosphor Icons](https://github.com/phosphor-icons/react)、マップはReact Flow、書体はNoto Sans JPとZen Kurenaidoです。TauriとViteの接続は[公式のViteガイド](https://v2.tauri.app/start/frontend/vite/)に従い、このプロジェクトの出力先 `dist/client` を参照しています。

`worker/` と `scripts/prepare-sites-build.mjs` はスターター由来のWeb公開用構成です。今回、外部への公開は行っていません。
