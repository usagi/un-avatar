# `.unavatar` Format v0.1 Preview

作成日: 2026-05-31

この文書は U.N. Avatar v2 で追加する `.unavatar` preview format の設計メモである。実装前の固定案であり、破壊的変更を避けるため `specVersion` は必ず持たせる。

## 1. 位置づけ

`.unavatar` は U.N. Avatar Runtime が直接読める Runtime-ready avatar format である。

```text
avatar.unavatar
= valid GLB 2.0
+ extensions.UN_avatar
```

既存 `.una` / `.una.d` は v1 bootstrap / CLI smoke 用の仮置き format であり、v2 では廃止対象とする。実際の Renderer 入力として使われていないため、後方互換性維持は必須にしない。`.una` を `.unavatar` へ改名・転用しない。

## 2. 互換性方針

- `.unavatar` は GLB 2.0 として成立する。
- 拡張子は `.unavatar` だが、GLB magic / JSON chunk / BIN chunk は標準 GLB に従う。
- glTF 標準部には scene、nodes、meshes、skins、morph targets、textures、PBR fallback material を入れる。
- `UN_avatar` は v0.1 では `extensionsUsed` に入れる。
- v0.1 では原則 `extensionsRequired` に入れない。
- U.N. Avatar 拡張を無視した glTF viewer でも、最低限の見た目を表示できる状態を目指す。

Skinning は glTF 標準の `skins`、node `skin`、primitive `JOINTS_0` / `WEIGHTS_0`、inverse bind matrices を正本にする。Runtime importer はこれを既存 `UnaSceneSnapshot.skins` / `UnaSceneNode.skin` / `UnaMeshBuffers.joints` / `weights` へ落とし、VRM と同じ GPU skinning pipeline に接続する。`.unavatar` 専用の別 skinning renderer は作らない。

Renderer は skin の joint count と inverse bind matrix count の小さい方を effective palette として扱う。現在の GPU palette limit は 512 bones であり、これを超える skin、JOINTS/WEIGHTS の片方だけを持つ primitive、effective palette 外の joint index は diagnose warning の対象にする。Runtime は破綻回避のため範囲外 joint を clamp するが、正しい出力は exporter 側で valid palette を出すことを前提にする。

Morph は glTF mesh morph targets と mesh default weights を正本にする。`.unavatar` wardrobe の `blendShapeWeight` operation は import 時に primitive default morph weights へ反映する。Renderer は起動時と document revision 更新時に scene default morph weights を draw 側へ再読込し、既に upload 済みの morph weight buffer state を invalidation する。通常フレームでは default morph の再走査を行わず、expression override / active expression weights だけを per-frame 合成する。

glTF node には必要に応じて `extras.UN_avatar_node` を付与する。これは標準 glTF viewer には無視されるが、U.N. Avatar Runtime は wardrobe / dynamics / diagnostics の stable target として使う。

```json
{
  "nodes": [
    {
      "name": "Body_b",
      "mesh": 5,
      "skin": 5,
      "extras": {
        "UN_avatar_node": {
          "nodeId": "node_0123456789abcdef",
          "path": "Body_b"
        }
      }
    }
  ]
}
```

- `nodeId` は Exporter が root 名を除いた hierarchy path + sibling index から生成する stable id。export 用 clone の root 名変更で変化してはいけない。
- `path` は UI 表示と古い `.unavatar` の fallback 解決用。正本ではない。
- v0.1 Runtime は `target.nodeId` を最優先し、node extras にない古いファイルでは root `extensions.UN_avatar.nodes[]` registry、最後に path fallback を使う。

## 3. Extension Layout

v0.1 は単一 extension `UN_avatar` にまとめる。

```json
{
  "asset": {
    "version": "2.0",
    "generator": "UNAvatar Unity Exporter"
  },
  "extensionsUsed": ["UN_avatar"],
  "extensions": {
    "UN_avatar": {
      "specVersion": "0.1.0",
      "manifest": {},
      "humanoid": {},
      "materials": [],
      "expressions": [],
      "variants": [],
      "wardrobe": {},
      "dynamics": [],
      "provenance": {}
    }
  }
}
```

