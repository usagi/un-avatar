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

Runtime 側は既存 `UnaMaterialPbr` / `UnaMtoonMaterial` へ寄せて実装し、最初から別の巨大 material system を作らない。

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
        "id": "noble-trace-color-1",
        "displayName": "Noble Trace Color 1",
        "source": "unity_capture_diff",
        "assetGroups": ["outfit:noble-trace-color-1"],
        "operations": [
          { "type": "subtreeVisibility", "target": { "nodeId": "node_color_1", "path": "Color 1" }, "visible": true },
          { "type": "subtreeVisibility", "target": { "nodeId": "node_color_13", "path": "Color 13" }, "visible": false },
          { "type": "subtreeVisibility", "target": { "nodeId": "node_base_maid", "path": "Maid" }, "visible": false },
          { "type": "subtreeVisibility", "target": { "nodeId": "node_color_1_hat", "path": "Color 1/Armature.1" }, "visible": false },
          { "type": "blendShapeWeight", "target": { "nodeId": "node_body_b", "path": "Body_b" }, "name": "Knee socks____ニーソ専用", "value": 0.0 }
        ]
      }
    ]
  }
}
```

v0.1 の operation 候補。

- `subtreeVisibility`
- `nodeVisibility`
- `rendererVisibility`
- `blendShapeWeight`
- `materialOverride`
- `expressionWeight`
- `dynamicsEnable`

最初に実装するのは `subtreeVisibility` / `nodeVisibility` / `blendShapeWeight`。衣装 mesh / accessory mesh の ON/OFF と body shrink / sock / underwear などの blendshape 差分が切替機能の最小価値になる。

### Target Identity

operation の `target` は保存上 `nodeId` を正とし、`path` を表示と fallback に使う。

- `nodeId`: exporter が node に付与する stable id。同名 object や `Armature.1` の衝突を避ける。
- `path`: Unity hierarchy path。UI 表示、手動修復、diagnostics 用。

### Operation Precedence

wardrobe set の適用順。

1. `.unavatar` の base state を適用する。
2. 選択された `wardrobe.sets[].operations` を上から順に適用する。
3. `subtreeVisibility` より `nodeVisibility` / `rendererVisibility` のような具体指定が優先する。
4. 同じ具体度なら後勝ち。
5. Supervisor profile 側のユーザー override は `.unavatar` 内蔵 set より後に適用する。

この規則により、`Color 1` 全体を ON にしつつ `Color 1/Armature.1` だけ OFF にできる。

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

- `All Wardrobe Sets In One .unavatar`: 複数衣装・小物を 1 ファイルに同梱し、`wardrobe.sets` で切り替える。MVP。
- `Current State Only`: Unity 上で現在有効な見た目だけを `.unavatar` に出す。fallback / debug export。
- `Split Wardrobe Sets Into Separate .unavatar Files`: 大型衣装や配布都合向け。共有データ重複は許容する。

## 9. Dynamics

`dynamics` は VRM SpringBone と VRC PhysBone を Runtime primitive へ正規化した情報を持つ。

```json
{
  "dynamics": [
    {
      "id": "hair_front",
      "source": "vrc_physbone",
      "roots": [120],
      "colliders": [0, 1],
      "stiffness": 0.35,
      "drag": 0.2,
      "gravity": [0.0, -0.4, 0.0],
      "radius": 0.03
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
