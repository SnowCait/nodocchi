# Structured diagnostics

`bot-scenario` は入力局面に続けて、最終 action と判断経路を section ごとに表示します。この文書は「出力をどう読むか」を扱います。各 policy の詳細は [麻雀 AI の概要](ai/overview.md) から辿ってください。

## 全体像

主な section は次のとおりです。

| section | 内容 |
| --- | --- |
| `Scenario` / `Table state` | 入力した局面と観測済み table state |
| `Final decision` | 最終 action と採用経路 |
| `Normal discard` | 通常打牌 evaluation の有無、選択 action、候補数 |
| `Normal discard candidates` | 通常打牌候補ごとの評価と比較 |
| `Player threats` | player ごとの reach / meld facts と OpenHandThreat classification |
| `Push/Pull` | threat と offense を組み合わせた押し引き |
| `Reach` | 通常打牌後のテンパイに対するリーチ判断 |
| `Defense` | リーチ者向け防御候補 |
| `OpenHand defense` | High OpenHandThreat 向け防御候補 |
| `Combined defense` | リーチと High OpenHandThreat が同時にいる場合の候補 |
| `Summary` | 最終選択と次点を末尾で要約 |

## Final decision と AgentActionSource

`Final decision.source` は、どの production selector が action を採用したかを表す `AgentActionSource` です。

```text
Final decision
  action: 1m
  source: DefenseFallback
  defense kind: Genbutsu
```

代表的な source:

| source | 意味 |
| --- | --- |
| `Hora` / `Ryukyoku` / `Reach` | 和了、流局、リーチ |
| `NormalDiscard` | 通常打牌 selector |
| `DefenseFallback` | リーチ者向け防御 fallback |
| `OpenHandDefenseFallback` | High OpenHandThreat 向け fallback |
| `CombinedThreatDefenseFallback` | 複合 threat 向け fallback |
| `Pon` | 鳴き判断で選んだポン |
| `LegalDahaiFallback` / `None` | 上位判断で選べない場合の fallback |

防御 source では category や kind も表示されます。

```text
Final decision
  action: 5m
  source: OpenHandDefenseFallback
  open hand defense category: SafeAgainstAllTargets
```

## Normal discard と candidates

`Normal discard` は通常打牌を評価したか、選択 action と候補数を要約します。和了などの早期 return では `not evaluated` です。

各合法打牌について、打牌後の向聴、Acceptance、ドラ、フリテン、1向聴・2向聴以上の指標などを表示します。`selected: yes` が通常打牌 selector の選択です。最終 action は Push/Pull や Reach、防御 fallback によって別の action になることがあります。

その打牌でテンパイになる候補には `permanent furiten` / `history furiten after discard` / `ron` を表示します。`ron` は両者を合わせた総合ロン可否で、全候補が選択候補と同じ評価時点 (その打牌を切り終えた後) の facts を使います。

`--verbose` は候補の詳細、`--lookahead` は2手先の概要を追加します。2手先の概要は仮想ツモ牌を向聴数が下がるもの (`draws`) と維持するもの (`same-shanten`) に分けて種類数と残枚数を表示し、`--verbose` の牌ごとの詳細では `transition` にその分類を表示します。`--lookahead --verbose` では、1向聴候補の向聴数を維持する枝だけをテンパイまでもう1段追い、`downstream value` と候補ごとの `same-shanten downstream value` を追加します。指標と comparator の読み方は [打牌選択](ai/discard-selection.md) を参照してください。

## Player threats

player ごとの観測 facts と `classify_open_hand_threat()` の結果を表示します。

```text
player 1
  opponent: yes
  reached: no
  dealer: no
  seat wind: S
  discards: 0
  melds: 2
  open melds: 2
  kans: 0
  meld kinds: Chi 1, Pon 1
  meld dora: 2
  meld red dora: 1
  open meld dora: 2
  open meld red dora: 1
  open confirmed value honor: 1
  open visible han proxy: 3
  open hand threat: High
  open hand threat reason: TwoOrMoreWithVisibleHan
```

