# UNAvatar v2 近々の仮計画

この文書は、lilToon-like AudioLink 初期対応後の短期作業順を固定する。

## 現在位置

- AudioLink は v2 初期範囲として十分に完了した扱いにする。
- lilToon-like rendering は互換性優先を維持する。今後の見た目調整は、lilToon 本家実装または具体的な観測差分を根拠にする。
- lilToon 互換が成立したので、MToon / lilToon を別 renderer として並べるのではなく、UNToon semantic material と dynamic variant planning へ整理する。正本は [`untoon-dynamic-variant-architecture.md`](untoon-dynamic-variant-architecture.md)。
- 次の大きな価値は VRC model import / runtime behavior。Wardrobe 高速切替、expression、runtime action、parameter / contact diagnostics は v2 初回検証に十分な線まで進んだため、以後の主 blocker は UNDynamics / PhysBone behavior implementation とする。
- これらを足す前に、runtime state が読みにくくならない程度のリファクタリングと最適化を行う。

## 近々の順序

1. 現状の v2 renderer / runtime 実装をほどほどにリファクタリングし、最適化する。
2. VRC import base の `.unavatar` skinning / morph を既存 GPU skinning / morph pipeline に接続・検証し、UNToon dynamic variant planning の resource reservation に接続する。
3. VRM SpringBone / VRC PhysBone source を UNPhysics umbrella 下の UNDynamics runtime model へ lower する正規化境界を設計し、runtime model view に接続する。
4. renderer 再起動なしの Wardrobe hot switch を実装する。
5. VRC Expression Menu、toggle、hotkey、将来の ring menu emulation 向け runtime action model を作る。
6. action model の上に imported animation / expression / material / visibility evaluation を足す。
7. Wardrobe / action / contact parameter の初期所有関係は固定済みとして扱い、PhysBone behavior implementation を UNDynamics runtime model 上で進める。
8. instant switching が正しく安定してから、お着替え transition effect を足す。

## リファクタリング / 最適化範囲

この段階では中程度に留める。美観だけを理由に、動いている subsystem を大きく作り直さない。

現在の進捗:

- `UnaRuntimeModel` / `UnaRuntimeModelMut` は scene、humanoid、expression、runtime dynamics を読む境界として導入済み。
- renderer、skeleton retarget、CLI diagnose は、frame loop / solver / diagnostics で source-format field を直接読む箇所を減らし、runtime accessor 経由へ寄せている。
- `HumanoidRetargetContext` は `UnaRuntimeRetargetInputs` から構築できるようになり、renderer の retarget runtime は document source field ではなく runtime model view から compile する。
- `UnaRuntimeDynamics` / `UnaRuntimeDynamicsMut` は SpringBone / PhysBone source settings を直接渡す逃げ道を閉じ、groups / colliders / counts / dynamic node iterator / source id enable mutation の view として solver / renderer / wardrobe importer に渡す。Dynamics enable mutation は source group の authored default ではなく `UnaRuntimeState.dynamics_enabled_overrides` へ書く。
- まだ `UnaDocument` 自体は source data と runtime state を同居させる transitional container であり、Wardrobe hot switch 前に resolved wardrobe state / active asset groups / action state / runtime parameter values の所有境界をさらに分ける。
- scene node は source node id と runtime resolved node id を別フィールドとして保持し、runtime node target は source id 優先のまま resolved id / path / index fallback へ解決できる。MA Replace Object のような resolver 派生 node は source id を authored target として残し、resolved id を cache / diagnostics 用に付与する。
- `.unavatar` / glTF / GLB import は、root `UN_avatar` extension に Modular Avatar payload がある場合は resolver を正本にし、payload がない別アーマチュア衣装は Humanoid 同名骨 fallback で retarget する。同名 Humanoid 接続点にぶら下がる non-Humanoid 補助骨 subtree は world pose を保って主 armature へ reparent する。ただし fallback は constraints、PhysBone behavior、blendshape / material side effects、曖昧な重複骨名の完全解決までは復元しない。
- 2026-06-09 時点の目視確認では、これまで見つかっていた `mizuki-split.unavatar` の visual regression は期待動作まで解決済み。Perfect Sync 対応 `.unavatar` は表情 / blendshape と sparse MA payload export の検証対象にする。`.unavatar` importer は VRC/ARKit Perfect Sync の既知 52 morph 名を runtime expression catalog に登録し、body / wardrobe morph まで全登録しない。VRC Menu / Animator / Modular Avatar 由来の morph は runtime action / resolver から参照された名前だけを別途 catalog に入れる。

優先領域:

- immutable source package data と runtime state を分ける。
  - `.unavatar` / glTF source data
  - resolved wardrobe state
  - pose、morph、material、expression、action state、dynamics state
  - GPU resources / cache
- active asset groups は runtime state、asset group ownership は scene source data に属する。両者の合成は core の document-level scoped asset selection query に集約し、scene が無い場合も active groups を missing として扱う。renderer / diagnose / wardrobe apply report / future physics は同じ解釈を使う。Unity Exporter は renderer ごとの mesh primitive / material / image index 診断を出し、宣言済み wardrobe asset group と renderer / PhysBone source path が一致する範囲で `wardrobe.assetGroupOwnership` を自動生成する。
- wardrobe visibility と morph change を renderer control、VRC menu action、shortcut、将来の animation evaluation から再利用できる形にする。
- render thread の work は bounded / nonblocking に保つ。AudioLink で固定した方針を skinning、animation、physics にも適用する。
- 生成 fallback resources、bind groups、optional material textures 周辺の brittle な indexing assumption は、実害が見える箇所から減らす。
- refactor 中も lilToon compatibility behavior を維持する。既知の mismatch 修正に必要でない semantic rewrite は避ける。
- 広い snapshot churn より、state resolution、resource indexing、command application の focused test を優先する。

## Performance Work Queue

mizuki-split class の `.unavatar` でも起動体験は実用域に近づいたが、v2 初回リリースまで継続して loading / upload / shader / splash / runtime CPU を削る。速度最適化は必ずプロファイルかコード上の明確な不要 work を根拠にし、品質劣化やモデル固有 hack で代替しない。

優先順:

1. PostProcess pipeline lazy creation
   - 現状は outline / bloom / FXAA / SMAA などの post pipeline を起動時にまとめて作る。実際に有効な AA、Bloom、avatar silhouette outline、SSAO / color adjust / screen refraction に必要な pipeline だけを作る。
   - resize / runtime option change で後から必要になった pipeline はその時点で作る。起動時に未使用 shader を compile しない。
   - 2026-06-13 に `PostProcess::new` / resize 時の unconditional FXAA / color-adjust pipeline creation を廃止し、各 encode path の初回呼び出しでだけ作るようにした。SMAA / Bloom / Silhouette Outline は既存の lazy pipeline path を維持する。
   - debug axes pipeline も通常起動では作らず、`show_axes` が有効になった時だけ作る。
   - contact shadow は既定 OFF の viewer-space effect なので、pipeline だけでなく bind layout / uniform buffer / bind group も初回使用まで作らない。
