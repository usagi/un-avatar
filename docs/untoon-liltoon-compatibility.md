# UNToon v2 lilToon Compatibility Plan

UNToon v2 は lilToon 互換を基準にする。v1 の MToon 互換 shader は参考実装であり、派生元や制約条件ではない。MToon-like な係数へ lilToon 表現を押し込まない。`.unavatar` / lilToon 入力は実装上 `UnaLilToonLikeMaterial` を正本として import し、VRM0/1 や MToon 入力は v2 が安定した後に変換層または別 pipeline として扱う。

## Reference

- lilToon reference: `jp.lilxyzw.liltoon` 2.3.2, MIT License
- Local reference package: `C:/Users/the/AppData/Local/VRChatProjects/mizuki/Packages/jp.lilxyzw.liltoon`
- Primary shader files:
  - `Shader/Includes/lil_common_functions.hlsl`
  - `Shader/Includes/lil_pass_forward_normal.hlsl`
  - `Shader/Includes/lil_common_frag.hlsl`
  - `Shader/Includes/lil_common_frag_alpha.hlsl`
  - `Shader/Includes/lil_common_vert.hlsl`
- Current first-pass source blocks:
  - `lil_common_functions.hlsl`: `lilTooningScale`, `lilBlendColor`
  - `lil_common_frag.hlsl`: `lilGetShading`, `lilGetMatCap`, reflection / rim / emission blocks

WGSL は独立実装とする。ただし、挙動を移植する機能では lilToon の該当 HLSL block を source reference として明示的に読む。

## Material Architecture

- `UnaLilToonLikeMaterial`: v2 開発中の正本。lilToon-compatible な Shadow / MatCap / Reflection / Rim / Outline をここへ増やす。安定後にこれを正式な UNToon material として扱う。
- `UnaMtoonMaterial`: legacy MToon / VRM import 用。v2 lilToon 互換機能をここへ追加しない。
- `UnaMaterialPbr`: glTF/PBR由来の共通 material container。UNPBR の設計正本ではなく、`liltoon_like` / `mtoon` などの model-specific payload をぶら下げる既存構造。
- Renderer は v2 開発中、`material.liltoon_like` があればこれを優先する。v1 互換性は v2 安定後に MToon-like 変換または別 pipeline として扱う。

## Development Rule

画像差分から係数を闇雲に合わせない。次の単位で互換性を確認する。

1. Source properties: Unity material / `.unavatar` が保持すべき property と texture slot
2. Import mapping: `UN_avatar_material` raw params から `UnaLilToonLikeMaterial` runtime parameter への変換
3. Shader behavior: lilToon 本家の計算単位に対応する WGSL 実装
4. Validation sample: Base / original / noble1 / noble13 での見え方確認
5. Known gaps: 未対応 feature と近似理由

`UnaLilToonLikeMaterial` の内部 parameter は、できるだけ単位・次元の整合性を持つ値にする。長さは meters、角度は radians、色は linear RGB、強度や blend weight は原則 `[0..1]` の無次元量として扱う。lilToon の Unity material property は source profile であり、互換 import layer が v2 parameter へ変換する。

## Feature Order

### 1. Main Color / Alpha / Cull / Render Queue

Goal: main texture と base color が正しく、Opaque / Cutout / Transparent の基本が破綻しない状態。

Reference:
- `lil_common_frag_alpha.hlsl`
- `lil_common_frag.hlsl` main texture and premultiply sections

Current status:
- Unity UV は exporter で glTF convention に変換済み。
- Ordinary lilToon Opaque は texture alpha だけで Mask に昇格しない。
- Transparent は lilToon と同じ premultiply + premultiplied blend path へ寄せ始めている。

### 2. Main Shadow

Goal: `_UseShadow`, `_ShadowColor`, `_ShadowBorder`, `_ShadowBlur`, `_ShadowStrength`, shade texture の基本を `UnaLilToonLikeMaterial` に入れる。

Reference:
- `lil_common_frag.hlsl` Shadow block

Notes:
- lilToon は MToon の `shadingShift/shadingToony` と同じ意味ではない。
- lilToon-like v2 では `_ShadowBorder` / `_ShadowBlur` を正本として保持し、MToon 入力は後段でこの表現へ変換する。
- `_Shadow2nd*` / `_Shadow3rd*` は後段。まず 1st shadow を安定させる。

### 3. Normal Map

Goal: normal texture scale と tangent-space normal を安定させ、shadow / rim / matcap の入力を Unity に近づける。

Reference:
- `lil_common_frag.hlsl` Normal blocks
- `lil_common_vert.hlsl` tangent / matcap UV inputs

Notes:
- Normal がずれると MatCap / Rim / Reflection の比較がすべて曖昧になるため、Main Shadow の次に独立確認する。

### 4. MatCap

Goal: `_UseMatCap`, `_MatCapTex`, `_MatCapColor`, `_MatCapMainStrength`, `_MatCapBlend`, `_MatCapBlendMode`, `_MatCapEnableLighting` の 1st MatCap を実装する。

Reference:
- `lil_common_frag.hlsl` `lilGetMatCap`

Notes:
- 現状の UNA は MatCap / Reflection 加算が強く、衣装が銀色に寄りやすい。
- `_MatCapMainStrength = 0` の material は texture があっても寄与しない。
- 2nd MatCap は後段。

### 5. Reflection / Specular

