# 押し引きと threat

`decide_push_pull()` は通常打牌の offense evaluation と、他家リーチ・副露の観測 facts を組み合わせて action mode を決めます。Defense 側で threat を再分類せず、`classify_open_hand_threat()` など production helper の結果を共有します。

## threat の種類

| threat | 条件 | reason 系列 |
| --- | --- | --- |
| Riichi threat | 他家リーチが1人以上 | `*AgainstReach` |
| High OpenHandThreat | 他家リーチがなく、High の非リーチ副露相手が1人以上 | `*AgainstHighOpenHand` |
| Combined threat | 他家リーチと High OpenHandThreat が同時に存在 | `*AgainstCombinedThreat` |

`Present` の副露相手は明確な threat に数えません。

## OpenHandThreat

非リーチ副露相手を観測 facts だけから `None` / `Present` / `High` に分類する暫定 heuristic です。テンパイ確率、放銃率、正確な打点ではありません。

| level | 意味 |
| --- | --- |
| `None` | open meld が0。暗槓だけの相手も含む |
| `Present` | open meld はあるが High 条件を満たさない |
| `High` | 現在の警戒条件のいずれかを満たす |

現在の High 条件:

- open meld が3つ以上
- open meld が2つ以上かつ `open visible han proxy >= 2`
- 親が open meld を2つ以上持つ
- open meld が2つ以上かつ河が9枚以上
- open meld が1つ以上かつ河が12枚以上

`open visible han proxy` は公開副露から確定する役牌翻と既存 `open_meld_dora_count` の合計です。役牌翻は `dragon + round_wind + seat_wind` なのでダブ風は2翻、通常役牌は1翻です。unknown wind は推測せず、暗槓と一般役も含めません。

複数条件に一致した場合、diagnostic reason は production code の固定優先順で1つだけ表示します。自分、リーチ済み、player id が不明な席は classification 対象外です。

この classification 自体は Push/Pull policy とは分離されています。したがって、1副露かつ河12枚以上の相手は引き続き `High` です。そのうえで、他家リーチがなく、`High` target がすべて「1副露かつ河12枚以上」の場合に限り、通常打牌後がテンパイなら strong-tenpai threshold を満たさなくても `Push` します。複数の `High` target がいても、全員がこの条件なら同じです。

## offense state と mode

| 自分の状態 | mode | reason |
| --- | --- | --- |
| 明確な threat なし | `Push` | `NoThreat` |
| offense evaluation なし | `Fold` | `MissingOffenseAgainst*` |
| 強いテンパイ | `Push` | `StrongTenpaiAgainst*` |
| 強いと確認できないテンパイ | `Fold` | `WeakTenpaiAgainst*` |
| 終盤1副露だけが High target のテンパイ | `Push` | `TenpaiAgainstLateOneMeldHighOpenHand` |
| 一向聴 | `Fold` | `IishantenAgainst*` |
| 二向聴以上 | `Fold` | `TwoOrMoreShantenAgainst*` |

`PushPullMode` には `Push` / `Neutral` / `Fold` がありますが、現在の暫定 policy は `Neutral` を返しません。`Neutral` は将来の一向聴押し引きと action ordering のために残っています。