将来、仕様が安定したら `UN_avatar_manifest`、`UN_avatar_humanoid`、`UN_avatar_materials` などへ分割できる構造にする。ただし初期は parser / validator / Unity Exporter の実装量を抑えるため単一 extension を正本にする。

## 4. Manifest

`manifest` は生成物全体の情報を持つ。

```json
{
  "specVersion": "0.1.0",
  "manifest": {
    "generator": "UNAvatar Unity Exporter",
    "generatorVersion": "0.1.0",
    "sourceType": "vrc_unity_prefab",
    "createdAt": "2026-05-31T00:00:00Z"
  }
}
```

`sourceType` 候補。

- `vrm0`
- `vrm1`
- `gltf`
- `vrc_unity_prefab`
- `unity_scene_object`
- `unknown`

## 5. Humanoid

glTF skin だけでは `hips` / `head` / `leftHand` などの意味が分からないため、humanoid bone mapping を保持する。

```json
{
  "humanoid": {
    "humanBones": {
      "hips": 12,
      "spine": 15,
      "chest": 18,
      "neck": 24,
      "head": 25,
      "leftUpperArm": 31,
      "leftLowerArm": 32,
      "leftHand": 33,
      "rightUpperArm": 41,
      "rightLowerArm": 42,
      "rightHand": 43
    },
    "restPose": "unknown",
    "height": 1.58
  }
}
```

v0.1 parser は `humanBones` を `HumanoidProfile` へ正規化する。bone name は既存実装に合わせて小文字正規化してよい。

## 6. Materials

v0.1 は glTF PBR material を必ず fallback とし、`UN_avatar.materials` は toon / VRC source hint として扱う。

UNToon v2 の基準は lilToon-compatible 表現である。v1 の MToon 互換 shader / `UnaMtoonMaterial` は実装上の出発点として使うが、`.unavatar` v2 では lilToon の主要表現を MToon 互換枠へ押し込めない。VRM0/VRM1 MToon は UNToon v2 へ変換される入力 profile として扱い、shade / matcap / rim / emission / outline / alpha / cull / render queue などを UNToon 側の共通表現へ正規化する。renderer は固定の Full / Portable shader tier ではなく、モデル単位の required feature と GPU resource budget から dynamic variant を構成する。詳細は [`untoon-dynamic-variant-architecture.md`](untoon-dynamic-variant-architecture.md) を正本にする。

```json
{
  "materials": [
    {
      "material": 3,
      "profile": "una_toon",
      "sourceProfile": "liltoon",
      "baseColorTexture": 0,
      "shadeTexture": 1,
      "normalTexture": 2,
      "matcapTexture": 3,
      "emissionTexture": 4,
      "shadeColor": [0.72, 0.68, 0.66],
      "rim": {
        "enabled": true,
        "color": [1.0, 0.9, 0.8],
        "strength": 0.3
      },
      "outline": {
        "enabled": true,
        "width": 0.008,
        "color": [0.05, 0.04, 0.04],
        "lightingMix": 0.6
      },
      "alphaMode": "opaque",
      "doubleSided": false,
      "renderQueue": 2450
    }
  ]
}
```

v0.1 の優先順。

1. glTF PBR fallback 表示
2. MToon / lilToon の main texture
3. shade texture / shade color
4. normal map
5. matcap
6. rim
7. emission
8. outline
9. alpha / cull / render queue hint

Runtime 側は既存 `UnaMaterialPbr` / `UnaMtoonMaterial` の実装資産を使って段階移行する。ただし設計上の正本は UNToon v2 であり、MToon-like は legacy 実装名または VRM 入力 profile を指す。

glTF material には `extras.UN_avatar_material` を付与できる。これは glTF PBR fallback では表現しきれない Unity / VRC / lilToon 由来の source hint と、UNToon へ正規化した初期値を保持する material-local extension である。Runtime importer はこの payload を `UnaMaterialPbr.unavatar_material` に保持し、段階的な UNToon/lilToon 互換実装で参照できるようにする。

