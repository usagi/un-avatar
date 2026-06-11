# UNEvaluation v2 Design

`UNEvaluation` は、source data と solver の間で runtime state を評価する層である。
module 名は `runtime_eval` を使う。

v1 の SpringBone / action 実装は参照資産として使ってよい。
ただし v2 の正本は、source data を直接 mutate せず、評価済み runtime state を solver / renderer へ渡す設計とする。

## Responsibility

UNEvaluation が扱うもの:

- active wardrobe set / active asset groups
- latched runtime action state
- runtime parameters
- animation / expression / continuous parameter evaluation
- contact-derived transient parameter declarations and future values
- dynamics enabled override の評価結果

UNEvaluation が扱わないもの:

- bone dynamics solver integration
- VRC / VRM / Modular Avatar source-specific component semantics の直接実行
- mesh / material upload residency
- VRChat client 完全互換

UNDynamics、UNInteraction、UNConstraints は UNEvaluation の評価結果を読む側であり、source package を直接読んで優先順位を決めない。

## State Layers

runtime target の合成は単純な全体優先順位だけではなく、target owner ごとの ownership で扱う。
基本層は以下の通り。

1. source default
2. base safe state
3. active wardrobe
4. latched action
5. continuous evaluator

ただし、衝突解決は target type ごとの policy を持つ。

| Target | Policy |
| --- | --- |
| wardrobe set selection | wardrobe / explicit action only。contact は触らない。 |
| node visibility / material slot / dynamics enable | explicit user action を continuous evaluator より優先する。 |
| expression parameter / contact-derived transient parameter | continuous evaluator を latched action より優先する。 |
| source default / base safe state | runtime layer がない target の fallback。 |

この方針の理由:

- Wardrobe は「今この衣装セットでいてほしい」というユーザーの明示 intent。
- Latched action は「押した状態を保ってほしい」というユーザーの明示 intent。
- Continuous evaluator は「現在の入力状態へ追従してほしい」という入力 intent。

continuous evaluator を常に最上位にすると、ユーザーが押した visibility / material / dynamics toggle が即座に戻される可能性がある。
一方、expression / contact parameter は入力状態そのものなので continuous を優先する。

## Owner Keys

評価結果は owner key を持つ。
owner key は同じ target へ複数の runtime source が書く時の説明可能性と diagnostics に使う。

推奨 owner key:

- `source_default`
- `base_safe_state`
- `wardrobe:<set_id>`
- `action:<action_id>`
- `parameter:<name>`
- `animation:<source_id>`
- `contact:<source_id>`

v2 初期では priority / lock は導入しない。
必要になった場合は、target type ごとの policy に action policy を後から足す。

実装メモ:

- core は `UnaRuntimeAction::evaluation_target_writes()` で action effect 由来の owner key / target kind / target key を read-only view として公開し、CLI diagnose / renderer runtime status / Supervisor diagnostics から観測できる。
- core は `UnaRuntimeActionSet::evaluation_target_write_collisions()` で同一 target kind/key に複数 action owner が書くケースを diagnostics として列挙する。同一 action 内の複数 effect は、その action の内部評価なので初期診断では衝突扱いしない。
- core runtime model は node visibility、material property、material slot、dynamics enabled の現在値を read-only に取得できる。inactive-state default restore はこの現在値 API と別途保持する baseline/source-default を突き合わせて設計する。
- core は `runtime_action_restore_readiness` diagnostics で、action effect ごとに restore target か、current value を読めるか、読めた current value、baseline が必要か、保存済み baseline で restore-ready かを分類する。
- core は restore readiness から read-only の restore baseline candidate list も派生する。これは「今 capture するなら baseline として保存される値」の diagnostics であり、restore 実行はまだ行わない。
- core は restore baseline candidates から deterministic capture plan を作れる。capture plan は `UnaRuntimeState.restore_baselines` へ owner-keyed runtime state として保存できる。
- renderer は runtime action activation の effect 適用前に、その action の restore baseline を capture する。既に保存済みの owner/target baseline は上書きせず、初回変更前の値を保つ。
- 保存済み restore baseline がある action effect は `runtime_action_restore_readiness` で `ready=true` として観測できる。
- core は `runtime_action_restore_apply_plan` diagnostics で、現在の parameter state から inactive と判定された action の restore 書き戻し候補を read-only に列挙できる。active / missing / no-condition action は apply-ready にはしない。
- core は ready な inactive action restore apply entry を node visibility、material color/scalar、material slot、dynamics enabled に書き戻せる。renderer は runtime action activation 後に現在 parameter state で inactive になった action の restore を適用する。
- renderer `set_parameter` は condition metadata で active になった action を deterministic order で全件適用し、該当 action が無い parameter change でも inactive restore を走らせる。これは continuous evaluator の初期段階であり、アニメータ風の frame-by-frame blend / layer evaluation ではない。
- core runtime model は action trigger / condition、contact receiver、runtime state から source-neutral runtime parameter definitions を作れる。contact transient parameter と action/menu parameter の同名共有は `runtime_parameter_conflicts` diagnostics で観測する。通常の 0/1 toggle のような同一 action parameter の値違いは衝突扱いしない。
- この diagnostics / capture / restore は source package を mutate しない。inactive-state default restore、continuous evaluator、衝突診断の前提情報として使う。
- `action:<action_id>` owner は latched action state の説明用であり、v2 初期では priority / lock を意味しない。

