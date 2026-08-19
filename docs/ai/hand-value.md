# 手牌評価

将来の翻数・符・点数計算の共通基盤として、完成済みの手牌がどの和了形へ分解できるかを pure logic で列挙し、その分解ごとに成立する役を判定します。牌構成だけで確定する役に加え、和了時の観測事実を表す `WinningContext` を受け取り、役牌と状況依存役も分解ごとに判定します。さらに、判明している和了牌がどの雀頭 / 面子を完成させたのかを分解ごとに解釈し、その解釈まで含めた `decomposition` × `interpretation` 単位で和了牌依存の役を判定します。そのうえで、通常役とは別型の `Yakuman` として named 役満の成立事実だけを同じ単位で判定します。翻数や点数はまだ扱いません。

## 実装済みと未実装

実装済み:

- 完成手の構造解析 (`analyze_completed_hand()`)
- `Standard` / `Chiitoitsu` / `Kokushi` の3 family
- 通常形の複数解釈をすべて列挙
- fixed meld (Chi / Pon / Daiminkan / Ankan / Kakan) 対応
- decomposition ごとの structural Yaku 判定 (`evaluate_structural_yaku()`)
- 牌構成・面子構成だけで確定する通常役 (下表)
- 和了時 context (`WinningContext`)
- 役牌 (三元牌 / 場風 / 自風)
- リーチ / ダブルリーチ / 一発
- 門前清自摸和
- 槍槓 / 嶺上開花 / 海底摸月 / 河底撈魚
- structural と状況依存をまとめた decomposition ごとの判定 (`evaluate_yaku()`)
- 判明している和了牌の解釈 (`interpret_winning_tile()`)
- decomposition ごとの複数の和了牌 placement
- 待ち形 (両面 / 嵌張 / 辺張 / 単騎 / シャンポン)
- 七対子の待ち
- 国士無双の special wait (13面待ち / 単一牌種待ち)
- decomposition × interpretation ごとの Yaku 判定 (`evaluate_winning_yaku()`)
- 平和
- 三暗刻
- 通常 `Yaku` と分離した named 役満の成立事実 (`Yakuman`)
- decomposition ごとの structural 役満判定 (`evaluate_yakuman()`)
- decomposition × interpretation ごとの役満判定 (`evaluate_winning_yakuman()`)
- 国士無双 (`KokushiMusou`)
- 九蓮宝燈 (`ChuurenPoutou`)
- 緑一色 (`Ryuuiisou`)
- 四暗刻 (`Suuankou`)
- 四槓子 (`Suukantsu`)
- 清老頭 (`Chinroutou`)
- 字一色 (`Tsuuiisou`)
- 大三元 (`Daisangen`)
- 小四喜 (`Shousuushii`)
- 大四喜 (`Daisuushii`)

未実装:

- 和了牌が不明な場合の完全な scoring policy
- 天和 / 地和
- 人和の特殊 scoring
- 責任払い (包)
- 翻数集計
- 符計算
- ドラの点数化
- 確定した点数
- 確定した `HandValue`