Goal: `_UseReflection`, `_Smoothness`, `_Metallic`, `_Reflectance`, `_ApplySpecular`, `_ApplyReflection`, `_ReflectionBlendMode` の基本を実装する。

Reference:
- `lil_common_frag.hlsl` `lilReflection`

Notes:
- noble13 の pants / sleeves はこの差分が目立つ。
- Cubemap / environment reflection は UNAvatar の環境実装と密接なので、最初は conservative approximation とし、過剰な銀色化を避ける。

### 6. Rim / Rim Shade

Goal: `_UseRim`, `_RimColor`, `_RimMainStrength`, `_RimBorder`, `_RimBlur`, `_RimFresnelPower`, `_RimBlendMode` を実装する。

Reference:
- `lil_common_frag.hlsl` `lilGetRim`
- `lil_common_frag.hlsl` `lilGetRimShade`

Notes:
- v1 の global Rim Override とは分離する。Authored は material ごと、Override は明示的な viewer effect として扱う。

### 7. Outline

Goal: lilToon outline を material feature として再設計する。

Reference:
- lilToon outline pass / outline properties

Notes:
- 現在は比較を乱すため OFF 基準で検証する。
- v1 の outline override は lilToon-like material feature とは別扱いにする。

## Near-Term Target

まず `Main Shadow` までを安定させる。Base / original で肌と髪が過度に白飛びせず、noble1 / noble13 で服の shadow tone が Unity に近づくことを合格条件にする。

## Compatibility Checklist

Status legend:

`[ ]`: not started
`[~]`: partial / approximate
`[x]`: implemented and regression-checked against sample wardrobe states
`[defer]`: intentionally deferred with reason

`[~]` の項目は必ず sub-level に `done` と `remaining` を書く。何ができていて、何が残っているかが曖昧な `[~]` は使わない。

### Material Identity / Render State

- `[~]` lilToon material detection: `sourceShader`, `family`, shader variant names
  - done: Unity Exporter が `sourceShader` / `family` と raw material params を `UN_avatar_material` に保持し、Importer が lilToon family を検出する。
  - remaining: shader variant ごとの feature set (`Lite`, `Cutout`, `Transparent`, `TwoPass`, `Outline`, `Fur`, `Gem`, `Refraction`) を compatibility report で分類する。
- `[~]` alpha mode: Opaque / Cutout / Transparent / Transparent ZWrite
  - done: lilToon shader name、renderQueue、`_ZWrite` から Opaque / Mask / Blend / Transparent ZWrite の基本を推定する。
  - remaining: `_TransparentMode`、two-pass variants、refraction/fur variants、subpass cutoff を本家仕様に沿って分類する。
- `[~]` blend state: lilToon premultiply path for transparent materials
	- done: Transparent 系は shader-side premultiply + premultiplied blend path へ寄せ始めている。
	- done: `_SrcBlend` / `_DstBlend` / `_BlendOp` / `_SrcBlendAlpha` / `_DstBlendAlpha` / `_BlendOpAlpha` / `_SrcBlendAlphaFA` / `_DstBlendAlphaFA` / `_BlendOpAlphaFA` / `_AlphaBoostFA` を lilToon-like material に保持し、transparent 出力 alpha へ `_AlphaBoostFA` を反映する。
	- remaining: RGB / alpha / forward-add alpha blend state の組み合わせを wgpu pipeline blend state へより厳密に反映する。
- `[~]` cull mode: `_Cull`, double-sided handling
  - done: `_Cull` / double-sided state を Runtime cull mode へ反映する。
  - done: sample wardrobe の `_OutlineZTest = Less` に合わせて outline pass depth compare を Less に寄せる。
  - remaining: outline pass cull、per-material outline ZTest、transparent/fur/refraction variant 固有の cull 差分を分離する。
- `[~]` lighting / toon AA controls
  - done: `_LightMinLimit` / `_LightMaxLimit` / `_MonochromeLighting` / `_VertexLightStrength` / `_AAStrength` / `_GSAAStrength` を lilToon-like material に保持する。
  - done: `_AAStrength` を shadow / specular / rim の toon ramp blur へ反映し、light min/max と monochrome lighting を main toon direct light に反映する。
  - remaining: vertex light、GSAA、MatCap/specular/rim 個別 lighting path の min/max 適用を本家に合わせる。
- `[~]` render queue ordering: Unity renderQueue / lilToon queue conventions
  - done: `UN_avatar_material.renderQueue` を lilToon-like material に保持し、Transparent / Transparent ZWrite の draw color path を source renderQueue 昇順で並べる。
  - remaining: Opaque / Mask queue、transparent distance sorting、outline / fur / refraction subpass ordering、stencil / offset との相互作用を Unity/lilToon に合わせる。
- `[~]` stencil / color mask / offset
  - done: Unity Exporter report に non-default stencil / color mask / offset / outline color mask / outline offset を unsupported material render state として集約出力する。
  - remaining: Runtime pipeline の stencil / color mask / depth offset 対応、material sorting との相互作用を設計する。

### Texture Coordinates

- `[x]` Unity mesh UV to glTF convention conversion in Unity Exporter
- `[~]` main texture Tiling / Offset to `uvOffsetScale`
  - done: main texture property の Tiling / Offset を `uvOffsetScale` として保持し、Unity UV から glTF convention へ変換する。
  - remaining: main texture 以外の各 slot ごとの `_ST`、UV mode、UV set selection を保持する。
