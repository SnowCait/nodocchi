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
| `Tenpai continuation` | 現在聴牌候補のダマ継続 (diagnostics only) |
| `Player threats` | player ごとの reach / meld facts と OpenHandThreat classification |
| `Push/Pull` | threat と offense を組み合わせた押し引き |
| `Reach` | 通常打牌後のテンパイに対するリーチ判断 |
| `Reach / Damaten comparison` | Reach / Damaten の判断材料をまとめた統合観測 (diagnostics only) |
| `Defense` | リーチ者向け防御候補のうち採用したもの |
| `Defense candidates` | 全合法 Dahai の防御評価 |
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
| `Hora` / `Ryukyoku` / `Reach` | 和了、九種九牌、リーチ |
| `NormalDiscard` | 通常打牌 selector |
| `DefenseFallback` | リーチ者向け防御 fallback |
| `OpenHandDefenseFallback` | High OpenHandThreat 向け fallback |
| `CombinedThreatDefenseFallback` | 複合 threat 向け fallback |
| `Call` | 鳴き判断で選んだ Chi / Pon |
| `LegalDahaiFallback` / `None` | 上位判断で選べない場合の fallback |

防御 source では category や kind も表示されます。

`Call` の `iishanten self-tsumo` は、鳴き後も1向聴になる候補について production が比較した
`Pass ExpectedSelfTsumoValue / Call ExpectedSelfTsumoValue` です。`unknown` は reaction 元、山の
残枚数、または terminal scoring を確定できないことを表し、0点として扱いません。Call が Pass
より厳密に高い場合だけ鳴き、同値では Pass を維持します。`iishanten acceptance` は引き続き観測用で
policy には使いません。`Summary` にも同じ値、比較結果、鳴き後の採用打牌を表示します。

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

候補ごとの `expected self-tsumo value` は1向聴、`two-shanten progress self-tsumo value` は
2向聴 production の第1段、`two-shanten full self-tsumo value` はドラ差 gate 対象 pair の
再比較に実際に使った期待支払い [点] です。Progress と Full は別 field で、表示用に
探索も点数計算もやり直しません。軸を使えない局面と、確定できない候補は `unknown` です。

`--verbose` は候補の詳細、`--lookahead` は2手先の概要を追加します。2手先の概要は仮想ツモ牌を向聴数が下がるもの (`draws`) と維持するもの (`same-shanten`) に分けて種類数と残枚数を表示し、`--verbose` の牌ごとの詳細では `transition` にその分類を表示します。`--lookahead --verbose` では、1向聴候補の向聴数を維持する枝だけをテンパイまでもう1段追い、`downstream value` と候補ごとの `same-shanten downstream value` を追加します。指標と comparator の読み方は [打牌選択](ai/discard-selection.md) を参照してください。

`Lookahead` の節の先頭には `self-tsumo continuation` として、その局面で共通の `unknown tiles` (自分から見て未確認の物理牌) と `current future own draws` (現在打牌後に自分へ残っている自摸機会) を表示します。材料が揃わない局面では `not evaluated` です。テンパイへ到達した枝には `tsumo continuation` として、`path probability` (その経路を引く確率) と `terminal tenpai` の内訳 (`mode` / `unknown tiles` / `future own draws` / `winning variants` / `hit probability` / `weighted tsumo payment` / `continuation value`)、およびその経路分の `path continuation value` を表示します。確率は 0〜1 の小数、打点は点数です。

## Two-shanten expected self-tsumo value

`--two-shanten-self-tsumo` は、打牌候補集合の最善向聴数が2向聴の場合に、候補ごとの ExpectedSelfTsumoValue [点] を表示します。節の先頭には `Lookahead` と同じ `self-tsumo continuation` (`unknown tiles` / `current future own draws`) を出し、材料が揃わない局面では `not evaluated`、値を確定できない候補は `unknown` です。