役の定義は [World Riichi Championship Rules](https://www.worldriichi.org/wrc-rules) ([WRC Rules 2025 PDF](https://static1.squarespace.com/static/634a7884c297a25f06589b79/t/6834d67360e19c1da6c0d12c/1748293243651/WRC+Rules+2025.pdf) の `11.5 Yaku list`) と [EMA Riichi Competition Rules](https://mahjong-europe.org/portal/index.php?Itemid=166&id=30&option=com_content&view=article) を一次情報とします。

## 入力と出力

`analyze_completed_hand(concealed_tiles, fixed_melds)` は門前部分の物理牌 (`TileId`) と確定済み `Meld` を受け取り、`CompletedHandAnalysis` を返します。赤ドラや将来の符計算で物理牌が必要になるため、入力の `TileId` はそのまま保持します。面子分解自体は `TileCounts` 上の `TileType` 単位で行います。

```text
CompletedHandDecomposition
├─ Standard   … 雀頭 + 門前面子 + fixed meld 数
├─ Chiitoitsu … 7種の対子牌
└─ Kokushi    … 対子になった么九牌
```

門前面子は `Sequence { start }` と `Triplet { tile }` を区別します。Kan は物理牌4枚でも構造上は1面子で、`FixedMeldCount` として数えます。

| 入力 | 結果 |
| --- | --- |
| 完成形 | 成立する分解をすべて含む `CompletedHandAnalysis` |
| 合法な牌集合だが未完成 | 分解が空の `CompletedHandAnalysis` |
| fixed meld が5つ以上 / 同一牌種が5枚以上 | `CompletedHandError` |

## 通常形の列挙と重複排除

通常形は同じ牌姿でも複数の面子解釈が成立します。一盃口・三色・一通・三暗刻・符などを後から正しく評価するため、最初に見つけた1分解ではなく distinct な分解をすべて返します。

探索は完成形限定の exact DFS です。

```text
雀頭候補を1つ選ぶ
  ↓
残りの最小牌を暗刻として除去する branch / 順子として除去する branch
  ↓
必要面子数を満たし牌が尽きたら1分解
```

牌の除去は既存の `TileCounts::remove_pair()` / `remove_triplet()` / `remove_sequence()` を使い、同じ牌操作を再実装しません。必要な門前構造牌数は `2 + 3 * (4 - fixed_meld_count)` で、fixed meld は探索対象にしません。

`111m` と `123m` を同時に含む形のように、探索順違いで同じ分解へ到達する場合があります。各分解の面子列を canonical order へ並べ替えたうえで全体を sort + dedup し、重複を返しません。返却順は牌種順に決まり deterministic です。

## 特殊形

- 七対子: 異なる7牌種の対子。同一牌種4枚を2対子と数えません。既存 `chiitoitsu_shanten()` と一致します。
- 国士無双: 13種の么九牌がそろい、そのうち1種が対子。対子牌種を保持します。
- どちらも fixed meld が1つでも存在する場合は成立しません。

通常形と七対子のように複数 family が同時に成立する牌姿では、最小向聴の形を1つ選ぶのではなく両方返します。

## decomposition ごとの structural Yaku

`evaluate_structural_yaku(&analysis)` は `CompletedHandAnalysis` を唯一の入力とし、`YakuEvaluation` を分解と同じ順序で返します。各 evaluation は判定元の `CompletedHandDecomposition` と、その分解で成立する `Yaku` を保持します。

```rust
let analysis = analyze_completed_hand(&concealed_tiles, &fixed_melds)?;
for evaluation in evaluate_structural_yaku(&analysis) {
    let decomposition = evaluation.decomposition();
    let yaku = evaluation.yaku();
}
```

通常形は同じ牌姿でも解釈が複数あり、解釈ごとに成立する役が変わります。例えば `11122233344455m` は暗刻4つの解釈で `Toitoi` が成立し、順子を含む解釈では成立しません。そのため analysis 全体へ役を union せず、必ず分解ごとに役集合を紐付けます。`Standard` と `Chiitoitsu` が同時に成立する牌姿でも、`Ryanpeikou` 側と `Chiitoitsu` 側をそれぞれの分解が保持します。将来の翻数・符・点数計算が有効な解釈を比較できるよう、役の出所となった分解を失わない形にしています。

役判定は完成手をもう一度分解しません。門前面子は `ConcealedMeld`、fixed meld は既存 `Meld` を `MeldShape` (`Sequence` / `Triplet` / `Kan`) へ正規化した結果だけを見ます。`Meld::shape()` は牌構成を検証し、`Chi` なのに連続3牌でない、`Pon` / Kan なのに同一牌でないといった不正な meld では `None` を返します。不正な fixed meld を含む分解には役を付けません。

門前性は `is_menzen()` が `Meld::is_open()` だけで決めます。`Ankan` は fixed meld ですが門前を維持し、`Chi` / `Pon` / `Daiminkan` / `Kakan` は門前を壊します。`fixed_meld_count == 0` を門前判定に使いません。

### 対象の役

| Yaku | 対象 family | 条件 |
| --- | --- | --- |
| `Tanyao` | Standard / Chiitoitsu | 手牌全体に么九牌がない。副露手でも成立 |
| `Chiitoitsu` | Chiitoitsu | 七対子の分解であること |
| `Toitoi` | Standard | 4面子すべてが刻子または槓子 |
| `Iipeikou` | Standard | 門前で同一順子の組が1組 |
| `Ryanpeikou` | Standard | 門前で同一順子の組が2組 |
| `SanshokuDoujun` | Standard | 同じ数字の順子が3スートすべてにある |
| `Ittsu` | Standard | 同一スートに123 / 456 / 789 の順子がある |
| `Chanta` | Standard | 全面子と雀頭が么九牌を含み、順子と字牌を1つ以上含む |
| `Junchan` | Standard | 全面子と雀頭が老頭牌を含み、字牌がなく順子を1つ以上含む |
| `Honroutou` | Standard / Chiitoitsu | 全牌が老頭牌または字牌 |
| `SanshokuDoukou` | Standard | 同じ数字の刻子 / 槓子が3スートすべてにある |
| `Sankantsu` | Standard | 槓子がちょうど3つ |
| `Shousangen` | Standard | 三元牌の刻子 / 槓子が2種、残り1種が雀頭 |
| `Honitsu` | Standard / Chiitoitsu | 1つの数牌スートと字牌だけで構成され、双方を1枚以上含む |
| `Chinitsu` | Standard / Chiitoitsu | 1つの数牌スートだけで構成される |

順子・刻子の判定では門前面子と fixed meld の両方を見ます。`SanshokuDoujun` と `Ittsu` は `Chi` を、`SanshokuDoukou` は `Pon` / `Daiminkan` / `Ankan` / `Kakan` を含みます。`Iipeikou` / `Ryanpeikou` だけは門前の順子だけを構成要素にします。

`Yaku` は成立した事実だけを表し、翻数を持ちません。`Sanshoku` / `Ittsu` / `Chanta` / `Junchan` / `Honitsu` / `Chinitsu` の食い下がりは後続の翻数 layer の責務です。ドラと赤ドラも役ではないため `Yaku` に含めません。

### 役同士の排他と役満の境界

同時に返さない組み合わせは判定時点で排他にし、後段で消しません。

- `Ryanpeikou` が成立する分解へ `Iipeikou` を付けません。
- `Junchan` が成立する分解へ `Chanta` を付けません。字牌を含まない手は `Junchan` だけになります。
- `Chinitsu` が成立する分解へ `Honitsu` を付けません。字牌だけの手はどちらにもなりません。

役満は `Yaku` へ混ぜず、別型の `Yakuman` として [役満](#役満) で判定します。両者が二重にならないよう、境界を次のように固定しています。

- `Sankantsu` は槓子がちょうど3つのときだけ成立します。4槓子は `Suukantsu` へ残します。
- `Shousangen` は三元牌2種が刻子 / 槓子で残り1種が雀頭のときだけ成立します。三元牌3種が刻子の `Daisangen` 形では成立しません。
- `CompletedHandDecomposition::Kokushi` は structural Yaku の対象外で、常に空の役集合を返します。

そのため、この評価器が返す空の役集合を「この手には役がない」と解釈する production policy へまだ使えません。和了牌に依存する役と役満は別 API が返すため、この評価器は pure な基盤としてだけ使い、`act()` / `diagnose()` の行動選択へは接続しません。各分解の役一覧は sort と dedup 済みで deterministic です。

## 和了時 context

`WinningContext` は和了時点の麻雀ルール上の観測事実だけを持つ pure な型です。`bot-core::GameContext` や RiichiLab の Observation、Chiihou の state には依存させません。client 固有の局面型を `bot-logic` へ持ち込まないことで、評価器を局面表現から独立させます。

| field | 型 | 意味 |
| --- | --- | --- |
| `win_method` | `WinMethod` | `Ron` / `Tsumo` |
| `round_wind` | `Option<TileType>` | 場風。unknown は `None` |
| `seat_wind` | `Option<TileType>` | 自風。unknown は `None` |
| `riichi` | `RiichiStatus` | `Unknown` / `NotDeclared` / `Riichi` / `DoubleRiichi` |
| `ippatsu` | `Option<bool>` | 一発の条件を満たすか |
| `rinshan` | `Option<bool>` | 嶺上牌での和了か |
| `chankan` | `Option<bool>` | 他家の加槓を捉えた和了か |
| `remaining_live_tiles` | `Option<u32>` | 和了時点の山の残りツモ可能枚数 [枚] |

`WinningContext::new(win_method)` は win method 以外をすべて unknown にし、`with_round_wind()` などの builder で判明した事実だけを埋めます。

### unknown の扱い

どの軸も unknown を `false` と推測しません。

- `riichi` は `Unknown` / `NotDeclared` / `Riichi` / `DoubleRiichi` を1つの型で表し、「リーチ宣言の有無が不明」と「リーチしていないと確認済み」を区別します。独立した2つの bool にしないため、`Riichi` と `DoubleRiichi` が同時に立つ矛盾した状態を作れません。
- `ippatsu` / `rinshan` / `chankan` は既存 `HistoryFuritenFacts` と同じ tri-state semantics です。`None` は取得不能、`Some(false)` は該当しないことを確認済み、`Some(true)` は該当を確認済みを表します。履歴イベントを評価器側で河などから再構築せず、`None` のときは該当役を付けません。
- `remaining_live_tiles` は役名そのものの bool ではなく観測事実です。`Some(0)` は「和了時点で live wall に残りツモ可能牌がない」ことを表し、既存 `TableStateFacts::remaining_tiles` (山の残りツモ可能枚数 [枚]) と同じ semantics です。`None` を `0` と推測しません。

`riichi` が unknown / `NotDeclared` のまま `ippatsu = Some(true)` を渡しても `Ippatsu` は付きません。リーチが確認できている場合だけ成立します。

client 配線はこの PR の対象外です。RiichiLab / Chiihou から `DoubleRiichi` / `Ippatsu` / `Rinshan` / `Chankan` を正確に取得できない場合は unknown のままにし、`reached == true` から `ippatsu = false` を導くような推測をしません。

## structural + 状況依存の統合

`evaluate_yaku(&analysis, context)` は structural Yaku と状況依存 Yaku をまとめて分解ごとに返します。

```rust
let analysis = analyze_completed_hand(&concealed_tiles, &fixed_melds)?;
let context = WinningContext::new(WinMethod::Tsumo)
    .with_round_wind(round_wind)
    .with_seat_wind(seat_wind)
    .with_riichi(RiichiStatus::Riichi);
for evaluation in evaluate_yaku(&analysis, context) {
    let decomposition = evaluation.decomposition();
    let yaku = evaluation.yaku();
}
```

structural Yaku を再判定せず、`evaluate_structural_yaku()` の結果へ状況依存 Yaku を足してから sort + dedup します。`Tanyao` / `Chiitoitsu` / `Toitoi` / `Iipeikou` などの判定は1か所だけに残ります。`evaluate_structural_yaku()` は牌姿だけで確定する pure な層として引き続き公開します。

戻り値の表現はどちらも `YakuEvaluation` です。同じ shape の型を層ごとに複製しません。既存名の `StructuralYakuEvaluation` は `YakuEvaluation` の type alias として残します。

ここでも analysis 全体へ役を union せず、分解ごとに役集合を紐付けます。不正な fixed meld を含む `Standard` 分解には状況依存 Yaku も付けません。structural Yaku だけが空で役牌やリーチが残る、という不整合を作らないためです。判定は既存 `Meld::shape()` を source of truth にします。

### 役牌

役牌は `Standard` 分解の刻子・槓子だけを対象にし、順子は対象外です。門前の暗刻と fixed meld (`Pon` / `Daiminkan` / `Ankan` / `Kakan`) の両方を見ます。面子の牌種は既存 `MeldShape::triplet_tile_type()` から取ります。

| Yaku | 条件 |
| --- | --- |
| `YakuhaiWhite` | 白の刻子 / 槓子 |
| `YakuhaiGreen` | 發の刻子 / 槓子 |
| `YakuhaiRed` | 中の刻子 / 槓子 |
| `YakuhaiRoundWind` | `round_wind` と一致する風牌の刻子 / 槓子 |
| `YakuhaiSeatWind` | `seat_wind` と一致する風牌の刻子 / 槓子 |

三元牌は round / seat context に関係なく成立します。風牌は既知の軸だけを見て、unknown な軸から役牌を推測しません。東場で自風が unknown なら、東の刻子は `YakuhaiRoundWind` だけが確定します。

`Yaku::Yakuhai` のような generic な1 variant にはしません。役一覧は sort + dedup されるため、重複数で翻数を表す設計ではダブ風や複数役牌の情報が失われるからです。場風と自風が同じ東場・東家の東刻子は `YakuhaiRoundWind` と `YakuhaiSeatWind` の2つの成立事実として保持され、将来の翻数 layer が自然に2翻として集計できます。

役牌は `Shousangen` と排他ではありません。白刻子・發刻子・中雀頭の手では、同じ分解が `Shousangen` / `YakuhaiWhite` / `YakuhaiGreen` を保持します。

### 状況依存役

| Yaku | 条件 |
| --- | --- |
| `Riichi` | 門前かつ `riichi == Riichi` |
| `DoubleRiichi` | 門前かつ `riichi == DoubleRiichi` |
| `Ippatsu` | 門前かつリーチ宣言が確認でき、`ippatsu == Some(true)` |
| `MenzenTsumo` | 門前かつ `win_method == Tsumo` |
| `Chankan` | `win_method == Ron` かつ `chankan == Some(true)` |
| `RinshanKaihou` | `win_method == Tsumo` かつ `rinshan == Some(true)` |
| `Haitei` | `win_method == Tsumo` かつ `remaining_live_tiles == Some(0)` かつ `rinshan == Some(false)` |
| `Houtei` | `win_method == Ron` かつ `remaining_live_tiles == Some(0)` かつ `chankan == Some(false)` |

門前判定は既存 `is_menzen()` が source of truth です。`Ankan` は門前を維持するため、暗槓だけの手でも `Riichi` / `MenzenTsumo` が成立します。副露手では context がリーチを示していても `Riichi` / `DoubleRiichi` / `Ippatsu` を付けません。

「最初の打牌だったか」「リーチ後に鳴きや槓が入っていないか」「他家が加槓したか」は評価器が河や meld から再構築しません。すべて `WinningContext` の事実を source of truth にします。

`RinshanKaihou` は自摸和了なので、門前なら `MenzenTsumo` と同時に成立します。

### 最後の牌の出所 (`Haitei` / `Houtei`)

WRC Rules 2025 `11.5.1 One han yaku` では、`Haitei` は live wall の最後の牌を自摸しての和了です。その最後の牌が槓の replacement tile なら `RinshanKaihou` だけが成立し `Haitei` は付きません。`Houtei` は wall の最終牌が引かれた後に捨てられた牌をロンした和了です。`Chankan` は加槓に使われた牌をロンする特殊和了で、その牌は捨て牌ではありません。

`remaining_live_tiles == Some(0)` は「live wall に残りツモ可能牌がない」ことしか表さず、和了牌がどこから来たかは決めません。そのため成立には牌の出所を確定する fact をもう1つ要求します。

- `Haitei` は `rinshan == Some(false)`、つまり「嶺上牌での和了ではないと確認済み」を要求します。
- `Houtei` は `chankan == Some(false)`、つまり「加槓牌のロンではないと確認済み」を要求します。

どちらも `None` は unknown なので成立を推測しません。`rinshan == None` の自摸和了を `Haitei`、`chankan == None` のロンを `Houtei` と扱いません。これは `WinningContext` 全体の「unknown を `false` と推測しない」方針と同じ扱いです。

`Houtei` の判定に `rinshan` は使いません。WRC Rules 2025 では最終 wall tile が live wall / dead wall のどちらから引かれたかを問わないため、replacement tile を引いた後の捨て牌をロンした場合も `Houtei` の対象になり得ます。

### 状況依存役の排他

不可能な組み合わせは判定時点で作りません。矛盾した入力を `Result` で拒否するのではなく、成立条件を満たすものだけ返す既存 API style に合わせています。

- `Riichi` と `DoubleRiichi` は `RiichiStatus` が1つの状態しか取れないため同時に立ちません。
- `Haitei` と `RinshanKaihou` は排他です。嶺上牌は live wall の最後の牌ではないため、`rinshan == Some(true)` の自摸和了へ `Haitei` を付けません。
- `Houtei` と `Chankan` は排他です。加槓牌は捨て牌ではないため、`chankan == Some(true)` のロンへ `Houtei` を付けません。
- `WinMethod` と矛盾する役を付けません。`Tsumo` へ `Chankan` / `Houtei` を、`Ron` へ `MenzenTsumo` / `RinshanKaihou` / `Haitei` を付けません。

### まだ入れない役

- `Pinfu` / `Sanankou` は和了牌と待ち形の解釈が必要なため、`evaluate_yaku()` へは入れません。同じ分解でも和了牌の placement で結果が変わるため、`YakuEvaluation` へ和了牌を optional field として足さず、後述の [decomposition × interpretation ごとの Yaku](#decomposition--interpretation-ごとの-yaku) を別の層として持ちます。
- `Renhou` は WRC では5翻相当ですが、他の役やドラと複合しない特殊な scoring semantics を持つため、この cumulative な役 layer へ入れません。
- 役満は `Yaku` へ足さず、別型の `Yakuman` として [役満](#役満) で判定します。
- 翻数は引き続き `Yaku` に持たせません。`Riichi = 1` / `DoubleRiichi = 2` のような換算と食い下がりは後続の翻数 layer の責務です。通常ドラ / 裏ドラ / 赤ドラも役ではないため `Yaku` に入れません。

## 和了牌の解釈

`interpret_winning_tile(&analysis, winning_tile)` は完成手の構造と判明している和了牌だけを入力に取り、その和了牌がどの雀頭 / 面子を完成させたのかを分解ごとに列挙する pure helper です。平和 / 三暗刻 と将来の四暗刻 / 符計算が同じ解釈を再利用でき、役ごとに「和了牌が刻子を完成させたか」を再計算しないようにするための層です。

```rust
let analysis = analyze_completed_hand(&concealed_tiles, &fixed_melds)?;
for interpretation in interpret_winning_tile(&analysis, winning_tile) {
    let decomposition = interpretation.decomposition();
    let group = interpretation.group();
    let wait = interpretation.wait();
}
```

入力は `TileType` です。待ち形の解釈に赤5と黒5の区別は影響せず、赤ドラ集計で必要になる物理牌は `CompletedHandAnalysis` が `TileId` として既に保持しているため、ここで物理牌を再構築しません。

完成手をここで再分解しません。`analyze_completed_hand()` が返した `CompletedHandDecomposition` を唯一の構造 source of truth にし、向聴数・受け入れ・待ち列挙も再計算しません。

| 入力 | 結果 |
| --- | --- |
| 完成形 + 門前部分にある和了牌 | 成立する解釈をすべて含む `Vec` |
| 未完成 (`is_complete() == false`) | 空の `Vec` |
| 和了牌の牌種が門前部分にない | 空の `Vec` |

専用のエラー型を足さず、既存 API と同じく成立するものだけを返す表現にしています。この helper は pure な基盤としてだけ使い、`act()` / `diagnose()` の行動選択へは接続しません。

### 待ち形は手牌全体の待ち一覧ではない

WRC Rules 2025 (`11.3 Minipoints`) の待ち符は「手牌全体に他の待ちがあるか」ではなく、和了牌が実際に完成させた group / pair を基準にします。そのため `WaitType` は手牌の待ち一覧ではなく、和了牌1枚の placement から決まります。

| `WaitType` | 意味 |
| --- | --- |
| `Ryanmen` | 順子を両面形から完成させた |
| `Kanchan` | 順子の中央牌を完成させた |
| `Penchan` | WRC の edge wait。`12` へ `3`、`89` へ `7` |
| `Tanki` | 雀頭を完成させた |
| `Shanpon` | 門前の刻子を完成させた |
| `KokushiSingle` | 国士無双で対子牌以外を完成させた |
| `KokushiThirteenSided` | 国士無双で対子牌を完成させた |

`Penchan` は完成した順子が `123` / `789` であることでは決まりません。和了牌が順子のどの位置を埋めたかで決まります。

| 完成した順子 | 和了牌 | `WaitType` |
| --- | --- | --- |
| `123` | `3` | `Penchan` |
| `123` | `2` | `Kanchan` |
| `123` | `1` | `Ryanmen` |
| `789` | `7` | `Penchan` |
| `789` | `8` | `Kanchan` |
| `789` | `9` | `Ryanmen` |

牌番号の演算は既存 `TileType::sequence()` / `number()` を使い、raw index の演算を別実装しません。

`WaitType` は事実だけを表し、符を持ちません。七対子の対子完成も構造としては `Tanki` ですが、七対子が固定25符であることとは結び付けません。符への換算は後続の符 layer の責務です。

### 和了牌が何を完成させたか

`WaitType` だけでなく、和了牌が完成させた group 自体も `WinningGroup` として保持します。

| `WinningGroup` | 対応する `WaitType` |
| --- | --- |
| `Pair { tile }` | `Tanki` / `KokushiThirteenSided` |
| `Sequence { start }` | `Ryanmen` / `Kanchan` / `Penchan` |
| `Triplet { tile }` | `Shanpon` |
| `KokushiSingle { tile }` | `KokushiSingle` |

平和は `WinningGroup::Sequence` と `WaitType::Ryanmen` を、三暗刻・四暗刻・符は `WinningGroup::Triplet` を後続 layer が直接読めます。`WinningGroup::meld_shape()` は順子・刻子を既存 `MeldShape` へ正規化します。

### `WinningContext` との責務分離

待ち形は完成手の構造と和了牌だけで決まるため、この helper は `WinMethod` を受け取りません。和了時の event facts (Ron / Tsumo、場風、自風、リーチ、一発、嶺上、槍槓、残りツモ可能枚数) は `WinningContext` の責務のままにします。

ここでは「和了牌がどの刻子を完成させたか」という事実だけを保持し、その刻子が暗刻か明刻かは後続 layer が `WinningTileInterpretation` と `WinningContext::win_method()` を組み合わせて決めます。WRC Rules 2025 `11.5.2 San'ankō` では、和了牌が完成させた刻子を自摸なら暗刻、ロンなら明刻として扱うためです。同じ組み合わせを `11.3 Minipoints` の符計算でも使います。

### 同じ decomposition の複数解釈

同じ `CompletedHandDecomposition` でも、和了牌の置き場所が複数あることがあります。分解ごとに `WaitType` を1つへ決め打ちせず、`decomposition` と和了牌の placement の組み合わせをすべて返します。

```text
123m 345m 456p 789s 55p  和了牌 3m
  ↓ 同じ分解
3m が 123m を完成 → Penchan
3m が 345m を完成 → Ryanmen
```

```text
123m 33m 456p 789p 123s  和了牌 3m
  ↓ 同じ分解
3m が雀頭を完成 → Tanki
3m が 123m を完成 → Penchan
```

この層は評価ではなく事実の列挙なので、最も良い待ちを1つ選びません。どの解釈が最終得点になるかは、将来の `decomposition` × `interpretation` → 役 / 符 → 点数の比較で決めます。

### fixed meld は解釈の対象にしない

`Chi` / `Pon` / `Daiminkan` / `Ankan` / `Kakan` は和了前から確定済みです。和了牌の placement 候補は `StandardDecomposition::pair()` と `StandardDecomposition::concealed_melds()` だけで、fixed meld を和了牌が完成させたという解釈は作りません。和了牌と同じ牌種の `Pon` があっても `Shanpon` にはならず、門前側の解釈だけを返します。

和了牌は和了時の門前部分に入った牌です。牌種が `CompletedHandAnalysis::concealed_tiles()` にない場合は解釈を返さず、fixed meld にしか存在しない牌種を和了牌として解釈しません。

### 七対子と国士無双

- 七対子: `ChiitoitsuDecomposition::pairs()` を source of truth にし、和了牌が7種の対子牌のいずれかなら `Tanki` を返します。牌数から七対子を再判定しません。fixed meld がある七対子を `CompletedHandAnalysis` が生成しない既存 semantics もそのままです。
- 国士無双: `KokushiDecomposition::pair()` から判定します。和了牌が対子牌なら和了前は13種すべて1枚だったので `KokushiThirteenSided`、それ以外なら和了前は別牌が対子で和了牌の牌種だけが欠けていたので `KokushiSingle` です。WRC Rules 2025 は特定待ちによる double yakuman を採用しませんが、この層は点数ではなく待ち構造の事実なので、通常の5種へ押し込めず情報を残します。役満の判定と点数は引き続き未実装です。

### 重複排除と決定的な順序

同一の門前順子が2組ある場合、どちらへ和了牌を割り当てても同じ `WinningGroup` と `WaitType` になり、将来の scoring 結果も一致します。この意味的に同一な解釈は canonical な `(WinningGroup, WaitType)` へそろえたうえで sort + dedup し、探索順の違いだけで重複を返しません。`Penchan` と `Ryanmen` のように scoring 上の意味が異なる解釈は dedup しません。

順序は分解の順序が外側で、その中を `WinningGroup` → `WaitType` の順に並べます。同じ入力なら常に同じ順序を返します。

### 和了牌が不明な場合

WRC Rules 2025 では和了牌が不明で役や符が曖昧になる場合、その曖昧な役 / 符は得点できません。この helper は判明している和了牌だけを解釈し、和了牌が不明であることを `TileType` のダミー値では表現しません。和了牌が不明な場合は後続の scoring layer が `interpret_winning_tile()` を呼ばず、和了牌に依存する役と符を付けない、という切り分けにします。

## decomposition × interpretation ごとの Yaku

`evaluate_winning_yaku(&analysis, context, winning_tile)` は `evaluate_yaku()` と `interpret_winning_tile()` を組み合わせ、和了牌の placement まで確定した単位で役を返します。

```rust
let analysis = analyze_completed_hand(&concealed_tiles, &fixed_melds)?;
let context = WinningContext::new(WinMethod::Ron)
    .with_round_wind(round_wind)
    .with_seat_wind(seat_wind);
for evaluation in evaluate_winning_yaku(&analysis, context, winning_tile) {
    let interpretation = evaluation.interpretation();
    let decomposition = evaluation.decomposition();
    let yaku = evaluation.yaku();
}
```

`WinningYakuEvaluation` は `WinningTileInterpretation` と役一覧だけを持ちます。和了牌 / `WinningGroup` / `WaitType` / 分解を別 field へ複製せず、`WinningTileInterpretation` を唯一の source of truth にします。`decomposition()` は `interpretation().decomposition()` をそのまま返します。

`Pinfu` / `Sanankou` 以外の役をここで再判定しません。同じ分解の `evaluate_yaku()` の結果をそのまま引き継ぎ、和了牌依存の役だけを足してから sort + dedup します。`Tanyao` / `Riichi` / 役牌のような structural / 状況依存役の判定は `evaluate_structural_yaku()` と `evaluate_yaku()` の1か所だけに残ります。

### 既存 `evaluate_yaku()` との責務分離

| API | 責務 |
| --- | --- |
| `evaluate_structural_yaku()` | 牌姿だけで確定する役 |
| `evaluate_yaku()` | structural + `WinningContext` の状況依存役 |
| `evaluate_winning_yaku()` | 上記 + 和了牌の placement に依存する役 |

`evaluate_yaku()` へ和了牌を必須引数として足さず、既存 semantics のまま残します。和了牌が不明な場合は `evaluate_yaku()` を使い、`Pinfu` / `Sanankou` を付けません。将来の確定 `HandValue` は `evaluate_winning_yaku()` を使う想定です。

`interpret_winning_tile()` が空を返す場合、`evaluate_winning_yaku()` も空を返します。未完成手、和了牌の牌種が門前部分にない場合、有効な解釈がない場合に和了牌を別ロジックで推測しません。

### 解釈ごとに役が変わる

同じ分解でも和了牌の placement によって成立する役が変わります。最も高い解釈を選ぶのは後続の点数比較 layer の責務なので、この層では候補をすべて保持します。

```text
123m 345m 456p 789s 55p  和了牌 3m
  ↓ 同じ分解
3m が 123m を完成 → Penchan → Pinfu 不成立
3m が 345m を完成 → Ryanmen → Pinfu 成立
```

```text
234m 333m 555p 777s 99p  和了牌 3m をロン
  ↓ 同じ分解
3m が 234m を完成 → 暗刻3つを維持 → Sanankou 成立
3m が 333m を完成 → その刻子は明刻 → Sanankou 不成立
```

analysis 全体や分解単位で役を union しません。返却する解釈の個数と順序は `interpret_winning_tile()` と一致し、役一覧が同じになった解釈同士を dedup しません。dedup の source of truth は `interpret_winning_tile()` だけです。

不正な fixed meld を含む `Standard` 分解には、既存 `evaluate_yaku()` と同じく和了牌依存の役も付けません。判定は `Meld::shape()` と `standard_meld_shapes()` を source of truth にし、「structural / 状況依存役は空なのに `Pinfu` / `Sanankou` だけ付く」という不整合を作りません。

### `Pinfu`

WRC Rules 2025 `11.5.1 Pinfu` の条件をそのまま実装します。次をすべて満たす解釈だけが `Pinfu` です。

- `Standard` 分解であること
- 門前であること (`is_menzen()`)
- fixed meld を含めた4面子すべてが `MeldShape::Sequence`
- 雀頭が非役牌と確認できること
- `WinningGroup::Sequence` であること
- `WaitType::Ryanmen` であること

門前判定は `is_menzen()` が source of truth です。`Ankan` は門前を維持しますが `MeldShape::Kan` なので「4面子すべて順子」を満たさず、別条件で不成立になります。`Chi` / `Pon` / `Daiminkan` / `Kakan` は門前ではないため不成立です。

4面子は `StandardDecomposition::concealed_melds()` だけでなく `standard_meld_shapes()` で fixed meld も含めて見ます。同じ面子構築を `Pinfu` 用に再実装しません。

`Kanchan` / `Penchan` / `Tanki` / `Shanpon` と国士の待ちでは成立しません。`WaitType` に加えて `WinningGroup` が順子であることも明示的に確認します。

#### unknown な風を客風と推測しない

雀頭が三元牌なら常に不成立、数牌なら風 context に関係なく成立可能です。風牌の雀頭だけは「非役牌であることを確認できた」ことを要求します。

| 雀頭 | `round_wind` | `seat_wind` | `Pinfu` |
| --- | --- | --- | --- |
| 数牌 | 任意 | 任意 | 成立可能 |
| 三元牌 | 任意 | 任意 | 不成立 |
| 西 | 東 | 南 | 成立可能 |
| 西 | 東 | unknown | 不成立 |
| 西 | unknown | 南 | 不成立 |
| 東 | 東 | unknown | 不成立 |

`TileType::is_value_honor(round_wind, seat_wind)` は unknown な軸を「一致しない」として扱うため、そのまま使うと unknown を客風と推測してしまいます。風牌の雀頭では `round_wind` と `seat_wind` の両方が判明している場合だけ `is_value_honor()` に問い合わせ、片方でも unknown なら `Pinfu` を付けません。`WinningContext` 全体の「unknown を `false` と推測しない」方針と同じ扱いです。

### `Sanankou`

WRC Rules 2025 `11.5.2 San'ankō` の「暗刻 / 暗槓が3つ」を、和了後の concealed set 数から判定します。門前限定ではありません。`Chi` を1つ副露していても暗刻3つがそろえば成立します。`is_menzen()` を必要条件にしません。

concealed set は `concealed_set_count(&interpretation, fixed_melds, win_method)` が数えます。`Sanankou` 固有の helper ではなく、`StandardDecomposition` / fixed meld / 解釈 / `WinMethod` から和了後の暗刻・暗槓数だけを返す neutral な pure helper です。将来の `Suuankou` と符計算が同じ判定を再実装しなくて済みます。

| 面子 | concealed set |
| --- | --- |
| 門前の暗刻 (`ConcealedMeld::Triplet`) | 数える |
| `Ankan` | 数える |
| `Pon` / `Daiminkan` / `Kakan` / `Chi` | 数えない |
| 牌構成が不正な fixed meld | 数えない |

open / closed の判定は既存 `Meld::is_open()` と `Meld::shape()` が source of truth で、fixed meld の判定を別実装しません。

#### ロンで完成した刻子は明刻

WRC Rules 2025 では、和了牌が刻子を完成させた場合、自摸なら暗刻、ロンなら明刻として扱います。

| 解釈 | `WinMethod` | その面子 |
| --- | --- | --- |
| `WinningGroup::Triplet` | `Tsumo` | 暗刻として数える |
| `WinningGroup::Triplet` | `Ron` | 明刻として数えない |
| `WinningGroup::Sequence` / `Pair` | 任意 | 他の暗刻はそのまま維持 |

最終的な分解上は `ConcealedMeld::Triplet` でも、ロンでその刻子を完成させた解釈では暗刻ではありません。逆に「ロンだから暗刻を一律1つ減らす」こともしません。減らすのは `Ron` かつ `WinningGroup::Triplet` の解釈だけです。暗刻3つを持ちロン牌が順子や雀頭を完成させた場合は `Sanankou` が成立します。

#### `Suuankou` との境界

`Sankantsu` が槓子ちょうど3つで `Suukantsu` を残しているのと同じく、`Sanankou` も和了後の concealed set がちょうど3のときだけ成立します。

| 形 | concealed set | 結果 |
| --- | --- | --- |
| 暗刻4つを自摸 | 4 | `Sanankou` なし (`Yakuman::Suuankou`) |
| 暗刻4つで雀頭をロン | 4 | `Sanankou` なし (`Yakuman::Suuankou`) |
| 4刻子形でそのうち1つをロンで完成 | 3 | `Sanankou` |

`Suuankou` 自体は通常 `Yaku` ではなく [役満](#役満) 側で判定します。

### production policy へは接続しない

この評価器も pure な `HandValue` 基盤としてだけ使い、`ShantenAgent` / リーチ判断 / 押し引き / ベタオリ / 打牌比較 / lookahead / 鳴き判断へは接続しません。`act()` / `diagnose()` の行動選択は変わらず、通常の行動選択のために `CompletedHandAnalysis` / `WinningTileInterpretation` / 役評価を新しく構築しません。診断表示のために役判定を再実装することもしません。検証は `bot-logic` の unit test を中心に行います。

## 役満

named 役満は通常役と別の型 `Yakuman` で表します。`Yaku` enum へ役満 variant を足しません。後続 layer の扱いが3つとも異なるためです。

```text
通常 Yaku → 翻数集計
Yakuman   → 役満 / limit-hand scoring
ドラ      → bonus 翻
```

`Yakuman` は「その役満が成立している」という事実だけを表します。点数・倍率・親子の支払いを持たず、`value()` / `multiplier()` / `points()` のような API も持ちません。

```rust
pub enum Yakuman {
    KokushiMusou,
    ChuurenPoutou,
    Ryuuiisou,
    Suuankou,
    Suukantsu,
    Chinroutou,
    Tsuuiisou,
    Daisangen,
    Shousuushii,
    Daisuushii,
}
```

### decomposition ごとの structural 役満

`evaluate_yakuman(&analysis)` は `CompletedHandAnalysis` だけを入力に取り、和了牌と `WinMethod` がなくても確定する役満を分解ごとに返します。返却順と個数は `CompletedHandAnalysis::decompositions()` と一致します。

```rust
let analysis = analyze_completed_hand(&concealed_tiles, &fixed_melds)?;
for evaluation in evaluate_yakuman(&analysis) {
    let decomposition = evaluation.decomposition();
    let yakuman = evaluation.yakuman();
}
```

通常役と同じく analysis 全体へ役満を union しません。牌構成だけで決まる役満が複数の分解すべてで成立する場合は、それぞれの分解が同じ事実を持ちます。どの解釈を採用するかは後続の `HandValue` layer の責務です。

### decomposition × interpretation ごとの役満

`Suuankou` だけは分解・和了牌の placement・`WinMethod` の3つがそろって初めて確定します。既存 `evaluate_winning_yaku()` と同じ責務分離で、`evaluate_winning_yakuman(&analysis, context, winning_tile)` を別 API にします。

```rust
for evaluation in evaluate_winning_yakuman(&analysis, context, winning_tile) {
    let interpretation = evaluation.interpretation();
    let yakuman = evaluation.yakuman();
}
```

この API は `evaluate_yakuman()` と `interpret_winning_tile()` を組み合わせるだけです。各解釈へ同じ分解の structural 役満をそのまま引き継ぎ、解釈依存の役満だけを足してから sort + dedup します。`Suuankou` のために structural 役満を再判定しません。`WinningYakumanEvaluation` は `WinningTileInterpretation` と役満一覧だけを持ち、和了牌 / `WinningGroup` / `WaitType` / 分解を複製しません。

| API | 責務 |
| --- | --- |
| `evaluate_yakuman()` | 牌姿と面子構成だけで確定する役満 |
| `evaluate_winning_yakuman()` | 上記 + 和了牌の placement と `WinMethod` に依存する `Suuankou` |

### 成立条件

| Yakuman | 判定 API | 条件 |
| --- | --- | --- |
| `KokushiMusou` | `evaluate_yakuman()` | `CompletedHandDecomposition::Kokushi` であること |
| `ChuurenPoutou` | `evaluate_yakuman()` | fixed meld が1つもなく、全14牌が同一数牌スートで、1と9が3枚以上、2〜8が各1枚以上 |
| `Ryuuiisou` | `evaluate_yakuman()` | 全牌が `2s` / `3s` / `4s` / `6s` / `8s` / 發のいずれか |
| `Chinroutou` | `evaluate_yakuman()` | 全牌が老頭牌 (`TileType::is_terminal()`) |
| `Tsuuiisou` | `evaluate_yakuman()` | 全牌が字牌 (`TileType::is_honor()`) |
| `Daisangen` | `evaluate_yakuman()` | 三元牌3種すべてが刻子 / 槓子 |
| `Shousuushii` | `evaluate_yakuman()` | 風牌の刻子 / 槓子が3種で、残り1種の風牌が雀頭 |
| `Daisuushii` | `evaluate_yakuman()` | 風牌4種すべてが刻子 / 槓子 |
| `Suukantsu` | `evaluate_yakuman()` | `Standard` の4面子すべてが `MeldShape::Kan` |
| `Suuankou` | `evaluate_winning_yakuman()` | 門前かつ和了後の concealed set が4 |

`Ryuuiisou` は發を必須にしません。判定は raw index ではなく `TileType::is_sou()` / `number()` / `Dragon::Green` の意味で行います。`Chinroutou` は `is_yaochu()` ではなく `is_terminal()` を使い、字牌を含む手を除外します。

牌構成で決まる `ChuurenPoutou` / `Ryuuiisou` / `Chinroutou` / `Tsuuiisou` は `Standard` に限定しません。七対子の `EE SS WW NN PP FF CC` にも `Tsuuiisou` が成立します。

`Daisangen` / `Shousuushii` / `Daisuushii` / `Suukantsu` は open / concealed を問いません。副露した刻子・槓子も既存 `standard_meld_shapes()` の結果として同じに数えます。

### `Suuankou` のロン / 自摸の境界

WRC Rules 2025 の四暗刻は暗刻 / 暗槓を4つ持ち、自摸和了か雀頭を完成させるロンで成立します。concealed set は既存 `concealed_set_count()` を再利用し、四暗刻のために暗刻数を数え直しません。ロンで刻子を完成させた解釈でその刻子が明刻になる semantics も既存のままです。

| 形 | concealed set | 結果 |
| --- | --- | --- |
| 暗刻4つを自摸 (刻子完成) | 4 | `Suuankou` |
| 暗刻4つを自摸 (雀頭完成) | 4 | `Suuankou` |
| 暗刻4つで雀頭をロン | 4 | `Suuankou` |
| 4刻子形でそのうち1つをロンで完成 | 3 | `Suuankou` なし (`Sanankou`) |

`Ankan` は concealed set として数え、`Pon` / `Daiminkan` / `Kakan` は数えません。`Ankan` は門前を維持するため `is_menzen()` も満たします。

### `ChuurenPoutou` と槓子の境界

WRC Rules 2025 の九蓮宝燈は門前限定で、槓を宣言していると成立しません。nodocchi では Kan が fixed meld に入るため、`is_menzen()` だけでは `Ankan` を除外できません。そのため fixed meld が1つもないことを条件にします。

牌構成だけで判定し、`Standard` 分解の順子 / 刻子を解析しません。`1112345678999` の基底形に同スートの余剰1枚が加わった牌構成を、既存 `TileCounts` の牌種カウントで確認します。

### 特定待ちを double 役満にしない

WRC Rules 2025 は特定の待ちによる double 役満を採用しません。既存 `WaitType` の structural fact はそのまま残しますが、役満の事実は1つだけです。

| 待ち | Yakuman |
| --- | --- |
| `WaitType::KokushiSingle` | `KokushiMusou` 1つ |
| `WaitType::KokushiThirteenSided` | `KokushiMusou` 1つ |
| 四暗刻単騎 (雀頭のロン) | `Suuankou` 1つ |
| 純正九蓮相当の待ち | `ChuurenPoutou` 1つ |

`KokushiMusouThirteenSided` / `SuuankouTanki` / `DoubleSuuankou` のような variant を作りません。`Daisuushii` も特定待ちで2倍にしません。

13翻以上の非役満手 (counted yakuman) は WRC では scoring limit 側の扱いで、named 役満ではありません。`KazoeYakuman` variant を `Yakuman` へ入れません。

### 複数の役満を1つに絞らない

WRC Rules 2025 では異なる役満が複数成立した場合、複合役満として数えます。そのため評価器は「最も強そうな役満を1つ選ぶ」実装にせず、同じ分解 / 解釈で成立した事実をすべて保持します。

```text
EEE SSS WWW NNN PP を自摸
  ↓ 同じ解釈
Suuankou + Tsuuiisou + Daisuushii
```

複合時の最終点数はまだ計算しません。この層の出力は `decomposition` / `interpretation` と `Vec<Yakuman>` までです。

`Daisuushii` が成立する分解へ `Shousuushii` を同時に付けません。上位下位が重なる組み合わせは判定時点で排他にします。

### 通常役との関係

役満が成立しても既存 `evaluate_yaku()` / `evaluate_winning_yaku()` の通常役を削除・抑制しません。役満手でも `Toitoi` / 役牌 / `MenzenTsumo` などの通常役の事実がそのまま返ります。「named 役満が1つ以上なら通常翻とドラを加算しない」といった最終ルールは後続の `HandValue` / scoring layer の責務です。役満 layer と通常役 layer を相互依存させません。

### 不正な fixed meld

`CompletedHandAnalysis` は fixed meld の構造を完全には検証しません。既存役と同じく `Meld::shape()` と `standard_meld_shapes()` を検証の source of truth にします。不正な fixed meld を含む `Standard` 分解には役満を付けません。牌構成だけで決まる役満も同様で、「面子構成は不正だが牌構成が合っていたので役満」という不整合を作りません。`Chiitoitsu` / `Kokushi` は設計上 fixed meld を持たないため、同じ検証を重複させません。

### 決定的な結果

各 evaluation の `Vec<Yakuman>` は sort + dedup 済みで deterministic です。ただし役満一覧が同じという理由で異なる分解 / 解釈を dedup しません。候補の identity は既存 `CompletedHandDecomposition` と `WinningTileInterpretation` が source of truth です。

### まだ入れない役満と scoring

- `Tenhou` / `Chiihou` は入れません。現在の `WinningContext` には、配牌時の親の自摸和了か、子の第一自摸か、それ以前に鳴きや暗槓が発生したかを確定できる事実がありません。`seat_wind == East` かつ自摸だけで `Tenhou` と推測せず、非親の自摸だけで `Chiihou` と推測しません。first-turn / 中断履歴を tri-state の事実として設計してから実装します。
- `Renhou` は WRC では5翻役で、他の役やドラと複合しない特殊な scoring semantics を持ちます。役満ではないため `Yakuman` へ入れず、`Yaku` へも安易に足さず、後続の特殊 scoring layer で扱います。
- 責任払い (包) は入れません。WRC では `Daisangen` / `Daisuushii` / `Suukantsu` に責任払いが関係する場合がありますが、成立した役満そのものと支払い責任は別責務です。誰が最後の面子 / 槓を鳴かせたかを推測・追跡しません。
- 役満の点数、親 / 子の支払い、複合役満の最終点数、翻数、符、ドラ、確定した `HandValue` はまだ実装しません。

### production policy へは接続しない

役満評価器も pure な `HandValue` 基盤としてだけ使い、`ShantenAgent` / リーチ判断 / 押し引き / ベタオリ / 打牌比較 / lookahead / 鳴き判断へは接続しません。`act()` / `diagnose()` / `diagnose_with_options()` の行動選択は変わらず、通常の行動選択のために `CompletedHandAnalysis` / `WinningTileInterpretation` / 役満評価を新しく構築しません。診断表示のために役満判定を別実装することもしません。検証は `bot-logic` の unit test を中心に行います。

## 向聴数との関係

分解は向聴数探索とは独立した完成形専用の parser で、`standard_shanten()` の探索を複製していません。向聴数は検証用の source of truth として使い、分解が得られる牌姿では対応する既存向聴数が `-1` になることを test で固定しています。

production code と pure helper が正確な挙動の source of truth です。
