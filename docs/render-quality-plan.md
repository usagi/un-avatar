# UN Avatar レンダリング品質・AA 方針

この文書は、renderer の画質系オプションと関連する描画順・色空間・テクスチャ処理の実装方針をまとめる。短期MVPの正本は [`runtime-mvp.md`](runtime-mvp.md) のままとし、本書はその後に育てる品質レイヤーの設計メモとする。

## 基本方針

- AA は **OFF / FXAA / SMAA / MSAA** の4段階を目標にする。
- 現状は **OFF / FXAA / SMAA / MSAA** をrenderer option `aa`、CLI `--aa`、profile `render_quality.aa` で選択できる。
- TAA 系は当面扱わない。履歴buffer、motion vector、透過、Spout出力との整合が重く、現在の骨組み安定化の本線から外れるため。
- 画質オプションは配信用途を優先し、軽量・安定・予測可能な順で実装する。
- runtime control / profile では高頻度に触る項目だけを出し、詳細品質設定は profile 側に寄せる。

## AA モード

| Mode | 目的 | 実装メモ | 優先 |
| ------ | ------ | ---------- | ------ |
| OFF | 最低遅延・検証・低負荷 | sample_count=1、post AAなし | P0 |
| FXAA | 軽量な最初のAA | fullscreen post pass。Spout/window共通にかける | P1 |
| SMAA | FXAAより輪郭保持を重視 | edge / blend / neighborhood の3 pass post AA。Spout/window共通にかける | P2 |
| MSAA | geometry edge品質 | 4x color/depth multisample targetへ描画し、windowまたはSpout textureへresolve | P2 |

### AA 実装順

1. `aa = off|fxaa|smaa|msaa` を renderer profile / CLI / runtime status に通す。
2. 最終出力直前の post pass として FXAA / SMAA を実装する。
3. MSAA は 4x offscreen multisample target + resolve として実装する。
4. 今後の磨き込みでは、SMAAのプリセットtexture化、Alpha-to-Coverage、透明描画順、髪material orderとの相性を評価する。

## Alpha-to-Coverage と透明

- Alpha-to-Coverage は MSAA 有効時のみ意味があるため、MSAA 実装と同じ段階で扱う。
- Mask材質や髪の細いalpha cutoutに有効だが、MToonの見た目を壊す場合があるので material policy でON/OFF可能にする。
- Transparent材質は従来通り depth writeなしのblend pass を基本にする。
- 透明描画順は `opaque -> mask -> outline -> transparent` を土台にする。transparent はまず authoring / glTF draw order を保持し、shader 種別で大きく並べ替えない。camera distance sort は VRC / lilToon 系の髪や半透明パーツで authoring order を壊すリスクがあるため、後段で material / mesh 単位の opt-in として評価する。
- 髪マテリアルは特に破綻しやすいため、material name / VRM renderQueue / MToon transparent hints を使った order bucket を別途持つ。
- 透明 PNG などで alpha=0 の texel に黒や白の未使用RGBが残っている場合、bilinear sampling で縁や穴に色漏れが出る。`.unavatar` の原本画像は変更せず、renderer の processed texture cache / GPU upload 用RGBAだけ transparent RGB bleed fill を適用し、近傍の不透明色で透明texelのRGBを補完する。

## Mipmap / Texture LOD / Anisotropic Filtering

- 現状はimport画像にCPU生成mip chainを作り、各levelを `queue.write_texture` する。
- sampler は `min=Linear`, `mag=Linear`, `mipmap=Linear` を基本にする。
- anisotropic filtering はまず 4x を既定とする。将来、低負荷profileではOFF、品質profileでは8xを候補にする。
- Normal map / specular系は遠景や配信圧縮でちらつきやすいので、LODに応じた抑制を検討する。

## Normal / Specular 抑制

- Normal map強度、MToon shade/rim/specular相当、PBR roughness/specularは配信時に過剰なノイズ源になり得る。
- 品質profileとして `normal_strength`, `specular_strength`, `rim_strength` を持つ。
- mip levelや画面上のroughness/normal頻度に応じた自動抑制は後段とし、まずはmaterial policyの係数として実装する。

## Outline 解像度とポスト処理

- MToon outlineはgeometry outlineを基本に維持する。
- MSAAなしではoutline edgeが目立つため、FXAA/SMAAの主要評価対象にする。
- outline専用の低解像度passは当面避ける。輪郭幅・透明背景・Spout出力の整合が崩れやすい。
- 将来的にoutlineだけ別解像度にする場合は、深度/法線ベースのpost outlineとして独立評価する。