```json
{
  "materials": [
    {
      "name": "body_b",
      "pbrMetallicRoughness": {},
      "extras": {
        "UN_avatar_material": {
          "sourceShader": "Hidden/lilToonOutline",
          "family": "liltoon",
          "unMaterialModel": "UNToon",
          "renderQueue": 2450,
          "floatParams": {
            "_Cutoff": 0.001,
            "_UseShadow": 1,
            "_OutlineWidth": 0.08
          },
          "colorParams": {
            "_ShadeColor": [0.9, 0.86, 0.88, 1.0]
          },
          "mtoon": {
            "shadeColorFactor": [0.9, 0.86, 0.88],
            "outlineWidthMode": "world_coordinates",
            "outlineWidthFactor": 0.0008,
            "outlineWidthFactorUnit": "meters"
          }
        }
      }
    }
  ]
}
```

`floatParams` / `colorParams` は source shader property の保存領域であり、glTF viewer 互換表示には不要。Renderer はまず `mtoon` / UNToon 正規化値を使い、未対応機能や挙動差の解消に raw params を参照する。texture property はファイルサイズ膨張を避けるため、v0.1 では必要な slot だけ `mtoon.*TextureIndex` または `*TextureIndexAsset` として明示的に保持する。

lilToon source では `_Cutoff` property の存在だけで `MASK` と判定しない。通常 Opaque shader にも `_Cutoff` があるためである。Runtime importer は raw `_SrcBlend` / `_DstBlend` / `_AlphaToMask` を source blend state として最優先し、これが取れない場合に Cutout / Transparent / Refraction / Fur などの source shader hint、glTF alphaMode、render queue hint、必要なら極小 cutoff を組み合わせて UNToon alpha mode を決める。raw alpha params の `Mask` / `Blend` は明示値として優先するが、`Opaque` 相当値は shader variant hint を潰さない。`renderQueue >= 3000` は transparent、`2450 <= renderQueue < 3000` は source blend state が無い場合の cutout hint として扱う。

lilToon source の `_UseEmission`、`_EmissionColor`、`_EmissionMainStrength` は UNToon emission の source hint として読む。`_UseEmission = 0` は texture / color が残っていても emission 寄与を 0 とし、feature toggle を優先する。

Cull mode は material 共通値として Cull Off / Front / Back を保持する。glTF `doubleSided` は Cull Off / Back しか直接表現できないため、`.unavatar` では `UN_avatar_material.floatParams` の `_Cull` / `_CullMode` を読み、Unity/lilToon の `0=Off`、`1=Front`、`2=Back` を Runtime の `cull_mode` へ正規化する。

UV transform は material 共通値 `uvOffsetScale = [offset_x, offset_y, scale_x, scale_y]` と、UNToon 正規化値 `mtoon.uvOffsetScale` に保持する。Unity Exporter は main texture property の Tiling / Offset を読み、baseColorTexture には glTF 標準の `KHR_texture_transform` も出す。Renderer は shader 内で `uv * scale + offset` として適用する。

Unity の Mesh UV と glTF の texture coordinate convention は V 方向の扱いが異なるため、Unity Exporter は `.unavatar` 出力時に `TEXCOORD_0.y = 1 - unityUv.y` へ変換する。Unity material の Tiling / Offset も同じ座標系へ変換し、`offset_y = 1 - scale_y - unity_offset_y` として `KHR_texture_transform` / `mtoon.uvOffsetScale` に書く。`UN_avatar.textureCoordinateConvention = "gltf"` はこの変換済みを示す。preview 中の古い `.unavatar` は互換維持対象にせず、必要なら current exporter で再出力する。

UV animation は `mtoon.uvAnimationScrollXSpeedFactor`、`mtoon.uvAnimationScrollYSpeedFactor`、`mtoon.uvAnimationRotationSpeedFactor`、`mtoon.uvAnimationMaskTextureIndex` に保持できる。Unity Exporter は MToon の `_UvAnimScrollX/Y/Rotation` と lilToon の `_MainTex_ScrollRotate` を初期対応として読み、Renderer は frame time と mask texture を使って base / shade / normal / occlusion / rim / emissive / outline mask の UV を同じ規則で動かす。

### Texture Storage

v0.1 exporter は texture asset の source bytes を優先して `.unavatar` に埋め込む。Exporter は重い texture transcode / recompress / resize を行わない。Unity Editor 上で形式変換するほど品質劣化、世代劣化、encoder 差、export 時間増加、検証困難化のリスクが増えるためである。

