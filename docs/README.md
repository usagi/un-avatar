# U.N. Avatar Documents

v1 公開時点で残す文書の索引。

## まず読む

| 文書 | 内容 |
| --- | --- |
| [`../README.md`](../README.md) | ユーザー向け概要、使い方、開発者向けコマンド |
| [`roadmap.md`](roadmap.md) | 現在の実装状況、v1 の範囲、今後の候補 |
| [`v2-roadmap.md`](v2-roadmap.md) | `.unavatar` / VRC Unity Exporter を中核にした v2 計画 |
| [`runtime-mvp.md`](runtime-mvp.md) | VRM / VMC / MToon / wgpu / Spout2 runtime の境界 |
| [`third-party-licenses.md`](third-party-licenses.md) | 配布物に含める third-party license 表示 |

## 開発・設計メモ

| 文書 | 内容 |
| --- | --- |
| [`development-guidelines.md`](development-guidelines.md) | ローカル検証、xtask、リリース前確認の基本 |
| [`render-quality-plan.md`](render-quality-plan.md) | AA、texture cache / compression、renderer 品質方針 |
| [`v2-near-term-plan.md`](v2-near-term-plan.md) | AudioLink 初期対応後の短期作業順、リファクタリング、Wardrobe hot switch 方針 |
| [`v2-ui-gui-operation-plan.md`](v2-ui-gui-operation-plan.md) | v2 Supervisor / Renderer tray / launcher の UI・GUI 運用設計 |
| [`unavatar-format-v0.1.md`](unavatar-format-v0.1.md) | `.unavatar` GLB extension preview spec |
| [`unity-exporter-v0.1.md`](unity-exporter-v0.1.md) | Unity Editor Exporter の境界、配置、MVP |
| [`unevaluation-v2.md`](unevaluation-v2.md) | v2 runtime evaluation、owner policy、Contacts parameter phase 設計 |
| [`unity-exporter-png-encoding.md`](unity-exporter-png-encoding.md) | Unity Exporter の RAW RGBA PNG encoding 方針と fpng benchmark |
| [`untoon-dynamic-variant-architecture.md`](untoon-dynamic-variant-architecture.md) | MToon / lilToon を UNToon semantic へ統合し、モデル要求から shader/resource variant を作る設計 |
| [`compute-fur-cards-design.md`](compute-fur-cards-design.md) | lilToon Fur の Geometry Shader 互換を Compute で実現する設計 |
| [`liltoon-fur-technical-target.md`](liltoon-fur-technical-target.md) | lilToon Fur の本家挙動と UNAvatar 側の技術目標 |
| [`modular-avatar-compatibility.md`](modular-avatar-compatibility.md) | Modular Avatar bake 相当 resolver の対応計画 |
| [`unity-exporter-dependency-research.md`](unity-exporter-dependency-research.md) | Unity Exporter の GLB writer / Modular Avatar bake 依存調査 |
| [`v2-open-decisions.md`](v2-open-decisions.md) | v2 実装前に相談・決定する項目 |
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
