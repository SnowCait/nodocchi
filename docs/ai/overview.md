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

その暗槓も、**暗槓前後の打点を既存評価で比較できた局面だけ**が対象です。速度 (向聴・受け入れ) が悪化しないことだけを根拠に暗槓することはありません。

### 暗槓の成立条件

```text
合法な Ankan がある
AND 他家にリーチ者がいない
AND 既存 Push/Pull policy が Push と判定している (リーチを採用した局面では検討しない)
AND 自分のツモを経たと確認できる
AND 自分の副露済み面子数が分かり、暗槓後も上限内
AND consumed 4枚を手牌 + ツモ牌から取り除いてカンの形になる
AND 暗槓後の向聴数 == 暗槓しない場合の通常打牌後の向聴数
AND 暗槓後の受け入れ (残枚数・牌種数) >= 同じ通常打牌後の受け入れ
AND 両側の攻撃打点を既存評価で確定でき、攻撃モードも一致する
AND 暗槓後の攻撃打点 >= 同じ通常打牌後の攻撃打点
AND 成立した暗槓候補がちょうど1件
```

暗槓する場合としない場合を、どちらも「13枚相当で次のツモを待つ state」に揃えて比べます。

```text
暗槓しない: 14枚 → 通常打牌 → 13枚 (副露 N)   → 次のツモを待つ
暗槓する  : 14枚 → 暗槓     → 10枚 (副露 N+1) → 嶺上牌を待つ
```

`10枚 + 副露 N+1` と `13枚 + 副露 N` はどちらも `13 - 3 × 副露数` 枚なので、既存の向聴・受け入れ・打点をそのまま同じ尺度で比べられます。比較の基準にする打牌は production の通常打牌選択が実際に選んだ 1 件そのもので、カン判断のために打牌を選び直しません。

### 向聴の比較

[Acceptance](discard-selection.md) は「その牌を 1 枚加えると**現在の向聴数**が下がる牌」なので、向聴段階が違う state の受け入れ枚数・牌種数は同じ意味の値ではありません。1 向聴の受け入れ 8 枚とテンパイの待ち 4 枚を `4 < 8` として比べません。したがって向聴の比較は 3 通りに分けます。

| 暗槓後 vs 通常打牌後 | 扱い |
| --- | --- |
| 悪化 | `ShantenRegresses` |
| 改善 | 受け入れも打点も同じ尺度で比べられないので `ShantenImprovedNotComparable` |
| 同じ | 受け入れと打点の比較へ進む |

向聴が改善するのは、暗槓しない側の合法打牌が制限されていて 4 枚目を切れない局面などに限られます。向聴が進んだ分の価値を既存 primitive で確定できないので、「向聴が改善したから暗槓する」という結論もここでは作りません。

### 打点の比較

速度 (向聴・受け入れ) が悪化しないことだけでは、暗槓によって役・待ち構成・確定打点が落ちる局面を弾けません。そのため速度の比較を通った候補には、押し引き・リーチ判断が使うのと同じ攻撃打点 (`TenpaiOffenseValue`) の比較を必ず要求します。

| 側 | 手牌 | 打点 |
| --- | --- | --- |
| 暗槓しない | 通常打牌後 13 枚 + 既存副露 | `evaluate_tenpai_offense_value` |
| 暗槓する | 暗槓後 10 枚 + 既存副露 + 今回の暗槓 | `evaluate_tenpai_offense_with_hands` |

どちらも同じ hypothetical baseline (リーチ手なら `current_reach_baseline_context`、ダマ手なら `damaten_baseline_context`) と同じ既知のドラ表示牌で評価し、生きた和了牌 variant の残枚数で加重した合計 (`OffenseValue::weighted_total`) を比べます。押し引きが threshold 判定に使うのと同じ値で、カン専用の打点評価も集約規則も持ちません。

### 評価不能として暗槓しない局面

次のどれかに当たる候補は `ValueNotEvaluable` にして暗槓せず、通常打牌をそのまま維持します。速度非劣化だけを根拠に暗槓へ倒すことはしません。

- どちらかの side がテンパイでない
- どちらかの side の攻撃モードが `Unknown`、またはリーチ手とダマ手で食い違う
- どちらかの side の攻撃打点が `Unknown` (役なし・ロン不可・点数計算の入力不足・裏ドラ未確定)

テンパイ以外を対象外にするのは、1 向聴以降の価値尺度が ExpectedSelfTsumoValue 系になるためです。暗槓後の state は嶺上牌ぶん 1 回多くツモれて、残り自摸機会の元になる山の残枚数も変わるので、暗槓しない側と同じ horizon の値になりません。差を埋める補正を推測で置かない限り比較にならないため、今回は接続していません。テンパイの攻撃打点はロン和了 1 回分の確定打点で、残り自摸機会に依存しないのでこの非対称性を持ちません。

### 複数の暗槓候補

同じ局面で 2 件以上の暗槓が成立した場合、production では**どれも選びません** (`MultipleEligibleCandidates`)。合法 action の列挙順は server が決めるものなので、AI の tie-break に使いません。候補間を妥当に比較できる既存 comparator がまだ無いので、将来のためだけの独自 ranking も作りません。

### 今回評価に含めないもの

次の要素は既存評価だけでは値を確定できないため、係数や推定値を置かずに評価へ含めていません。

| 要素 | 扱い |
| --- | --- |
| 新ドラ | 中身が未知なので、自分の打点にも他家の打点にも加算しない。暗槓側の打点を過小評価する方向なので、比較は暗槓に不利な側へ倒れる |
| 嶺上牌 | 未知なので、特定の牌を引いた後の state として評価しない。追加ツモ 1 回分も加算しない |

暗刻が暗槓になることで増える符は、既存 scoring が暗槓を含む固定面子から求めた値がそのまま打点比較へ入ります。この層で符を数え直しません。

他家リーチ中に暗槓しないのは、未知の新ドラがリーチ者の打点をどれだけ押し上げるかを既存評価で測れないためです。ExpectedSelfTsumoValue による暗槓前後の比較、複数候補の比較、加槓の搶槓リスク、大明槓の reaction モデルは今後の課題として `bot_core::kan_decision` に TODO で残しています。

判断内訳は [Structured diagnostics](../diagnostics.md#kan) の `Kan` section に出ます。

## 文書の分担

- [打牌選択](discard-selection.md): shanten、Acceptance、1向聴・2向聴以上の牌効率指標、lookahead
- [押し引きと threat](push-pull.md): reach threat、OpenHandThreat、combined threat、Push / Neutral / Fold
- [防御](defense.md): リーチ、High OpenHandThreat、複合 threat に対する safety と fallback
- [フリテン](furiten.md): 恒常フリテン、履歴依存フリテン、structural / live waits
- [手牌評価](hand-value.md): 完成手の構造解析、役・役満の成立判定、通常役の翻数、符、ドラの bonus 翻、通常手の基本点と limit、ロン / ツモの支払点、確定した `HandValue`。本場・供託・責任払い (包) は未実装
- [Structured diagnostics](../diagnostics.md): 上記判断が出力のどこに現れるか

production code と pure helper が正確な挙動の source of truth で、境界条件は tests と [`bot-scenario` fixtures](../bot-scenario.md#fixture-との使い分け) が固定します。