`.unavatar` 内部の texture 正本は、任意 binary + MIME + 必要に応じた metadata とする。PNG / JPEG に限定しない。WebP、KTX2/BasisU、DDS/BCn、EXR、将来の独自圧縮を保持できる余地を残す。glTF core 互換だけで表現できない MIME / GPU compressed texture は、標準 extension または `UN_avatar` extension metadata で参照関係を持つ。

PNG / JPEG で表現できない HDR / float / half float texture は、glTF core image へ無理に押し込まない。Exporter は次の優先順位で出力する。

1. Source file bytes: EXR / HDR / KTX2 / DDS など、Unity asset の元ファイルが取得できる場合は source bytes を `UN_avatar` 側の texture asset として保持する。
2. KTX2 raw fallback: RenderTexture や Unity 内生成 texture など source file bytes がない場合は、GPU readback で `RGBA16F` などへ正規化し、KTX2 に格納する。これは optimizer の圧縮 KTX2 ではなく、source fidelity を守るための container fallback とする。
3. glTF fallback image: glTF core PBR material 用に必要な場合だけ、PNG/JPEG など低互換 fallback を別 image として持てる。ただし UNAvatar runtime の正本は `UN_avatar` 側の source / KTX2 asset を優先する。

`KHR_texture_basisu` は Basis Universal / KTX2 圧縮済み texture との互換経路として使う。非圧縮 `RGBA16F` KTX2 や EXR source を無理に `KHR_texture_basisu` として扱わない。これらは `UN_avatar.textures` の source asset として表現し、runtime が直接 decode / upload する。

`UN_avatar.textureAssets` は glTF core `images` では扱えない texture source の入口にする。汎用 blob ではなく texture source 専用 container であり、`bufferView` は GLB BIN chunk 内の source bytes を指す。`mimeType` は source format の正本、`sourcePixelFormat` は source file/header 由来の実形式を記録し、どちらも GPU upload 形式ではない。例えば `RGB16F` EXR は source として `RGB16F` のまま記録し、wgpu backend が必要な場合だけ upload 時に `RGBA16Float` へ拡張する。Radiance HDR は `mimeType = "image/vnd.radiance"` / `sourcePixelFormat = "RGBE8"` として保持する。source が PNG / JPEG の texture は glTF core `images` にそのまま保持してよい。source が EXR / HDR / KTX2 / DDS の texture は `UN_avatar.textureAssets` に source bytes のまま保持する。どちらの場合も `textureShape = "TextureCube"` / `"Cube"` は、source binary を変換する指示ではなく、runtime が texture を cube として解釈するための metadata である。

`sampler` は glTF sampler と同じ数値定数 (`magFilter`, `minFilter`, `wrapS`, `wrapT`) を inline object として持てる。これは EXR など glTF core `textures` を経由しない source asset でも Unity の Filter / Wrap 設定を落とさないためである。glTF core image 経由の texture は通常の `textures[].sampler` を正とする。

glTF core `images[].extras.UN_avatar_image` と `UN_avatar.textureAssets[]` は `colorSpace` (`srgb` / `linear` / `data`), `textureType`, `textureShape`, `sRGB` を保持できる。Renderer は material slot 由来の role を第一に使い、source metadata が `linear` / `data` または `sRGB=false` の場合は RGBA fallback upload でも sRGB texture format にしない。これは Normal / mask / data texture を色テクスチャとして劣化扱いしないための境界である。

`sourceLayout` は source image が texture shape へ変換される前の配置 hint である。Unity `TextureImporter.generateCubemap` が取得できる場合は `unity_auto_cubemap` など `unity_*` 値を保持し、併せて `unityGenerateCubemap` に Unity enum 名をそのまま残す。取得できない場合だけ寸法から `latlong`, `horizontal_strip`, `vertical_strip`, `horizontal_cross`, `vertical_cross`, `unknown_cube_source` などを推定する。これは source binary を変換する指示ではなく、runtime upload / PMREM cache 生成時の解釈 hint である。