2. Pipeline cache / prewarm
   - Vulkan `PipelineCache` は既に導入済み。透明 window など backend 制約で Vulkan cache を使えない場合は UI / profile diagnostics に明示する。
   - Supervisor / renderer warmup mode は、実用前に共有 pipeline cache と texture / compressed texture cache を作る用途として扱う。warmup は通常起動の見た目品質を落とす代替ではない。Renderer CLI は `--prewarm-scene-cache` で、ウィンドウなしに対象profile / wardrobe setのGPU sceneを構築して終了できる。
   - 通常の Quick Launch は prewarm 専用 renderer を同期実行しない。warmup はユーザーが明示的に `キャッシュ準備` を押した時だけ行い、2 回目以降の実用起動を速くするための準備作業として扱う。
   - `キャッシュ準備` は Renderer 起動中でも profile actions に表示する。launch-time quality 設定を変えたあと、既存 Renderer を止めずに次回起動用 cache を明示準備できるようにする。live Renderer がある場合は別プロセス warmup による一時的な GPU / disk 負荷を hint で明示する。
   - Supervisor の `キャッシュ準備` 完了通知は、Renderer stderr の `gpu scene texture prepare summary` と `pipeline cache store` から processed / compressed texture cache と pipeline cache の結果を要約する。`mizuki-split / field_drape` の prewarm 確認では `processed_cache=38/0/0`、`compressed_cache=21/0/0`、pipeline cache store あり、`scene cache prewarm total=4591.5ms`。
