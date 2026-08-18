# フリテン

フリテン関連の情報は、手牌と自分の河から計算できる恒常フリテンと、event 履歴が必要な同巡内・リーチ後見逃しを分けて保持し、ロン可否だけを総合値として一元的に判定します。

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

各値は `Some(true)` / `Some(false)` / `None` (unknown) を区別します。恒常フリテンと合わせて `TenpaiWaitAvailability::can_ron()` に統合済みです。

JSON scenario では `history_furiten` で明示できます。schema は [bot-scenario](../bot-scenario.md#history_furiten) を参照してください。

## 総合ロン可否

恒常フリテンと、評価対象時点の履歴依存フリテン2軸を合わせて `can_ron_from_furiten()` が判定します。ロン可否を求める入口はこれ1つで、call site ごとに組み合わせ規則を書き直しません。

| 入力 | `can_ron()` |
| --- | --- |
| 恒常 / `same_turn` / `riichi_missed_win` のどれか1軸でもフリテン確定 | `Some(false)` |
| 3軸すべて非フリテン確定 | `Some(true)` |
| それ以外 | `None` |

unknown を `false` と推測しません。一方、1軸でもフリテンが確定していれば他の軸が unknown でもロンできないと断定できます。例えば恒常フリテンが `Unknown` でも `same_turn = Some(true)` なら `Some(false)` で、恒常フリテンが `Yes` なら履歴が両軸とも unknown でも `Some(false)` です。逆に恒常フリテンが `No` かつ `same_turn = Some(false)` でも、`riichi_missed_win` が unknown なら `None` のままです。

## 現在時点と打牌後の評価時点

`GameContext::history_furiten()` は**現在時点 (今回の打牌の前)** の facts です。一方 `TenpaiWaitAvailability` は「選択した打牌を切った後」のテンパイを評価するので、履歴依存フリテンも同じ評価時点へ補正してから使います。補正は `GameContext::history_furiten_after_own_discard()` が1か所で行い、選択済み1件の経路と全候補 diagnostics へ同じ値を渡します。補正後の facts は `TenpaiWaitAvailability::history_furiten()` が保持するので、`can_ron()` の根拠を別実装で再計算する必要はありません。

| 今回の打牌 | `same_turn` | `riichi_missed_win` |
| --- | --- | --- |
| 自分のツモを経た通常の打牌 | `Some(false)` で確定 | 維持 |
| Chi / Pon 後など自分のツモを経ていない打牌 | 現在の値を維持 | 維持 |

自摸 → 打牌を終えた時点では同巡内フリテンが解除されるため、元の値が `Some(true)` でも unknown でも `Some(false)` と確定できます。鳴きの後は自分のツモを経ていないので解除しません。`riichi_missed_win` は局終了まで続くのでどちらでも維持します。

「今回の打牌の前に自分がツモしたか」は `GameContext::drawn_tile()` を source of truth にします。RiichiLab は自摸牌込み手牌から自摸牌を分離した値、地鳳は `GET sutehai?` のツモ牌、JSON scenario は `draw` を渡し、どの経路でも `hand_tiles` と合わせて自摸後14枚を作る値なので、他家の自摸牌は入りません。`None` は「ツモしていない」ではなく「自分のツモを経たと確認できない」なので、その場合も現在の値を維持して unknown を推測で埋めません。

したがって診断上は「現在 `same_turn: true` なのに `ron: yes`」という組み合わせが起こりますが、これは自摸後の打牌で解除された正常な状態です。diagnostics の読み方は [Structured diagnostics](../diagnostics.md#table-state-と-history-furiten) を参照してください。

## action policy との関係

この総合ロン可否は事実の表現であり、フリテンを理由に action を変える policy はまだありません。

- フリテンでもリーチを止めません。`can_ron()` が `Some(false)` でもリーチ判断は待ち枚数だけを見ます
- Push/Pull の強いテンパイ判定は `PermanentFuriten` を見ます。履歴依存フリテンで threshold を変えません
- 履歴依存フリテンを理由に降りません

`PushPullTenpaiWaitFacts::can_ron` のように diagnostics が転記している値は総合値になりますが、押し引きの判断そのものは変わりません。

なお、PR #148 の `temporary_passed_tiles` は「相手 player の最後の手牌変化以降に通った牌」を使う OpenHand / Combined Defense 用の一時ロン安全性で、自分の履歴依存フリテンとは対象 player も用途も別です。統合しません。

## 入力経路ごとの known / unknown

| 経路 | 履歴依存フリテン |
| --- | --- |
| RiichiLab live client | legal Hora と実際に送信した action を source of truth に event state を追跡 |
| RiichiLab capture replay | 単一 request の observation から過去 event を復元できないため unknown |
| JSON scenario | `history_furiten` に指定した値。省略した軸は unknown |
| Chiihou | 現在の decision API は immutable snapshot から返信を作り結果を match state へ戻す経路がないため unknown |

unknown を安全側・危険側へ補完しません。capture replay と fixture の違いは [bot-scenario](../bot-scenario.md#riichilab-capture-の再生) を参照してください。
