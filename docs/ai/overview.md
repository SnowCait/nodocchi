# 麻雀 AI の概要

この repository には複数の Agent があります。代表的な `ShantenAgent` は向聴数と見え牌を基に通常打牌を評価し、リーチ・押し引き・防御・鳴き (Chi / Pon)・カン (暗槓) を同じ decision path で選びます。`MenzenAgent` は基本判断を共有しつつ門前を崩す鳴きを除外します。

## production decision flow

`ShantenAgent` の大まかな優先順は次のとおりです。

```text
Hora
  ↓
九種九牌 (Ryukyoku) の宣言 / 続行
  ├─ Declare → Ryukyoku
  └─ Continue ↓
鳴き (Chi / Pon)
  ↓
通常打牌を評価
  ↓
PlayerThreatFacts と Push/Pull を評価
  ↓
mode に応じて Reach / カン / 通常打牌 / 防御 fallback を選択
  ↓
合法打牌 fallback / None
```

`Push` では Reach → カン (暗槓) → 通常打牌 → 防御 fallback、`Neutral` では通常打牌 → 防御 fallback、`Fold` では防御 fallback → 通常打牌の順になります。現在の Push/Pull policy は `Neutral` を返しませんが、action 順序上の mode として残っています。

通常打牌 evaluation は一度だけ作り、Push/Pull の offense 入力や Reach と共有します。threat facts も同じ局面から一度構築し、Push/Pull、OpenHandThreat、Defense target、diagnostics が共有します。

## 九種九牌 (Ryukyoku)

`LegalAction::Ryukyoku` は MJAI Protocol の `ryukyoku`、つまり九種九牌を意味します。**九種九牌が合法かどうかは入力側 (server / scenario) が source of truth** で、nodocchi は么九牌の種類数を数え直して成立条件を再判定しません。合法手として渡された時点で成立しているものとして扱い、判断するのは「宣言するか続行するか」だけです。

判断材料は現在の自摸後 concealed hand (`hand_tiles` + `drawn_tile`) の向聴数で、既存の [向聴計算](discard-selection.md) をそのまま使います。九種九牌専用の向聴計算や国士専用の么九牌カウントは持ちません。

```text
standard shanten <= 2
OR chiitoitsu shanten <= 2
OR kokushi shanten <= 3
```

のいずれかを満たせば宣言せず続行し、それ以外は `Ryukyoku` を選びます。

| 手役 | 続行する向聴数 |
| --- | --- |
| 通常手 | 2向聴以下 |
| 七対子 | 2向聴以下 |
| 国士無双 | 3向聴以下 |

国士だけ1向聴分広いのは意図した policy です。3条件は同格で、複数同時に成立しても優先順位は付けません。点棒状況・親子・受け入れ枚数は判断材料にしません。

自摸牌が分からない、自摸後14枚にならないなどで現在の手牌を復元できない局面では、向聴数を推測して続行せず従来どおり `Ryukyoku` を選びます。向聴数は `unknown` のまま診断へ残します。