3. Texture upload / source cache
   - inactive wardrobe image decode は defer 済み。次は deferred cubemap conversion、source-native upload、processed texture cache の役割を分離する。
   - decoded 済み画像の encoded source bytes は保持せず、deferred placeholder のみ lazy decode 用に保持する方針を維持する。
   - 2026-06-13 の `mizuki-split / field_drape` 比較では processed texture cache 有効時は `texture prepare total=1.1-1.3s`、`processed_cache=40/0/0`、`compressed_cache=19/0/0`。`--no-processed-texture-cache` は `texture prepare total=5.8s` まで悪化したため、cache read が見えても既定無効化はしない。次に削るなら cache artifact の read 量 / upload 量 / active resident texture 数を分けて測る。
   - 2026-06-13 に GLB import の initial resident image decode を lazy 化。`mizuki-split / field_drape` は `gltf_import_slice image_decode_ms=0`、`texture prepare total=1.10-1.17s`、`rgba=0ms`、`processed_cache=39/0/0`、`compressed_cache=21/0/0`、`bench-gpu-scene total=3.67s`。lazy decode は processed texture cache policy に連動し、cache 無効時は従来通り eager decode する。
   - `.unavatar` / GLB path import の `UN_avatar.textureAssets` は、scene 構築後まで GLB 全体の `Vec` を保持せず、必要な bufferView 範囲だけを source file から read して decode する。`mizuki-split / field_drape` では `textureAssets decoded=3 source_bytes=11094650 decode_ms=38 file_backed=true`。これは起動時間短縮ではなく、779MB 級 GLB の import 中 RAM peak を下げる変更。
   - processed cache hit の read timing を分離した結果、`texture prepare total=1.02-1.07s` の内訳は `cache_read=795-834ms`、`processed=0.3ms`、`upload=202-210ms`、`processed_cache_read_mb=2503MB`。64-85MB の 4K mip-chain cache artifact が多数 resident になっているため、次の起動短縮対象は処理計算ではなく、cache artifact の read 量 / ファイル形式 / resident texture 数 / upload call 数。
   - 2026-06-13 post-summary-tool bench: current default hot `cargo xtask run-renderer --release --profile mizuki-split --wardrobe-set field_drape -- --bench-frames 180 --no-fps-title` は `import_ms=790.7`, `texture_ms=979.2`, `cache_read_ms=749.9`, `upload_ms=208.4`, `mesh_ms=129.5`, `fps=60.0`。`cargo xtask summarize-renderer-log` はこの比較用TSVを出し、top texture も出す。
   - glTF import vertex payload cache の最後の参照を clone せず cache から move するように変更。`mizuki-split / field_drape` では `read_meshes_ms=224 -> 194`、`scene_snapshot_ms=249 -> 208`、`cache_clone=45ms -> 6ms`。texture hot path は実行ばらつきがあるため別評価。
   - `gltf_import_slice_ms=101` の内訳は `gltf_parse_ms=101`, `gltf_buffers_ms=0`, `gltf_image_decode_ms=0`。GLB の部分 read 化は `std::fs::read` だけでなく `gltf::Gltf::from_slice` 境界の再設計が必要。小手先の mmap 試行は過去ログで改善しなかったため、次は独自 GLB root/bin parse と glTF document construction の責務分離を設計してから触る。
   - slow texture line は image index だけでなく source image name / mime も出す。`mizuki-split / field_drape` では `Body_b` / `underwear` と、field_drape の `Tex_Hat` / `Tex_Dress` / `Tex_*_ao` / `Tex_*_Metallic` などの巨大 PNG cache artifact が主要 read source。ここから先の削減は「使っていない texture を resident にしない」か「source semantics に基づいて cache artifact 形式を変える」設計判断として扱う。
   - role 別 texture prepare summary を追加。`mizuki-split / field_drape` では `Data=20/39 read=1239.8MB/386.6ms upload=88.9ms`、`GenericColor=28/161 read=1071.2MB/328.6ms upload=100.9ms`、`Clothing=3/9 read=192.0MB/60.8ms upload=12.1ms`。Data mask を安直に圧縮・mip削減すると見た目 / lilToon互換へ影響しうるため、次は Data / GenericColor の source semantics と linear/BC7-unorm 等の形式設計を先に詰める。
   - Data texture の明示 opt-in 用に BC7 linear (`Bc7RgbaUnorm`) upload/cache path を追加。既定 `balanced` は Data を `source` のまま維持するため回帰なし。`render_quality.texture_compression_advanced.data = "high_quality"` の実験では、初回cache生成は `texture prepare total=9.93s` と重いが、2回目以降は `total=729.6ms`、`processed_cache_read_mb=1263.2`、`Data read=0.0MB`、`Data compressed_hits=20` まで下がる。既定化は Data mask の見た目差分と warmup / prewarm 運用を確認してから判断する。
   - `balanced` の `texture_compression_advanced` は、存在する role 別設定を圧縮方針へ反映する。既定は従来通り `Face/Eyes=source`、`Normal/Occlusion=gpu_native`、`Emissive=high_quality`、`Clothing/GenericColor=auto`、`Data=source`。`clothing = "high_quality"` / `generic_color = "high_quality"` は明示 opt-in として BC7 sRGB path へ入り、既定の見た目・初回cache生成コストは変えない。
   - `mizuki-split / field_drape` で `clothing = "high_quality"` と `generic_color = "high_quality"` を実験すると、初回は compressed cache 生成で `texture prepare total=13.9s` と重い。一方 hot cache では `texture prepare total=1019.7ms -> 793.4ms`、`processed_cache_read_mb=2417.7 -> 1239.8`、`GenericColor read=985.9MB -> 0.0MB`、`Clothing read=192.0MB -> 0.0MB`。通常既定化ではなく、ユーザー明示の高品質/省VRAM profile または cache prewarm と組にする候補として扱う。
   - Supervisor の render quality `High` recommendation は `clothing = "high_quality"` / `generic_color = "high_quality"` を設定する。`Light` / `Balanced` recommendation は advanced texture compression を既定へ戻すため、profile preset の往復で latent high-quality cache cost が残らない。`data = "high_quality"` は見た目確認前なので developer / manifest opt-in のままにする。
   - Profile stage の quality summary は texture limit だけでなく compression / cache / BC7 opt-in も表示する。High preset の hidden `texture_compression_advanced` が `Balanced + BC7 color / cache on` として見えるため、キャッシュ準備の必要性をユーザーが追いやすい。
   - 同じ条件で `data = "high_quality"` も加えると、初回は Data の compressed cache 生成でさらに `texture prepare total=9.2s` の miss run になるが、全 cache hot 後は `texture prepare total=364.3ms`、`processed_cache_read_mb=0.0`、`compressed_cache=59/0/0`。高速化余地は大きいが、Data mask の見た目確認と明示的な cache prewarm UI なしに既定化しない。
   - 2026-06-13 に processed / compressed texture cache reader の buffer を 1MiB に拡大。`mizuki-split / field_drape` hot cache bench では `texture prepare total=2037.8ms -> 1240.1ms`、`cache_read=1660.7ms -> 966.4ms`。読み込む cache bytes は変えず、OS read 粒度だけを改善する。
   - 2026-06-13 に lilToon-like material の初期 resident texture selection を feature toggle で絞った。本家 lilToon は `_UseMain2ndTex` / `_UseShadow` / `_UseMatCap` / `_UseEmission` / `_UseOutline` などの無効時に該当 block を実行しないため、UNToon 側も無効 feature の texture slot を起動時 resident へ入れない。`mizuki-split / field_drape` hot cache bench では `texture prepare total=1240.1ms -> 1188.8ms`、`cache_read=966.4ms -> 923.1ms`、`processed_cache_read_mb=2503.0 -> 2417.7`。
   - 同じ方針で lilToon reflection cube も `_UseReflection && _ApplyReflection`、gem profile は gem source cube として resident 判定する。`cube_override_factor` / cube texture slot だけでは通常 lilToon の cube resident へ入れない。`mizuki-split / field_drape` hot cache bench では `texture prepare total=1188.8ms -> 1030.0ms`、`cube=29.2ms -> 14.0ms`、`cache_read=923.1ms -> 779.5ms`。placeholder から本物の cubemap を即時生成する lazy-cube 試行は `cube=5490.8ms` まで悪化したため採用しない。
   - shader feature planning も lilToon `_Use*` toggle を正本にし、texture slot が存在するだけでは matcap / reflection / anisotropy / rim / emission / backlight / glitter / second normal variant feature を立てない。`mizuki-split / field_drape` では draw variant 数は 4 のままだが、shader module creation は `81.7ms -> 75.1ms`、mesh prepare は `177.0ms -> 160.7ms`。
   - lilToon parallax も `_UseParallax` を正本にし、`_ParallaxMap` slot が存在するだけでは resident texture / shader feature / draw uniform を有効化しない。`mizuki-split / field_drape` hot cache bench では既に resident 集合が絞られていたため追加の texture 削減は見えず、`texture prepare total=1139.7ms`、`processed_cache_read_mb=2417.7`。これは速度改善値ではなく、slot presence を runtime feature と誤認しないための correctness 固定として扱う。
   - `_MainColorAdjustMask` は `_Use*` toggle ではなく HSV/Gamma / gradation 補正の mask なので、mask slot だけでは resident texture / shader feature を立てない。一方、`_MainTexHSVG` が既定 `[0,1,1,1]` から外れる場合や `_UseMainGradationTex` が有効な場合は mask がなくても main color adjustment variant を立てる。`mizuki-split / field_drape` では resident 数は変わらず `texture prepare total=1216.3ms` で、これも速度改善値ではなく見た目欠落防止の correctness 固定として扱う。
   - IDMask は本家 lilToon のコメント通り `_IDMaskCompile` が compile 用明示フラグだが、互換性のため `_IDMask1..8` / `_IDMaskPrior1..8` / `_IDMaskControlsDissolve` の非ゼロ値でも IDMask feature を有効化する。UNToon の shader feature planning と draw uniform は同じ runtime-controls predicate を使い、uniform だけ有効で shader variant が落ちる状態を作らない。
   - UDIM Discard は `_UDIMDiscardCompile` に加え、row mask (`_UDIMDiscardRow*_*`) が非ゼロなら feature を有効化する。本家 `lilUDIMDiscard` の実処理は row mask を見て discard を決めるため、UNToon の shader feature planning と draw uniform は同じ row predicate を使い、row だけ設定された material で variant が落ちる状態を作らない。
   - Supervisor の `.unavatar` wardrobe option 取得は root `UN_avatar.wardrobe` だけを読むため、GLB 全体を `fs::read` せず JSON chunk だけを読む。thumbnail / technical stats のように BIN 参照が必要な metadata path とは分ける。
