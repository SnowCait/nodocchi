# 押し引きと threat

`decide_push_pull()` は通常打牌の offense evaluation と、他家リーチ・副露の観測 facts を組み合わせて action mode を決めます。Defense 側で threat を再分類せず、`classify_open_hand_threat()` など production helper の結果を共有します。

## threat の種類

| threat | 条件 | reason 系列 |
| --- | --- | --- |
| Riichi threat | 他家リーチが1人以上 | `*AgainstReach` |
| actionable OpenHandThreat | 他家リーチがなく、`Caution` / `Danger` の非リーチ相手が1人以上 | `*AgainstHighOpenHand` |
| Combined threat | 他家リーチと actionable OpenHandThreat が同時に存在 | `*AgainstCombinedThreat` |

`None` / `Present` の相手は明確な threat に数えません。reason 系列の `*AgainstHighOpenHand` は従来の名前のままで、`Caution` と `Danger` のどちらに対しても使います。例外として、actionable target がすべて `Caution` の局面のテンパイ専用 reason `TenpaiAgainstCautionOpenHand` があります ([Caution-only のテンパイ](#caution-only-のテンパイ))。

## OpenHandThreat

非リーチ相手を観測 facts だけから `None` / `Present` / `Caution` / `Danger` に分類する暫定 heuristic です。テンパイ確率、放銃率、正確な打点ではありません。

完成面子の数は暗槓を含む fixed meld 全体 (`meld_count`) で数えます。暗槓は公開副露ではありませんが完成済みの面子なので、手の進行度には個数ぶん反映します。大明槓・加槓は公開副露なので `meld_count` と `open_meld_count` の両方に1面子として入り、暗槓として二重には数えません。

| level | 意味 | 条件 |
| --- | --- | --- |
| `None` | fixed meld なし | 完成面子が0 |
| `Present` | fixed meld はあるが警戒条件を満たさない。暗槓だけの序盤の相手もここ | 完成面子が1つ以上で、`Caution` / `Danger` の条件をどれも満たさない |
| `Caution` | 局進行だけを根拠にした警戒 | `Danger` の条件を満たさず、下の `Caution` 条件のいずれかを満たす |
| `Danger` | 面子数・確定打点・親を根拠にした強い警戒 | 下の `Danger` 条件のいずれかを満たす |

`Danger` 条件:

- 完成面子が3つ以上 (`ThreeOrMoreOpenMelds` / `ThreeOrMoreFixedMelds`)
- 完成面子が2つ以上かつ `fixed meld visible han proxy >= 2` (`TwoOrMoreWithVisibleHan` / `TwoOrMoreFixedMeldsWithVisibleHan`)
- 親 (`is_dealer == Some(true)`) が完成面子を2つ以上持つ (`DealerWithTwoOrMoreOpenMelds` / `DealerWithTwoOrMoreFixedMelds`)

`Caution` 条件:

- 完成面子が2つ以上かつ河が9枚以上 (`TwoOrMoreOpenMeldsFromNineDiscards` / `TwoOrMoreFixedMeldsFromNineDiscards`)
- 完成面子が1つ以上かつ河が12枚以上 (`OpenMeldFromTwelveDiscards` / `FixedMeldFromTwelveDiscards`)

複数の条件を満たす場合は常に強い level を採用します (`Danger` > `Caution` > `Present` > `None`)。たとえば3面子かつ河12枚以上、または2面子かつ `fixed meld visible han proxy >= 2` かつ河9枚以上の相手は `Danger` です。

`fixed meld visible han proxy` は暗槓を含む全 fixed meld から確定する役牌翻と `meld_dora_count` の合計です。暗槓内のドラ・赤ドラ・確定役牌も観測済みの打点要素として数え、暗槓が複数あればそのぶん累積します。`open visible han proxy` は同じ組み立てを公開副露だけに限ったもので、自分の打点 proxy との比較などに残しています。どちらも役牌翻は `dragon + round_wind + seat_wind` なのでダブ風は2翻、通常役牌は1翻です。unknown wind は推測せず、一般役も含めません。

複数条件に一致した場合、diagnostic reason は production code の固定優先順で1つだけ表示します。優先順は上の `Danger` 条件、`Caution` 条件の並びどおりで、`Danger` の reason が常に `Caution` の reason より優先されます。たとえば3副露が成立している相手に `OpenMeldFromTwelveDiscards` は表示しません。同じ条件が公開副露だけで成立するなら `ThreeOrMoreOpenMelds` などの `OpenMeld` 系、暗槓を含めて初めて成立するなら `ThreeOrMoreFixedMelds` などの `FixedMeld` 系になります。自分、リーチ済み、player id が不明な席は classification 対象外です。

### actionable OpenHandThreat

`Caution` と `Danger` はどちらも actionable OpenHandThreat です。判定は `OpenHandThreatAssessment::is_actionable()` (`level == Caution || level == Danger`) が唯一の source of truth で、押し引き・OpenHand Defense・Combined Defense・hard-safe target の収集はすべてこの predicate を共有します。`Present` / `None` と classification 対象外の席は actionable ではありません。

`Caution` / `Danger` の分割は、Push/Pull pressure を段階化するための classification の整理です。分割前の `High` と同じ集合を `Caution` と `Danger` に分けたもので、production policy が両者を区別するのは Push/Pull のテンパイ判定だけです ([Caution-only のテンパイ](#caution-only-のテンパイ))。Defense fallback、Combined Defense target、hard-safe target、一向聴以下の Push/Pull、Reach / Combined threat の Push/Pull は `Caution` と `Danger` を区別しません。

この classification 自体は Push/Pull policy とは分離されています。したがって、完成面子1つかつ河12枚以上の相手は `Caution` で、actionable OpenHandThreat として扱います。そのうえで、通常打牌 selector が選んだ打牌後がテンパイで、その打牌そのものが現在の全 threat target に hard-safe なら、strong-tenpai threshold を満たさなくても `Push` します ([選択打牌の hard-safe 例外](#選択打牌の-hard-safe-例外))。

### Caution-only のテンパイ

他家リーチがなく、actionable target が1人以上いて、その全員が `Caution` の局面を Caution-only と呼びます。`Danger` が1人でも含まれれば Caution-only ではありません。`Present` / `None` の相手は actionable target ではないので判定に影響しません。

| actionable target | Caution-only |
| --- | --- |
| `Caution` | yes |
| `Caution` + `Caution` | yes |
| `Caution` + `Present` | yes |
| `Danger` | no |
| `Caution` + `Danger` | no |

判定は `has_only_caution_open_hand_threats()` で、classification の `OpenHandThreatLevel::Caution` を source of truth にします。押し引き側で面子数・河枚数や `OpenHandThreatReason` の variant から `Caution` を組み立て直しません。したがって、完成面子2つ以上かつ河9枚以上の相手も、完成面子1つ以上かつ河12枚以上の相手も、`Danger` 条件を満たさなければ同じ扱いです。

Caution-only で通常打牌後がテンパイ (`min_shanten_after_discard <= 0`) なら、strong-tenpai threshold を満たさなくても `Push` します。待ち枚数・残枚数加重合計・確定打点・恒常フリテンの条件は加えないので、恒常フリテンや判定不能で強いテンパイと確認できないテンパイでも押します。reason の優先順は次のとおりです。

1. `StrongTenpaiAgainstHighOpenHand`
2. `SafeTenpaiAgainstHighOpenHand`
3. `TenpaiAgainstCautionOpenHand`
4. `WeakTenpaiAgainstHighOpenHand`

| 局面 | テンパイの扱い | 一向聴以下の扱い |
| --- | --- | --- |
| Caution-only | `Push` | 現行 policy (`Caution` / `Danger` 共通) |
| actionable target に `Danger` を含む | 現行の strong-tenpai / hard-safe policy | 現行 policy (`Caution` / `Danger` 共通) |
| Riichi threat / Combined threat | 現行 policy (Caution-only の例外なし) | 現行 policy |

この例外は actionable OpenHandThreat 単独に限り、Riichi threat と Combined threat には適用しません。リーチ者と `Caution` の相手が同時にいる局面は Combined threat の現行 policy のままです。一向聴・二向聴・三向聴以上も `Caution` だからという理由では押さず、下の表のとおり `Caution` / `Danger` 共通の policy を使います。

## offense state と mode

| 自分の状態 | mode | reason |
| --- | --- | --- |
| 明確な threat なし | `Push` | `NoThreat` |
| offense evaluation なし | `Fold` | `MissingOffenseAgainst*` |
| 強いテンパイ | `Push` | `StrongTenpaiAgainst*` |
| 選択したテンパイ打牌が全 threat target に hard-safe | `Push` | `SafeTenpaiAgainst*` |
| actionable target がすべて `Caution` のテンパイ (strong / hard-safe でない) | `Push` | `TenpaiAgainstCautionOpenHand` |
| それ以外の強いと確認できないテンパイ | `Fold` | `WeakTenpaiAgainst*` |
| ExpectedSelfTsumoValue が threshold 以上の一向聴 | `Push` | `ValuableIishantenAgainst*` |
| actionable OpenHandThreat 単独で、選択した一向聴打牌が全 actionable target に hard-safe | `Push` | `SafeIishantenAgainstHighOpenHand` |
| それ以外の一向聴 | `Fold` | `IishantenAgainst*` |
| actionable OpenHandThreat 単独で、選択したちょうど二向聴の打牌が全 actionable target に hard-safe | `Push` | `SafeTwoShantenAgainstHighOpenHand` |
| それ以外の二向聴以上 | `Fold` | `TwoOrMoreShantenAgainst*` |

向聴数ごとの hard-safe 例外をまとめると次のとおりです。いずれも根拠は通常打牌 selector が選んだ打牌そのものの hard-safe fact で、相手の推定打点ではありません。

| 打牌後 | 選択打牌が全 threat target に hard-safe なときの扱い |
| --- | --- |
| テンパイ | Riichi / actionable OpenHandThreat / Combined のすべてで `Push` ([選択打牌の hard-safe 例外](#選択打牌の-hard-safe-例外)) |
| 一向聴 | actionable OpenHandThreat 単独だけ `Push` ([一向聴の選択打牌 hard-safe 例外](#一向聴の選択打牌-hard-safe-例外)) |
| 二向聴 | actionable OpenHandThreat 単独だけ `Push` ([二向聴の選択打牌 hard-safe 例外](#二向聴の選択打牌-hard-safe-例外)) |
| 三向聴以上 | `Fold` |

`PushPullMode` には `Push` / `Neutral` / `Fold` がありますが、現在の暫定 policy は `Neutral` を返しません。一向聴では Reach できないため `Push` と `Neutral` の action 順序に実質的な違いがなく、一向聴の判定も `Push` / `Fold` の二値です。`Neutral` は action ordering と、攻撃価値と safety を同時に比較する将来の中間モードのために残っています。

「強いテンパイ」は通常打牌で実際に選んだ牌を切った後の `TenpaiWaitAvailability` から判断します。打牌前14枚の受け入れではなく、見え牌を反映したツモ和了可能な待ちです。要求する条件は恒常フリテンと、[攻撃を継続した場合の確定打点](#攻撃継続時の確定打点)で決まります。

| 打牌後テンパイ | 要求する条件 |
| --- | --- |
| 恒常フリテン `no` で攻撃打点を確定できた | 残枚数加重合計が 15,600 点以上 (他家リーチ者に親が含まれる場合は 23,400 点以上) |
| 恒常フリテン `no` で攻撃打点を確定できない | 残枚数 6枚以上 |
| 恒常フリテン `yes` | 残枚数 8枚以上 |
| 恒常フリテン unknown | 強いと推測しない |

残枚数加重合計は生きた和了牌 variant の残枚数と支払点の積の総和で、待ち枚数と打点の両方を1つの値に含みます。平均へ割り算せず、この合計をそのまま threshold と比較します。threshold は inclusive です。

15,600 点は旧 policy の代表的な境界 `3900 × 4枚` / `5200 × 3枚` をそのまま連続的な threshold へ置き換えた値です。したがって `2000 × 8枚` や `8000 × 2枚` は押し、`7700 × 2枚 = 15,400` や `12000 × 1枚` は押しません。

他家リーチ者に親が含まれる場合だけ、放銃時の失点が大きいので 1.5 倍の 23,400 点を要求します。リーチ者が複数いても、親が1人でも含まれていればこちらを使います。子リーチだけ、または actionable OpenHandThreat 単独なら 15,600 点です。

恒常フリテンのテンパイはロンできずツモ依存になるため、この加重合計 policy を適用せず残枚数だけで判断します。攻撃打点を確定できない場合の 6枚も、親リーチだからといって増やしません。

自分が親かどうかでは threshold を変えません。一向聴の受け入れや簡易打点 proxy は diagnostics に残しますが、現在の Push/Pull 判定には使いません。

選択打牌の hard-safe 例外と [Caution-only のテンパイ](#caution-only-のテンパイ) の例外はテンパイが対象です。一向聴は下の [一向聴の攻撃価値](#一向聴の攻撃価値) と、actionable OpenHandThreat 単独に限った [一向聴の選択打牌 hard-safe 例外](#一向聴の選択打牌-hard-safe-例外)、二向聴は actionable OpenHandThreat 単独に限った [二向聴の選択打牌 hard-safe 例外](#二向聴の選択打牌-hard-safe-例外) だけで、それ以外の二向聴以上は従来どおり `Fold` です。Caution-only の例外は actionable target に `Danger` が1人でも含まれる場合は使いません。Riichi threat、Combined threat でも使わず、従来の strong-tenpai threshold を維持します。

## 選択打牌の hard-safe 例外

通常打牌 selector が選んだ打牌後がテンパイで、かつ**選んだ打牌そのもの**が現在の全 threat target に hard-safe なら、strong-tenpai threshold を満たさなくても `Push` します。threat の種類は問わず、Riichi threat・actionable OpenHandThreat・Combined threat のすべてに適用します。

target 集合も target ごとの hard-safe 判定も [防御](defense.md) の既存 source of truth をそのまま共有し、押し引き側で safety rule を書き直しません。

| target | hard-safe の根拠 |
| --- | --- |
| リーチ者 | そのリーチ者への現物 (本人の河、または `post_reach_passed`) |
| `Caution` / `Danger` の非リーチ相手 (`actionable OpenHandThreat` target) | 本人の河、または現在有効な一時通過牌 |

`Caution` / `Danger` の target は [OpenHandThreat](#openhandthreat) の classification が source of truth なので、公開副露がある相手に限らず、暗槓だけで `Caution` / `Danger` になった相手も含みます。

Combined threat では、全リーチ者と全 `Caution` / `Danger` 非リーチ相手の双方についてこの条件を満たす必要があります。1 target でも満たさなければ例外は成立しません。

reason は threat の種類ごとに分かれるので、diagnostics からどの threat に対して safe だったかが分かります。

| threat | reason |
| --- | --- |
| Riichi threat | `SafeTenpaiAgainstReach` |
| actionable OpenHandThreat | `SafeTenpaiAgainstHighOpenHand` |
| Combined threat | `SafeTenpaiAgainstCombinedThreat` |

適用条件は厳密です。

- 手牌内に安全牌があるだけでは適用しません。通常打牌 selector が実際に選んだ打牌そのものが hard-safe である必要があります。テンパイ維持打牌以外の別候補を探索して `Push` にすることもありません。
- hard-safe ではないスジ・ハーフスジ・ワンチャンス・exact model risk の低さは根拠にしません。
- 打牌後がテンパイの場合が対象です。一向聴と二向聴は actionable OpenHandThreat 単独だけ [一向聴の選択打牌 hard-safe 例外](#一向聴の選択打牌-hard-safe-例外) / [二向聴の選択打牌 hard-safe 例外](#二向聴の選択打牌-hard-safe-例外) があり、三向聴以上には広げません。
- `post_reach_passed` や一時通過牌の扱いは、各 threat の防御が既に hard-safe としている semantics をそのまま使います。

## 一向聴の攻撃価値

明確な threat がある一向聴では、[打牌選択](discard-selection.md)が既に求めている `expected self-tsumo value` だけを見ます。押し引き側で前方探索も打点集計も受け入れ集計も行わず、選んだ打牌の集計値をそのまま比較します。

| 他家リーチ者に親 | 押すために要求する ExpectedSelfTsumoValue |
| --- | --- |
| 含まれない | 1,000 点以上 |
| 含まれる | 1,500 点以上 |

threshold は inclusive です。親リーチのときだけ、テンパイと同じく基本 threshold の 1.5 倍を要求します。リーチ者が複数いても、actionable OpenHandThreat との複合でも、親が含まれなければ 1,000 点のままです。自分が親かどうかでは変えません。

ExpectedSelfTsumoValue はテンパイの残枚数加重合計とは別の数値系なので、同じ threshold で比較しません。

値を確認できない場合 (材料が揃わない局面、打点を確定できない枝がある候補) は `Fold` です。受け入れ枚数・一向聴形・weighted tenpai wait・weighted prospective value・簡易打点 proxy・ドラ枚数へは fallback しません。一向聴から押すのはリスクが高いので、十分な攻撃価値を確認できた場合だけ押す保守的な policy にしています。

二向聴以上ではこの値を使いません。

## 一向聴の選択打牌 hard-safe 例外

actionable OpenHandThreat 単独 (他家リーチなし) の一向聴では、ExpectedSelfTsumoValue が threshold 未満、または確認できなくても、通常打牌 selector が選んだ一向聴打牌そのものが全 actionable target に hard-safe なら `Push` します。reason は `SafeIishantenAgainstHighOpenHand` です。

| 条件 | mode | reason |
| --- | --- | --- |
| ExpectedSelfTsumoValue が threshold 以上 | `Push` | `ValuableIishantenAgainstHighOpenHand` |
| actionable OpenHandThreat 単独で、選択打牌が全 actionable target に hard-safe | `Push` | `SafeIishantenAgainstHighOpenHand` |
| それ以外 | `Fold` | `IishantenAgainstHighOpenHand` |

ExpectedSelfTsumoValue の条件を先に評価するので、両方を満たす場合は `ValuableIishantenAgainstHighOpenHand` のままです。hard-safe の判定はテンパイの [選択打牌の hard-safe 例外](#選択打牌の-hard-safe-例外) と同じ fact をそのまま使い、手牌内の別の安全牌は根拠にしません。新しい threshold・倍率・受け入れ枚数・一向聴形の条件も加えません。

Riichi threat と Combined threat の一向聴にはこの例外を適用せず、ExpectedSelfTsumoValue の threshold だけで判断します。二向聴は下の [二向聴の選択打牌 hard-safe 例外](#二向聴の選択打牌-hard-safe-例外) を参照してください。

## 二向聴の選択打牌 hard-safe 例外

actionable OpenHandThreat 単独 (他家リーチなし) で、通常打牌 selector が選んだ打牌後がちょうど二向聴、かつその打牌そのものが全 actionable target に hard-safe なら `Push` します。reason は `SafeTwoShantenAgainstHighOpenHand` です。[一向聴の選択打牌 hard-safe 例外](#一向聴の選択打牌-hard-safe-例外) を二向聴へ限定して広げたものです。

| 条件 | mode | reason |
| --- | --- | --- |
| actionable OpenHandThreat 単独で、選択したちょうど二向聴の打牌が全 actionable target に hard-safe | `Push` | `SafeTwoShantenAgainstHighOpenHand` |
| actionable OpenHandThreat 単独で、選択打牌が hard-safe でない二向聴 | `Fold` | `TwoOrMoreShantenAgainstHighOpenHand` |
| actionable OpenHandThreat 単独の三向聴以上 (hard-safe でも) | `Fold` | `TwoOrMoreShantenAgainstHighOpenHand` |
| Riichi threat / Combined threat の二向聴以上 (hard-safe でも) | `Fold` | `TwoOrMoreShantenAgainst*` |

`Push` の根拠は相手の推定打点ではなく、「今この巡に production が切る牌では全 actionable target にロンされない」という hard-safe fact だけです。hard-safe の判定はテンパイの [選択打牌の hard-safe 例外](#選択打牌の-hard-safe-例外) と同じ fact をそのまま使い、手牌内の別の安全牌やスジ・ワンチャンス・exact model risk の低さは根拠にしません。actionable target が複数いる場合は全員に hard-safe である必要があります。

actionable OpenHandThreat に分類されていることは要求しますが、それ以上の相手条件 (`Caution` か `Danger` か、親か子か、`fixed meld visible han proxy`、classification reason、副露数、河枚数) は加えません。親や visible han の高い `Danger` の target でも、選択打牌が hard-safe なら押します。2向聴 ExpectedSelfTsumoValue・受け入れ枚数・巡目などの攻撃価値 threshold も加えません。

この判断には選択打牌が必要です。ただし最善向聴 cohort に hard-safe な候補が1件もなければ選ばれる打牌も hard-safe になり得ないので、その局面は [通常打牌選択より前の確定 Fold](#通常打牌選択より前の確定-fold) のまま降ります。通常打牌選択まで進むのは cohort に hard-safe な候補がある局面だけで、その場合も押すかどうかは実際に選ばれた打牌の hard-safe fact だけで決まります。

## 攻撃継続時の確定打点

打牌後がテンパイで恒常フリテンでない場合、攻撃を継続したときの打点を簡易 proxy ではなく確定した支払点として求めます。点数計算そのものは [手牌評価](hand-value.md) の既存 layer に任せ、押し引き側は「どの和了状況で評価するか」と「待ちごとの結果をどう1つの値へ畳むか」だけを決めます。

攻撃モードはまず自分が既にリーチしているかで決まります。既リーチならそのテンパイはリーチ手として確定していて、合法 action に Reach が出ないのはリーチ済みだからです。まだリーチしていない場合だけ、これからリーチするかをリーチ判断と同じ policy で決め、押し引き側で同じ条件を書き直しません。合法 Reach の有無は legal action を source of truth にし、別経路で Reach 可否を推測し直しません。

自分が既リーチかは自席と `reached` から求めます。自席が不明で判断できない場合は、未リーチともリーチ済みとも推測せず攻撃モードを確定しないものとして扱い、打点も使いません。

| 攻撃モード | 評価する和了状況 |
| --- | --- |
| 既リーチ / これからリーチする手 | ロン・リーチ宣言済み (通常立直 / ダブル立直)・一発なし・河底なし・裏ドラ0 |
| ダマにする手 | ダマ打点の比較に使うのと同じ baseline |
| 確定できない | 打点を使わない |

### 通常立直とダブル立直

リーチ手として評価する場合、通常立直1翻とダブル立直2翻のどちらで点数計算するかを1か所で決め、ロン baseline とツモ baseline が同じ結論を共有します。片方だけが通常立直のまま残ることはありません。

読む事実は既リーチかどうかで変わります。

| 状態 | 読む事実 |
| --- | --- |
| まだリーチしていない | 今この局面で Reach を選ぶとダブル立直が確定するか |
| 既にリーチしている | 宣言済みの自分のリーチがダブル立直だったか |
| 自席が不明 | どちらも読まない |

どちらもダブル立直だと確定した場合だけダブル立直2翻で評価し、確定できない場合は推測せず最低保証として通常立直1翻で評価します。observation の `reached` にはダブル立直かどうかの情報が無いので、リーチ済みという事実だけからダブル立直を推測しません。

「今リーチすればダブル立直になる」状態は第一巡が終われば失われますが、成立済みのダブル立直は局が終わるまで残ります。第一巡の終了で、既に成立したダブル立直の2翻を落とすことはありません。

第一巡は局の開始から4人全員の第一打が終わるまでで、その間にチー・ポン・大明槓・暗槓・加槓のいずれかがあればそこで終わります。局の開始を観測していない履歴では「第一巡ではない」とも推測せず、判別できないものとして扱います。

1手先・2手先で初めてテンパイする lookahead の枝のリーチは、第一巡を過ぎてから宣言する将来のリーチです。現在の局面がダブル立直を宣言できる状態でも、将来テンパイの枝は常に通常立直1翻で評価します。

裏ドラは未来情報なので期待値を推測しません。ただし裏ドラ表示牌を未観測のままにして打点を不定にするのではなく、裏ドラ表示牌が0枚の「裏0の最低保証打点」として確定させます。一発・河底・槍槓のような偶発要素も加算しません。場風・自風・ドラ表示牌は現在の既知 fact をそのまま使い、不明なら不明のまま渡します。

打点は生きた待ちごと、さらに和了牌の物理牌 (赤5 / 黒5) ごとに求め、その支払点を残枚数で加重して集約します。待ち牌種の間も赤 / 黒 variant の間も同じ残枚数 weight で集約します。残枚数0の variant は集約へ入れません。本場・供託は加えません。名前の付いた役満はその実点数をそのまま使います。押し引きが比較するのはこの加重合計で、平均は diagnostics の表示にだけ使います。

ダマにする手の打点はロン和了を前提にした baseline なので、ダマでロンできると確定した場合しか使いません。ロン可否は [フリテン](furiten.md) の診断が source of truth で、恒常フリテンだけでなく同巡内フリテン・リーチ後見逃しも統合した結論です。どれでロンできなくなっても打点を確定できないものとして扱い、ロンできないことを0点にはしません。

生きた variant のどれか1つでも支払点を確定できない場合は、推測で平均を作らず打点を使わない残枚数だけの policy へ落とします。攻撃モードを確定できない、点数計算の入力が不足している、裏ドラが確定しない、ダマでは役が無い、ダマではロンできない、打牌後の手牌を組み立てられない、といった理由はすべてここに含まれ、役なしを0点として平均へ入れることはしません。

## 簡易打点 proxy

`PushPullOffenseState` の打点関連フィールドは、打牌後の自分の手牌全体から確認できる打点要素だけを数える簡易 proxy です。対象は次の2つで、それぞれ一度だけ数えます。

| 対象 | 数えるもの |
| --- | --- |
| 打牌後の concealed hand | 通常ドラ、赤ドラ、役牌刻子・槓子候補 |
| 自分の確認できている fixed meld | 通常ドラ、赤ドラ、役牌翻 |

fixed meld のドラ・赤ドラ・役牌の判定は threat 側と同じ `meld_threat_facts()` / `fixed_meld_value_facts()` を使い、押し引き側で数え直しません。Chi / Pon / Daiminkan / Ankan / Kakan をすべて対象にし、Kan は物理牌4枚を数えます。Chi は字牌を含まないので役牌翻を持ちませんが、ドラ・赤ドラは通常どおり数えます。役牌翻は `dragon + round_wind + seat_wind` なので、東場の東家の東ポンのようなダブ風は2翻です。場風・自風が不明な軸は推測して加算しません。

暗槓は公開副露ではありませんが自分の手牌価値の一部なので、この proxy には含めます。相手の [OpenHandThreat](#openhandthreat) も暗槓を含む `fixed meld visible han proxy` を使うので、自分と相手で同じ数え方になります。公開副露だけを見る `open visible han proxy` はそれとは別の semantics として残しています。`player_id` が不明で自分の fixed meld を特定できない場合は、確認できない fixed meld の打点を推測して加算しません。

`simple value proxy` は `dora after discard + value honor han proxy after discard` です。`red dora after discard` は `dora after discard` の内数なので加算しません。

これは正確な `HandValue` ではありません。一般役はまだ含めず、符・点数計算も行いません。現在の Push/Fold policy はこの proxy を使わず、diagnostics と将来の打点評価のための観測値として持ちます。

## action ordering

- `Push`: Reach → 通常打牌 → 対応する防御 fallback
- `Neutral`: 通常打牌 → 防御 fallback。Reach は抑制
- `Fold`: 対応する防御 fallback → 通常打牌

防御 fallback の target と safety は [防御](defense.md) を参照してください。

## 通常打牌選択より前の確定 Fold

`Fold` は防御 fallback を通常打牌より優先するので、防御 fallback が action を選べる限り、最終 action は通常打牌選択の結果に依存しません。二向聴以上の `Fold` は 2向聴 ExpectedSelfTsumoValue も受け入れも見ないため、通常打牌選択を先に行っても最終 action には使いません。ただし actionable OpenHandThreat 単独のちょうど二向聴は、選択打牌が hard-safe かで `Push` / `Fold` が変わるので、下の cheap gate で `Push` になり得ないと確定できた場合だけ対象にします。

そこで production の `act()` は、次の3つを通常打牌選択より前に確認できた場合だけ、通常打牌選択そのものを省略します。

1. 明確な threat がいる
2. 合法打牌候補の最善向聴が二向聴以上。ただし actionable OpenHandThreat 単独でちょうど二向聴なら、最善向聴 cohort に全 actionable target へ hard-safe な候補が1件もない
3. その threat 構成に対応する防御 fallback が action を選べる

向聴数は合法打牌候補の既存の1手評価 (`min_shanten_after_discard`) の最小値だけを使います。打牌比較は向聴数を最初に比べるので、選ばれる打牌は必ず最善向聴と同じ向聴数の候補 (最善向聴 cohort) のどれかです。したがって最善向聴が二向聴以上なら、選択打牌の hard-safe fact で判断が変わる局面を除いて選択結果によらず判断は同じです。

actionable OpenHandThreat 単独でちょうど二向聴の場合は、同じ1手評価から最善向聴 cohort の候補を取り出し、それぞれを選んだと仮定したときの hard-safe fact を通常打牌選択後と同じ helper (target 抽出と safety は [防御](defense.md) の source of truth) で確かめます。三向聴以上になる候補は選ばれ得ないので見ません。

| 最善向聴 cohort | early 判定 | 最終判断 |
| --- | --- | --- |
| 全 actionable target に hard-safe な候補が1件もない | `Fold` に確定 (`TwoOrMoreShantenAgainstHighOpenHand`)。通常打牌選択を省略 | — |
| hard-safe な候補が1件以上ある | 保留して通常打牌選択へ進む | 選ばれた打牌が hard-safe なら `Push` (`SafeTwoShantenAgainstHighOpenHand`)、そうでなければ `Fold` |

cohort に hard-safe な候補があることは押す根拠ではありません。selector が別の hard-safe でない二向聴打牌を選べば `Fold` です。

cheap gate も含めて、2向聴 ExpectedSelfTsumoValue も前方探索も打点計算も行いません。

判定は押し引き側の同じ helper を通り、threat の分類も `TwoOrMoreShantenAgainst*` reason も変わりません。二向聴の hard-safe 例外で early 判定を保留するかどうかも、最終判断と同じ helper が決めます。省略しても mode・reason・最終 action・防御 fallback の種別は従来と同じです。

次の局面では省略せず、従来どおり通常打牌選択から判断します。

- 明確な threat がいない
- 最善向聴が一向聴以下 (テンパイの強いテンパイ例外と一向聴の ExpectedSelfTsumoValue 例外があるため)
- actionable OpenHandThreat 単独で最善向聴がちょうど二向聴、かつ最善向聴 cohort に hard-safe な候補がある ([二向聴の選択打牌 hard-safe 例外](#二向聴の選択打牌-hard-safe-例外) の判定に選択打牌が必要なため)
- 防御 fallback が action を選べない
- 構造化診断を構築する経路 (`diagnose()` は通常打牌候補と choice 1/2/3 を表示するため、通常打牌選択そのものを必要とする)

`diagnose()` は従来どおり通常打牌選択まで通すので、`diagnose(...).selected_action == act(...)` は変わりません。省略した局面では `PushPullInputs::offense` を構築しないため、押し引きの opt-in ログは `offense_*` を `None` にし、判断に使った合法打牌候補の最善向聴を `early_fold_best_shanten_after_discard` に出します。ログのために攻撃評価を追加で構築することはありません。