続行した場合は `Ryukyoku` を合法手から取り除かず、そのまま鳴き以降の既存判断へ進みます。鳴き・打牌選択・リーチ・押し引き・防御の policy はこの判断で変わりません。判断内訳は [Structured diagnostics](../diagnostics.md#summary-と-runner-up) の `Summary` に出ます。

## カン (Kan)

カンは鳴き (Chi / Pon) とは別の判断層 (`bot_core::kan_decision`) が持ちます。Ankan / Kakan は自摸番の action、Daiminkan は他家打牌への reaction で、どれも「カン → 未知の嶺上牌 → 打牌」になるため、Chi / Pon の `Call → 鳴き後打牌` 評価モデルをそのまま使えないからです。

**カンが合法かどうかは入力側 (server / scenario) が source of truth** です。リーチ後に暗槓できるか (待ちが変わらないか) も含めて、bot-core 側で判定し直しません。`legal_actions` に `Ankan` が並んでいることだけを合法の根拠にします。

現在 production で選べるのは**暗槓だけ**です。加槓と大明槓は候補として診断に並びますが、必ず `KakanNotConnected` / `DaiminkanNotConnected` の理由で選びません。

### 暗槓の成立条件

```text
合法な Ankan がある
AND 他家にリーチ者がいない
AND 既存 Push/Pull policy が Push と判定している (リーチを採用した局面では検討しない)
AND 自分のツモを経たと確認できる
AND 自分の副露済み面子数が分かり、暗槓後も上限内
AND consumed 4枚を手牌 + ツモ牌から取り除いてカンの形になる
AND 暗槓後の向聴数 <= 暗槓しない場合の通常打牌後の向聴数
AND 暗槓後の受け入れ (残枚数・牌種数) >= 同じ通常打牌後の受け入れ
```

暗槓する場合としない場合を、どちらも「13枚相当で次のツモを待つ state」に揃えて比べます。

```text
暗槓しない: 14枚 → 通常打牌 → 13枚 (副露 N)   → 次のツモを待つ
暗槓する  : 14枚 → 暗槓     → 10枚 (副露 N+1) → 嶺上牌を待つ
```

`10枚 + 副露 N+1` と `13枚 + 副露 N` はどちらも `13 - 3 × 副露数` 枚なので、[既存の向聴・受け入れ](discard-selection.md)をそのまま同じ尺度で比べられます。比較の基準にする打牌は production の通常打牌選択が実際に選んだ1件そのもので、カン判断のために打牌を選び直しません。つまり「4枚が面子以外の使い道を持っていなかった」ことを確認できた暗槓だけを選びます。枚数や向聴だけを見た閾値は持ちません。

### 今回評価に含めないもの

次の要素は既存評価だけでは値を確定できないため、係数や推定値を置かずに評価へ含めていません。

| 要素 | 扱い |
| --- | --- |
| 新ドラ | 中身が未知なので、自分の打点にも他家の打点にも加算しない |
| 嶺上牌 | 未知なので、特定の牌を引いた後の state として評価しない。追加ツモ1回分も加算しない |
| 暗槓で増える符 | 既存 scoring は持つが、暗槓後の将来打点はこの層で評価しない |

他家リーチ中に暗槓しないのは、この未知の新ドラがリーチ者の打点をどれだけ押し上げるかを既存評価で測れないためです。したがって現在の条件は「既存評価で測れる速度が悪化しない範囲」に限定した最小の production behavior で、暗槓の打点上昇も新ドラのリスクも判断材料に入っていません。ExpectedSelfTsumoValue による暗槓前後の比較、加槓の搶槓リスク、大明槓の reaction モデルは今後の課題として `bot_core::kan_decision` に TODO で残しています。

判断内訳は [Structured diagnostics](../diagnostics.md#kan) の `Kan` section に出ます。

## 文書の分担

- [打牌選択](discard-selection.md): shanten、Acceptance、1向聴・2向聴以上の牌効率指標、lookahead
- [押し引きと threat](push-pull.md): reach threat、OpenHandThreat、combined threat、Push / Neutral / Fold
- [防御](defense.md): リーチ、High OpenHandThreat、複合 threat に対する safety と fallback
- [フリテン](furiten.md): 恒常フリテン、履歴依存フリテン、structural / live waits
- [手牌評価](hand-value.md): 完成手の構造解析、役・役満の成立判定、通常役の翻数、符、ドラの bonus 翻、通常手の基本点と limit、ロン / ツモの支払点、確定した `HandValue`。本場・供託・責任払い (包) は未実装
- [Structured diagnostics](../diagnostics.md): 上記判断が出力のどこに現れるか

production code と pure helper が正確な挙動の source of truth で、境界条件は tests と [`bot-scenario` fixtures](../bot-scenario.md#fixture-との使い分け) が固定します。
