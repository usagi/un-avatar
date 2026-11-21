# U.N. Avatar Documents

U.N. Avatar の公開文書と実装・設計メモの索引。初めて使う場合は `README.md` と `v2-getting-started.md` を優先する。開発メモは現行実装と食い違う場合があるため、最終的な挙動は現行コード、README、release package の動作を優先する。

配布版にも初回操作・更新と復元・Exporterの説明を同梱している。Webへ移動するリンクは開発・設計資料で、更新により配布版と内容が異なる場合がある。

## まず読む

| 文書 | 内容 |
| --- | --- |
| [`../README.md`](../README.md) | ユーザー向け概要、v2 でできること、初回導線 |
| [`v2-getting-started.md`](v2-getting-started.md) | 初回起動、VRC Exporter、Wardrobe の操作手順 |
| [`update-and-recovery.md`](update-and-recovery.md) | 更新前のバックアップ、設定と外部素材の保持、旧版への復元 |
| [`v2-roadmap.md`](v2-roadmap.md) | `.unavatar` / VRC Unity Exporter を中核にした v2 計画 |
| [`roadmap.md`](https://github.com/usagi/un-avatar/blob/main/docs/roadmap.md) | 実装状況、v1 から v2 への範囲、今後の候補 |
| [`runtime-mvp.md`](https://github.com/usagi/un-avatar/blob/main/docs/runtime-mvp.md) | VRM / VMC / MToon / wgpu / Spout2 runtime の境界 |
| [`unaa-z-camera-effect.md`](https://github.com/usagi/un-avatar/blob/main/docs/unaa-z-camera-effect.md) | Renderer単位のUNAA/Z identityと原子的カメラ演出契約 |
| [`third-party-licenses.md`](third-party-licenses.md) | 配布物に含める third-party license 表示 |

## 開発・設計メモ

| 文書 | 内容 |
| --- | --- |
| [`development-guidelines.md`](development-guidelines.md) | ローカル検証、xtask、リリース前確認の基本 |
| [`render-quality-plan.md`](https://github.com/usagi/un-avatar/blob/main/docs/render-quality-plan.md) | AA、texture cache / compression、renderer 品質方針 |
| [`v2-near-term-plan.md`](https://github.com/usagi/un-avatar/blob/main/docs/v2-near-term-plan.md) | AudioLink 初期対応後の短期作業順、リファクタリング、Wardrobe hot switch 方針 |
| [`v2-ui-gui-operation-plan.md`](v2-ui-gui-operation-plan.md) | v2 Supervisor / Renderer tray / launcher の UI・GUI 運用設計 |
| [`unavatar-format-v0.1.md`](https://github.com/usagi/un-avatar/blob/main/docs/unavatar-format-v0.1.md) | `.unavatar` GLB extension preview spec |
| [`unity-exporter-v0.1.md`](unity-exporter-v0.1.md) | Unity Editor Exporter の境界、配置、MVP |
| [`unevaluation-v2.md`](https://github.com/usagi/un-avatar/blob/main/docs/unevaluation-v2.md) | v2 runtime evaluation、owner policy、Contacts parameter phase 設計 |
| [`untoon-dynamic-variant-architecture.md`](https://github.com/usagi/un-avatar/blob/main/docs/untoon-dynamic-variant-architecture.md) | MToon / lilToon を UNToon semantic へ統合し、モデル要求から shader/resource variant を作る設計 |
| [`modular-avatar-compatibility.md`](https://github.com/usagi/un-avatar/blob/main/docs/modular-avatar-compatibility.md) | Modular Avatar bake 相当 resolver の対応計画 |
| [`adr/`](https://github.com/usagi/un-avatar/tree/main/docs/adr) | 後から変えにくい設計判断の記録 |

## 歴史的な設計メモ

次の文書は公開ユーザードキュメントではなく、実装経緯を追うための設計メモとして残す。

| 文書 | 内容 |
| --- | --- |
| [`development-plan.md`](https://github.com/usagi/un-avatar/blob/main/docs/development-plan.md) | 初期の製品計画 |
| [`crate-io-plugin-plan.md`](https://github.com/usagi/un-avatar/blob/main/docs/crate-io-plugin-plan.md) | crate 分割、IO / plugin host の初期設計 |
| [`process-renderer-gui-design.md`](https://github.com/usagi/un-avatar/blob/main/docs/process-renderer-gui-design.md) | Supervisor / Renderer プロセス分離と GUI / IPC の初期設計 |
| [`unity-exporter-png-encoding.md`](https://github.com/usagi/un-avatar/blob/main/docs/unity-exporter-png-encoding.md) | Unity Exporter の RAW RGBA PNG encoding 方針と fpng benchmark |
| [`unity-exporter-dependency-research.md`](https://github.com/usagi/un-avatar/blob/main/docs/unity-exporter-dependency-research.md) | Unity Exporter の GLB writer / Modular Avatar bake 依存調査 |
| [`compute-fur-cards-design.md`](https://github.com/usagi/un-avatar/blob/main/docs/compute-fur-cards-design.md) | lilToon Fur の Geometry Shader 互換を Compute で実現する設計 |
| [`liltoon-fur-technical-target.md`](https://github.com/usagi/un-avatar/blob/main/docs/liltoon-fur-technical-target.md) | lilToon Fur の本家挙動と UNAvatar 側の技術目標 |
| [`v2-open-decisions.md`](https://github.com/usagi/un-avatar/blob/main/docs/v2-open-decisions.md) | v2 実装前に相談・決定していた項目 |
| [`profile-settings-ui-v1-design.md`](https://github.com/usagi/un-avatar/blob/main/docs/profile-settings-ui-v1-design.md) | Supervisor Console の Profiles / Renderers UI 情報設計 |
| [`bone-based-colliders-v1.md`](https://github.com/usagi/un-avatar/blob/main/docs/bone-based-colliders-v1.md) | ボーンベースコライダーの v1 設計 |
| [`spring-bone-physics-v1.md`](https://github.com/usagi/un-avatar/blob/main/docs/spring-bone-physics-v1.md) | v1 SpringBone 実装資産と v2.1 UNDynamics への移行メモ |

これらの歴史的メモと現行実装が食い違う場合は、現行コード、[`../README.md`](../README.md)、[`v2-getting-started.md`](v2-getting-started.md)、[`roadmap.md`](https://github.com/usagi/un-avatar/blob/main/docs/roadmap.md)、[`runtime-mvp.md`](https://github.com/usagi/un-avatar/blob/main/docs/runtime-mvp.md) を優先する。
