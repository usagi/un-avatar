# U.N. Avatar Roadmap

v1 公開時点の実装状況と、今後の候補を短くまとめる。

## v1 の範囲

v1 は **VRM / glTF avatar renderer + Supervisor Console + UNMF/Z / VMC input + Window / Spout2 output** を最初の安定対象とする。

完了済みの主な範囲:

- Supervisor Console による profile 管理、Renderer 起動 / 停止 / 監視
- 複数 Renderer の同時起動
- VRM / glTF import
- MToon-like rendering
- GPU skinning / GPU morph
- UNMF/Z input
- VMC/UDP input
- SpringBone simulation
- Bone-based collider generation / debug display
- Window output
- Transparent / frameless / topmost / click-through window controls
- Background color
- Screenshot saving
- Spout2 Sender output on Windows
- Runtime status / control channel
- Camera controls and preview
- Lighting controls
- Look / post effects controls
- Texture resize policy, mipmaps, processed texture cache, block-compressed texture cache
- Profile avatar thumbnail cache
- Diagnostics / logs view
- `cargo xtask build`, `run`, `render-smoke`, `package`, `release-package`

## v1 で意図的に広げない範囲

- Avatar authoring / modeling tool
- Motion capture itself
- Webcam input
- NDI output
- Recording / timeline / animation editor
- Full material-authoring UI
- Scene editor
- Multi-user network session
- General-purpose game engine integration

これらは将来候補であり、v1 の安定化より優先しない。

## 次の候補

必要性が見えた順に検討する。

- v2 planning: `.unavatar` GLB 互換形式、VRC / Unity Exporter、lilToon / PhysBone / expressions / variants 対応は [`v2-roadmap.md`](v2-roadmap.md) を正とする
- Release QA: DPI / small window / long profile name / first-run profile storage の確認を増やす
- Documentation: ユーザー向け導入手順、OBS + Spout2 の短い手順、トラブルシュート
- Renderer performance: 実測ベースで startup / frame CPU / GPU time を継続改善
- Texture pipeline: KTX2 / BasisU container and transcode、ASTC / ETC2 upload path
- Runtime control: 実際に頻繁に使う操作だけを追加
- Shortcuts / MIDI / REST API: 表情やクイック操作の外部トリガー
- Presets: camera / lighting / look のプリセット
- Plugin / IO: 実利用が見えた形式から追加

## リリース前確認

リリース前は [`development-guidelines.md`](development-guidelines.md) の確認を行う。

```sh
cargo xtask ci
cargo xtask run --release
cargo xtask release-package --version 1.0.0
```

## 関連文書

- [`runtime-mvp.md`](runtime-mvp.md)
- [`render-quality-plan.md`](render-quality-plan.md)
- [`profile-settings-ui-v1-design.md`](profile-settings-ui-v1-design.md)
- [`third-party-licenses.md`](third-party-licenses.md)
