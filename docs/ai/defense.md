# 防御

防御は threat の種類に応じて3つの経路を持ちます。target の決め方とロン安全の根拠を分け、Push/Pull が `Fold` の場合に防御 fallback を通常打牌より優先します。

## Riichi Defense

他家リーチ者に対する `Defense` です。リーチ者が1人か複数かで、現物の次に使う根拠が変わります。

| 局面 | 優先順 |
| --- | --- |
| 共通 | Genbutsu が最優先 |
| 単独リーチ | Genbutsu → exact hidden-hand ron risk |
| 複数リーチ | Genbutsu → 従来の HonorSafety / wall / Suji ordering |

exact hidden-hand model を使うのは単独リーチの Riichi Defense だけです。複数リーチと、[OpenHand Defense](#openhand-defense) / [Combined Defense](#combined-defense) は従来 behavior のままです。

### Genbutsu

リーチ者本人の河と、リーチ成立後に他家から切られて通った `post_reach_passed` の両方を現物とします。複数リーチでは全 target に対する安全性を集約します。現物は単独・複数どちらでも最優先で、exact model より先に選びます。

### 単独リーチの exact ron risk

リーチ者が1人で、共通現物がない場合の選択です。数牌と字牌を分けず、同じ exact hidden-hand model の上で比較します。「両面は何点」「嵌張は何点」のような固定の wait-shape coefficient は持ちません。

数えるのは、そのリーチ者が持ち得る隠れ手牌 (hidden hand) の状態です。見え牌から牌種ごとの残枚数 `remaining[t]` が決まり、隠れ手牌1状態 `H` の physical weight は、その手牌を実際の物理牌で作る組み合わせ数

```text
Π C(remaining[t], count_H[t])
```

になります。この weight で2つの量を数えます。

```text
T(p)
= 公開情報と整合する
  全 structural tenpai hidden-hand states の physical weight

R(p, x)
= そのうち target x で現在ロン可能な
  hidden-hand states の physical weight
```

`T(p)` は target に依存しないので、リーチ者ごとに1回だけ計算します。

#### `R/T` が表すもの

`R(p, x) / T(p)` は、**この exact combinatorial hidden-hand model 上で、target `x` が現在ロン可能な hidden-hand state の比率**です。

実放銃率でも実際のロン確率でもなく、牌譜統計から求めた empirical probability でも、相手の打ち方を表す opponent behavior probability でもありません。model は behavioral prior も牌譜統計も持たず、「公開情報と矛盾しない隠れ手牌」を物理牌の組み合わせ数として数えているだけです。

#### production の比較

同じ単独リーチ者・同じ局面では `T(p)` が全 target で共通です。そのため production selection は

```text
ron_capable_weight R(p, x)
```

の小さい候補をそのまま安全と比較できます。候補ごとに浮動小数点の `R/T` を計算してはいません。同じ牌種は1回だけ評価し、赤5と黒5は同じ evidence を共有します。

#### 数える手役形

structural tenpai には次の hand family を含みます。

- Standard (4面子1雀頭)
- Chiitoitsu
- Kokushi

Standard の structural waits には Ryanmen / Kanchan / Penchan / Shanpon / Tanki などが自然に含まれます。待ち形を列挙して係数を与えるのではなく、隠れ手牌に1枚足して和了形になるかどうかで決まります。

#### 重複排除

数える単位は隠れ手牌の `TileCounts` です。同じ `TileCounts` が複数の面子分解を持つ場合や、Standard と Chiitoitsu の両方に解釈できる場合でも、1つの hidden-hand state として1回だけ数えます。decomposition 数は weight に混ざりません。

#### フリテンと見え切った待ち

分母 `T(p)` は structural tenpai の state space です。リーチ者自身の河や `post_reach_passed` によってフリテンになっている手牌も、公開情報と物理的に矛盾しない限り分母には残ります。

分子 `R(p, x)` は「現在 target `x` でロン可能」な state だけです。フリテンは手牌単位の性質なので、待ちのいずれかがロン不能牌になっている state は、target 自体が通っていなくても分子から除外されます。

同じ理由で、structural wait が見え切って残り0枚でも、そのテンパイ隠れ手牌自体は成立し得るので `T(p)` には残ります。`T(p)` は「今その牌を引ける」ではなく「その隠れ手牌があり得る」を数えます。

#### exact model が使えない場合

対象がリーチしていない、副露を持つ、固定面子が多すぎる、player を取得できないなど model の前提と矛盾する入力では、推測で補完せず legacy Riichi Defense へ fallback します。denominator `T(p)` が0の場合や、`R > T` のように model invariant と矛盾する evidence が出た場合も同じく fallback します。通常の単独リーチでは exact path を使います。

### 複数リーチと legacy fallback

リーチ者が2人以上いる場合、exact hidden-hand model は使いません。joint hidden-hand exact model も、各リーチ者の `R/T` の和・max・independence approximation も計算していません。現物の次は従来どおり次の safety で比較します。

- HonorSafety
- wall / one-chance
- Suji / HalfSuji

単独リーチで exact model が使えない場合も同じ経路です。

### HonorSafety

字牌は見え枚数で安全度を分類します。同じ安全 rank 内では相手にとっての役牌価値を使い、`GuestWind` → `SingleValueHonor` → `DoubleWind` の切りやすい順で比較します。不明な場風・自風を推測しません。

### Suji / HalfSuji

相手の河に基づいて数牌のスジ安全度を評価します。端寄りの片側だけが通る場合を `HalfSuji`、両側の根拠が揃う場合を `Suji` として区別します。

### wall / one-chance

見え牌から順子待ち経路を評価し、`NoChance` / `OneChance` などの wall rank を作ります。wall は target に依存しません。数牌では wall とスジを共有 helper で統合します。

## OpenHand Defense

`open hand threat: High` の非リーチ副露相手だけを target にします。classification は [OpenHandThreat](push-pull.md#openhandthreat) を共有し、Defense 側で High 条件を再実装しません。`Present` / `None`、自分、リーチ済み、player id 不明の席は target 外です。

候補の大分類は次の順です。

1. `SafeAgainstAllTargets`
2. `HonorSafety`
3. `SuitedSafety`

第一分類 `SafeAgainstAllTargets` は、本人の河または現在有効な一時通過牌によって全 target にロンされない牌です。「全 target 自身の河にある」という意味ではありません。字牌・役牌価値・壁・スジは legacy Riichi Defense と同じ helper を共有します。複数 target の集約では、まだその牌でロン可能な相手のうち最も危険な評価を採ります。

数牌は `NoChance` → `OneChance` → `Suji` → `HalfSuji` の順で fallback を探し、`NoSafety` だけなら選びません。選べる防御候補がない場合は通常打牌へ戻ります。

exact hidden-hand model はここでは使いません。単独リーチ向けの exact path は Riichi Defense 限定です。

## Combined Defense

リーチ者と High OpenHandThreat が同時に存在する複合 threat で使います。target には種類 `Riichi` / `HighOpenHand` を保持し、全 target にロン安全なら `SafeAgainstAllThreats` とします。

候補の大分類は次の順です。

1. `SafeAgainstAllThreats`
2. `HonorSafety`
3. `SuitedSafety`

ロン安全な target はその牌をロンできないため、その相手の無スジや役牌価値を集約から除きます。wall は見え牌由来なので全 target で共有します。

OpenHand Defense と同じく、複合 threat でも exact hidden-hand model は使いません。

## target ごとのロン安全根拠

ここは3経路で混同しない重要な差です。

| target | ロン安全の根拠 |
| --- | --- |
| `Riichi` | 本人の河 + `post_reach_passed` |
| `HighOpenHand` | 本人の河 + 現在有効な `temporary_passed` |

`post_reach_passed` は「リーチ成立後に通った」というリーチ固有の事実で、リーチ者の手牌が変化しないため局中継続します。`temporary_passed` は非リーチを含む各 player について「最後の手牌変化後に通った」事実で、対象 player の次のツモ、chi / pon / daiminkan / ankan / kakan で消えます。両者は寿命が異なる別 state で、前者を非リーチ副露相手へ流用しません。

単独リーチの exact model が使うロン不能牌もこの `Riichi` の根拠と同じで、リーチ者本人の河と `post_reach_passed` です。

入力方法は [bot-scenario の post_reach_passed](../bot-scenario.md#post_reach_passed) と [temporary_passed](../bot-scenario.md#temporary_passed)、出力の読み方は [Structured diagnostics](../diagnostics.md#combined-defense) を参照してください。

## fallback と source of truth

selection は production selector が source of truth です。diagnostics は同じ selector の結果を `selected` として表示し、`act()` と `diagnose()` で別の防御ロジックを持ちません。単独リーチの exact evidence も、選択に使ったものと同じ evaluation を表示します。`Push` では通常打牌の優先順を変えず、`Fold` のときだけ該当 threat 用 fallback を先に試します。