`textureShape = "TextureCube"` / `"Cube"` の reflection source は、UNToon v2 / lilToon-compatible path では true cubemap として扱う。Renderer は source bytes を decode したあと、layout metadata と source image dimensions に基づき cube faces へ展開し、`texture_cube` / cube texture view で sample する。2D equirectangular approximation は diagnostic / compatibility fallback であり、lilToon-compatible high-capability path の正本ではない。PMREM / roughness mip chain は source package を書き換えず、runtime cache または explicit optimizer output として生成する。

`RGB16F` source を wgpu upload で `RGBA16Float` に拡張するのは、wgpu の portable `TextureFormat` に `RGB16Float` が無く、一般的な GPU API でも 3ch half float texture は 4ch half float より互換性が低いためである。`.unavatar` と CPU decoded representation は `RGB16F` を維持し、alpha=1 の追加は renderer upload boundary の明示的 fallback とする。

```json
{
  "textureAssets": [
    {
      "id": "texture-asset-0",
      "name": "cubemap2",
      "mimeType": "image/exr",
      "sourceExtension": ".exr",
      "sourcePixelFormat": "RGB16F",
      "colorSpace": "linear",
      "channels": "rgb",
      "textureType": "Default",
      "textureShape": "TextureCube",
      "sourceLayout": "unity_auto_cubemap",
      "unityGenerateCubemap": "AutoCubemap",
      "sRGB": false,
      "sampler": {
        "magFilter": 9729,
        "minFilter": 9729,
        "wrapS": 10497,
        "wrapT": 10497
      },
      "width": 4096,
      "height": 2048,
      "bufferView": 42,
      "byteLength": 26987131
    }
  ]
}
```

Material 側は glTF texture index で表せない場合に asset id を参照する。

```json
{
  "extras": {
    "UN_avatar_material": {
      "mtoon": {
        "matcapTextureIndexAsset": "texture-asset-0",
        "reflectionCubeTextureIndexAsset": "texture-asset-0"
      }
    }
  }
}
```

UNToon v2 / lilToon-compatible renderer は `reflectionCubeTextureIndex` / `reflectionCubeTextureIndexAsset` を authored reflection cube source として扱う。source binary は PNG なら PNG、EXR なら EXR のまま `.unavatar` に保持し、runtime decode 後に cube texture として upload / sample する。`textureShape` が cube であるにもかかわらず 2D reflection map として扱うのは compatibility fallback であり、diagnostics に記録する。roughness mip / PMREM は `.unavatar` 本体の source bytes を置換せず、runtime cache または optimizer の派生データとして扱う。

`.unavatar` は通常処理では immutable source package として扱う。Runtime load、profile 変更、wardrobe 切替、cache 生成は `.unavatar` 本体を書き換えない。`.unavatar` は Unity project / `.unitypackage` から生成された派生物ではあるが、ユーザーにとっては配布・保存・再利用するアバターファイルでもあるため、source 忠実性とポータビリティーを優先する。

後段最適化は 2 種類に分ける。

- Local cache: `%APPDATA%/UN Avatar/...` または OS 標準 cache 配下に、source hash、MIME、material role、GPU backend、adapter / driver capability、quality policy、optimizer version を key として保持する。GPU 交換や別 PC 複製では再生成可能な派生データとする。
- Optimized package: `un-avatar-optimizer input.unavatar output.unavatar` のような明示操作で、`.unavatar` 内部 binary を置換または追加する。元ファイル上書きではなく別名出力を基本にする。

Runtime の GPU upload は次の順に判断する。

1. Source Native: KTX2/BCn/ASTC/ETC2/DDS など、GPU が直接扱える source binary は再圧縮せず upload する。
2. Decoded Native: PNG/JPEG/WebP/EXR 等を decode した結果が GPU 対応 format なら、RGBA8 固定にせずその format で upload する。
3. Optimized Cache: quality policy に従い、ローカル cache の BC7/BC5/BC6H/KTX2 等を使う。
4. Fallback: decoder / GPU capability / policy の都合で使えない場合に RGBA8、RGBA16F などへ変換し、diagnostics に記録する。

Texture compression policy は user/profile 設定として 4 段階を持つ。既定は `balanced`。

