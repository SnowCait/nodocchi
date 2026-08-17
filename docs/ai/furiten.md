# フリテン

フリテン関連の情報は、手牌と自分の河から計算できる恒常フリテンと、event 履歴が必要な同巡内・リーチ後見逃しを分けて保持します。

## 自分の河による恒常フリテン

構造上の待ち牌種が自分の河に1つでもあれば恒常フリテンです。フリテン時はロンできず、ツモ和了は可能です。自分の席や河を特定できなければ false と推測せず unknown にします。

`PermanentFuriten` は `No` / `Yes` / `Unknown` を区別し、Reach と Push/Pull の `TenpaiWaitAvailability` で共有します。

## structural waits と live waits

- structural waits: 手牌構造上の待ち牌種
- live waits: 見え牌を反映して山に残っている structural waits
- discarded waits: structural waits のうち自分の河にある牌種

フリテン判定は structural waits と河の交差で行います。見え切って残り0枚でも待ち構造から消しません。残枚数0の待ちが自分の河にあれば恒常フリテンであり続けるためです。

## tsumo_remaining を維持する意味

`tsumo_remaining` はロン可否とは別に、見え牌を差し引いたツモ和了可能枚数を保持します。恒常フリテンでもツモ和了はできるため、待ちの残枚数を0に潰しません。Push/Pull の強いテンパイ判定ではフリテン時の境界を上げ、ツモ依存を反映します。

## HistoryFuritenFacts

履歴依存フリテンは次の2軸です。

| field | 意味 |
| --- | --- |
| `same_turn` | 同巡内フリテン |
| `riichi_missed_win` | リーチ後にアガリを見逃したことによる局中継続フリテン |

各値は `Some(true)` / `Some(false)` / `None` (unknown) を区別します。現在は facts の保持と diagnostics までで、`can_ron()` や action policy には統合していません。この文書整理では production logic を変更していません。

JSON scenario では `history_furiten` で明示できます。schema は [bot-scenario](../bot-scenario.md#history_furiten) を参照してください。

## 入力経路ごとの known / unknown

| 経路 | 履歴依存フリテン |
| --- | --- |
| RiichiLab live client | legal Hora と実際に送信した action を source of truth に event state を追跡 |
| RiichiLab capture replay | 単一 request の observation から過去 event を復元できないため unknown |
| JSON scenario | `history_furiten` に指定した値。省略した軸は unknown |
| Chiihou | 現在の decision API は immutable snapshot から返信を作り結果を match state へ戻す経路がないため unknown |

unknown を安全側・危険側へ補完しません。capture replay と fixture の違いは [bot-scenario](../bot-scenario.md#riichilab-capture-の再生) を参照してください。