- `[~]` `KHR_texture_transform` for glTF fallback material
  - done: glTF fallback の baseColorTexture に `KHR_texture_transform` を出す。
  - remaining: baseColor 以外の textureInfo transform と `.unavatar` extension asset 参照時の transform 表現を揃える。
- `[ ]` per-texture UV set selection / UV mode
- `[~]` lilToon `_MainTex_ScrollRotate`
  - done: Unity Exporter が `_MainTex_ScrollRotate` を `uvAnimationScrollX/Y/RotationSpeedFactor` へ正規化し、Importer / Renderer が base UV animation として適用する。
  - remaining: main texture 以外の slot 別 UV mode / UV transform との組み合わせを本家へ合わせる。
- `[~]` UV animation mask
  - done: Exporter / importer / renderer が `uvAnimationMaskTextureIndex` を保持し、animated UV の mask.r として使う。
  - remaining: lilToon slot 別 mask / UV mode と scroll-rotate pivot の本家互換性を検証する。

### Main Color / Base Layer

- `[~]` `_MainTex` / `_BaseMap`
  - done: main texture slot を glTF baseColorTexture と lilToon-like main texture として読み込む。
  - remaining: lilToon layer blending、main texture color adjust、per-slot UV mode を実装する。
- `[~]` `_Color` / `_BaseColor`
  - done: base color factor として保持し、main texture に乗算する。
  - remaining: color space、HDR color、lilToon main color adjustment との順序を本家に合わせる。
- `[ ]` `_MainTexHSVG`
- `[ ]` gradation map
- `[defer]` 2nd / 3rd main texture layers: after first shadow / matcap / reflection are stable

### Alpha / Masks

- `[~]` `_Cutoff`
	- done: Mask material の cutoff として保持し、fragment discard に使う。
	- remaining: alpha mask、dither、Transparent ZWrite color pass との関係を整理する。
- `[~]` Opaque texture-alpha handling: do not infer cutout from ordinary Opaque
  - done: ordinary lilToon Opaque は texture alpha だけで Mask へ昇格しない。
  - remaining: Cutout shader / explicit alpha mode / renderQueue と texture alpha の診断を compatibility report に出す。
- `[~]` `_AlphaMaskMode`
	- done: source raw params を v2 alpha mask mode として保持し、mode 1/2/3/4 を fragment alpha に反映する。
	- remaining: dither、Transparent ZWrite と Unity render queue の組み合わせを sample で検証する。
- `[~]` `_AlphaMask`
	- done: Exporter / importer / v2 material で texture reference を保持し、`mask.r * _AlphaMaskScale + _AlphaMaskValue` を alpha へ適用する。
	- remaining: per-texture UV transform / UV set、mask LOD、alpha-to-mask との関係を本家へ合わせる。
- `[~]` `_SubpassCutoff`
	- done: source raw param を lilToon-like blend state に保持し、Transparent ZWrite depth prepass の discard cutoff として使う。
	- remaining: two-pass / refraction / fur variants の subpass ordering と Unity render queue の組み合わせを検証する。
- `[~]` dither / alpha-to-mask
	- done: `_AlphaToMask` を alpha mode 推定だけでなく lilToon-like blend state に保持する。
	- remaining: wgpu alpha-to-coverage / dither pattern 実装、MSAA 設定、Transparent ZWrite との相互作用を検証する。
- `[defer]` refraction alpha interactions

### Shadow

- `[~]` `_UseShadow`
  - done: `_UseShadow = 0` の material では shade color fallback を寄与させない。
  - done: lilToon source material では lilToon-like v2 shadow branch を使い、MToon shadow transition とは別式にした。
  - remaining: Unity/lilToon との比較で border/blur の係数域を調整し、影マスク・受光・2nd/3rd shadow との合成を詰める。
- `[~]` `_ShadowColor`
  - done: `_ShadowColor` / `_ShadeColor` を 1st shadow color source として保持する。
  - remaining: `shadowColorTex` alpha blend と `_ShadowColorType` LUT path を実装する。
- `[~]` `_ShadowColorTex`
  - done: Unity Exporter が `_ShadowColorTex` を `shadowColorTextureIndex` として明示保存する。Importer は `UnaLilToonLikeMaterial.shadow.color_texture_index` へ読み込み、Renderer は lilToon-like shadow color texture を MToon shade texture より優先して bind する。
  - remaining: `_ShadowColorType == LUT` path、texture alpha blend の本家順序、2nd/3rd shadow color texture を実装する。
- `[~]` `_ShadowStrength`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、lilToon shadow branch の shade/base mix 強度に接続した。
  - remaining: lilToon 本家の direct/indirect light 影響範囲と一致するか、Base / original / noble1 / noble13 で確認する。
- `[~]` `_ShadowBorder`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、0..1 の toon border として保持して shader に渡す。
  - remaining: lilToon 本家の `_ShadowBorder` 係数と同じ見え方になるように、NdotL 変換と範囲を検証する。
- `[~]` `_ShadowBlur`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、border 周辺の linear ramp 幅として shader に渡す。
  - remaining: 本家の blur curve / smoothstep 相当処理との差分を確認し、必要なら関数を差し替える。
- `[~]` `_ShadowBorderRange`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、1st shadow の border 下限拡張として WGSL に接続した。
  - remaining: `_ShadowBorderColor` との gradation mix と AO mask 合成後の扱いを本家 `lilGetShading` に合わせる。