`meld dora` などは暗槓を含む fixed meld 全体、`open meld dora` などの `open` 値は公開副露だけです。`open visible han proxy` は production helper から表示します。classification の意味と条件は [押し引きと threat](ai/push-pull.md#openhandthreat) を source document とします。

`player_id` などが不明な値は推測せず `unknown` / `None` と表示します。暗槓は `melds` と `kans` には入り、`open melds` には入りません。

## Push/Pull

```text
Push/Pull
  mode: Fold
  reason: TwoOrMoreShantenAgainstHighOpenHand
  opponent reach count: 0
```

`mode` は最終 action の優先順に影響し、`reason` は offense state と threat 種類を示します。詳しい境界は [押し引きと threat](ai/push-pull.md) を参照してください。

`tenpai offense value` は打牌後テンパイを攻撃継続した場合の確定打点です。`offense mode` は既リーチ / これからリーチする手 (`Reach`) かダマにする手 (`Damaten`) か (自分が既リーチかを判断できない場合は `Unknown`)、`weighted average` は生きた待ちの支払点を残枚数で加重平均した打点、`weighted total` は割り算する前の残枚数加重合計です。確定できない場合はどちらも `unknown` になります。`strong tenpai requirement` は押すために要求する条件で、打点を確定できた非フリテンでは `weighted total >= 15600` (他家リーチ者に親が含まれる場合は `23400`)、確定できない場合と恒常フリテンでは `live wait >= 6` / `live wait >= 8` になります。打牌後がテンパイでなければどちらも評価しません。意味は [攻撃継続時の確定打点](ai/push-pull.md#攻撃継続時の確定打点) を参照してください。

`iishanten forward metrics` は、通常打牌選択が選んだ打牌が1向聴の場合の前方集計値です。`weighted prospective value` は将来テンパイの確定打点を1手目と最終和了牌の残枚数で重み付けした合計、`weighted tenpai wait` はその枝のテンパイ待ちの残枚数・種類数です。production の打牌選択が比較に使った値をそのまま表示し、表示用に2手先探索も打点集計もやり直しません。平均へ正規化した値でも局収支EVでもありません。打点を確定できない枝がある場合は `unknown` で、0点として扱いません。打牌後が1向聴でなければ `none` です。現在の押し引き判断はこの値を使いません。

`dora after discard` / `red dora after discard` / `value honor han proxy after discard` / `simple value proxy after discard` は、打牌後の concealed hand と自分の確認できている fixed meld (暗槓を含む) の両方を数えた簡易打点 proxy です。production の `PushPullOffenseState` をそのまま表示し、表示用に数え直しません。正確な打点ではなく、一般役・符・点数計算は含みません。意味は [簡易打点 proxy](ai/push-pull.md#簡易打点-proxy) を参照してください。

## Reach

`Reach` は通常打牌で選んだ牌を切った後のテンパイ形に基づく判断です。押し引きが `Push` のときだけ評価し、それ以外は `not evaluated` です。

```text
Reach
  evaluated
  decision: no
  reason: InsufficientLiveWait
  selected discard: N
  shanten: 0
  live wait: 2 remaining / 1 types
  permanent furiten: no
  history furiten after discard: same turn false / riichi missed win false
  ron: yes
  tenpai waits: 5s
  live tenpai waits: 5s
  discarded waits: none
```

`tenpai waits` は構造上の待ち、`live tenpai waits` は見え牌を反映して残っている待ちです。`ron` は恒常フリテンと `history furiten after discard` を合わせた総合ロン可否です。フリテンについては [フリテン](ai/furiten.md) を参照してください。

## Defense

リーチ者向けの候補を `Genbutsu`、字牌 safety、壁、スジなどで表示します。`selected` は production fallback が採用した候補です。詳しい safety と優先順は [防御](ai/defense.md#riichi-defense) を参照してください。

## OpenHand defense

High OpenHandThreat の target と候補ごとの safety を表示します。

```text
OpenHand defense
  targets: 1, 3
  selected action: 5m
  selected category: SafeAgainstAllTargets
```

主な行:

| 行 | 内容 |
| --- | --- |
| `targets` | High の相手。いなければ `none` |
| `discarded by target[n]` | target 自身の河に同じ牌種があるか |
| `discarded by all targets` | 全 target 自身の河にあるか |
| `ron safe[n]` | 本人の河または現在有効な一時通過牌により target にロンされないか |
| `ron safe for all targets` | 全 target にロンされないか |
| `honor safety` | 字牌の見え枚数による safety |
| `opponent honor value` | まだロン可能な target に対する最も危険な役牌価値 |
| `wall` | 壁 / ワンチャンス |
| `suji safety[n]` / `suji safety` | target 個別 / 集約後のスジ safety |
| `category` | `SafeAgainstAllTargets` / `HonorSafety` / `SuitedSafety` |

`discarded by *` は target 本人の河だけを表す観測事実、`ron safe *` は本人の河と現在有効な一時通過牌を合わせたロン安全性です。`SafeAgainstAllTargets` の source of truth は後者なので、`discarded by all targets: no` と同時に成立することがあります。

target 選択と `post_reach_passed` / `temporary_passed` の違いは [OpenHand Defense](ai/defense.md#openhand-defense) を参照してください。

## Combined defense

リーチ者と High OpenHandThreat が同時にいる場合の target と候補を表示します。

```text
Combined defense
  targets: 1(Riichi), 3(HighOpenHand)
  selected action: 5m
  selected category: SafeAgainstAllThreats
```

`ron safe[n kind]` は target ごとの根拠でロン安全か、`safe against all threats` は全 target に安全かを示します。リーチ者と副露相手では根拠が異なります。詳細は [Combined Defense](ai/defense.md#combined-defense) を参照してください。

## Summary と runner-up

出力末尾で最終選択と次点を確認できます。

```text
Summary
  selected: 7s
  source: DefenseFallback
  selected detail: SuitedSafety(Suji)
  runner-up: 4p
  runner-up source: DefenseFallback
  runner-up detail: SuitedSafety(HalfSuji)
```

`runner-up` は最終選択を除いた場合に次に選ばれる候補です。存在しない場合は `-` です。`selected` と `runner-up` は同じ source とは限りません。

## Table state と History furiten

取得できない table state は `unknown` と表示し、観測済みの `0` と区別します。現在は AI policy へ使用していません。入力 schema は [bot-scenario](bot-scenario.md#table-state) を参照してください。

`History furiten` section は**現在時点 (今回の打牌の前)** の履歴依存フリテンを known / unknown のまま表示します。

```text
History furiten
  same turn: true
  riichi missed win: false
```

ロン可否の判定に使うのは、これを**打牌後**へ補正した facts です。補正後の値は打牌候補と `Reach` の `history furiten after discard` 行に出ます。自分のツモを経た打牌では同巡内フリテンが解除されるため、

```text
History furiten
  same turn: true
...
  history furiten after discard: same turn false / riichi missed win false
  ron: yes
```

のように現在時点が `true` でもロンできる、という組み合わせが正常な出力になります。詳しい規則は [フリテン](ai/furiten.md) を参照してください。
