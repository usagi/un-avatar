# 機能マップの初期実装

目的から操作する既存画面と、全体を俯瞰する機能マップを同格の入口として提供する。13機能から全10Profile設定区分およびRenderer / Logs / Appへ移動する。新しい設定ストアやruntime更新コマンドは追加していない。

- 日本語の用途と技術用語・全角表記の検索。
- 対象Profileを選択してもRendererを起動・停止・切替しない。
- 設定区分へ移動後にスクロールし、そのsectionへフォーカスする。
- モデル・入力→描画→カメラ・出力の分類。運用の入口は別段にまとめる。

## 取得失敗ループ

ブラウザー用IPCにget_avatar_setting_detailsがなく、取得失敗時にloadedProfileDetailKeyを空へ戻していた。詳細を読むeffectがこのcache keyも依存として追跡するため、失敗→cache reset→再取得のmicrotask loopが発生した。effectの依存を選択Profileへ限定し、loader内のcacheの読み取りをuntrackで包んだ。失敗後の再読込は既存の更新／対象選択で行う。

dev mockには詳細取得と空のcache statusを実装し、Tauri mock標準のshouldMockEventsを有効にした。実ファイルやRendererは操作しない。

## 確認

- Svelte check: 0 errors / 0 warnings。TypeScript check、Vite build成功。
- `node --test scripts/feature-catalog.test.mjs`: 2件成功。検索と既存全10区分の到達先・focusを確認。
- ブラウザー: Debug Avatar選択→Spout2検索→Debug Avatarの出力欄へ移動・focus。
- 一時的に詳細取得へエラーを注入し、エラー表示後も機能マップへ移動できた。注入コードは検証後に除去した。
- `cargo xtask release-guard`: 成功（2026-09-29）。
- 機能マップは区分単位。全個別フィールドの検索索引・目的別フォーム再構成・共通UI packageへの抽出は未完了。
- 配布EXE更新、Renderer / OBS出力の実機検証は今回行っていない。