- `[~]` `_ShadowMainStrength`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、lilToon-like shadow branch の `indirectCol = lerp(indirectCol, indirectCol * albedo, value)` 相当へ接続した。
  - remaining: shadow color texture / LUT / 2nd shadow との順序を本家に合わせる。
- `[~]` `_ShadowEnvStrength`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、環境光による shadow 側の albedo 回帰量として WGSL に接続した。
  - remaining: Unity の `fd.indLightColor` 相当を U.N. Avatar の lighting model として定義し、現在の ambient scalar 近似を置き換える。
- `[~]` `_ShadowBorderColor`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、shadow border の direct color gradation mix に接続した。
  - remaining: border mix の入力値と AO mask 適用順を本家 `lilGetShading` と照合する。
- `[~]` `_ShadowStrengthMask`
  - done: Unity Exporter が `_ShadowStrengthMask` を `shadowStrengthMaskTextureIndex` として保存する。Importer は `UnaLilToonLikeMaterial.shadow.strength_mask_texture_index` へ読み込み、Renderer は 1st shadow strength に mask.r を掛ける。
  - remaining: mask LOD、face SDF shadow mode、2nd/3rd channel 利用を本家に合わせる。
- `[~]` `_ShadowBorderMask`
  - done: Unity Exporter が `_ShadowBorderMask` を `shadowBorderMaskTextureIndex` として保存する。Importer は `UnaLilToonLikeMaterial.shadow.border_mask_texture_index` へ読み込み、Renderer は 1st shadow の `NdotL` 相当値に mask.r を掛けて toon border へ渡す。
  - remaining: `_ShadowAOShift`、`_ShadowPostAO`、mask LOD、2nd/3rd channel 利用、本家の `lilTooningNoSaturateScale` path を実装する。
- `[~]` `_ShadowBlurMask`
  - done: Unity Exporter が `_ShadowBlurMask` を `shadowBlurMaskTextureIndex` として保存する。Importer は `UnaLilToonLikeMaterial.shadow.blur_mask_texture_index` へ読み込み、Renderer は 1st shadow blur に mask.r を掛ける。
  - remaining: mask LOD、2nd/3rd channel 利用、本家の `aastrencth` と blur scale の対応を検証する。
- `[~]` `_ShadowReceive`
  - done: source raw params を v2 shadow parameter として保持する。
  - remaining: Unity/lilToon の shadow attenuation 相当入力を UNAvatar lighting に追加して、`lns *= lerp(1.0, calculatedShadow, _ShadowReceive)` へ接続する。
- `[~]` `_ShadowNormalStrength`
  - done: source raw params を保持し、1st shadow の `NdotL` に使う normal を geometry normal と normal-mapped normal の補間に接続する。
  - remaining: 2nd/3rd shadow normal strength、backface behavior、normal map scale との順序を本家へ合わせる。
- `[~]` 2nd / 3rd shadow layers
  - done: 2nd / 3rd shadow color、border、blur、normal strength の raw params を保持し、1st shadow branch へ近似接続した。
  - remaining: 本家 `lilGetShading` の layer ordering、mask texture、feature gate、3rd shadow color alpha semantics を Unity reference で検証する。
- `[~]` `_Shadow2ndColor`
  - done: source raw color params を保持し、alpha を strength として 1st shadow の indirect color を 2nd shadow color へ寄せる近似へ接続した。
  - remaining: `_UseShadow2nd` 相当の feature gate、texture alpha blend、2nd shadow color texture、3rd shadow との本家合成順を検証する。
- `[~]` `_Shadow2ndBorder`
  - done: source raw params を保持し、2nd shadow toon threshold へ接続した。
  - remaining: 本家 `lilGetShading` の 1st / 2nd threshold 関係と mask 合成を照合する。
- `[~]` `_Shadow2ndBlur`
  - done: source raw params を保持し、2nd shadow toon blur へ接続した。
  - remaining: mask、anti-alias strength、3rd shadow との関係を本家へ合わせる。
- `[~]` `_Shadow2ndNormalStrength`
  - done: source raw params を保持し、2nd shadow NdotL 用 normal を geometry normal と normal-mapped normal の補間へ接続した。
  - remaining: 2nd normal map、backface behavior、normal map scale との順序を本家へ合わせる。
- `[~]` `_Shadow2ndReceive`
  - done: source raw params を保持する。
  - remaining: Unity/lilToon の shadow attenuation 相当入力を UNAvatar lighting に追加して 2nd shadow receive へ接続する。
- `[~]` `_Shadow3rdColor`
  - done: source raw color params を保持し、alpha を strength として 2nd shadow 後の indirect color を 3rd shadow color へ寄せる近似へ接続した。
  - remaining: `_UseShadow3rd` 相当の feature gate、texture alpha blend、2nd shadow との順序を本家へ合わせる。
- `[~]` `_Shadow3rdBorder`
  - done: source raw params を保持し、3rd shadow toon threshold へ接続した。
  - remaining: 本家 `lilGetShading` の 2nd / 3rd threshold 関係と mask 合成を照合する。
- `[~]` `_Shadow3rdBlur`
  - done: source raw params を保持し、3rd shadow toon blur へ接続した。
  - remaining: mask、anti-alias strength、3rd shadow color alpha の扱いを本家へ合わせる。