- `source`: source/native upload と忠実性を優先する。cache / transcode は最小限。
- `balanced`: 既定。color は安全な範囲で BC7/KTX2 等、normal は BC5、HDR/float は native または BC6H/RGBA16F を優先する。
- `memory`: 表現力低下や変換を許容し、GPU memory / disk cache 削減を優先する。
- `compat`: GPU 互換性と失敗しにくさを優先し、RGBA8/RGBA16F 等の広く扱える形式へ寄せる。

圧縮有効時でも、source の表現力を BCn 等で安全に表現できない場合は、policy と GPU capability に応じて native upload を維持する。フォールバックは原則として精度を落とさない。`R16G16B16`、`RGB16F`、`RGBA16F` などを安易に RGBA8 へ変換せず、wgpu が直接持てない RGB 系だけ upload 境界で `RGBA16Unorm` / `RGBA16Float` のような同等以上の精度を持つ形式へ拡張する。RGBA8 互換変換は compatibility mode または source/native upload が不可能な最後の fallback に限る。

## 7. Expressions

`expressions` は表情や配信用操作の単位を表す。v0.1 では morph binding を最小実装し、material / visibility は variants と同じ operation model を使う。

```json
{
  "expressions": [
    {
      "id": "smile",
      "displayName": "Smile",
      "bindings": [
        {
          "type": "morph",
          "node": 42,
          "target": 3,
          "weight": 1.0
        },
        {
          "type": "materialColor",
          "material": 5,
          "property": "emission",
          "value": [1.0, 0.4, 0.4, 1.0]
        }
      ]
    }
  ]
}
```

Runtime MVP は morph binding から始める。material binding / visibility binding は variants foundation と合わせて実装する。

## 8. Wardrobe Sets / Variants / Outfits

複数衣装、アクセサリ、小物切替は `wardrobe` で扱う。原則として 1 avatar = 1 `.unavatar` に全資産を同梱し、`wardrobe.sets` の差分 patch で切り替える。

`variants` は v0.1 初期設計名として残るが、衣装切替の正本は `wardrobe` とする。glTF の material variants と混同しないためである。

```json
{
  "wardrobe": {
    "baseSet": "base",
    "sets": [
      {
        "id": "base",
        "displayName": "Base",
        "default": true,
        "operations": []
      },
      {
        "id": "Noble Trace Color 1",
        "displayName": "Noble Trace Color 1",
        "source": "unity_capture_diff",
        "assetGroups": ["outfit:noble-trace-color-1"],
        "operations": [
          { "type": "subtreeEnabled", "target": { "nodeId": "node_color_1", "path": "Color 1" }, "visible": true },
          { "type": "subtreeEnabled", "target": { "nodeId": "node_color_13", "path": "Color 13" }, "visible": false },
          { "type": "subtreeEnabled", "target": { "nodeId": "node_base_maid", "path": "Maid" }, "visible": false },
          { "type": "subtreeEnabled", "target": { "nodeId": "node_color_1_hat", "path": "Color 1/Armature.1" }, "visible": false },
          { "type": "blendShapeWeight", "target": { "nodeId": "node_body_b", "path": "Body_b" }, "name": "Knee socks____ニーソ専用", "value": 0.0 }
        ]
      }
    ]
  }
}
```

`wardrobe.sets[].id` は profile / CLI / renderer から参照される外部キーなので、Exporter はユーザー入力名を勝手に slug 化しない。`displayName` は UI 表示用、`assetGroups[]` は lazy upload / unload 用の内部 grouping key であり、ここでは slug 化してよい。

v0.1 の operation 候補。

- `subtreeEnabled`
- `nodeEnabled`
- `rendererEnabled`
- `blendShapeWeight`
- `materialOverride`
- `expressionWeight`
- `dynamicsEnable`

最初に実装するのは `subtreeEnabled` / `nodeEnabled` / `blendShapeWeight`。衣装 mesh / accessory mesh の ON/OFF と body shrink / sock / underwear などの blendshape 差分が切替機能の最小価値になる。旧 draft / prototype 由来の `subtreeVisibility` / `nodeVisibility` / `rendererVisibility` は importer では legacy alias として読むが、Exporter は出力時に `subtreeEnabled` / `nodeEnabled` / `rendererEnabled` へ正規化する。