## Contacts

Contacts は v2 初期では interaction / parameter source として扱う。
直接 PhysBone solver を動かさない。

段階:

1. Phase A: Contacts metadata + diagnostics
   - source id、kind、tags、shape、parameter、counts、warning。
   - 実装済み。
2. Phase B: Contact parameter declaration
   - Receiver が emit し得る parameter を UNEvaluation に宣言する。
   - 値はまだ書かない。
   - UI / diagnostics / action resolver が namespace を把握できる。
   - core runtime view、renderer runtime status count / bounded list、CLI diagnose の declared parameter count / sample は実装済み。
3. Phase C: Diagnostics-only contact probe
   - overlap 計算はするが parameter state へは書かない。
   - `would_emit parameter=X value=1` を diagnose / debug status に出す。
   - 座標系、tag match、shape overlap、誤爆を検証する。
   - core runtime view、CLI diagnose、renderer runtime status の current runtime scene pose probe は実装済み。Renderer では motion retarget / dynamics が scene pose を更新した後の document scene を読む。Sphere は sphere、Capsule / Unknown は bounding sphere 近似で扱い、runtime parameter state は変更しない。
4. Phase D: Opt-in parameter emission
   - profile flag または `.unavatar` capability で明示有効化する。
   - `.unavatar` capability / contacts flag の opt-in 判定、emitted count、reset-to-zero count は CLI diagnose / renderer runtime status / Supervisor diagnostics で観測できる。
   - contact 切断時は 0。
   - 複数 Receiver が同じ parameter を書く場合は max / OR。
   - owner key は `contact:<source_id>`。

v2 初期リリース目標は Phase A + Phase B まで。
Phase C は debug-only diagnostics として追加済み。
Phase D は opt-in 時のみ current runtime scene pose probe から runtime parameter state へ 1/0 を書く初期実装まで追加済み。既定 OFF は維持する。

## Constraints And Interactions

VRC Constraints、VRM node constraints、Modular Avatar resolver 由来 constraint は `UNConstraints` へ寄せる。
v2 初期では transform dependency metadata、reset / rebuild 対象、diagnostics の役割に留める。
solver integration は transform evaluation layer が安定してから行う。

grabbing / posing は `UNInteraction` の capability metadata とする。
v2 初期では UI / diagnostics / future hook 用であり、物理的な操作挙動は持たせない。

## Diagnostics

runtime status / diagnose は少なくとも以下を分ける。

- source-authored value
- evaluated runtime value
- solver-applied value

Contacts については Phase B 以降、declared parameter count を出す。
Phase C 以降、would-emit count / sample を出す。
Phase D 以降、emitted parameter count / owner key / reset-to-zero count を出す。

この分離を崩すと、source importer、runtime evaluation、solver のどこで壊れたか追跡できなくなる。