- `[~]` `_Shadow3rdNormalStrength`
  - done: source raw params を保持し、3rd shadow NdotL 用 normal を geometry normal と normal-mapped normal の補間へ接続した。
  - remaining: 2nd normal map、backface behavior、normal map scale との順序を本家へ合わせる。
- `[~]` `_Shadow3rdReceive`
  - done: source raw params を保持する。
  - remaining: Unity/lilToon の shadow attenuation 相当入力を UNAvatar lighting に追加して 3rd shadow receive へ接続する。
- `[defer]` shadow AO masks: after base shadow model is stable

### Normal / Geometry Basis

- `[~]` normal texture slot
  - done: glTF normalTexture / lilToon-like normal slot として読み込む。
  - remaining: lilToon 2nd normal、normal map UV mode、normal strength mask を保持する。
- `[~]` normal scale
  - done: normalTexture scale を shader uniform に渡す。
  - remaining: Unity/lilToon tangent-space parity、green channel convention、backface normal behavior を検証する。
- `[~]` tangent-space parity validation against Unity
  - done: glTF TANGENT を `.unavatar` mesh buffer へ保持し、wgpu vertex layout / WGSL normal map TBN へ接続した。tangent 欠落 mesh は CPU 側で position / uv / normal から tangent を生成し、shader は常に tangent TBN を使う。
  - remaining: Unity/lilToon reference との green channel convention、bitangent sign、backface normal behavior を画像比較で検証する。
- `[~]` 2nd normal map
  - done: Unity Exporter / glTF `UN_avatar_material.mtoon` の `normal2ndTextureIndex` / `normal2ndScaleFactor` を保持し、import 後は `UnaLilToonLikeMaterial.normal` に入れる。texture pipeline では normal map role として扱う。
  - remaining: WGSL で 1st normal と 2nd normal の合成順、UV mode、scale/mask、green channel convention を lilToon 本家へ合わせる。
- `[ ]` anisotropy normal interactions

### MatCap

- `[~]` `_UseMatCap`
  - done: `_UseMatCap = 0` の場合は source color fallback から MatCap 寄与を落とす。
  - done: lilToon source material では `_UseMatCap = 0` を `matcapBlend = 0` として runtime に渡す。
  - remaining: texture slot があるが `_UseMatCap = 0` の material を sample wardrobe で確認し、material feature flag として明示分離する。
- `[~]` `_MatCapTex`
  - done: 1st MatCap texture index / source asset ref を読み込む。
  - remaining: MatCap UV 計算、VR parallax、Z rotation cancel、blend mask を本家仕様へ近づける。
- `[~]` `_MatCapColor`
  - done: `_MatCapColor` を strength と混ぜずに material color として保持する。
  - remaining: HDR color / alpha と `_MatCapColorTex` 相当の拡張が必要か確認する。
- `[~]` `_MatCapMainStrength`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、MatCap color を albedo に寄せる blend 量として WGSL に接続した。
  - remaining: lilToon 本家の lighting/shadow 合成後の albedo 参照位置と一致するか確認する。
- `[~]` `_MatCapBlend`
	- done: glTF `UN_avatar` extras / Unity property から読み取り、1st MatCap の final blend weight として WGSL に接続した。
	- remaining: transparency mask を合成する。
- `[~]` `_MatCapBlendMode`
  - done: Normal / Add / Screen / Multiply の 0..3 を読み取り、lilToon `lilBlendColor` 相当の WGSL branch に接続した。
  - remaining: mode default と unknown value の扱いを本家 material default と照合する。
- `[~]` `_MatCapEnableLighting`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、MatCap color と directional light color の mix 量として WGSL に接続した。
  - remaining: Unity の forward base light color / shadowmix との対応を確認し、必要なら環境光側も含める。
- `[~]` `_MatCapNormalStrength`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、MatCap UV 計算用 normal を geometry normal と normal-mapped normal の補間へ接続した。
  - remaining: lilToon の `fd.matcapN` と完全一致する tangent / view-space 入力、VR parallax、backface behavior を検証する。
- `[~]` `_MatCapShadowMask`
  - done: glTF `UN_avatar` extras / Unity property から読み取り、MatCap blend weight を current shadowmix approximation (`shading`) で抑制する係数へ接続した。
  - remaining: lilToon の `fd.shadowmix`、MatCap mask texture、2nd MatCap との合成順を本家へ合わせる。
- `[~]` `_MatCapLod`
  - done: source raw params を保持し、1st MatCap texture sample の explicit LOD に接続した。
  - remaining: mip availability、texture compression 後の LOD、2nd MatCap LOD との扱いを本家へ合わせる。
- `[~]` `_MatCapBlendMask`
	- done: Exporter / importer / v2 material で texture reference を保持し、MatCap blend factor に mask.r を掛ける。
	- remaining: mask UV mode、VR parallax、2nd MatCap との合成順を本家へ合わせる。
- `[~]` `_MatCapBackfaceMask`
	- done: source raw params を保持し、backface の 1st MatCap blend weight に掛ける。
	- remaining: transparent / outline / cull mode との本家条件を照合する。
- `[~]` 2nd MatCap
	- done: `_UseMatCap2nd`、`_MatCap2ndTex`、`_MatCap2ndColor`、`_MatCap2ndMainStrength`、`_MatCap2ndBlend`、`_MatCap2ndBlendMode`、`_MatCap2ndEnableLighting`、`_MatCap2ndNormalStrength`、`_MatCap2ndLod`、`_MatCap2ndBackfaceMask` を保持し、2nd MatCap contribution へ接続した。
	- done: `_MatCap2ndBlendMask` を保存 / import し、2nd MatCap blend weight に mask.r を掛ける。
  - remaining: VR parallax、Z rotation cancel、1st MatCap との本家合成順、sampler 分離要否を検証する。