## Tonemap 後シャープネス

- シャープネスは tonemap / color grading / AA 後の最終LDRで行う。
- FXAA後の過剰sharpは輪郭ノイズを戻すため、`off|low|medium` 程度の控えめな段階にする。
- 配信圧縮を考えると、既定はOFFまたはLOWにする。

## sRGB / Linear 一貫性

- base color / emissive 等の色テクスチャは sRGB texture として扱う。
- normal / roughness / metallic / mask / data texture は linear として扱う必要がある。
- 現状は多くを `Rgba8UnormSrgb` に寄せているため、material texture slot ごとのformat選択が必要。
- shader内部は linear でlightingし、最後にsurface format / post pipelineで sRGB 出力へ揃える。
- Spout出力は受け側の期待が揺れるため、送信formatと色空間のdiagnostics表示を持つ。

## 実装優先順位

1. [x] mipmap生成 + trilinear sampling。
2. [x] 異方性フィルタリング。
3. [x] AA: OFF / FXAA / SMAA / MSAA。TAA系は当面対象外。
4. [x] GPU morph。morph deltaはGPU-resident buffer化し、frame更新はweight buffer書き込みへ寄せる。
5. [x] GPU skinningの次段最適化。skin単位palette共有、compact palette buffer、u16/u32 index buffer選択、draw時のframe/skin bind cache、opaque/mask draw grouping、static material / dynamic transform uniform分離まで実装済み。
6. [~] Texture budget / cache / compression。LODは単体アバター用途では当面不要。テクスチャ解像度制限は劣化リスクがあるためmanifest既定OFF、明示時のみ `off` / `auto` / `8k` / `4k` / `2k` / `1k` から選ぶ。processed texture cacheは既定ONで、resize/mipmap済みRGBAをディスクキャッシュする。texture compressionはmanifest上で `source` / `balanced` / `memory` / `compat` を選ぶ。既定は `balanced`。旧 `auto` / `advanced` は移行互換 alias として `balanced` に読む。Supervisor Avatar Settingsからはこの3項目をlaunch-time texture policyとして編集できる。

### Texture Compression Policy

- `texture_compression = "source"` は忠実性優先。ソースをRGBAへ展開したあとの画質を維持し、lossy compressionは行わない。
- `texture_compression = "balanced"` は既定。テクスチャの使われどころと実行時GPU featureから保守的に選ぶ。WindowsではBCnを第一候補にする。現在は、BC対応GPUで服/generic系の不透明色テクスチャをBC1 sRGBへ圧縮し、normal mapをBC5 linearへ圧縮する。emissiveなど `high_quality` 扱いの色テクスチャはBC7 sRGBへ圧縮できる。顔・瞳・data・非対象画像は既定でsource/nativeまたはRGBAへfallbackする。Data texture は `[render_quality.texture_compression_advanced] data = "high_quality"` などの明示指定時だけ BC7 linear (`Bc7RgbaUnorm`) を使える。`clothing = "high_quality"` / `generic_color = "high_quality"` は明示指定時だけ BC7 sRGB を使い、既定 `auto` では従来の BC1 / source fallback を維持する。
- `texture_compression = "memory"` は、容量とGPU memoryを優先して `balanced` より圧縮寄りに選ぶ。
- `texture_compression = "compat"` は、BCnなどGPU固有圧縮を避け、広く扱えるupload形式へ寄せる。
- UASTC / ETC1S はGPU upload形式そのものではなく、KTX2 / BasisU系のcache/intermediateとして扱う。現段階ではBC1 / BC5 / BC7の圧縮済みblock mip chainをcache artifactとして保存し、後続で同じartifact層へKTX2 / BasisU containerを追加する。UASTCは顔・瞳・emissiveなど高品質寄り、ETC1Sは小容量寄りの選択肢にする。
- ASTC / ETC2 はGPU featureを検出してruntime summaryに出す。現段階ではCPU encoderをまだ持たないためRGBA fallbackし、BC非対応環境でKTX2 / BasisU transcodeを入れた時点でASTC / ETC2 upload候補へ昇格する。
- 顔・瞳・UI的に見られやすい色テクスチャは `source` または `high_quality` を既定寄りにする。normal / occlusion / data系は、sRGBではなくlinear/data扱いを守ったうえでBC5 / BC4 / BC1系などruntime-native候補へ寄せる。
- BCn圧縮済みmip chainの幅・高さは、元画像の論理寸法ではなく4x4 block境界へ切り上げたupload寸法として扱う。非4倍数の画像で論理寸法へ戻すと、DX12/Vulkanの圧縮テクスチャ作成またはuploadが停止するリグレッションになる。

