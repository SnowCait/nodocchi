# 手牌評価

将来の翻数・符・点数計算の共通基盤として、完成済みの手牌がどの和了形へ分解できるかを pure logic で列挙し、その分解ごとに牌構成だけで確定する役を判定します。翻数や点数はまだ扱いません。

## 実装済みと未実装

実装済み:

- 完成手の構造解析 (`analyze_completed_hand()`)
- `Standard` / `Chiitoitsu` / `Kokushi` の3 family
- 通常形の複数解釈をすべて列挙
- fixed meld (Chi / Pon / Daiminkan / Ankan / Kakan) 対応
- decomposition ごとの structural Yaku 判定 (`evaluate_structural_yaku()`)
- 牌構成・面子構成だけで確定する通常役 (下表)

未実装:

- 状況依存役 (リーチ / 一発 / 門前清自摸和 / 海底 / 河底 / 嶺上開花 / 槍槓)
- 場風・自風 context が必要な役牌
- 和了牌・待ち形に依存する役 (平和 / 三暗刻など)
- 役満
- 翻数集計
- 符計算
- 点数計算
- 和了牌と待ち形 (両面 / 嵌張 / 辺張 / 単騎 / シャンポン) の解釈

役の定義は [World Riichi Championship Rules](https://www.worldriichi.org/wrc-rules) と [EMA Riichi Competition Rules](https://mahjong-europe.org/portal/index.php?Itemid=166&id=30&option=com_content&view=article) を一次情報とします。

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

`evaluate_structural_yaku(&analysis)` は `CompletedHandAnalysis` を唯一の入力とし、`StructuralYakuEvaluation` を分解と同じ順序で返します。各 evaluation は判定元の `CompletedHandDecomposition` と、その分解で成立する `Yaku` を保持します。

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
- `CompletedHandDecomposition::Kokushi` は今回の通常役の対象外で、常に空の役集合を返します。

そのため、この評価器が返す空の役集合を「この手には役がない」と解釈する production policy へまだ使えません。状況依存役・役牌・役満が入るまでは pure な基盤としてだけ使います。各分解の役一覧は sort と dedup 済みで deterministic です。

## 向聴数との関係

分解は向聴数探索とは独立した完成形専用の parser で、`standard_shanten()` の探索を複製していません。向聴数は検証用の source of truth として使い、分解が得られる牌姿では対応する既存向聴数が `-1` になることを test で固定しています。

production code と pure helper が正確な挙動の source of truth です。
