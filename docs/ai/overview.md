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

`Push` では Reach → カン (暗槓) → 通常打牌 → 防御 fallback、`Neutral` ではカン → 通常打牌 → 防御 fallback、`Fold` ではカン → 防御 fallback → 通常打牌の順になります。現在の Push/Pull policy は `Neutral` を返しませんが、action 順序上の mode として残っています。カンをどの mode でも検討するのは、自己リーチ後は降りようがないからです。

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

**カンが合法かどうかは入力側 (server / scenario) が source of truth** です。リーチ後に暗槓できるか (待ちが変わらないか) も含めて、bot-core 側で判定し直しません。`legal_actions` に `Ankan` / `Kakan` が並んでいることだけを合法の根拠にします。

現在 production で選べるのは**暗槓と加槓**です。大明槓は候補として診断に並びますが、必ず `DaiminkanNotConnected` の理由で選びません。加槓の条件は [加槓 (Kakan) v1](#加槓-kakan-v1) に分けて書きます。

### 自己リーチ前と自己リーチ後で policy を分ける

暗槓の判断は、自分が既にリーチしているかどうかで**別の policy** になります。分かれ目は `GameContext::own_reached()` だけで、`reached` の index を推測しません。

| `own_reached()` | policy |
| --- | --- |
| `Some(false)` | 暗槓前後を既存評価で比較し、悪化しないと確認できた場合だけ暗槓する |
| `Some(true)` | 合法な暗槓を原則そのまま採用する暫定 policy |
| `None` | 自席を特定できない。リーチ済みだともしていないとも推測せず `OwnReachUnknown` |

分ける理由は、自己リーチ後には**比較対象が違う**ためです。リーチ後は合法な打牌が現在のツモ牌1枚に限られるので、暗槓しない場合の選択肢は自由な通常打牌ではなく強制ツモ切りになります。したがって「通常打牌をどう選ぶか」という比較そのものが成り立ちません。

この分岐は暗槓だけのものです。加槓は元になる Pon があるので通常は自己リーチと両立しませんが、server / context が矛盾した値を持っても自己リーチ状態を推測せず、`Some(true)` は `KakanAfterOwnReach`、`None` は `OwnReachUnknown` として加槓しません。

カン判断は押し引きの結論にかかわらず通ります。自己リーチ後は降りようがないので、押し引きが `Fold` と判断した局面でもカンを検討する必要があるためです。押し引きを見るのは自己リーチ前の暗槓と加槓です。

## 自己リーチ後の暗槓 (暫定 policy)

```text
own_reached() == Some(true)
AND legal_actions に Ankan がある
AND consumed 4枚を手牌 + ツモ牌から取り除いてカンの形になる
AND 自分の副露済み面子数が分かり、暗槓後も上限内
→ Ankan (EligibleAnkanAfterOwnReach)
```

向聴・受け入れ・攻撃打点・押し引き・他家リーチはどれも採用条件にしません。和了できる局面では `Hora` が先に決まるので、カンが和了より先に選ばれることもありません。

リーチ後は暗槓しなければ現在のツモ牌を強制ツモ切りするしかないので、比較は

```text
暗槓 vs 現在のツモ牌の強制ツモ切り
```

になります。どちらにも既存評価では同じ尺度に載らない損得があります。

| 暗槓する側のリスク | 暗槓しない側のリスク |
| --- | --- |
| 新しい槓ドラで他家の打点が上がる | 強制ツモ切りした牌で放銃する |
| 山とツモ順が変わる | 嶺上牌という追加のツモ機会を失う |
| | 槓ドラ・槓裏による自分の打点上昇機会を失う |

これらを1つの EV として比べる基盤が現在の nodocchi にはありません。中途半端な係数や heuristic を置かないため、今回は「server が合法とした暗槓は原則行う」という暫定 policy にしています。

将来、**強制ツモ切り牌の ron risk・新しい槓ドラによる opponent threat / 打点変化・嶺上牌という追加のツモ機会・自分の槓ドラ / 槓裏による打点変化・点棒 / 順位状況・終盤や流局条件**を同じ尺度で評価できるようになった時点で、この暫定 policy を見直します。

## 自己リーチ前の暗槓

自己リーチ前の暗槓は、**暗槓前後の打点を既存評価で比較できた局面だけ**が対象です。速度 (向聴・受け入れ) が悪化しないことだけを根拠に暗槓することはありません。

### 暗槓の成立条件

```text
own_reached() == Some(false)
AND 合法な Ankan がある
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
| 暗槓する | 暗槓後 10 枚 + 既存副露 + 今回の暗槓 | `evaluate_tenpai_offense_with_reach_legality` |

どちらも同じ hypothetical baseline (リーチ手なら `current_reach_baseline_context`、ダマ手なら `damaten_baseline_context`) と同じ既知のドラ表示牌で評価し、生きた和了牌 variant の残枚数で加重した合計 (`OffenseValue::weighted_total`) を比べます。押し引きが threshold 判定に使うのと同じ値で、カン専用の打点評価も集約規則も持ちません。

攻撃モードが違う 2 つの値は同じ尺度ではないので、モードが一致しない場合は比較しません。

### 暗槓後のリーチ合法性

暗槓後の攻撃モードを決める「リーチが合法か」は、現在局面の `legal_actions` を流用せず、共有条件 `is_reach_legal()` を暗槓後の手牌の事実 (門前・既リーチ・持ち点・山の残りツモ可能枚数・テンパイ) へ適用して求めます。

リーチ宣言には `remaining_tiles >= REACH_MIN_REMAINING_TILES` が要ります。暗槓後は嶺上牌を 1 枚引くので、その時点でツモできる枚数は現在より 1 枚少なくなります。2 手先評価の枝のように「何巡先のテンパイか」が確定しない未来とは違い、暗槓後は現在の枚数さえ分かればこの 1 枚分を既知 fact として導けます。

| 現在の `remaining_tiles` | 暗槓後 | remaining tiles 条件 |
| --- | --- | --- |
| `Some(5)` | `Some(4)` | 満たす |
| `Some(4)` | `Some(3)` | 満たさない (暗槓後はリーチできない) |
| `Some(0)` | `Some(0)` | 満たさない。unknown へ倒してリーチ可能側にしない |
| `None` | `None` | 推測しない。共有条件の unknown 規則へ委ねる |

例えば残り 4 枚の局面では、暗槓しない側はまだリーチできる一方、暗槓後はリーチできません。この 2 つを同じ尺度の打点として比べないよう、モードが食い違う組み合わせは `ValueNotEvaluable` になります。

### 評価不能として暗槓しない局面

次のどれかに当たる候補は `ValueNotEvaluable` にして暗槓せず、通常打牌をそのまま維持します。速度非劣化だけを根拠に暗槓へ倒すことはしません。

- どちらかの side がテンパイでない
- どちらかの side の攻撃モードが `Unknown`、またはリーチ手とダマ手で食い違う
- どちらかの side の攻撃打点が `Unknown` (役なし・ロン不可・点数計算の入力不足・裏ドラ未確定)

テンパイ以外を対象外にするのは、1 向聴以降の価値尺度が ExpectedSelfTsumoValue 系になるためです。暗槓後の state は嶺上牌ぶん 1 回多くツモれて、残り自摸機会の元になる山の残枚数も変わるので、暗槓しない側と同じ horizon の値になりません。差を埋める補正を推測で置かない限り比較にならないため、今回は接続していません。テンパイの攻撃打点はロン和了 1 回分の確定打点で、残り自摸機会に依存しないのでこの非対称性を持ちません。

## 加槓 (Kakan) v1

加槓は暗槓と同じ自摸番の action ですが、追加する4枚目を他家に**搶槓**される可能性がある点が決定的に違います。v1 では搶槓リスクを推定せず、搶槓ロンが起こり得ないと hard fact で確定できる局面だけへ限定します。

### 加槓の成立条件

```text
legal_actions に Kakan がある
AND own_reached() == Some(false)
AND 自分のツモを経たと確認できる
AND 他家にリーチ者がいない
AND 既存 Push/Pull policy が Push と判定している
AND 加槓の形が成り立ち、対応する既存 Pon を特定できる
AND Pon → Kakan の post-state を組み立てられる
AND 加槓牌について全他家からの搶槓ロン不能を hard fact で確定できる
AND 加槓後の向聴数 == 加槓しない場合の通常打牌後の向聴数
AND 加槓後の受け入れ (残枚数・牌種数) >= 同じ通常打牌後の受け入れ
AND 両側の攻撃打点を既存評価で確定でき、攻撃モードも一致する
AND 加槓後の攻撃打点 >= 同じ通常打牌後の攻撃打点
AND 成立したカン候補がちょうど1件
```

1つでも満たせない場合は加槓しません。向聴・受け入れ・打点の比較規則と「向聴が改善しても採用しない」扱いは暗槓と同じものを共有し、加槓専用の comparator は作りません。

### Pon → Kakan の post-state

加槓は固定面子の**追加**ではなく**置換**です。

| 項目 | 加槓後 |
| --- | --- |
| 副露済み面子数 | 前後で変わらない |
| 元の Pon | 副露 list から消える |
| Kakan | 元の Pon と同じ位置を置き換える |
| Kakan の tiles | 元 Pon の3枚 + 追加牌1枚 |
| Kakan の `called_tile` | 元 Pon の `called_tile` をそのまま保持する |
| concealed hand | 手牌 + ツモ牌から追加牌1枚だけを取り除く |

`LegalAction::Kakan` の `tile` は追加する4枚目であって、Kakan 面子の `called_tile` ではありません。

```text
加槓しない: 14枚 → 通常打牌 → 13枚 (副露 N) → 次のツモを待つ
加槓する  : 14枚 → 加槓     → 13枚 (副露 N) → 嶺上牌を待つ
```

### 物理牌 (TileId) の扱い

RiichiLab の mjai → `TileId` 変換は黒牌の物理 copy ID を復元できず、同じ牌種の黒牌はすべて同じ代表 ID へ潰れます。そのため `consumed` の `TileId` が既存 Pon の物理牌と完全一致することを validation 条件にせず、牌種 semantics だけを確かめます。

```text
consumed.len() == 3
consumed 3枚が同一 TileType
追加牌も同一 TileType
対応する既存 Pon が同一 TileType
追加牌が現在の concealed hand + ツモ牌に存在する
```

一方、追加牌を実際に手牌から取り除くときは赤5と黒5を区別します。牌種だけで一致させると赤5を誤って槓へ持っていき、手牌に残る赤ドラを取り違えるためです。

### 搶槓 hard-safe

v1 でもっとも重要な条件です。既存 hidden-hand model の通常ロン評価は `chankan = false` を前提にした箇所があるため、

```text
通常の打牌では役なしでロンできない
加槓では搶槓 (Chankan) が役として付いてロンできる
```

という手を取りこぼします。そこで通常打牌用の exact ron-risk をそのまま流用せず、**搶槓 risk を推定しません**。

他家リーチ中は加槓しないので残る3家は非リーチです。その全員について

```text
is_discarded_by_player(加槓牌, player, ctx) == true
```

を要求します。自身の河にその牌種がある player は恒常フリテンでロンできないので、搶槓も起こり得ないと確定できます。判定は既存 `is_discarded_by_player()` / `is_discarded_by_all_players()` が source of truth です。

次のものは hard-safe の根拠に**使いません**。

| 使わない evidence | 理由 |
| --- | --- |
| `temporary_passed` | 「一時フリテンで今はロンできない」だけで、搶槓で新しく役が付く手を排除できない |
| `same_hand_passed` | 手牌不変の見逃し観測であって hard fact ではない |
| スジ | 河由来の推測で、ロン不能を確定しない |
| 壁 / OneChance | 見え枚数由来の推測で、ロン不能を確定しない |
| 字牌の safety rank | 同上 |
| 通常 Dahai 用 exact `R/T` | `chankan = false` 前提を含み、搶槓の役を評価していない |
| 「Push だから大丈夫」 | 押し引きの結論は放銃可否の事実ではない |

この条件はかなり保守的で、意図したものです。加槓は同じ牌種4枚のうち3枚を Pon、1枚を手牌に持つので、その牌種が他家の河にあり得るのは Pon の元になった打牌をした1人だけです。したがって v1 の production では、実際の局面で加槓が成立することはほとんどありません。搶槓の `R/T` を exact に評価できるようになるまでの暫定 policy として、成立しない側 (加槓しない) へ倒してあります。

### 他家リーチ中

```text
any_opponent_reached() == true
→ 加槓しない (OpponentReached)
```

加槓には一発を消す利点がありますが、新しい槓ドラによる相手の打点上昇・一発消去・搶槓 risk・嶺上牌を同じ尺度で比較できる基盤がまだありません。今回はその比較モデルを作りません。

### 加槓後の攻撃打点

加槓後も手は開いたままです。元が Pon なので門前には戻らず、誤って門前手やリーチ手として評価しません。新しい槓ドラの中身は未知なので加槓後の攻撃打点へ加算せず、嶺上牌も暗槓と同じく評価へ含めません。

### 将来: 搶槓 exact model

`WinningContext { chankan: true }` を使った opponent hidden-hand model を整備し、リーチ者・副露者・門前非リーチ者のすべてについて搶槓の `R/T` を評価できるようにするのは別タスクです。その時点で「全3家に hard-safe でなければ加槓しない」という v1 の制限を緩和します。TODO は `bot_core::kan_decision` に残しています。

## 複数のカン候補

同じ局面で 2 件以上のカン (暗槓・加槓を問わない) が成立した場合、production では**どれも選びません** (`MultipleEligibleCandidates`)。自己リーチの前後どちらでも同じ扱いです。合法 action の列挙順は server が決めるものなので、AI の tie-break に使いません。候補間を妥当に比較できる既存 comparator がまだ無いので、将来のためだけの独自 ranking も作りません。

## 今回評価に含めないもの

次の要素は既存評価だけでは値を確定できないため、係数や推定値を置かずに評価へ含めていません。暗槓と加槓で共通です。

| 要素 | 扱い |
| --- | --- |
| 新ドラ | 中身が未知なので、自分の打点にも他家の打点にも加算しない。カン側の打点を過小評価する方向なので、比較はカンに不利な側へ倒れる |
| 嶺上牌 | 未知なので、特定の牌を引いた後の state として評価しない。追加ツモ 1 回分も加算しない |
| 搶槓 risk | 推定しない。加槓は搶槓ロン不能を hard fact で確定できる場合だけに限る |

暗刻が暗槓になることで増える符は、既存 scoring が暗槓を含む固定面子から求めた値がそのまま打点比較へ入ります。この層で符を数え直しません。

自己リーチ前に他家リーチ中の暗槓をしないのは、未知の新ドラがリーチ者の打点をどれだけ押し上げるかを既存評価で測れないためです。ExpectedSelfTsumoValue によるカン前後の比較、複数候補の比較、自己リーチ後の「暗槓 vs 強制ツモ切り」比較、搶槓 exact model、大明槓の reaction モデルは今後の課題として `bot_core::kan_decision` に TODO で残しています。

判断内訳は [Structured diagnostics](../diagnostics.md#kan) の `Kan` section に出ます。

## 文書の分担

- [打牌選択](discard-selection.md): shanten、Acceptance、1向聴・2向聴以上の牌効率指標、lookahead
- [押し引きと threat](push-pull.md): reach threat、OpenHandThreat、combined threat、Push / Neutral / Fold
- [防御](defense.md): リーチ、High OpenHandThreat、複合 threat に対する safety と fallback
- [フリテン](furiten.md): 恒常フリテン、履歴依存フリテン、structural / live waits
- [手牌評価](hand-value.md): 完成手の構造解析、役・役満の成立判定、通常役の翻数、符、ドラの bonus 翻、通常手の基本点と limit、ロン / ツモの支払点、確定した `HandValue`。本場・供託・責任払い (包) は未実装
- [Structured diagnostics](../diagnostics.md): 上記判断が出力のどこに現れるか

production code と pure helper が正確な挙動の source of truth で、境界条件は tests と [`bot-scenario` fixtures](../bot-scenario.md#fixture-との使い分け) が固定します。