### Reflection / Specular

- `[~]` `_UseReflection`
  - done: source raw params と reflection texture asset ref を保持し、lilToon-like shader branch の specular / reflection 有効判定へ接続した。
  - remaining: Unity の environment reflection、roughness mip、normal strength、forward add 条件を本家に合わせる。
- `[~]` `_Smoothness`
  - done: source raw params を保持し、lilToon-like specular power の近似値へ接続した。
  - remaining: GSAA、perceptual roughness、roughness mip selection として本家 `lilReflection` に合わせる。
- `[~]` `_SmoothnessTex`
  - done: Exporter / importer / v2 material で texture reference を保持し、smoothness factor に mask.r を掛ける。
  - remaining: texture UV mode、GSAA、roughness mip selection との順序を本家へ合わせる。
- `[~]` `_Metallic`
  - done: source raw params を保持し、specular color の `lerp(_Reflectance, albedo, metallic)` 相当へ接続した。
  - remaining: base color energy reduction、metallic texture、environment reflection との正確な順序を実装する。
- `[~]` `_MetallicGlossMap`
  - done: Exporter / importer / v2 material で texture reference を保持し、metallic factor に mask.r を掛ける。
  - remaining: smoothness/metallic channel convention、energy reduction、environment reflection との順序を本家へ合わせる。
- `[~]` `_Reflectance`
  - done: source raw params を保持し、specular color と reflection strength の近似値へ接続した。
  - remaining: color space と dielectric specular の本家係数へ合わせる。
- `[~]` `_ApplySpecular`
	- done: source raw params を保持し、lilToon-like specular contribution の gate として接続した。
	- remaining: shadowmix / attenuation との合成順を本家に合わせる。
- `[~]` `_ApplySpecularFA`
	- done: source raw params を forward-add specular gate として保持する。
	- remaining: forward-add lighting path、shadowmix / attenuation との合成順を本家に合わせる。
- `[~]` `_SpecularToon`
  - done: v2 reflection parameter として保持し、enabled 時は specular highlight を toon scale path へ分岐する。
  - remaining: anisotropy path、normal strength、forward-add attenuation と合わせた本家 `lilCalcSpecular` 互換へ近づける。
- `[~]` `_SpecularBorder`
	- done: v2 reflection parameter として保持し、toon specular border に接続する。
	- remaining: roughness 変換を含めて本家係数域を検証する。
- `[~]` `_SpecularBlur`
	- done: v2 reflection parameter として保持し、`_AAStrength` を掛けた toon specular blur に接続する。
	- remaining: 本家の `lilTooningScale` 係数域と一致するか検証する。
- `[~]` `_SpecularNormalStrength`
  - done: source raw params を保持し、specular highlight 用 normal を geometry normal と normal-mapped normal の補間へ接続した。
  - remaining: lilToon の `fd.N` と完全一致する tangent / view-space 入力、GSAA、backface behavior を検証する。
- `[~]` `_ReflectionNormalStrength`
  - done: source raw params を保持し、reflection UV / fresnel 用 normal を geometry normal と normal-mapped normal の補間へ接続した。
  - remaining: lilToon の `fd.reflectionN`、roughness mip、cubemap / equirect policy、backface behavior と合わせて検証する。
- `[~]` `_ApplyReflection`
  - done: source raw params を保持し、reflection texture / environment approximation の gate として接続した。
  - remaining: PMREM / roughness mip / environment source policy を定義して本実装に置き換える。
- `[~]` `_ReflectionColor`
  - done: source raw color params と `_ReflectionColorTex` reference を保持し、specular/reflection color と alpha strength に接続した。
  - remaining: transparent application、color space handling、HDR range を本家に合わせる。
- `[~]` `_ReflectionCubeTex` source asset import
  - done: EXR など glTF core image で扱えない reflection source asset を `UN_avatar.textureAssets` から image index へ解決する。
  - remaining: cubemap / equirect / PMREM / roughness mip としての意味論を決め、2D approximation と区別する。
- `[~]` `_ReflectionCubeEnableLighting`
  - done: source raw params を保持し、environment reflection approximation に main light color mix として接続した。
  - remaining: Unity/lilToon の `fd.lightColor`、cubemap HDR decode、roughness mip、forward-add 条件との一致を検証する。
- `[~]` `_ReflectionCubeColor`
  - done: source raw color params を保持し、environment reflection approximation の cube color factor として接続した。
  - remaining: `_ReflectionColor` / `_ReflectionColorTex` / cubemap HDR decode との本家合成順を検証する。
- `[~]` `_ReflectionCubeOverride`
  - done: source raw params を v2 reflection parameter として保持する。
  - remaining: reflection probe / authored cubemap の source policy を `.unavatar` metadata と renderer pipeline に分離して定義する。
- `[~]` `_ReflectionBlendMode`
  - done: source raw params を保持し、`lilBlendColor` 互換の Normal/Add/Screen/Multiply に接続した。
  - remaining: reflection color texture と environment reflection の本家合成順に合わせる。
