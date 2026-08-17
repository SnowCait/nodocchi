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

## offense state と mode

| 自分の状態 | mode | reason |
| --- | --- | --- |
| 明確な threat なし | `Push` | `NoThreat` |
| offense evaluation なし | `Fold` | `MissingOffenseAgainst*` |
| 強いテンパイ | `Push` | `StrongTenpaiAgainst*` |
| 強いと確認できないテンパイ | `Fold` | `WeakTenpaiAgainst*` |
| 一向聴 | `Fold` | `IishantenAgainst*` |
| 二向聴以上 | `Fold` | `TwoOrMoreShantenAgainst*` |

`PushPullMode` には `Push` / `Neutral` / `Fold` がありますが、現在の暫定 policy は `Neutral` を返しません。`Neutral` は将来の一向聴押し引きと action ordering のために残っています。

「強いテンパイ」は通常打牌で実際に選んだ牌を切った後の `TenpaiWaitAvailability` から判断します。打牌前14枚の受け入れではなく、見え牌を反映したツモ和了可能な待ちです。恒常フリテンが `no` なら残り6枚以上、`yes` ならツモ依存になるため8枚以上を暫定境界とします。unknown は強いと推測しません。

親リーチ、複数リーチ、自分が親の場合でも現在の境界は変えません。一向聴の受け入れや簡易打点 proxy は diagnostics に残しますが、現在の Push/Pull 判定には使いません。

## action ordering

- `Push`: Reach → 通常打牌 → 対応する防御 fallback
- `Neutral`: 通常打牌 → 防御 fallback。Reach は抑制
- `Fold`: 対応する防御 fallback → 通常打牌

防御 fallback の target と safety は [防御](defense.md) を参照してください。