4. Runtime CPU
   - Renderer bench (`--bench-frames`) と `UN_AVATAR_IMPORT_PROFILE` 指定時は通常 renderer 起動でも profiled import を使い、loading 停止の内訳をログで観測できるようにする。`mizuki-split / field_drape` では import total 約 `935.6ms`、主な内訳は `file_read_ms=175`、`image_source_metadata_ms=179`、`gltf_import_slice_ms=100`、`read_meshes_ms=228`、`append_texture_assets_ms=41`、wardrobe apply `27.5ms`。
   - `image_source_metadata.detail` profile は profiled import 時だけ有効にし、通常起動 path へ per-image atomic timing を入れない。`mizuki-split / field_drape` では metadata 停止の主因が encoded image dimensions ではなく texture cache key 用 source hash だった。source hash を byte-by-byte FNV から `DefaultHasher::write` へ変更し、`image_source_metadata_ms=152ms -> 27-28ms`、thread 合算 `hash_ms=472.7ms -> 82.6-84.7ms`。source hash 変更で既存 processed / compressed texture cache は一度 miss するが、2 回目 hot cache では `texture prepare total=1019.7ms`、`processed_cache=38/0/0`、`compressed_cache=21/0/0` へ戻る。
   - GLB path import の deferred image source は encoded bytes を全件 `Vec` コピーせず、container file path + absolute byte range を runtime metadata として保持する。`mizuki-split / field_drape` では `retained_deferred_encoded_image_count=228` から `file_backed_deferred_encoded_image_count=228` へ移行。hot cache texture prepare は `rgba=0ms` を維持するため起動時間短縮ではなく、wardrobe hot switch 用 lazy decode を残したまま RAM 常駐量を削る変更として扱う。
   - world matrix rebuild、UNPhysics / UNDynamics step、wardrobe residency refresh、fur card encode など、毎 frame 全体走査になっている箇所を dirty / active scope へ寄せる。
   - Runtime CPU の判断では swapchain / vsync 待ちを実処理 CPU と混ぜない。Renderer title / runtime status の `cpu_ms` は `cpu_record_ms - frame_surface_acquire_ms` の busy estimate とし、surface wait は別 field / title の `wait` として見る。
   - 定常 frame の標準 probe は `cargo xtask run-renderer --release --profile mizuki-split --wardrobe-set field_drape -- --bench-frames 180 --no-fps-title`。2026-06-13 時点の Vulkan/MSAA/balanced/profile 解像度では `fps_avg=60.0`、`cpu_no_surface_avg=1.4-1.5ms`、`surface=14.1-14.3ms`、`gpu_avg<1ms`。この条件で `cpu_record_avg` が約 16ms に見えるのはほぼ surface acquire wait であり、CPU 最適化対象ではない。
   - `mizuki-split / field_drape` の UNPhysics/UNDynamics は `active_groups=131`, `active_joints=537`, `colliders=56`。collider の local/world 解決を joint ごとではなく fixed dynamics step ごとに一度だけ行うように変更し、`frame_dynamics_ms=0.83/0.74 -> 0.48`、`cpu_no_surface_ms=1.28-1.43 -> 1.01` まで低下。Physics 挙動は同じ world-space collider 判定で維持する。
   - `UN_AVATAR_DYNAMICS_PROFILE=1` で UNPhysics/UNDynamics の fixed-step 内訳を renderer bench detail に出す。`mizuki-split / field_drape` では profile overhead 込みで `frame_dynamics_ms=0.57`、`dyn_world=0.13`、`dyn_colliders=0.00`、`dyn_solve=0.44`、solve 内は `dyn_solve_collision=0.16`、`dyn_solve_propagate=0.04`。次の最適化候補は world collider 解決ではなく、joint solver 本体と per-joint collider push-out。
   - collider push-out は非接触が大半のため、sphere / capsule 判定の距離比較を sqrt 前の二乗比較へ変更。profile overhead 込みでは `dyn_solve_collision=0.16 -> 0.14`、通常 bench は `frame_dynamics_ms=0.48` 維持。小幅だが挙動等価の算術削減として採用する。
   - UNDynamics angle limit は group 単位の不変値なので、`limit_type` の文字列判定と max angle 解決を joint ごとではなく `step_group` ごとに一度だけ行う。`mizuki-split / field_drape` profile bench では `frame_dynamics_ms=0.54 -> 0.50`、`dyn_solve=0.42 -> 0.38`。
   - collider push-out が非接触で元の tail を返した場合は、その後の direction 正規化と length 再投影を省略する。`mizuki-split / field_drape` では profile bench `frame_dynamics_ms=0.50 -> 0.46`、通常 bench `frame_dynamics_ms=0.48 -> 0.43`。
   - 2026-06-13 の `read_meshes` stage profile では `morphs=114ms`、`cache_clone=53ms`、`cache_insert=47ms`。vertex payload cache は無効化で `read_meshes=335ms`、cache min uses 3 で `303ms` に悪化したため、既定の min uses 2 を維持する。morph normal deltas は renderer の static/default morph と dynamic morph payload で使用しているため、品質判断なしに drop しない。
   - `read_primitive` の vertex payload cache 設定は mesh import 単位で一度だけ読む。payload key も呼び出し側で一度作って `vertex_payload_id` 解決と primitive read に共有する。`mizuki-split / field_drape` bench では cache 無効化が `read_meshes=281ms` へ悪化する一方、設定読み取り / key 再生成の削減は `read_meshes=228-232ms -> 222ms` 程度の小幅改善。
   - full Animator graph、dynamic reactive mesh gating、VRC Constraints solver integration は未設計領域として、速度目的のついで実装はしない。
5. Skin tone / optional analysis
   - skin tone matching、diagnostic dump、debug summaries は active / resident texture と明示 option に限定し、通常起動 path へ重い解析を混ぜない。

Outline policy:

- lilToon-compatible authored outline は、本家 lilToon の `_UseOutline` / `FORWARD_OUTLINE` / `_OutlineWidth` / `_OutlineWidthMask` / `_OutlineVectorTex` 系に基づく geometry outline として扱う。
- v1 の `AvatarOutlinePolicy::Override` による画面空間シルエット囲みは lilToon authored outline ではない。これは UN Avatar 独自の post effect / user override として残す場合も、lilToon compatibility path とは分離する。
- PostProcess lazy creation では、authored geometry outline と avatar silhouette post outline を別機能として扱い、post outline が OFF なら avatar outline post pipelines を作らない。

Supervisor look policy:

- v2 Supervisor では material authored outline と区別するため、画面空間の全体囲みは `Silhouette Outline` / `シルエットアウトライン` と呼ぶ。`Outline` 単独表記は material ごとの authored outline と紛らわしいため避ける。
- Authored outline / Rim / MatCap / Specular / AO は UNToon material へ正規化された source-authored parameter として扱う。v1 のように Supervisor profile から全 material へ同じ width / color / rim / matcap / specular / AO 値を押し付ける設定は v2 の標準 UI / Supervisor runtime command / renderer manifest application から廃止する。既存 manifest に残る `[effects.avatar.rim]` / `[effects.avatar.matcap]` / `[effects.avatar.specular]` / `[effects.avatar.ambient_occlusion]` は読み込みを壊さず無視する。
- Profile で直接扱う look controls は、Silhouette Outline、Bloom、SSAO、Contact shadow、color grading、背景などの screen / output / viewer-space effect を中心にする。material の authored value を壊す override は diagnostic / migration 互換に限定し、通常設定へ昇格しない。

Output / preview policy:

- Spout2 output resolution and local Window preview size are separate user concepts. Streamers often want OBS to receive 1920x1080 while the local preview stays small or minimized.
- v2 Supervisor exposes output modes as Window Preview, Spout2 + Preview, and Spout2 Only. `Spout2 Only` initially means Spout2 enabled, output resolution set for OBS, and the local preview window launched minimized; this keeps the current winit/wgpu surface path intact.
- Running renderers expose the same practical shortcut in the Output controls: `Spout2 Only` enables 1080p Spout2 and minimizes the local preview without stopping the renderer. Plain 720p / 1080p buttons only change Spout2 output and do not implicitly restore or minimize the preview.
- True headless output, where renderer rendering no longer depends on a visible/native surface, is a later renderer architecture task. Do not fake it by silently changing Spout resolution or coupling it back to window size.

## UNPhysics / UNDynamics Runtime Normalization

SpringBone / PhysBone は source format ごとの physics component ではなく、UNAvatar の UNPhysics umbrella 下にある UNDynamics runtime model へ正規化してから solver / renderer へ渡す。
正本は [`unphysics-undynamics-v2.md`](unphysics-undynamics-v2.md)。

初期方針:

- VRM SpringBone と VRC PhysBone は source metadata を保持しつつ、実行時には共通の UNDynamics group / chain / collider / parameter / limit / interaction view へ lower する。
- v1 で実装済みの SpringBone solver / collider 実装は solver backend 資産として利用してよい。ただし SpringBone を v2 physics model の基準にはせず、入力は VRM SpringBone 生データではなく、正規化済み UNDynamics runtime state とする。
- VRC PhysBone は v2 初期では完全再現を狙わず、source metadata を保持しながら UNDynamics terms へ lower する。現 solver backend が解けない term は metadata / diagnostics に残し、SpringBone semantics へ丸めたことにしない。
- 正規化境界は現在進めている runtime model view の一部として扱う。形式別の VRM / VRC / Unity component 判定を frame loop や solver 内へ散らさない。
- solver state は source scene を直接 mutate せず、resolved runtime state と pose buffer を入力にする方向へ寄せる。
- Stretch は `maxStretch` を SpringBone 固定長拘束の例外として雑に流し込まない。UNDynamics chain limit と solver writeback policy の問題として扱い、`.unavatar` の `writebackMode` / `writeback_mode` は runtime group へ lower する。現 solver backend は `rotation_translation` かつ安全な next-chain-node target がある joint だけ `max_stretch` upper bound を local translation writeback へ反映し、target の無い 2-node leaf / terminal imaginary tail は metadata / diagnostics に留める。`maxSquish` / `stretchMotion` curve の忠実再現は未対応。

この段階でやること:

- `UnaDocument` / `.unavatar` / VRM source から dynamics source を読み、UNDynamics runtime view の最小形を決める。
- Unity Exporter は現在有効な VRC PhysBone component を `.unavatar` `dynamics[]` へ source payload として出力し、Runtime importer が UNDynamics group / chain / collider / limit / interaction terms へ lower する。
- 現在対応済み: VRC PhysBone `rootTransform` / `ignoreTransforms` / `multiChildType=Ignore` / `endpointPosition` / `enabled` / `radius` / `pull` / `spring` / `stiffness` / `gravity` / `allowCollision=false` / stable source id / limit metadata / interaction metadata / interaction `parameter` の最小抽出と lower、leaf root `endpointPosition` の synthetic endpoint child 化、`sourceParams.ignoreTransforms` で全 child が ignored になる root の synthetic endpoint child 化、non-leaf endpoint warning、PhysBone interaction suffix runtime parameter definition 宣言、PhysBone radius / force / angle / stretch 系 AnimationCurve metadata の sourceParams 保存と CLI source count diagnostics、source collider metadata 保存、sphere / capsule / insideBounds collider の初期 solver / debug draw 接続、collider position / rotation / radius / height / insideBounds / shapeType の source-neutral 接続、Unity enum serialized numeric `shapeType` の Sphere / Capsule import、VRC Contact Sender / Receiver metadata export / import と source id 一意化、current runtime scene pose contact probe、VRC Constraints reference metadata 保存、angle limit の現 solver backend で扱える拘束への近似反映、branch root の複数 group 化、wardrobe `dynamicsEnable` による runtime group enable override、safe next-chain-node target への `max_stretch` translation writeback、UNMotion `signals` から宣言済み action / ModularAvatarParameters runtime parameter への Bool / Scalar 入力、CLI diagnostics、Supervisor profile `[physics.dynamics] enable_all_on_launch = true` による明示的な起動時 all dynamics runtime override。VRC PhysBone は source metadata / action target として保持し、authored default は source の `enabled` を尊重する。CLI diagnose / renderer runtime status / Supervisor diagnostics は groups が存在しても effective enabled group が 0 の状態を warning として出す。
- 残り: direct grabbing / posing evaluator、VRC Constraints solver integration、full Animator graph style evaluation。grabbing / posing action hook 用の source_id / root_path / base parameter / suffix parameter 候補は CLI diagnose / renderer runtime status に公開済み。endpointPosition は leaf root と、`sourceParams.ignoreTransforms` で non-ignored child が無くなる root の synthetic endpoint child 化まで固定済み。per-chain radius curve は base radius 倍率を chain tail ごとの `hit_radius_samples` として solver collider constraint へ近似反映する初期実装まで完了。stretch は `rotation_translation` かつ safe next-chain-node target がある group だけ `max_stretch` upper bound を反映し、targetless group は metadata / diagnostics に留める。これを v2 初回リリースの主要未完了領域として扱う。
- Wardrobe / action / animation が dynamics enabled state を切り替えられるよう、source data と runtime state の所有関係を明記する。
- PhysBone behavior の詳細再現は現在の主作業に移す。Wardrobe / Menu の UI polish、tray / global shortcut、broader eviction policy は UNDynamics の期待動作確認後に戻る。

## Wardrobe Hot Switch Target

リファクタリング後の最初の機能ターゲットは、renderer を再起動せずに `wardrobe_set` を切り替えること。

前提として、startup import 時点の `.unavatar` skinning / morph は共通 `UnaSceneSnapshot` 経由で GPU pipeline に接続済みとする。Wardrobe の `blendShapeWeight` operation は scene primitive の default morph weights を変えるため、hot switch は document revision を進め、draw 側の default morph weights を再読込し、既存 uploaded morph weights を invalidation する。通常フレームでは scene default morph の再走査を行わない。

初期 behavior:

- 選択された wardrobe operations を runtime resolved state へ適用する。
- process reload なしで visible draw set、関連 morph weights、wardrobe operation 由来の per-material overrides を更新する。これは deprecated profile-wide material override とは別物として扱う。
- 可能な範囲で upload 済み asset を再利用する。
- runtime status に active set を出す。
- hot switch path が成熟するまでは、現在の startup path を fallback として残す。

MVP control command:

```json
{"command":"set_wardrobe","set_id":"field_drape"}
```