### Target Identity

operation の `target` は保存上 `nodeId` を正とし、`path` を表示と fallback に使う。

- `nodeId`: exporter が node に付与する stable id。同名 object や `Armature.1` の衝突を避ける。
- `path`: Unity hierarchy path。UI 表示、手動修復、diagnostics 用。

### Operation Precedence

wardrobe set の適用順。

1. `.unavatar` の base state を適用する。
2. 選択された `wardrobe.sets[].operations` を上から順に適用する。
3. `subtreeEnabled` は対象 node の local enabled state を変更し、その実効可視が子孫へ継承される。子孫の local enabled state は変更しない。`nodeEnabled` も対象 node の local enabled state を変更する。
4. 同じ具体度なら後勝ち。
5. Supervisor profile 側のユーザー override は `.unavatar` 内蔵 set より後に適用する。

この規則により、`Color 1` 全体を ON にしつつ `Color 1/Armature.1` だけ OFF にできる。

Unity Exporter の preview 実装では、wardrobe operations は bake 後 snapshot から再生成しない。`base` も non-base set も Unity 上で capture した authored state / diff を正本にする。Modular Avatar bake は export mesh を整える処理であり、wardrobe の意味論を上書きする正本ではない。
Exporter は `activeInHierarchy` ではなく `activeSelf` を capture する。親が OFF のため実効的に見えていない子でも、子自身の Inspector チェックボックス状態は維持する必要があるためである。特定の子だけを落としたい場合は、親を `subtreeEnabled=true` した後に、その子へ `nodeEnabled=false` または `subtreeEnabled=false` を置く。
Unity Exporter は親を ON にする set の配下にある inactive child を明示的な `nodeEnabled=false` として出力する。これにより、衣装 root をまとめて ON にしつつ帽子や pants/skirt のような相互排他部品だけを OFF にできる。
Runtime は base state 適用時、親 OFF に由来する子孫の `false` を冗長 operation として無視する。base が子孫 local state まで `false` に潰すと、後続 set が衣装 root を ON にした際に配下の authored ON 子要素まで復元できなくなるためである。set 側で明示された子孫 `false` はそのまま尊重する。

### Asset Groups And Lazy Loading

`.unavatar` は全資産を保持するが、Runtime は最初から `assetGroups` 単位の lazy upload / unload を前提にする。

- mesh / texture / material / dynamics は `assetGroups` に所属できる。
- wardrobe set は利用する `assetGroups` を宣言する。
- Runtime は選択 set に必要な group だけ GPU upload し、不要 group は unload 対象にできる。
- 数百着規模の衣装を扱うユーザーを想定し、全衣装の GPU 常駐を前提にしない。

### Capture Diff Workflow

ユーザー負担を減らすため、Unity Exporter は手入力ではなく capture diff を標準導線にする。

1. Unity 上で素体状態を整えて `Capture Base`。
2. Unity 上で衣装状態を整えて `Capture Wardrobe Set`。
3. Exporter が active state / renderer enabled / blendshape weights の base 差分だけを operations として記録する。
4. Color 1 と Color 13 のような色違いは set 複製後、対象 outfit subtree の ON/OFF を差し替える。

### Export Mode

Unity Exporter は次の出力モードを持つ。

- `Current Only`: Unity 上で現在有効な見た目だけを Modular Avatar bake して `.unavatar` に出す。fallback / debug export。
- `Wardrobe (Baked)`: baked model と authored wardrobe operations を 1 `.unavatar` に同梱する現行 preview mode。
- `Wardrobe (Split)`: ベイク前 source graph と wardrobe set を分けて保持し、Runtime 側で選択 set を resolve/cache する v2 本命候補。多数衣装でのデータ量、編集性、lazy upload / unload を検証する。

## 9. Dynamics

`dynamics` は VRM SpringBone と VRC PhysBone を Runtime primitive へ正規化した情報を持つ。