- `[~]` environment reflection approximation policy
  - done: 現時点では reflection texture を equirectangular 2D approximation として扱い、`_UseReflection` / `_ApplyReflection` で明示的に gate する。
  - remaining: cubemap / equirect / PMREM / roughness mip の仕様を `.unavatar` metadata と renderer pipeline に分離して定義する。

### Rim / Rim Shade / Backlight

- `[~]` `_UseRim`
  - done: `UnaLilToonLikeMaterial.rim.enabled_factor` として保持し、lilToon-like shader branch の rim contribution gate へ接続した。
  - remaining: backface mask、VR parallax、directional rim、shadow mask、transparent application を本家 `lilGetRim` に合わせる。
- `[~]` `_RimColor`
  - done: `_RimColor` を alpha 付きの v2 rim color として保持し、shader で rim 色に使用する。legacy MToon fallback への強度乗算とは分離済み。
  - remaining: color space、`_RimIndirColor`、RimShade との合成順を本家に合わせる。
- `[~]` `_RimColorTex`
  - done: Unity Exporter が `_RimColorTex` を `rimMultiplyTextureIndex` として保存し、Importer は `UnaLilToonLikeMaterial.rim.texture_index` へ読み込む。Renderer は lilToon-like rim texture を legacy MToon rim texture より優先して bind する。
  - remaining: ST transform、alpha semantics、directional rim / indirect rim での共用順を本家に合わせる。
- `[~]` `_RimMainStrength`
  - done: v2 rim parameter として保持し、shader で `lerp(rimColor, rimColor * albedo, value)` 相当へ接続した。
  - remaining: texture alpha、indirect rim、RimShade との順序を照合する。
- `[~]` `_RimBorder`
  - done: v2 rim parameter として保持し、rim factor の toon border へ接続した。
  - remaining: anti-alias strength と本家 `lilTooningScale` 係数を一致させる。
- `[~]` `_RimBlur`
  - done: v2 rim parameter として保持し、rim factor の toon blur へ接続した。
  - remaining: anti-alias strength と本家 blur curve を一致させる。
- `[~]` `_RimFresnelPower`
  - done: v2 rim parameter として保持し、`pow(1 - abs(N dot V), power)` へ接続した。
  - remaining: VR parallax 時の view vector を実装する。
- `[~]` `_RimNormalStrength`
  - done: source raw params を保持し、rim Fresnel 用 normal を geometry normal と normal-mapped normal の補間へ接続した。
  - remaining: lilToon の `fd.rimN`、2nd normal map、backface behavior と合わせて検証する。
- `[~]` `_RimBackfaceMask`
  - done: source raw params を保持し、backface fragment の rim contribution gate へ接続した。
  - remaining: cull mode / outline pass / transparent backface の本家条件と照合する。
- `[~]` `_RimEnableLighting`
  - done: v2 rim parameter として保持し、rim color と main light color の mix に接続した。
  - remaining: forward add branch と blend mode 3 以上の本家条件を反映する。
- `[~]` `_RimDirStrength`
  - done: source raw params を保持し、rim contribution の directional factor 近似へ接続した。
  - remaining: 本家 directional rim の view-space/light-space 入力と `_RimDirRange` の係数域を検証する。
- `[~]` `_RimDirRange`
  - done: source raw params を保持し、directional rim factor の power 近似へ接続した。
  - remaining: lilToon の directional rim range と同じ見え方になるよう係数変換を調整する。
- `[~]` `_RimIndirColor`
  - done: source raw color params を保持し、indirect rim color contribution へ接続した。
  - remaining: `_RimIndirRange`、lighting、RimShade との本家合成順を検証する。
- `[~]` `_RimIndirRange`
  - done: source raw params を保持し、indirect rim strength gate へ接続した。
  - remaining: 本家の indirect rim range と alpha/strength semantics を照合する。
- `[~]` `_RimIndirBorder`
	- done: source raw params を保持し、indirect rim toon border へ接続した。
	- remaining: blur の係数域を本家へ合わせる。
- `[~]` `_RimIndirBlur`
	- done: source raw params を保持し、`_AAStrength` を掛けた indirect rim toon blur へ接続した。
	- remaining: border の係数域を本家へ合わせる。
- `[~]` `_RimBlendMode`
  - done: v2 rim parameter として保持し、`lilBlendColor` 互換の Normal/Add/Screen/Multiply に接続した。
  - remaining: lilToon の blend mode enum 全体、RimShade / indirect rim との合成順を実装する。
- `[~]` `_RimShadowMask`
  - done: source raw params を保持し、lilToon-like rim contribution を current shadowmix approximation (`shading`) で抑制する係数へ接続した。
  - remaining: lilToon の `fd.shadowmix`、directional / indirect rim 分離、shadow mask texture と合わせた合成順を検証する。
- `[~]` `_UseRimShade`
  - done: v2 rim shade parameter として保持し、有効時は本家 `lilGetRimShade` と同じく rim shade factor で `lit -> lit * color` へ補間する。
  - remaining: `_RimShadeMask`、normal strength、AO/lighting との順序を本家へ合わせる。
- `[~]` `_RimShadeColor`
  - done: source raw color params を保持し、rim shade darkening color と alpha strength に接続する。
  - remaining: HDR/color space と `_RimShadeMask` の合成を実装する。