この command は既に attach 済みの document を base wardrobe state へ戻してから対象 `.unavatar` wardrobe operation を適用し、document revision を進める。対象 set だけを現在状態へ重ねると、前回 set の visibility / morph default が累積してしまうため禁止。base wardrobe state は裸の素体ではなく、reset / fallback 時にも表示してよい安全な初期表示状態として扱う。draw transform / visibility / morph default は次の frame update で反映する。成功時は runtime status の `active_wardrobe_set` を更新する。初期実装では新規 GPU resource が必要な material / mesh を lazy upload せず、startup 時に読み込まれた resource set の範囲で切り替える。

現在対応済み:

- `set_wardrobe` runtime control command は正規化済み set id を受け、適用失敗理由を control response に返す。
- renderer は wardrobe 適用後に document revision を進め、draw transform / visibility / scene morph default / runtime requirements を次 frame で再読込する。Wardrobe hot switch は base reset と複数 operation を含むため、切替単位で dynamics nodes を rest pose へ戻し simulator / collider state を再構築する。
- renderer は wardrobe / runtime action の material slot 差し替え後、draw material uniform だけでなく material / outline material bind group も再生成し、startup 時に upload 済みの texture / sampler / cube map resource へ texture slot を再束縛する。
- `UnaRuntimeState.active_wardrobe_set` と `active_asset_groups` は wardrobe 適用成功時の resolved runtime state として更新され、`UnaRuntimeState.last_action_id` と `parameter_values` は runtime action 成功時だけ更新される。`UnaRuntimeState.dynamics_enabled_overrides` は wardrobe / runtime action の dynamics enable state を保持する。runtime status は document state から `active_wardrobe_set` / `active_asset_groups` / `last_action_id` / `runtime_parameter_values` を公開し、dynamics は effective enabled count、source authored enabled count、runtime override count を分けて出す。group bounded list も `authored_enabled` / `effective_enabled` / `runtime_enabled_override` を分けて出す。
- `dynamicsEnable` は `UnaRuntimeDynamicsMut` 経由で runtime dynamics enable override を切り替え、source group の authored default を直接変更しない。renderer は enable state 変更時に対象 source group の dynamic nodes と関連 constraint ref nodes だけを rest pose へ戻し、現在の dynamics / collider / physics 設定で simulator を再構築する。global dynamics reconfigure と QA 用の all dynamics runtime override は全 dynamic / constraint ref nodes を reset する。適用件数と missing dynamics id は renderer log で観測できる。Supervisor diagnostics は renderer `set_dynamics_enabled` command を使い、bounded group list の `source_id` を個別に enable / disable でき、`set_all_dynamics_enabled` command で全 runtime dynamics group の override を一括 enable / disable できる。Supervisor profile の `[physics.dynamics] enable_all_on_launch = true` は renderer 起動後に同じ all dynamics override を一度送る明示 opt-in であり、OFF から ON へ設定変更した時は起動済み renderer にも反映する。OFF へ戻す操作は次回起動設定だけを変える。

後回し:

- `Ambiguous group` の自動推論は `wardrobe` set の `assetGroupOwnershipHints`（`path` / `groupId`）で明示指定を受け付ける。これにより `wardrobe.assetGroupOwnership` は曖昧候補を誤る前提を避けつつ補助可能になる。なお broader eviction policy は未着手。
- richer Supervisor wardrobe menu hierarchy / search、renderer tray icon / global shortcut からの wardrobe / menu access、full external menu UI parity は v2 初回の physics blocker 解消後に回す。現時点の runtime action / menu candidate / renderer status は初期 QA に十分な基盤とする。
- crossfade、dissolve、sparkle などのお着替え effect。
- set ごとの physics reset / blend。
- user-facing ring-menu UI。

## VRC Action Model Target

最初から VRC Animator Controller の完全 clone を作らない。

最初の model は、複数入力から叩ける action を表す。

- Expression Menu item
- keyboard shortcut / Function key
- Supervisor control
- 将来の ring-menu UI
- 将来の animation event / parameter change

初期 action effects:

- node / subtree visibility
- wardrobe set selection
- expression / morph weight
- material color、emission、scalar override
- dynamics enable / disable marker

この action model の上に、後から VRC Expression Menu の Toggle、Button、SubMenu、simple Puppet controls を載せる。

現在対応済み:

- `UnaRuntimeActionSet` / `UnaRuntimeAction` / trigger / effect schema を core に追加した。
- `.unavatar` wardrobe sets は、base set を除き `WardrobeSet` effect を持つ runtime action candidate へ import される。
- `.unavatar` variants のうち ObjectToggle / active-state 由来の node visibility operations は `NodeVisibility` effect を持つ runtime action candidate へ import される。metadata だけの MenuItem は effect を確定できないため source payload に残す。
- `.unavatar` variants の material color / scalar / slot、expression weight、dynamics enable operations は `MaterialColor` / `MaterialScalar` / `MaterialSlot` / `ExpressionWeight` / `DynamicsEnabled` effect を持つ runtime action candidate へ import される。
- CLI diagnose は runtime action 件数、trigger 件数、effect 件数、trigger / effect kind 内訳、action id / label を観測できる。
- renderer runtime control は `activate_action` を受け、`action_id`、`supervisor_command`、`expression_menu_path`、または `parameter_name` + `parameter_value` で action を解決する。`set_parameter` は action の有無に関係なく runtime parameter state を更新し、matching `ParameterValue` action がある場合は同じ effect 適用経路を使う。`WardrobeSet` effect は既存 hot switch 経路、`DynamicsEnabled` effect は runtime dynamics mutation 経路、`ExpressionWeight` effect は既存 expression override 経路、`NodeVisibility` effect は runtime scene visibility mutation 経路で適用する。`MaterialColor` / `MaterialScalar` effect は PBR 共通値の初期範囲として base color、emissive、alpha、metallic、roughness/smoothness、alpha cutoff を runtime material mutation 経路で適用し、`MaterialSlot` effect は runtime mesh primitive の material slot を差し替えて draw material uniform を再同期する。
- effect 付き `.unavatar` variant に MenuItem / Expression Menu metadata operation が同居する場合は、runtime action の `ExpressionMenu` trigger path へ metadata path を取り込む。metadata-only MenuItem は引き続き action 化しない。
- runtime evaluation の正本は [`unevaluation-v2.md`](unevaluation-v2.md)。内部 module 名は `runtime_eval` とし、wardrobe / action / animation / parameter / contact の合成は target owner policy で扱う。v2 初期では priority / lock は導入せず、target type ごとの policy で explicit user action と continuous evaluator の衝突を解決する。
- core は runtime action effect から owner key / target kind / target key を派生する read-only evaluation target write view と、同一 target kind/key に複数 action owner が書く collision diagnostics を持ち、CLI diagnose / renderer runtime status / Supervisor diagnostics で観測できる。これは inactive-state default restore、continuous evaluator、衝突診断の前提であり、source data や runtime scene を直接 mutate しない。
- core runtime model は node visibility、material property、material slot、dynamics enabled の現在値を read-only に取得できる。
- core は runtime action restore readiness diagnostics を持つ。restore target は baseline 未保存なら `baseline_not_captured`、保存済みなら `ready=true` として観測できる。
- restore readiness から read-only restore baseline candidates も診断できる。これは capture 候補値の確認用。
- core は restore baseline candidates から deterministic capture plan を作れる。capture plan は `UnaRuntimeState.restore_baselines` へ owner-keyed runtime state として保存でき、保存済み baseline がある action effect は restore readiness で `ready=true` として観測できる。renderer は runtime action activation の effect 適用前に baseline を capture し、既存 baseline は上書きしない。core は inactive action の restore apply plan を出せる。renderer は activation 後に inactive action restore を node visibility、material color/scalar、material slot、dynamics enabled へ適用し、dynamics enabled restore が含まれる場合は対象 source group の dynamic nodes と関連 constraint ref nodes だけを rest pose へ戻して simulator / collider state を再構築する。

