# 手牌評価

将来の翻数・符・点数計算の共通基盤として、完成済みの手牌がどの和了形へ分解できるかを pure logic で列挙し、その分解ごとに成立する役を判定します。牌構成だけで確定する役に加え、和了時の観測事実を表す `WinningContext` を受け取り、役牌と状況依存役も分解ごとに判定します。翻数や点数はまだ扱いません。

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

未実装:

- 和了牌と待ち形 (両面 / 嵌張 / 辺張 / 単騎 / シャンポン) の解釈
- 平和
- 三暗刻
- 人和
- 役満
- 翻数集計
- 符計算
- 点数計算

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

役満は未実装です。将来の役満評価と二重にならないよう、境界を次のように固定しています。

- `Sankantsu` は槓子がちょうど3つのときだけ成立します。4槓子は `Suukantsu` へ残します。
- `Shousangen` は三元牌2種が刻子 / 槓子で残り1種が雀頭のときだけ成立します。三元牌3種が刻子の `Daisangen` 形では成立しません。
- `CompletedHandDecomposition::Kokushi` は structural Yaku の対象外で、常に空の役集合を返します。

そのため、この評価器が返す空の役集合を「この手には役がない」と解釈する production policy へまだ使えません。和了牌に依存する役と役満が入るまでは pure な基盤としてだけ使い、`act()` / `diagnose()` の行動選択へは接続しません。各分解の役一覧は sort と dedup 済みで deterministic です。

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

- `Pinfu` / `Sanankou` は和了牌と待ち形の解釈が必要なため入れません。特に `Sanankou` は「和了牌がどの刻子を完成させたか」が必要で、Ron / Tsumo だけでは決まりません。
- `Renhou` は WRC では5翻相当ですが、他の役やドラと複合しない特殊な scoring semantics を持つため、この cumulative な役 layer へ入れません。
- 役満は引き続き未実装です。
- 翻数は引き続き `Yaku` に持たせません。`Riichi = 1` / `DoubleRiichi = 2` のような換算と食い下がりは後続の翻数 layer の責務です。通常ドラ / 裏ドラ / 赤ドラも役ではないため `Yaku` に入れません。

## 向聴数との関係

分解は向聴数探索とは独立した完成形専用の parser で、`standard_shanten()` の探索を複製していません。向聴数は検証用の source of truth として使い、分解が得られる牌姿では対応する既存向聴数が `-1` になることを test で固定しています。

production code と pure helper が正確な挙動の source of truth です。