枝は `2向聴 → (Progress / 一度だけの SameShanten) → 1向聴 → 既存の1向聴 continuation` で、確率も打点も1向聴の `expected self-tsumo value` と同じ尺度です。この section は解析用の Full 全候補診断です。production は ForwardTargets cohort 全体を Progress-only で順位付け、上位2候補の Progress-only 値が strict に異なり、かつ `discarded_dora_count` が異なる場合だけ Full で pairwise 再比較します。Progress と Full を同じ全候補配列に混ぜません。起点の向聴数が違うため1向聴の値と同じ field にも混ぜません。読み方は [打牌選択](ai/discard-selection.md#2向聴-expectedselftsumovalue) を参照してください。

`--lookahead --verbose` の same-shanten downstream とは独立した診断で、互いに含みません。`--two-shanten-self-tsumo` 単独では downstream 探索は走らず、両方出す場合は `--two-shanten-self-tsumo --verbose` を指定します。

```text
Two-shanten expected self-tsumo value
  self-tsumo continuation
    unknown tiles: 122
    current future own draws: 16
  5m: 125.188
  8m: 133.548
  9s: 131.729
```

## Tenpai continuation

`--lookahead` は、現在打牌後が聴牌になる候補について `現在聴牌 → 非和了ツモ → 最善打牌 → 再び聴牌` の枝を表示します。通常表示は候補ごとに `current wait` (現在聴牌の待ちと残枚数) と `continuation branches` (成立した枝の数と残枚数合計) だけで、`--verbose` で枝ごとに `drawn` (非和了ツモの物理牌と残枚数) / `next discard` / `new wait` / `mode` / `prospective value` を出します。待ちが変わる枝と、ツモ切りで元の待ちを維持する枝の両方を含みます。

枝は既存2手先評価の枝のうち実戦上の非和了ツモ (向聴数を維持する牌と、構造上は和了形でもダマツモでは役が無く和了できない牌)、次打牌は既存 comparator の選択、打点は既存の将来打点評価の値そのもので、この節のために探索も点数計算もやり直しません。ダマツモで実際に和了できる牌を引いた枝は含みません。既にリーチしている局面と、自分の席が分からず未リーチかどうかを判断できない局面では節そのものを出しません。

候補ごとに `self-tsumo comparison` として、「今すぐリーチ」と「ダマで1巡継続」を同じ期待ツモ支払いで並べます。

```text
Tenpai continuation
  3s
    current wait: 4s(3) / 3 remaining
    continuation branches: 35 / 119 remaining
    self-tsumo comparison
      reach now: 1460.235
      damaten continuation: 2094.467
      damaten immediate tsumo: 49.180
      damaten after non-winning draw: 2045.286
```

| 行 | 意味 |
| --- | --- |
| `reach now` | 今リーチして手変わりせず、残り自摸機会全体でツモ和了する期待支払い |
| `damaten continuation` | ダマで1巡継続した場合の合計。下の2行の和 |
| `damaten immediate tsumo` | ダマのまま最初の1自摸で現在の待ちをツモ和了する期待支払い |
| `damaten after non-winning draw` | 非和了牌を引いて手変わりした先の terminal tenpai の期待支払い合計 |

どれも `Lookahead` の `self-tsumo continuation` と同じ確率模型・同じ単位 (点数) で、`unknown tiles` と `current future own draws` もその節に表示した局面共通の値をそのまま使います。山の残枚数が分からない局面など、材料が揃わない値は 0 ではなく `unknown` です。合法手に `LegalAction::Reach` が無い局面の `reach now` も `unknown` です (現在局面のリーチ可否は production のリーチ判断と同じく実際の合法手が source of truth です)。**この比較にも winner や `should_reach` はありません。**

**この節の値は打牌選択・押し引き・リーチ判断のどれにも使いません。** 現時点では diagnostics 専用で、selection には接続していません。判断への接続方針は [打牌選択](ai/discard-selection.md#現在聴牌のダマ継続-diagnostics-only) を参照してください。

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

`iishanten forward metrics` は、通常打牌選択が選んだ打牌が1向聴の場合の前方集計値です。`expected self-tsumo value` は経路確率とテンパイ到達後の期待ツモ支払いを掛けた期待値 [点]、`weighted prospective value` は将来テンパイの確定打点を1手目と最終和了牌の残枚数で重み付けした合計、`weighted tenpai wait` はその枝のテンパイ待ちの残枚数・種類数です。前2つは別の尺度なので、区別して読んでください。production の打牌選択が比較に使った値をそのまま表示し、表示用に2手先探索も打点集計もやり直しません。平均へ正規化した値でも局収支EVでもありません。打点を確定できない枝がある場合は `unknown` で、0点として扱いません。打牌後が1向聴でなければ `none` です。現在の押し引き判断はこの値を使いません。

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

`damaten verdict` はダマ打点から畳んだ結論だけで、待ちごとの実点数は下の `Reach / Damaten comparison` がリーチ Ron baseline と並べて出します。

`base reason` が `NamedYakumanDamaten` の場合は、恒常フリテンで全ての生きた Tsumo variant が named 役満と確定したためのダマです。ダマ打点 threshold の `HighValueDamaten` とは別の理由で、`damaten verdict` を持たず timing も評価しません。違いは [打牌選択](ai/discard-selection.md#base-reach--damaten-policy-の-categorical-rule) を参照してください。

## Reach / Damaten comparison

`Reach / Damaten comparison` は、選んだ打牌1件について Reach / Damaten の判断材料を1か所へ並べた観測です。`Reach` と同じく押し引きが `Push` のときだけ評価します。

```text
Reach / Damaten comparison
  diagnostics only, not connected to the Reach / Damaten decision
  discard: 3s
  production
    reason: EligibleNoDamatenYaku
    selected: yes
  self-tsumo (expected tsumo payment)
    reach now: 1460.235
    damaten continuation: 2094.467
    damaten immediate tsumo: 49.180
    damaten after non-winning draw: 2045.286
  Ron (payment when winning, no ron probability)
    reach legal: yes
    reach baseline
      weighted average: 2600
      weighted total: 7800
      4s: 3 remaining / 2600
    can ron: yes
    damaten baseline
      verdict: NoYaku
      4s: 3 remaining / no yaku
```

`production` は既存のリーチ判断の結論そのものです。`self-tsumo` は選んだ打牌に対応する `Tenpai continuation` の候補1件の比較そのもので、`--lookahead` を指定していない局面では `self-tsumo: unavailable` になります (統合表示のために2手先探索を追加しません)。`reach baseline` だけが今回新しく評価する観測値で、その点数計算は診断経路だけで行います。

`Ron` の2つの baseline はどちらも「その待ちで和了した場合の支払い」で、**ロンの発生確率を含みません**。`reach baseline` は今リーチした場合の最低保証打点 (リーチ1翻を含み、一発・裏ドラ・河底は加算しない) で、合法手に `LegalAction::Reach` があり、かつ `TenpaiWaitAvailability::can_ron() == Some(true)` の場合だけ評価します。リーチが非合法、フリテン、ロン可否 unknown の場合は `unavailable` です。`damaten baseline` は production 判断が評価したダマ打点そのもので、ダマでロンできない局面とロン可否が unknown の局面では評価しないまま `unavailable` になります (0 点として扱いません)。

**self-tsumo と Ron baseline は単位の違う別の軸で、足した合計は出しません。** フリテンやロン可否 unknown で Ron baseline が `unavailable` でも、self-tsumo はその軸の入力があれば引き続き評価します。winner も新しい `should_reach` も持たず、production の Reach / Damaten 判断には接続していません。詳細は [打牌選択](ai/discard-selection.md#reach--damaten-の判断材料-diagnostics-only) を参照してください。

## Defense

`Defense` は採用した防御候補、`Defense candidates` は全合法 Dahai の防御評価です。`selected` は production fallback が採用した候補そのもので、表示側で選び直しません。safety と優先順の定義は [防御](ai/defense.md#riichi-defense) を参照してください。

### selected kind

`selected kind` は採用した `DefenseFallbackKind` です。

| kind | 意味 |
| --- | --- |
| `Genbutsu` | 全リーチ者に共通する現物。単独・複数どちらでも最優先 |
| `ExactRonRisk` | exact hidden-hand model で選んだ候補。単独・複数どちらのリーチでも出る |
| `HonorSafety(rank)` | legacy path の字牌 safety |
| `SuitedSafety(rank)` | legacy path の数牌 safety (壁 / スジ) |

複数リーチでも、全リーチ者の exact model が利用可能なら `ExactRonRisk` になります。共通現物があれば従来どおり `Genbutsu` が最優先です。リーチ者の1人でも exact model が使えない局面は局面全体が legacy へ落ちるので、`HonorSafety(...)` / `SuitedSafety(...)` になります。同じ値は `Summary` の `defense detail` にも出ます。

### 単独リーチの exact ron risk evidence

単独リーチで exact model が使えた候補には、`Defense` と `Defense candidates` の両方に次の2行が出ます。

```text
4p
  selected: yes
  ...
  ron capable weight: 12345
  tenpai weight: 678901
```

| 行 | 意味 |
| --- | --- |
| `ron capable weight` | `R(p, x)`。その候補で現在ロン可能な hidden-hand states の physical weight |
| `tenpai weight` | `T(p)`。structural tenpai hidden-hand states 全体の physical weight |

同じ単独リーチ局面の候補どうしでは `tenpai weight` が共通なので、

```text
ron capable weight が小さい候補
=
exact model 上でより安全
```

と読めます。ただしこれは実放銃率ではありません。`ron capable weight / tenpai weight` を割合として読む場合も、あくまで combinatorial hidden-hand model 上の比率です。現物は `ron capable weight: 0` になります。定義は [リーチ者ごとの exact ron risk](ai/defense.md#リーチ者ごとの-exact-ron-risk) を参照してください。

共通現物がない局面では、この exact evidence は production selection が比較へ使ったものそのものです。共通現物を採用した局面でも候補ごとの evidence を表示しますが、そちらは diagnostics 表示のための追加収集で、選択結果を変えません。

### 複数リーチの player 別 exact evidence

複数リーチで全リーチ者の exact model が使えた場合、`Defense` と `Defense candidates` にリーチ者ごとの行が出ます。

```text
Defense
  evaluated
  selected action: 4p
  selected kind: ExactRonRisk
  opponent reach count: 2
  ...
  ron capable weight: -
  tenpai weight: -
  player 1 ron risk: 153021210679 / 4886615584793
  player 2 ron risk: 234842796892 / 4886615584793

Defense candidates

4p
  selected: yes
  ...
  player 1 ron risk: 153021210679 / 4886615584793
  player 2 ron risk: 234842796892 / 4886615584793

7s
  selected: no
  ...
  player 1 ron risk: 203424857360 / 4886615584793
  player 2 ron risk: 287210037589 / 4886615584793
```

形式は `player {id} ron risk: {ron_capable_weight} / {tenpai_weight}` で、左が `R(p, x)`、右が `T(p)` です。`T(p)` はリーチ者ごとに異なり得るので、左側の physical weight を player をまたいで直接比べても意味がありません。player 間は `R/T` の比として読んでください。

`ron capable weight` / `tenpai weight` は単独リーチ向けに残している backward-compatible な単一値です。複数リーチではこの2行が `-` になりますが、これは**「exact evaluation が無い」という意味ではありません**。`selected kind: ExactRonRisk` と player 別の行が出ていれば exact path です。複数リーチでは player 別の行を確認してください。

#### minimax の読み方

player 別の行は player id 順に並べた表示で、comparator の優先順ではありません。production は候補ごとにリーチ者の risk を危険な順へ並べ直し、その vector を辞書順で比較して最も小さい候補を選びます。上の例では `4p` / `7s` とも player 2 のほうが危険なので、まず player 2 の `R/T` どうしを比べ、同率なら player 1 を比べます。定義は [複数リーチの worst-first lexicographic minimax](ai/defense.md#複数リーチの-worst-first-lexicographic-minimax) を参照してください。

### exact model が使えない場合

リーチ者の1人でも exact model が使えない局面は、partial exact と partial legacy を混在させず局面全体が legacy fallback になります。この場合は `ron capable weight` / `tenpai weight` がどちらも `-` になり、player 別の行も出ません。部分的な exact evidence を selection の根拠として読まないでください。

`genbutsu` / `honor safety` / `opponent honor value` / `wall` / `suji` / `suji safety` / `suited safety` は、`ExactRonRisk` で選んだ候補にも表示されます。これらは従来の safety evidence を観察するための診断情報で、`ExactRonRisk` minimax の第2 key や tie-break ではありません。

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
| `ron safe[n]` | 本人の河または現在有効な一時通過牌により target にロンされないか (hard-safe) |
| `ron safe for all targets` | 全 target にロンされないか |
| `same hand passed[n]` | target の concealed hand が最後に変化して以降にこの牌が通ったか。成立時だけ表示 |
| `same hand passed for all targets` | 全 target が hard-safe または same-hand passed で覆われるか。成立時だけ表示 |
| `player n ron risk: R / T` | `ExactRonRisk` 使用時の target ごとの exact evidence |
| `honor safety` | 字牌の見え枚数による safety |
| `opponent honor value` | hard-safe でも same-hand passed でもない target に対する最も危険な役牌価値 |
| `wall` | 壁 / ワンチャンス |
| `suji safety[n]` / `suji safety` | target 個別 / 集約後のスジ safety |
| `category` | `SafeAgainstAllTargets` / `SameHandPassed` / `ExactRonRisk` / `HonorSafety` / `SuitedSafety` |

`discarded by *` は target 本人の河だけを表す観測事実、`ron safe *` は本人の河と現在有効な一時通過牌を合わせたロン安全性です。`SafeAgainstAllTargets` の source of truth は後者なので、`discarded by all targets: no` と同時に成立することがあります。

`same hand passed *` は hard-safe とは別の evidence で、成立している場合だけ行が出ます。行が無いことは「通っていない」と「履歴が unknown」のどちらでもあり得ます。`ron safe[n]: no` と `same hand passed[n]: yes` が同時に出るのは正常で、全 target がこの2つで覆われていれば `category` は `SameHandPassed` になります。

`ExactRonRisk` では selected header と各候補に target ごとの `player n ron risk: R / T` が出ます。target の1人でも exact model unavailable なら局面全体が既存 heuristic fallback へ戻り、この行は出ません。

```text
2s
  selected: yes
  discarded by all targets: no
  ron safe for all targets: no
  same hand passed for all targets: yes
  discarded by target[3]: no
  ron safe[3]: no
  same hand passed[3]: yes
  ...
  category: SameHandPassed
```

`same hand passed` の履歴を持つ入力では、`Scenario` section にも `same hand passed[n]` が牌種で出ます。現在この履歴を供給するのは RiichiLab live client だけで、JSON scenario と capture replay には入力 field が無いため unknown です。unknown の局面ではこれらの行が出ず、`category` も `SameHandPassed` になりません。

target 選択と `post_reach_passed` / `temporary_passed` / `same_hand_passed` の違いは [OpenHand Defense](ai/defense.md#openhand-defense) と [passed tile の区別](ai/defense.md#passed-tile-の区別) を参照してください。

## Combined defense

リーチ者と High OpenHandThreat が同時にいる場合の target と候補を表示します。

```text
Combined defense
  targets: 1(Riichi), 3(HighOpenHand)
  selected action: 5m
  selected category: SafeAgainstAllThreats
```

`ron safe[n kind]` は target ごとの根拠でロン安全か、`safe against all threats` は全 target に安全かを示します。リーチ者と副露相手では根拠が異なります。

`same hand passed[n kind]` と `same hand passed for all threats` は OpenHand defense と同じ evidence で、成立時だけ出ます。same-hand passed を根拠にできるのは `HighOpenHand` の target だけなので、`Riichi` の target にはこの行が出ません。`selected category` は `SafeAgainstAllThreats` / `SameHandPassed` / `HonorSafety` / `SuitedSafety` です。

詳細は [Combined Defense](ai/defense.md#combined-defense) を参照してください。

## Ryukyoku (九種九牌)

`LegalAction::Ryukyoku` が合法だった局面だけ、`Summary` に宣言 / 続行の結論と判断に使った向聴数が出ます。

```text
  ryukyoku: continue
  ryukyoku shanten: standard 4 / chiitoitsu 5 / kokushi 2
```

```text
  ryukyoku: declare
  ryukyoku shanten: standard 5 / chiitoitsu 4 / kokushi 4
```

`ryukyoku` は `declare` (九種九牌を宣言) か `continue` (宣言せず打牌判断へ進む) です。`ryukyoku shanten` は現在の自摸後手牌の通常手・七対子・国士の向聴数で、production が判断に使った [`calculate_shanten()`](ai/discard-selection.md) の結果そのものです。表示のために数え直しません。

**九種九牌が合法かどうかは入力側 (server / scenario) が source of truth** で、nodocchi は成立条件を再判定しません。したがってこの行は「九種九牌が成立しているか」ではなく「合法な九種九牌を宣言するか」を表します。合法でない局面には行そのものを出しません。Hora が同時に合法な局面は Hora が優先され、九種九牌を検討しないので行が出ません。

自摸牌が分からないなどで自摸後手牌を復元できない局面では、向聴数が `unknown` になり結論は `declare` を維持します。

```text
  ryukyoku: declare
  ryukyoku shanten: standard unknown / chiitoitsu unknown / kokushi unknown
```

条件と threshold は [麻雀 AI の概要](ai/overview.md#九種九牌-ryukyoku) を source document とします。

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