次の段階:

- VRC Expression Menu metadata から action label/path をより正確に取り込む。Modular Avatar MenuItem metadata は effect source が確定できるものから順次 action 化する。CLI diagnose は action kind count に加え、NodeVisibility / MaterialSlot action の主要 target を表示できる。Menu Item / Menu Group / Menu Installer / Menu Install Target は metadata component として分類し、保存済み label / control / parameter / target / install target を diagnose で観測できる。
- Modular Avatar Object Toggle は structured component payload から `NodeVisibility` action へ import される。Material Setter の direct renderer slot payload は scene-aware renderer reference resolver を通して `MaterialSlot` action へ import され、Material Swap の scene-aware From / To slot expansion も null material slot を含めて `MaterialSlot` action へ import される。component / fields / menuItem に明示された Expression Menu path metadata は `ExpressionMenu` trigger へ取り込み、明示 MenuItem control parameter/value metadata は `ParameterValue` trigger として保持する。MenuItem `subParameters` は puppet 系 control metadata として runtime action condition / CLI diagnose に保持するが、値付き trigger にはしない。Action label は component name / displayName に加え、MenuItem name / displayName / label と Control name を fallback として取り込む。MenuItem の Expression Menu path は明示 `menuPath` / `expressionMenuPath` / `path` payload がある場合だけ取り込み、階層を推測して合成しない。QuickSwapMode は本家 Inspector の `To` material 候補選択補助であり runtime reaction 登録には使われないため、runtime emulation 対象外とする。Generic wardrobe material color / scalar / slot operations も hot switch で適用され、CLI diagnose / wardrobe probe で material apply counts を観測できる。runtime action trigger 評価は core query helper に統合済み。CLI diagnose は MenuItem parameter/value から effect-backed runtime action への対応と `WardrobeSet` ids を `menu_action_candidates` / `menu_wardrobe_candidates` として公開し、nested menu path も保持する。Renderer runtime status は `WardrobeSet` effect を持つ action を `wardrobe_actions` として公開し、action id、label、set id、Expression Menu path、supervisor command、parameter trigger を UI が消費できる形にする。Supervisor controls は `menu_wardrobe_candidates` を wardrobe menu buttons として表示し、candidate の `action_id` で renderer `activate_action` を呼べる。候補上限を超える場合は UI に総件数と省略件数を表示する。CLI diagnose / renderer status は menu graph path walk が循環または invalid parent index で止まった候補に `menu_path_truncated` を出す。Unity Exporter は参照された `VRCExpressionsMenu` asset の path / guid と bounded control metadata を保存し、CLI diagnose / renderer status は external asset controls に synthetic `menu_key` を与え、parameter/value が一致する runtime action / wardrobe candidate へ展開できる。CLI diagnose は wardrobe asset group summary と missing group warning を出す。Renderer status は wardrobe asset upload plan を公開し、ownership metadata count、active groups の scoped resident count、renderer draw residency count / mesh buffer byte residency、image texture slot residency count、inactive image slot を参照する draw count、active draw が参照する inactive image/material slot count と bounded slot index preview、material slot residency count、pending scoped texture/material upload work count、直近の mesh buffer scoped load / unload count、image/cubemap texture scoped load / unload count を出す。残りは richer UI consumption と broader eviction policy。
- Runtime action は trigger/effect とは別に source component id、MenuItem parameter/value、`Inverted`、解決できた component source node と active parent nodes を condition metadata として保持する。`set_parameter` は condition metadata を trigger より優先して action を選び、parameter/value と `Inverted` の組み合わせを本家 ReactiveObject の 0.005 幅に合わせて判定する。親ノード付き reactive action は current runtime scene の source node と parent chain inherited visibility を gate として評価する。CLI diagnose、Renderer runtime status、Supervisor diagnostics は current runtime parameter に対する action condition state を diagnostics として公開し、action effect target summary も node visibility / material property / material slot / expression weight / dynamics enabled ごとに観測できる。runtime parameter definitions は action trigger / condition、contact receiver、PhysBone interaction suffix、runtime state、ModularAvatarParameters metadata から source-neutral に作成され、contact transient や PhysBone interaction suffix と action/menu parameter の同名共有、ModularAvatarParameters の type/default-value 衝突は conflict diagnostics として観測できる。PhysBone interaction suffix はまだ value emission しない。ModularAvatarParameters default は renderer attach 時に missing runtime parameter initial value として適用する。inactive-state default restore は baseline capture / apply plan / renderer activation 後 restore まで実装済み。`set_parameter` は condition metadata で active になった action を deterministic order で全件適用し、該当 action が無い parameter change でも inactive restore を走らせる。Renderer は runtime parameter snapshot が変わった時だけ既存 runtime action model を継続評価し、default / contact emission / UNMotion signal 由来の parameter 変化も action に反映する。UNMotion signal は宣言済み action / ModularAvatarParameters parameter への Bool / Scalar 入力に限定し、contact receiver と PhysBone interaction suffix には書かない。現時点では animator default application と full Animator graph style frame evaluation はまだ未実装。
- Contacts は v2 初期範囲の metadata + diagnostics と parameter declaration を core runtime view / renderer runtime status / CLI diagnose まで接続済み。Diagnostics-only contact probe は core runtime view、CLI diagnose、renderer runtime status の current runtime scene pose probe として追加済み。Renderer では motion retarget / dynamics が scene pose を更新した後の document scene を読む。Sphere / Capsule は exact overlap、Unknown は bounding sphere 近似で扱う。parameter emission は `[physics.contacts] parameter_emission = true` profile flag または `.unavatar` capability による opt-in とし、既定有効にしない。opt-in 時は frame loop で current runtime scene pose probe から runtime parameter state へ 1/0 を書き、同名 parameter は max / OR で merge し、値が変化した parameter は既存 runtime action evaluator へ流す。`.unavatar` 側 opt-in 判定、emitted count、reset-to-zero count は diagnose / renderer status / Supervisor diagnostics で観測でき、CLI diagnose / renderer runtime status / Supervisor diagnostics は would_emit な probe があるのに emission が無効な状態を receiver/source path、sender source、parameter sample 付き warning として出す。残りは full Animator graph style evaluation。
- Mesh Cutter / Shape Changer / VertexFilter component payload は metadata として保持しつつ MeshCutter / ShapeChanger を resolver-capable に分類する。CLI diagnose は target、combine mode、blendshape / mask / bone / axis filter summary を観測できる。Runtime resolver は blendshape-based delete filters を、MA と同じく morph delta threshold で頂点選択し、axis delete filters を `dot(axis, vertex-center) > 0`、skinned renderer axis delete filters を rest-pose baked vertex position、bone delete filters を skin joint weight の正規化比率で頂点選択し、mask delete filters を `maskTextureAssetId` で root `textureAssets` から復元した image / sampler と UV で頂点選択し、該当頂点を含む三角形を削除する。active approximate component は `ImportReport.approximations` と CLI warning で静的 subset 適用であることを観測できる。動的 reactive gating はまだ未実装。
- Resolver cache key の mesh render identity は vertex/index/morph/default weight payload hash も含むため、MeshCutter / ShapeChanger / RemoveVertexColor / skinned axis filter の静的 resolver 結果が同じ buffer 長に収まっても cache invalidation できる。
- Mask filter texture は既存 root `textureAssets` に統合し、MeshCutter / VertexFilter component payload は `maskTextureAssetId`、material index、delete mode だけを持つ。MeshCutter 専用 asset pool は作らない。Exporter は exportable な Mask texture を追加 texture asset として保存し、Importer は PNG/JPEG mask texture asset と Unity wrap metadata を decoded image / sampler として復元して resolver から参照する。Unity `TextureWrapMode.MirrorOnce` は CPU Mask filter sampling では本家 MA と同じ扱いにし、GPU material sampling では clamp fallback とする。
- Shape Changer Set-mode は enabled static payload の default morph weight として resolver で適用し、共有 mesh は clone する。Blendshape Sync は ShapeChanger Set 由来を含む static default-weight propagation と linear origin runtime expression bind を処理する。残りは full Animator graph style evaluation、non-linear runtime curve propagation、full Animator と同じ評価単位が必要な dynamic reactive mesh mutation。
- Modular Avatar / VRC Expression Menu metadata から取り込んだ material parameter 名を、必要に応じて lilToon 専用 parameter へ拡張する。

