# 麻雀 AI の概要

この repository には複数の Agent があります。代表的な `ShantenAgent` は向聴数と見え牌を基に通常打牌を評価し、リーチ・押し引き・防御・鳴き (Chi / Pon) を同じ decision path で選びます。`MenzenAgent` は基本判断を共有しつつ門前を崩す鳴きを除外します。

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
mode に応じて Reach / 通常打牌 / 防御 fallback を選択
  ↓
合法打牌 fallback / None
```

`Push` では Reach → 通常打牌 → 防御 fallback、`Neutral` では通常打牌 → 防御 fallback、`Fold` では防御 fallback → 通常打牌の順になります。現在の Push/Pull policy は `Neutral` を返しませんが、action 順序上の mode として残っています。

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

## 文書の分担

- [打牌選択](discard-selection.md): shanten、Acceptance、1向聴・2向聴以上の牌効率指標、lookahead
- [押し引きと threat](push-pull.md): reach threat、OpenHandThreat、combined threat、Push / Neutral / Fold
- [防御](defense.md): リーチ、High OpenHandThreat、複合 threat に対する safety と fallback
- [フリテン](furiten.md): 恒常フリテン、履歴依存フリテン、structural / live waits
- [手牌評価](hand-value.md): 完成手の構造解析、役・役満の成立判定、通常役の翻数、符、ドラの bonus 翻、通常手の基本点と limit、ロン / ツモの支払点、確定した `HandValue`。本場・供託・責任払い (包) は未実装
- [Structured diagnostics](../diagnostics.md): 上記判断が出力のどこに現れるか

production code と pure helper が正確な挙動の source of truth で、境界条件は tests と [`bot-scenario` fixtures](../bot-scenario.md#fixture-との使い分け) が固定します。