```json
{
  "dynamics": [
    {
      "id": "hair_front",
      "source": "vrc_physbone",
      "enabled": true,
      "roots": [
        {
          "nodeId": "node-hair-front",
          "path": "Armature/Hips/Spine/Chest/Neck/Head/HairFront"
        }
      ],
      "ignoreTransforms": [
        {
          "nodeId": "node-hair-accessory",
          "path": "Armature/Hips/Spine/Chest/Neck/Head/HairAccessory"
        }
      ],
      "multiChildType": "Ignore",
      "colliders": [0, 1],
      "pull": 0.2,
      "spring": 0.35,
      "stiffness": 0.35,
      "drag": 0.2,
      "gravity": [0.0, -0.4, 0.0],
      "radius": 0.03,
      "sourceParams": {
        "endpointPosition": [0.0, 0.1, 0.0],
        "allowCollision": true,
        "allowGrabbing": false,
        "allowPosing": false,
        "colliders": [
          {
            "shapeType": "Sphere",
            "root": {
              "nodeId": "node-head",
              "path": "Armature/Hips/Spine/Chest/Neck/Head"
            },
            "radius": 0.08,
            "position": [0.0, 0.1, 0.0]
          }
        ]
      }
    }
  ],
  "colliders": [
    {
      "id": "head_sphere",
      "node": 55,
      "shape": "sphere",
      "offset": [0.0, 0.1, 0.0],
      "radius": 0.08
    }
  ]
}
```

v0.1 は完全な PhysBone 再現を狙わない。まず既存 SpringBone runtime primitive へ近似変換する。
`roots` は glTF node index、`nodeId` / `path` object、または exporter node id 文字列を受け付ける。`enabled:false` の dynamics entry は runtime lower 時に無視する。
`ignoreTransforms` は root traversal から除外する。`multiChildType:"Ignore"` は分岐 root を最初の有効 child chain だけへ近似する。
`sourceParams.endpointPosition` は child を持たない root に synthetic endpoint child を追加して通常 chain へ正規化する。
`sourceParams.colliders` は VRC PhysBone collider の保存情報であり、Sphere / Capsule は local collider として SpringBone solver / debug draw へ接続する。`insideBounds:true` collider は tail を collider 内側へ留める制約として近似する。
`sourceParams.allowCollision:false` は source collider を solver へ渡さない。limits は runtime dynamics group の `limit` に正規化して保持し、diagnostics にも出すが、v0.1 初期の solver 挙動にはまだ反映しない。grabbing / posing は runtime dynamics group の `interaction` metadata に正規化して保持し、source feature count と group diagnostics にも出すが、v0.1 初期の interaction 挙動にはまだ反映しない。
`dynamics[].id` は runtime dynamics group の `source_id` として保持し、wardrobe / action state が dynamics enable state を参照するための stable key として使う。`name` は表示用 comment として扱う。

## 10. Provenance And License

`.unavatar` はメッシュ・テクスチャを含むため、由来と再配布条件の注意を保持する。

```json
{
  "provenance": {
    "source": {
      "type": "vrc_unity_prefab",
      "modelName": "Original Avatar",
      "exporter": "UNAvatar Unity Exporter"
    },
    "licenses": [
      {
        "name": "Original asset license",
        "url": "https://example.invalid/license",
        "redistributionAllowed": false
      }
    ],
    "redistributionAllowed": false,
    "note": "Follow the original asset license."
  }
}
```

Runtime / Supervisor はこの情報を profile 選択時に確認できるようにする。

## 11. Runtime Import MVP

最小 importer は次を行う。

1. `.unavatar` を GLB として読み込む。
2. 既存 glTF importer で scene snapshot を作る。
3. root `extensions.UN_avatar` を読む。
4. `manifest` / `humanoid` / `provenance` を `UnaDocument` へ反映する。
5. `materials` は読める範囲で既存 `UnaMaterialPbr` / `UnaMtoonMaterial` へ反映する。
6. `wardrobe` / `variants` / `expressions` / `dynamics` は未対応でも diagnostics に unsupported として残す。

## 12. Validator MVP

validator は次を検査する。

- GLB として読める。
- `extensions.UN_avatar.specVersion` がある。
- glTF 標準部に scene / mesh / material fallback がある。
- `humanoid.humanBones` の node index が範囲内。
- `materials[].material` が範囲内。
- `wardrobe.sets[].operations` の参照 node / material / dynamics id が存在する。
- `provenance.redistributionAllowed` が明示されている。