- `[~]` `_RimShadeNormalStrength`
  - done: source raw params を保持し、RimShade factor 用 normal を geometry normal と normal-mapped normal の補間へ接続した。
  - remaining: `_RimShadeMask`、2nd normal map、backface behavior との順序を本家へ合わせる。
- `[~]` backlight raw params
  - done: `_UseBacklight`、`_BacklightColor`、`_BacklightMainStrength`、`_BacklightNormalStrength`、`_BacklightBorder`、`_BacklightBlur`、`_BacklightDirectivity`、`_BacklightViewStrength`、`_BacklightReceiveShadow`、`_BacklightBackfaceMask` を v2 material に保持する。
  - remaining: sample では `_UseBacklight = 0` / `_BacklightMainStrength = 0` のため描画接続は deferred。Backlight 有効 material を用意してから本家 `lilGetBacklight` 相当へ接続する。

### Emission

- `[~]` `_UseEmission`
  - done: `UnaLilToonLikeMaterial.emission.enabled_factor` として保持し、lilToon-like shader branch の emission contribution gate へ接続した。
  - remaining: emission feature toggle を compatibility report に出し、2nd emission と gradation は分離する。
- `[~]` `_EmissionColor`
  - done: alpha 付きの v2 emission color として保持し、shader で emission 色と blend alpha に使用する。
  - remaining: HDR color handling、fluorescence、AudioLink、gradation との合成順を lilToon に合わせる。
- `[~]` `_EmissionMainStrength`
  - done: v2 emission parameter として保持し、shader で `lerp(emissionColor, emissionColor * albedo, value)` 相当へ接続した。
  - remaining: fluorescence、mask、gradation との順序を照合する。
- `[~]` `_EmissionMap`
  - done: glTF emissive texture / `_EmissionMap` を v2 emission texture として扱い、lilToon-like branch では white fallback 付きで bind する。
  - remaining: UV mode、mask、gradation、scroll/rotation の適用を実装する。
- `[~]` `_EmissionBlend`
  - done: v2 emission parameter として保持し、emission contribution alpha に接続した。
  - remaining: blink、transparent application、blend mask と合わせた最終係数を本家に合わせる。
- `[~]` `_EmissionBlendMode`
  - done: v2 emission parameter として保持し、`lilBlendColor` 互換の Normal/Add/Screen/Multiply に接続した。
  - remaining: lilToon の blend mode enum 全体と 2nd emission との合成順を実装する。
- `[ ]` emission gradation
- `[defer]` 2nd emission

### Outline

- `[~]` `_UseOutline` / outline width extraction
  - done: `_UseOutline` を v2 outline toggle として保持し、lilToon-like branch の authored outline pass を gate する。
  - remaining: outline pass variant、outline cull、transparent outline の扱いを material feature として再設計する。
- `[~]` `_OutlineWidth`
  - done: lilToon outline width を meters へ正規化して保持し、v2 authored outline width に接続する。
  - remaining: screen/world mode、camera distance scaling を本家の単位系に合わせる。
- `[~]` `_OutlineColor`
  - done: outline color factor として保持し、v2 authored outline color に接続する。
  - done: `_OutlineTex` の color / alpha を outline color に乗算する。
  - remaining: alpha discard、lighting mix との合成を本家に合わせる。
- `[~]` `_OutlineLitColor`
  - done: source raw color params を保持し、alpha がある場合は outline NdotL / `_OutlineLitScale` / `_OutlineLitOffset` による lit outline color への補間へ接続した。
  - remaining: view-space outline NdotL、`_OutlineLitShadowReceive` の shadow attenuation、`_OutlineLitApplyTex` の texture source を本家へ合わせる。
- `[~]` `_OutlineTex`
  - done: Exporter / importer / v2 material で texture reference を保持し、outline fragment shader で color / alpha texture として sample する。
  - remaining: sampler / UV transform / color space / alpha discard の本家互換性を照合する。
- `[~]` `_OutlineWidthMask`
  - done: Exporter / importer / v2 material で texture reference を保持し、outline width mask binding に接続する。
  - remaining: mask channel、UV mode、width scaling の本家互換性を照合する。
- `[~]` `_OutlineFixWidth`
  - done: v2 material parameter として保持する。
  - remaining: shader 側の fixed-width outline 計算へ接続する。
- `[~]` `_OutlineEnableLighting`
  - done: v2 material parameter として保持し、既存 outline lighting mix に接続する。
  - remaining: `_OutlineLitColor` / `_OutlineColor` との本家合成順を Unity reference で検証する。
- `[~]` `_OutlineZBias`
  - done: v2 material parameter として保持し、outline vertex shader の clip-space depth bias へ接続する。
  - remaining: Unity/lilToon の offset 符号・係数・near/far 依存を reference で検証する。
- `[defer]` outline validation: keep OFF during early lilToon-like material matching

### Advanced / Deferred

- `[defer]` Glitter
- `[defer]` Parallax / POM
- `[defer]` AudioLink
- `[defer]` Distance fade
- `[defer]` dissolve
- `[defer]` ID mask / UDIM discard
- `[defer]` fur / gem / refraction variants

## Completion Rule

各項目は次を満たしたら `[x]` にする。

1. Exporter が source property / texture slot を保持する。
2. Importer が `UnaLilToonLikeMaterial` parameter へ変換する。
3. Renderer WGSL が対応計算を持つ。
4. Base / original / noble1 / noble13 の少なくとも 1 つで差分確認する。
5. 未対応・近似の場合は compatibility report または docs に残す。