## PhysBone Placement

PhysBone behavior implementation は runtime state cleanup、UNDynamics normalization、Wardrobe hot switch の後に置いていたが、これらの前提は v2 初回検証に十分な線まで到達した。
以後は UNDynamics behavior を mainline とし、Wardrobe / Menu の残 polish は後段に送る。

理由:

- PhysBone roots、colliders、enabled state は active wardrobe と animation state に依存する。
- scene source data を直接 mutate する solver は、hot switch と相性が悪い。
- 初期実装では VRC PhysBone parameters を UNDynamics runtime terms へ lower する。ただし source data ではなく resolved UNDynamics runtime view を solver backend の入力にする。
- 現在は exporter/importer が PhysBone source を runtime dynamics group / collider data へ lower し、endpointPosition は leaf root と `sourceParams.ignoreTransforms` で non-ignored child が無くなる root の synthetic child として正規化し、normalized collider data も保持する。Modular Avatar PBBlocker は本家と同じく blocker target を親 PhysBone root の ignore set に合成し、Modular Avatar Global Collider は resolved root / radius / height / position / rotation から UNDynamics collider intent へ lower する。Contact Sender / Receiver と VRC Constraints reference metadata は source-neutral UNDynamics metadata として export / import し、CLI diagnose と renderer runtime status で count を観測できる。`allowCollision=false` は source collider を solver へ渡さない。local source collider の runtime node scale は solver / debug draw の両方で反映する。PhysBone collider は position / rotation / radius / height / insideBounds / shapeType を source-neutral `UnaDynamicsCollider` として solver / debug draw へ接続済み。angle limit は runtime dynamics group から現 solver backend で扱える拘束へ近似反映し、CLI diagnose は source limit を angle / stretch に分けて観測できる。radius curve は base radius 倍率を chain tail ごとの `hit_radius_samples` として solver collider constraint へ近似反映する。stretch limit / interaction metadata / interaction parameter は runtime dynamics group に保持し、PhysBone suffix parameters は runtime parameter definition として宣言する。safe next-chain-node target がある `rotation_translation` group は `max_stretch` upper bound を local translation writeback へ反映し、targetless stretch group は metadata / diagnostics に留める。CLI diagnose / renderer runtime status は grabbing / posing action hook 用の source_id / root_path / allow state / suffix parameter 候補も公開するが、`_IsGrabbed` / `_IsPosed` などの PhysBone suffix value emission はまだ反映しない。CLI diagnose / renderer runtime status / Supervisor diagnostics は targetless stretch group、grabbing / posing hook、VRC constraint ref の未反映範囲を warning として出し、CLI diagnose は source radius curve が runtime group へ lower された場合は per-joint hit radius 近似、lower できない場合は metadata-only として出す。VRC PhysBone group は source `enabled` を authored default として保持し、wardrobe `dynamicsEnable` と runtime action `DynamicsEnabled` は runtime state override だけを切り替える。override 変更時は対象 source group の dynamic nodes と関連 constraint reference nodes だけを rest pose へ戻して simulator を再構築し、他 source の現在姿勢には触れない。CLI diagnose と renderer runtime status は effective enabled と source authored enabled を分けて観測でき、renderer status は runtime override count、runtime limit group count、angle / stretch limit group count、grabbing / posing metadata group count、node path 付き dynamics group / collider / contact parameter declaration / contact probe / constraint ref bounded list も公開する。Contacts は UNEvaluation の Phase A-D に従い、v2 初期範囲の parameter declaration、current runtime scene pose probe、Sphere / Capsule exact overlap、opt-in runtime parameter emission を core runtime view / renderer runtime status / CLI diagnose / Supervisor diagnostics まで接続済み。残りは direct grabbing / posing evaluator、VRC Constraints solver integration。

## この段階の非目標

- VRChat client 完全再現。
- FX Layer / Animator Controller 完全互換。
- SpringBone solver と PhysBone solver の二重運用。
- source kind 分岐を frame loop / renderer / solver に散らす実装。
- Poiyomi 互換。
- style-only cleanup のための lilToon-like rendering rewrite。
- instant switching が安定する前の完璧な wardrobe transition effect。
