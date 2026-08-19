# 麻雀 AI の概要

この repository には複数の Agent があります。代表的な `ShantenAgent` は向聴数と見え牌を基に通常打牌を評価し、リーチ・押し引き・防御・限定的なポンを同じ decision path で選びます。`MenzenAgent` は基本判断を共有しつつ門前を崩す鳴きを除外します。

## production decision flow

`ShantenAgent` の大まかな優先順は次のとおりです。

```text
Hora
  ↓
Ryukyoku
  ↓
限定 Pon
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

## 文書の分担

- [打牌選択](discard-selection.md): shanten、Acceptance、1向聴・2向聴以上の牌効率指標、lookahead
- [押し引きと threat](push-pull.md): reach threat、OpenHandThreat、combined threat、Push / Neutral / Fold
- [防御](defense.md): リーチ、High OpenHandThreat、複合 threat に対する safety と fallback
- [フリテン](furiten.md): 恒常フリテン、履歴依存フリテン、structural / live waits
- [手牌評価](hand-value.md): 完成手の構造解析、役・役満の成立判定、通常役の翻数、符、ドラの bonus 翻。点数は未実装
- [Structured diagnostics](../diagnostics.md): 上記判断が出力のどこに現れるか

production code と pure helper が正確な挙動の source of truth で、境界条件は tests と [`bot-scenario` fixtures](../bot-scenario.md#fixture-との使い分け) が固定します。