関連項目として、Alpha-to-Coverage、透明描画順、髪material order、透明ソート、normal/specular/rim抑制、outline解像度、tonemap後sharpness、sRGB / linear一貫性は、上記 1〜5 の各段階で破綻しないように扱う。

## 完了条件

- AA mode OFF / FXAA / SMAA / MSAA をprofileで選べる。
- TAAが未実装であることをUI/diagnosticsで明確にできる。
- mipmap + trilinear + anisotropic filtering がtexture import pathで機能する。
- expression morph はCPU頂点再生成ではなく、GPU morph delta + weight buffer更新で動作する。
- GPU skinning palette はdraw単位ではなく、mesh node + skin単位で共有され、bufferは実joint数に応じて確保される。
- index buffer はprimitiveごとにu16/u32を選択し、小さいmeshのindex帯域を抑える。
- mesh draw はrender pass内でframe bind groupとskin palette bind groupを再利用し、opaque/mask/outline drawをskin palette単位で寄せて不要なbindを避ける。BLEND drawは透明順保護のため元順を保つ。
- draw uniform はstatic material uniformとdynamic transform uniformに分離し、VMC/retarget時のdraw更新はmodel行列のみを書き込む。
- texture resolution limit は既定OFF。指定時はロード時にRGBAを上限へ縮小してからmipmapを生成する。`auto` はSpout解像度、なければwindow解像度の長辺から1K/2K/4K/8K tierを選ぶ。
- processed texture cache は既定ON。`UN_AVATAR_TEXTURE_CACHE_DIR`、またはOS標準cache配下に、入力RGBA・寸法・resolution policy・cache versionでkey化したresize/mipmap済みRGBA mip chainを保存する。圧縮ON時はBC1 / BC5 / BC7の圧縮済みblock mip chainも同じcache配下へ保存し、再起動時のCPU圧縮を避ける。CLI `--no-processed-texture-cache` またはmanifest `render_quality.processed_texture_cache = false` で両方を無効化できる。
- processed texture cache のRGBAはアップロード用派生物であり、透明texelのRGB補完など見た目安定化の処理を含めてよい。`.unavatar` 内の source bytes / MIME は optimizer など明示的な変換を除いて dirty にしない。
- skin tone matching は既定OFFの実験機能。`render_quality.skin_tone_matching = true` のとき、ロード時に顔・体のbaseColorテクスチャから肌色クラスタを推定し、CIELAB上で首境界が目立ちにくい顔寄りの目標色へ寄せる。顔/体のサンプル色は、material名で対象primitiveを絞った上でモデル頂点position/UVから顔下端中央と体上端中央のテクスチャ座標を採る。UVサンプルが取れないモデルだけ全体肌色中央値へfallbackする。現段階ではON/OFFのみ。
- texture compression は既定 `balanced`。`source` は忠実性優先、`balanced` はrole別の保守的な自動圧縮、`memory` は容量優先、`compat` はGPU固有圧縮を避ける互換優先。現在はBC1 sRGB、BC5 linear、BC7 sRGB、明示opt-in Data用BC7 linearを実upload形式として使い、非対応GPU・非対象role・顔/瞳/data系既定はRGBA/native pathへ戻す。KTX2/BasisUの実codec/transcodeとASTC/ETC2 uploadは後段。
- BCn圧縮済みcacheはblock整列済みmip寸法を保存し、cache version変更なしに寸法解釈を変えない。
- runtime status はtexture policyとupload summaryを返す。summaryには画像枚数、縮小枚数、cache enabled/hit/miss/write、compressed cache hit/miss/write、compression mode / BC / ASTC / ETC2 support / compressed count / fallback count / compressed bytes、source RGBA bytes、mip込みupload bytes見積もり、source/upload最大長辺を含める。Supervisorはcompression fallbackが発生した場合、runtime noteとDiagnostics findingで圧縮がRGBAへ戻った理由を見えるようにする。
- transparent / hair material の描画順が診断可能で、破綻時にdebug logへ出せる。
- sRGB / linear texture slot の扱いがmaterial policyに明記されている。
- Spout出力とwindow表示でpost process結果が大きく乖離しない。
