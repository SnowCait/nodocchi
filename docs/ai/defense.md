# 防御

防御は threat の種類に応じて3つの経路を持ちます。target の決め方とロン安全の根拠を分け、Push/Pull が `Fold` の場合に防御 fallback を通常打牌より優先します。

## Riichi Defense

他家リーチ者に対する `Defense` です。リーチ者が1人でも複数でも、全リーチ者の exact model が利用可能なら exact ron-risk path を通ります。

| 局面 | 優先順 |
| --- | --- |
| 共通 | 全リーチ者への共通 Genbutsu が最優先 |
| 単独リーチ (exact 利用可) | Genbutsu → そのリーチ者の exact `R/T` 比較 |
| 複数リーチ (全員 exact 利用可) | Genbutsu → リーチ者ごとの exact `R/T` を worst-first に並べた lexicographic minimax |
| 1人でも exact 利用不可 | Genbutsu → 局面全体を legacy HonorSafety / wall / Suji ordering |

exact path はリーチ者を1人ずつ既存の single-player hidden-hand model で評価します。単独リーチはその評価が1要素になった場合、複数リーチは要素が2〜3個の vector になった場合です。exact hidden-hand model を使うのは Riichi Defense だけで、[OpenHand Defense](#openhand-defense) / [Combined Defense](#combined-defense) では使いません。

### Genbutsu

リーチ者本人の河と、リーチ成立後に他家から切られて通った `post_reach_passed` の両方を現物とします。**全リーチ者に共通する現物**は単独・複数どちらでも最優先で、exact minimax より先に選びます。

一部のリーチ者にだけ現物の牌は、この共通 Genbutsu にはなりません。exact path ではそのリーチ者に対する `R` が0になり、他のリーチ者の risk と並んで risk vector の1要素になります。

### リーチ者ごとの exact ron risk

共通現物がない場合の選択です。数牌と字牌を分けず、同じ exact hidden-hand model の上で比較します。「両面は何点」「嵌張は何点」のような固定の wait-shape coefficient は持ちません。

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

`T(p)` は target に依存しないので、リーチ者ごとに1回だけ計算します。複数リーチでも、この `T(p)` / `R(p, x)` はリーチ者 `p` 単独の model から求めます。

#### `R/T` が表すもの

`R(p, x) / T(p)` は、**この exact combinatorial hidden-hand model 上で、リーチ者 `p` が target `x` で現在ロン可能な hidden-hand state の比率**です。リーチ者ごとの individual exact structural risk evidence であり、複数リーチでも player ごとに独立した量として保持します。

実放銃率でも実際のロン確率でもなく、牌譜統計から求めた empirical probability でも、相手の打ち方を表す opponent behavior probability でもありません。model は behavioral prior も牌譜統計も持たず、「公開情報と矛盾しない隠れ手牌」を物理牌の組み合わせ数として数えているだけです。

#### 単独リーチの比較

同じ単独リーチ者・同じ局面では `T(p)` が全 target で共通です。そのため production selection は

```text
ron_capable_weight R(p, x)
```

の小さい候補をそのまま安全と比較できます。候補ごとに浮動小数点の `R/T` を計算してはいません。同じ牌種は1回だけ評価し、赤5と黒5は同じ evidence を共有します。

#### 複数リーチの worst-first lexicographic minimax

リーチ者が2人以上の場合、候補 `x` についてリーチ者ごとの exact risk を個別に求めます。

```text
player A: R(A, x) / T(A)
player B: R(B, x) / T(B)
player C: R(C, x) / T(C)
```

`T(p)` はリーチ者ごとに異なるので、`R(A, x)` と `R(B, x)` のような raw physical weight を player をまたいで直接比較してはいけません。player 間の比較は必ず `R/T` の exact ratio comparison (`RonRiskEvidence::compare_ratio()`) で行います。

候補ごとに、そのリーチ者たちの risk を**危険な順**へ並べます。

```text
[worst, second-worst, third-worst]
```

この vector を候補どうしで辞書順に比較し、最小の候補を選びます。

```text
候補 A: [20%, 5%]
候補 B: [12%, 10%]
候補 C: [18%, 3%]

→ B
```

`%` は説明のための表記です。production は比率を浮動小数点へ変換せず、`compare_ratio()` の cross multiplication で exact に比較します。1組でも比較不能なら値を推測せず、局面全体を legacy fallback へ落とします。

この policy が最小化するのは、**最も危険なリーチ者に対する individual exact structural risk** です。それが同率なら2番目、さらに同率なら3番目を比べます。次のいずれでもありません。

- リーチ者ごとの risk の単純和
- 平均 / 加重平均
- `1 - Π(1 - p)` のような独立事象の合成
- リーチ者どうしが独立という仮定
- joint hidden-hand probability

また、複数リーチの exact path はリーチ者を1人ずつ single-player hidden-hand model で評価したものです。複数リーチ者の隠れ手牌を同じ unknown 物理牌 pool から同時に割り当てる joint hidden-hand exact model ではありません。joint model を独立確率で近似しているのでもなく、joint な量を作らずに individual risk の minimax で比較しています。

単独リーチはこの vector が1要素になった場合にすぎず、結果は [単独リーチの比較](#単独リーチの比較) と同じです。

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

exact path を使うのは、**全リーチ者**の exact model が利用可能な場合だけです。1人でも

- 対象がリーチしていない、副露を持つ、固定面子が多すぎる、player を取得できないなど model の前提と矛盾する unsupported state
- denominator `T(p)` が0
- `R > T` のような model invariant との矛盾
- exact ratio comparison が不能

になった場合は、推測で補完せず、partial exact と partial legacy を混在させもせず、**局面全体**を [legacy safety fallback](#legacy-safety-fallback) へ落とします。通常のリーチ局面では exact path を使います。

### legacy safety fallback

exact model が利用できない場合の従来 selection です。全リーチ者の exact model が揃わない限り、単独リーチでも複数リーチでも局面全体がこの経路になります。現物の次は次の safety で比較します。

- HonorSafety
- wall / one-chance
- Suji / HalfSuji

この経路では joint hidden-hand exact model も、リーチ者ごとの `R/T` の和・max・independence approximation も計算しません。

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
2. `SameHandPassed`
3. `HonorSafety`
4. `SuitedSafety`

第一分類 `SafeAgainstAllTargets` は、本人の河または現在有効な一時通過牌によって全 target にロンされない牌 (hard-safe) です。「全 target 自身の河にある」という意味ではありません。

`SameHandPassed` は、全 target が hard-safe または same-hand passed で覆われ、少なくとも1人は same-hand passed だけを根拠とする牌です。same-hand passed は hard-safe ではないので第一分類には入れず、字牌・数牌の heuristic より先に選びます。候補が複数ある場合は hard-safe な target 数が多いものを優先し、同数なら合法 Dahai の元順序を維持します。根拠の違いは [passed tile の区別](#passed-tile-の区別) を参照してください。

字牌・役牌価値・壁・スジは legacy Riichi Defense と同じ helper を共有します。複数 target の集約では、その牌が hard-safe な target と same-hand passed のある target を除いた相手のうち、最も危険な評価を採ります。

数牌は `NoChance` → `OneChance` → `Suji` → `HalfSuji` の順で fallback を探し、`NoSafety` だけなら選びません。選べる防御候補がない場合は通常打牌へ戻ります。

exact hidden-hand model はここでは使いません。リーチ者ごとの exact path は Riichi Defense 限定です。

## Combined Defense

リーチ者と High OpenHandThreat が同時に存在する複合 threat で使います。target には種類 `Riichi` / `HighOpenHand` を保持し、全 target にロン安全なら `SafeAgainstAllThreats` とします。

候補の大分類は次の順です。

1. `SafeAgainstAllThreats`
2. `SameHandPassed`
3. `HonorSafety`
4. `SuitedSafety`

`SameHandPassed` の条件は OpenHand Defense と同じで、全 target が hard-safe または same-hand passed で覆われ、少なくとも1人は same-hand passed だけを根拠とする牌です。same-hand passed を根拠にできるのは `HighOpenHand` の target だけで、`Riichi` の target には適用しません。候補が複数ある場合は hard-safe な target 数が多いものを優先し、同数なら合法 Dahai の元順序を維持します。

その牌が hard-safe な target と same-hand passed のある target は、それより弱い heuristic の集約から除き、その相手の無スジや役牌価値を持ち込みません。wall は見え牌由来なので全 target で共有します。

OpenHand Defense と同じく、複合 threat でも exact hidden-hand model は使いません。

## target ごとのロン安全根拠

ここは3経路で混同しない重要な差です。

| target | hard-safe の根拠 |
| --- | --- |
| `Riichi` | 本人の河 + `post_reach_passed` |
| `HighOpenHand` | 本人の河 + 現在有効な `temporary_passed` |

`post_reach_passed` は「リーチ成立後に通った」というリーチ固有の事実で、リーチ者の手牌が変化しないため局中継続します。`temporary_passed` は非リーチを含む各 player について「一時フリテンで現在ロンできない」事実で、対象 player の次のツモ、chi / pon / daiminkan / ankan / kakan で消えます。両者は寿命が異なる別 state で、前者を非リーチ副露相手へ流用しません。

hard-safe ではない `same_hand_passed` はこの表に入りません。区別は [passed tile の区別](#passed-tile-の区別) を参照してください。

exact model が使うロン不能牌もこの `Riichi` の根拠と同じで、リーチ者本人の河と `post_reach_passed` です。

### passed tile の区別

「通った牌」は3種類あり、意味・強さ・寿命がそれぞれ違います。

| state | 意味 | hard-safe | 失効 | 使う target |
| --- | --- | --- | --- | --- |
| `post_reach_passed` | リーチ成立後に他家から切られて通った牌 | ○ | 局中継続 | `Riichi` |
| `temporary_passed` | 一時フリテンにより現在ロンできない牌 | ○ | 対象 player の次のツモ、鳴き・槓 | `HighOpenHand` |
| `same_hand_passed` | concealed hand が最後に変化して以降に実際に通った牌 | × | 手出し、ツモ切りか不明な打牌、鳴き・槓 | `HighOpenHand` |

`temporary_passed` は、対象 player がその牌を見逃した直後で一時フリテンによりロンできない、という現在の事実です。ツモを経ると一時フリテンが解けるので、対象 player の次の draw で失効します。chi / pon / daiminkan / ankan / kakan でも消えます。

`same_hand_passed` は、対象 player の concealed hand が最後に変化して以降に実際に通った牌です。一時フリテンはすでに解けている可能性があるので hard-safe ではありません。ただし「同じ手牌のままその牌を見逃した」という観測事実なので、Wall / OneChance / Suji のような見え牌・河由来の heuristic より強い safety evidence として扱い、hard-safe の次に置きます。ツモ切りは concealed hand を変えないので維持し、手出し、ツモ切りかどうか判別できない打牌、chi / pon / daiminkan / ankan / kakan では失効します。判別できない打牌を手牌不変とは推測しません。

`post_reach_passed` はリーチ固有の hard-safe (現物) で、リーチ者の手牌が変化しないため局中継続します。`same_hand_passed` は非リーチ副露相手 (`HighOpenHand`) の evidence で、`Riichi` の target には使いません。3つは互いに流用しない別 state です。

入力方法は [bot-scenario の post_reach_passed](../bot-scenario.md#post_reach_passed) と [temporary_passed](../bot-scenario.md#temporary_passed)、出力の読み方は [Structured diagnostics](../diagnostics.md#combined-defense) を参照してください。`same_hand_passed` は RiichiLab live client が MJAI event の `tsumogiri` から積み上げる履歴で、bot-scenario の入力 field はありません。

## fallback と source of truth

selection は production selector が source of truth です。diagnostics は同じ selector の結果を `selected` として表示し、`act()` と `diagnose()` で別の防御ロジックを持ちません。リーチ者ごとの exact evidence も、選択に使ったものと同じ evaluation を表示します。`Push` では通常打牌の優先順を変えず、`Fold` のときだけ該当 threat 用 fallback を先に試します。
