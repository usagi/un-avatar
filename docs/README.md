# U.N. Avatar Documents

v1 公開時点で残す文書の索引。

## まず読む

| 文書 | 内容 |
| --- | --- |
| [`../README.md`](../README.md) | ユーザー向け概要、使い方、開発者向けコマンド |
| [`roadmap.md`](roadmap.md) | 現在の実装状況、v1 の範囲、今後の候補 |
| [`runtime-mvp.md`](runtime-mvp.md) | VRM / VMC / MToon / wgpu / Spout2 runtime の境界 |
| [`third-party-licenses.md`](third-party-licenses.md) | 配布物に含める third-party license 表示 |

## 開発・設計メモ

| 文書 | 内容 |
| --- | --- |
| [`development-guidelines.md`](development-guidelines.md) | ローカル検証、xtask、リリース前確認の基本 |
| [`render-quality-plan.md`](render-quality-plan.md) | AA、texture cache / compression、renderer 品質方針 |
| [`profile-settings-ui-v1-design.md`](profile-settings-ui-v1-design.md) | Supervisor Console の Profiles / Renderers UI 情報設計 |
| [`bone-based-colliders-v1.md`](bone-based-colliders-v1.md) | ボーンベースコライダーの v1 設計 |
| [`spring-bone-physics-v1.md`](spring-bone-physics-v1.md) | SpringBone solver と profile 設定の設計 |
| [`adr/`](adr/) | 後から変えにくい設計判断の記録 |

## 歴史的な設計メモ

次の文書は v1 の公開ユーザードキュメントではなく、実装経緯を追うための内部設計メモとして残す。

| 文書 | 内容 |
| --- | --- |
| [`development-plan.md`](development-plan.md) | 初期の製品計画 |
| [`crate-io-plugin-plan.md`](crate-io-plugin-plan.md) | crate 分割、IO / plugin host の初期設計 |
| [`process-renderer-gui-design.md`](process-renderer-gui-design.md) | Supervisor / Renderer プロセス分離と GUI / IPC の初期設計 |

これらの歴史的メモと現行実装が食い違う場合は、現行コード、[`../README.md`](../README.md)、[`roadmap.md`](roadmap.md)、[`runtime-mvp.md`](runtime-mvp.md) を優先する。