「強いテンパイ」は通常打牌で実際に選んだ牌を切った後の `TenpaiWaitAvailability` から判断します。打牌前14枚の受け入れではなく、見え牌を反映したツモ和了可能な待ちです。要求する条件は恒常フリテンと、[攻撃を継続した場合の確定打点](#攻撃継続時の確定打点)で決まります。

| 打牌後テンパイ | 要求する条件 |
| --- | --- |
| 恒常フリテン `no` で攻撃打点を確定できた | 残枚数加重合計が 15,600 点以上 (他家リーチ者に親が含まれる場合は 23,400 点以上) |
| 恒常フリテン `no` で攻撃打点を確定できない | 残枚数 6枚以上 |
| 恒常フリテン `yes` | 残枚数 8枚以上 |
| 恒常フリテン unknown | 強いと推測しない |

残枚数加重合計は生きた和了牌 variant の残枚数と支払点の積の総和で、待ち枚数と打点の両方を1つの値に含みます。平均へ割り算せず、この合計をそのまま threshold と比較します。threshold は inclusive です。

15,600 点は旧 policy の代表的な境界 `3900 × 4枚` / `5200 × 3枚` をそのまま連続的な threshold へ置き換えた値です。したがって `2000 × 8枚` や `8000 × 2枚` は押し、`7700 × 2枚 = 15,400` や `12000 × 1枚` は押しません。

他家リーチ者に親が含まれる場合だけ、放銃時の失点が大きいので 1.5 倍の 23,400 点を要求します。リーチ者が複数いても、親が1人でも含まれていればこちらを使います。子リーチだけ、または High OpenHandThreat 単独なら 15,600 点です。

恒常フリテンのテンパイはロンできずツモ依存になるため、この加重合計 policy を適用せず残枚数だけで判断します。攻撃打点を確定できない場合の 6枚も、親リーチだからといって増やしません。

自分が親かどうかでは threshold を変えません。一向聴の受け入れや簡易打点 proxy は diagnostics に残しますが、現在の Push/Pull 判定には使いません。

終盤1副露 High の例外はテンパイだけが対象です。一向聴・二向聴以上は従来どおり `Fold` します。High target に2副露以上の相手が1人でも含まれる場合、Riichi threat、Combined threat では従来の strong-tenpai threshold を維持します。

## 攻撃継続時の確定打点

打牌後がテンパイで恒常フリテンでない場合、攻撃を継続したときの打点を簡易 proxy ではなく確定した支払点として求めます。点数計算そのものは [手牌評価](hand-value.md) の既存 layer に任せ、押し引き側は「どの和了状況で評価するか」と「待ちごとの結果をどう1つの値へ畳むか」だけを決めます。

攻撃モードはまず自分が既にリーチしているかで決まります。既リーチならそのテンパイはリーチ手として確定していて、合法 action に Reach が出ないのはリーチ済みだからです。まだリーチしていない場合だけ、これからリーチするかをリーチ判断と同じ policy で決め、押し引き側で同じ条件を書き直しません。合法 Reach の有無は legal action を source of truth にし、別経路で Reach 可否を推測し直しません。

自分が既リーチかは自席と `reached` から求めます。自席が不明で判断できない場合は、未リーチともリーチ済みとも推測せず攻撃モードを確定しないものとして扱い、打点も使いません。

| 攻撃モード | 評価する和了状況 |
| --- | --- |
| 既リーチ / これからリーチする手 | ロン・リーチ宣言済み・一発なし・河底なし・裏ドラ0 |
| ダマにする手 | ダマ打点の比較に使うのと同じ baseline |
| 確定できない | 打点を使わない |

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

暗槓は公開副露ではありませんが自分の手牌価値の一部なので、この proxy には含めます。相手の [OpenHandThreat](#openhandthreat) の `open visible han proxy` が暗槓を含まないのは公開情報だけを見る別の semantics で、意図的な違いです。`player_id` が不明で自分の fixed meld を特定できない場合は、確認できない fixed meld の打点を推測して加算しません。

`simple value proxy` は `dora after discard + value honor han proxy after discard` です。`red dora after discard` は `dora after discard` の内数なので加算しません。

これは正確な `HandValue` ではありません。一般役はまだ含めず、符・点数計算も行いません。現在の Push/Fold policy はこの proxy を使わず、diagnostics と将来の打点評価のための観測値として持ちます。

## action ordering

- `Push`: Reach → 通常打牌 → 対応する防御 fallback
- `Neutral`: 通常打牌 → 防御 fallback。Reach は抑制
- `Fold`: 対応する防御 fallback → 通常打牌

防御 fallback の target と safety は [防御](defense.md) を参照してください。
