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

exact path はリーチ者を1人ずつ既存の single-player hidden-hand model で評価します。単独リーチはその評価が1要素になった場合、複数リーチは要素が2〜3個の vector になった場合です。同じ `R/T` 表現と comparator は [OpenHand Defense](#openhand-defense) でも使いますが、[Combined Defense](#combined-defense) には接続しません。

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

### 単独リーチへの structural expected deal-in loss (diagnostics only)

`R/T` は「その牌でロンされ得る hidden-hand state の割合」だけを表し、**ロンされた場合にいくら払うか**を含みません。同じ `R/T` でも、ロン可能 state の打点分布が違えば失う点数の期待値は違います。そこで `R/T` と同じ state space・同じ weight の上で、打点まで含めた期待放銃損失を求めます。

対象は**他家リーチがちょうど1人**の局面の通常打牌1候補だけで、現時点では **diagnostics 専用**です。Push/Pull・Defense selection・打牌選択のどれにも接続しておらず、構築の有無で production の判断は変わりません。

```text
T(p)
= 公開情報と整合する
  全 structural tenpai hidden-hand states の physical weight

ExpectedDealInLoss(p, x)
= Σ_H [ w(H) * I(H が x で現在ロン可能) * Loss(H, x) ] / T(p)
```

`H` は `R/T` と同じ hidden-hand state、`w(H)` は同じ physical combination weight、`I(...)` はフリテン等を含めて実際に `x` でロンできるかどうかです。`Loss(H, x)` はその状態で `x` にロンされたときに**自分が追加で失う点数**です。

#### empirical な放銃損失ではない

これは実際の放銃率でも実際の期待失点でもありません。牌譜統計・相手の打牌傾向・経験的な放銃率や打点分布は一切使わず、確率測度は既存 `R/T` と同じ「公開情報と矛盾しない物理牌配置の組合せ重み」だけです。期待損失のために別の hidden-hand prior も probability model も作りません。

#### `Loss(H, x)` に含めるもの

打点は既存の hand-value / payment / settlement layer だけで求めます ([手牌評価](hand-value.md))。役・翻・符・親子・ロン支払点をこの layer で別実装しません。

| 含める | 含めない |
| --- | --- |
| 相手のリーチ / ダブル立直 / 一発 | 供託 (この打牌で追加で失う点ではない) |
| 場風・相手の自風 | 牌譜統計・相手の打牌傾向 |
| 通常ドラ・赤ドラ・裏ドラ | 経験的な放銃率・打点分布 |
| ロン和了時の役・符・支払点 | ツモ和了・横移動・順位 |
| 本場による自分から相手への追加支払い | 平均裏ドラ翻数のような固定係数 |

嶺上開花と槍槓は「通常の打牌でロンされる」評価そのものから `false` が確定するので、観測事実として渡します。河底は `remaining_tiles` から決まります。

#### 赤5の物理配置

`R/T` の state は牌種ごとの枚数 (`TileCounts`) なので、赤5と黒5を区別しません。一方で打点は変わるため、期待損失では state の physical weight を赤5の有無へ分解します。

赤5がまだ見えていない牌種では、残枚数のうち1枚が赤5なので

```text
C(remaining, count)
= C(remaining - 1, count - 1)   赤5を含む物理配置
+ C(remaining - 1, count)       黒5だけの物理配置
```

へ正しい組合せ weight で分かれます。赤5が既に見えている牌種は黒5だけの配置になります。分解した weight の合計は元の weight と一致しなければならず、一致しない場合は値を返しません。切る牌そのものが赤5の場合も、実際の物理牌 (`TileId`) をそのまま scoring へ渡します。

#### 裏ドラ

「平均裏ドラ○翻」のような統計値や固定係数は使いません。リーチ者の hidden hand `H` を仮定したあとに残る unseen physical tile pool から、現在のドラ表示牌数と同じ数の裏ドラ表示牌 slot の配置をすべて数え上げ、physical combination weight で平均します。

```text
Loss(H, x) = E_U[ ron payment(H, x, ura indicators U) ]
```

裏ドラ判定は既存 scoring の ura-dora 判定そのものです。未知の裏ドラを0翻に固定することも、確率で近似することもしません。`H` を除いた未知牌の枚数は state に依らず一定なので、裏ドラ配置の総 weight も局面ごとに一定です。

#### リーチ状況依存役と unavailable

リーチが通常立直かダブル立直か、その和了が一発になるかは `reached` からは分かりません。どちらも event 履歴から復元できる事実として player ごとに保持し ([`RiichiSituationFacts`](../../crates/bot-core/src/context.rs))、確定できない場合は**そのケースの期待損失を `unavailable`** にします。通常立直と決め打つ、一発を `false` と決め打つ、といった補完はしません。

`unavailable` になるのは、場風・相手の自風・山の残枚数・リーチの種別・一発・本場のいずれかが unknown な場合と、scoring layer が打点を確定できない場合です。これは「期待損失が0」とは別の結論です。ロン可能 state が存在しない現物では、scoring 事実に依らず期待損失は exact に0になります。

#### 整数 evidence

浮動小数点は production truth にしません。保持するのは「点数 × physical weight」の整数分子と整数分母です。

```text
StructuralExpectedDealInLossEvidence {
    loss_weighted_sum: u128,        // Σ_H Σ_U w(H) * w(U) * ロン支払点
    tenpai_weight: u128,            // T(p)。既存 R/T の分母そのもの
    ura_arrangement_weight: u128,   // 裏ドラ配置の総 weight
}

expected loss = loss_weighted_sum / (tenpai_weight * ura_arrangement_weight)
```

点数への変換と百分率は表示専用です。比較・集計の source of truth は整数 evidence で、比較も cross multiplication で exact に行います。overflow は checked arithmetic で扱い、計算不能を clamp / saturate してもっともらしい値にしません。

#### 実装は既存 `R` の数え上げを共有する

compressed hidden-hand model は打点に必要な特徴量 (どの牌種を何枚持つか) を潰した class へ畳み込んでいるため、class の代表打点を weight に掛けることはできません。そこで期待損失は enumerating model ([`ReachedHiddenHandStates`](../../crates/bot-core/src/defense/hidden_hand_states.rs)) の `R` の数え上げをそのまま観測し、加算された state を1件ずつ scoring へ通す diagnostics-only の reference enumeration にしています。候補生成・weight・重複排除・フリテン判定はどれも観測の有無で変わらず、数え直した `R` が production の `R` と食い違う場合は値を返しません。

production の高速 `R/T` path と comparator には手を入れていません。一方で state を1件ずつ点数計算するため、実局面では数百万 state 規模の評価になります (門前リーチ者1人・ドラ表示牌1枚の中盤局面で約263万 state・約1.5億回の scoring 評価、release build で約60秒)。production の `act()` からは呼ばず、診断経路で明示的に要求した場合だけ計算します。

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

`open hand threat: High` の非リーチ相手だけを target にします。classification は [OpenHandThreat](push-pull.md#openhandthreat) を共有し、Defense 側で High 条件を再実装しません。classification は暗槓も完成面子として数えるため、公開副露が無くても暗槓だけで `High` になった相手は target になります。`Present` / `None`、自分、リーチ済み、player id 不明の席は target 外です。

候補の大分類は次の順です。

1. `SafeAgainstAllTargets`
2. `SameHandPassed`
3. `ExactRonRisk`
4. exact unavailable の場合だけ `HonorSafety` / `SuitedSafety`

第一分類 `SafeAgainstAllTargets` は、本人の河または現在有効な一時通過牌によって全 target にロンされない牌 (hard-safe) です。「全 target 自身の河にある」という意味ではありません。

`SameHandPassed` は、全 target が hard-safe または same-hand passed で覆われ、少なくとも1人は same-hand passed だけを根拠とする牌です。same-hand passed は hard-safe ではないので第一分類には入れず、字牌・数牌の heuristic より先に選びます。候補が複数ある場合は hard-safe な target 数が多いものを優先し、同数なら合法 Dahai の元順序を維持します。根拠の違いは [passed tile の区別](#passed-tile-の区別) を参照してください。

上の2分類で決まらない場合は、各 High target の conditional-tenpai model から structural tenpai state の総 weight `T(p)` と、候補牌 `x` で役・current furiten を含めて現在ロン可能な state weight `R(p,x)` を exact に数えます。複数 target は Riichi Defense と同じく各 `R/T` を危険な順へ並べた lexicographic minimax で比較し、exact tie は合法 Dahai の元順序を維持します。same-hand passed は hard-safe ではなく `R=0` の条件にもなりません。

target の1人でも exact model unavailable なら partial exact と heuristic を混在させず、局面全体を従来の heuristic fallback へ戻します。字牌・役牌価値・壁・スジは legacy Riichi Defense と同じ helper を共有します。複数 target の heuristic 集約では、その牌が hard-safe な target と same-hand passed のある target を除いた相手のうち、最も危険な評価を採ります。

数牌は `NoChance` → `OneChance` → `Suji` → `HalfSuji` の順で fallback を探し、`NoSafety` だけなら選びません。選べる防御候補がない場合は通常打牌へ戻ります。

## Combined Defense

リーチ者と High OpenHandThreat が同時に存在する複合 threat で使います。target には種類 `Riichi` / `HighOpenHand` を保持し、全 target にロン安全なら `SafeAgainstAllThreats` とします。

候補の大分類は次の順です。

1. `SafeAgainstAllThreats`
2. `SameHandPassed`
3. `HonorSafety`
4. `SuitedSafety`

`SameHandPassed` の条件は OpenHand Defense と同じで、全 target が hard-safe または same-hand passed で覆われ、少なくとも1人は same-hand passed だけを根拠とする牌です。same-hand passed を根拠にできるのは `HighOpenHand` の target だけで、`Riichi` の target には適用しません。候補が複数ある場合は hard-safe な target 数が多いものを優先し、同数なら合法 Dahai の元順序を維持します。

その牌が hard-safe な target と same-hand passed のある target は、それより弱い heuristic の集約から除き、その相手の無スジや役牌価値を持ち込みません。wall は見え牌由来なので全 target で共有します。

Combined Defense には exact hidden-hand model を接続せず、従来の heuristic ordering を維持します。

## target ごとのロン安全根拠

ここは3経路で混同しない重要な差です。

| target | hard-safe の根拠 |
| --- | --- |
| `Riichi` | 本人の河 + `post_reach_passed` |
| `HighOpenHand` | 本人の河 + 現在有効な `temporary_passed` |

`post_reach_passed` は「リーチ成立後に通った」というリーチ固有の事実で、リーチ者の手牌が変化しないため局中継続します。`temporary_passed` は非リーチを含む各 player について「一時フリテンで現在ロンできない」事実で、対象 player の次のツモ、chi / pon / daiminkan / ankan / kakan で消えます。両者は寿命が異なる別 state で、前者を非リーチ相手へ流用しません。

hard-safe ではない `same_hand_passed` はこの表に入りません。区別は [passed tile の区別](#passed-tile-の区別) を参照してください。

この表の target 種類と hard-safe 判定は、防御 fallback の選択だけでなく [押し引き](push-pull.md#選択打牌の-hard-safe-例外) の例外判定からも共有します。そちらは複合 threat に限らずリーチ単独・`High` の非リーチ相手単独の局面でも同じ target 種類ごとの判定を使うため、target の収集には threat 構成を問わない共有 helper を通ります。防御 fallback の action 選択そのものは従来どおり threat 構成ごとの入口が担当します。

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

`post_reach_passed` はリーチ固有の hard-safe (現物) で、リーチ者の手牌が変化しないため局中継続します。`same_hand_passed` は非リーチ相手 (`HighOpenHand`) の evidence で、`Riichi` の target には使いません。3つは互いに流用しない別 state です。

入力方法は [bot-scenario の post_reach_passed](../bot-scenario.md#post_reach_passed) と [temporary_passed](../bot-scenario.md#temporary_passed)、出力の読み方は [Structured diagnostics](../diagnostics.md#combined-defense) を参照してください。`same_hand_passed` は RiichiLab live client が MJAI event の `tsumogiri` から積み上げる履歴で、bot-scenario の入力 field はありません。

## 防御 helper の他所からの参照

現物 (`is_genbutsu_for()`)・[Suji / HalfSuji](#suji--halfsuji) と [wall / one-chance](#wall--one-chance) を束ねた `SuitedSafetyEvidence`・[HonorSafety](#honorsafety) の rank と見え枚数は、防御以外の診断からも観測値として参照します。攻撃側では [Ron opportunity](discard-selection.md#ron-opportunity-structural-facts-only) が「自分がリーチした場合に待ち牌が他家からどう見えるか」を、これらの helper の結果そのままで持ちます。

参照するのは1牌種ぶんの evidence だけで、Defense selection の comparator は呼びません。攻撃側は自分の打牌を河へ置いた後の公開状態 (`GameContext::after_own_discard()`) へこれらの helper を通すだけで、判定規則そのものは共有します。exact `R/T` は意味が違うので攻撃側の診断へは渡しません ([`R/T` が表すもの](#rt-が表すもの))。防御の semantics そのものは変わりません。

## fallback と source of truth

selection は production selector が source of truth です。diagnostics は同じ selector の結果を `selected` として表示し、`act()` と `diagnose()` で別の防御ロジックを持ちません。リーチ者ごとの exact evidence も、選択に使ったものと同じ evaluation を表示します。`Push` では通常打牌の優先順を変えず、`Fold` のときだけ該当 threat 用 fallback を先に試します。

## 防御候補の ordering

選択だけでなく、**全合法 Dahai を production の優先順位どおりに並べた ordering** も同じ selector の実装から作ります。Riichi / OpenHand / Combined のいずれも、段の順序 (category precedence)・exact `R/T` comparator・複数 target の worst-first lexicographic minimax・exact unavailable 時の heuristic 順序・tie-break・合法 action 順を既存 selector と共有し、ordering 用の comparator を別に持ちません。production selection はこの ordering の先頭 (既存 selector が採用し得る最初の候補) と一致します。

赤5 / 黒5 は別順位に並べず、既存 selection と同じ黒5優先の正規化で牌種ごとに1候補として扱います。数牌 safety の `NoSafety` は既存 selector が採用しないので、ordering では末尾に順位だけ持たせて選択対象から外します。

ordering を観察する入口は [`--force-fold`](../bot-scenario.md#--force-fold) の diagnostic で、production の防御判断は変わりません。`--force-fold` の表示順は、この ordering を土台に exact `R/T` の段だけをベタ降り固有の [fold risk](../bot-scenario.md#手牌内の同一牌枚数と-fold-risk) で並べ替えたものです。同一牌を複数枚持つ場合の継続価値を近似する ranking 用 heuristic で、[passed tile の区別](#passed-tile-の区別) にある通過後 safety の寿命の違いを厳密にモデル化したものではありません。ここで作る ordering 自体はその影響を受けません。

### 0-risk candidate の根拠

「ロンされない」と言える候補は、表示上の percentage ではなく既存の確定 fact / integer evidence で決めます。根拠は2種類あり、混ぜません。

- **hard-safe**: 既存 policy 上、全 defense target からロンされないと確定している候補。Riichi Defense の全リーチ者共通 [Genbutsu](#genbutsu)、OpenHand Defense の `SafeAgainstAllTargets`、Combined Defense の `SafeAgainstAllThreats`。
- **exact model の `R == 0`**: hard-safe ではないが、exact evidence が利用可能で対象となる全 target について `ron_capable_weight == 0` の候補。単独 target ではその player の `R == 0`、複数 target では全 player の `R == 0` だけが該当し、一部 target だけ `R == 0` の候補は 0-risk ではありません。

判定は必ず integer evidence の `R == 0` で行います。`R = 1` / `T = 50000` のように表示が `0.00%` でも `R > 0` なので 0-risk ではありません。

3枚以上見えている字牌 (`HonorSafetyRank::ThreeOrMoreVisible`) は [HonorSafety](#honorsafety) の heuristic safety です。exact model が利用可能で実際に `R == 0` なら 0-risk、`R > 0` なら通常候補で、exact model が使えない場合は heuristic のまま安全確定とは推測しません。

診断では、production selection が exact 比較より前の段で決着した場合も candidate exact evidence を収集します。

- Riichi Defense: 全リーチ者共通の [Genbutsu](#genbutsu) で決着した後も収集します。
- OpenHand / Combined Defense: `SafeAgainstAllTargets` / `SafeAgainstAllThreats` / `SameHandPassed` で決着した後も収集します。

production selector は変わりません。既存 evaluator はこれらの段で決着すると exact model を走らせず早期 return し、`act()` と `diagnose*()` はその早期 return をそのまま使います。診断向けの追加収集は選択後に行うので、選択打牌・category precedence・段の順序・早期 return の性能特性はいずれも変わりません。exact 比較まで進んだ局面では selection が構築した evidence をそのまま使い、再収集しません。

局面情報が足りず exact model 自体を構築できない場合は従来どおり exact unavailable として扱い、存在しない percentage を作りません ([exact model が使えない場合](#exact-model-が使えない場合))。「hard-safe で早期 return したため exact を試していない」ことと「exact model を構築しようとして unavailable だった」ことは区別します。
