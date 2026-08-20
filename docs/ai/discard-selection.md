# 打牌選択

通常打牌は打牌後の手牌を評価し、向聴数、Acceptance、ドラ、フリテン、先読み指標などを comparator で比較します。この文書は問題局面を調査するための主要な見方をまとめます。全 enum と tie-break の正確な順序は production code と tests を source of truth としてください。

## shanten と Acceptance

各候補は打牌後の通常手・七対子・国士無双を含む最小向聴を評価します。Acceptance は次に引くことで向聴が進む牌種と、見え牌を差し引いた残枚数です。

`bot-scenario` の `Normal discard candidates` では候補ごとの shanten、acceptance remaining / types、ドラやフリテンの情報を確認できます。見え牌には手牌、ツモ牌、ドラ表示牌、河、副露、`extra_visible_tiles` が反映されます。

## 七対子テンパイの tie-break

打牌後が七対子テンパイになる候補同士は、受け入れ残枚数と受け入れ牌種類数が同値の場合に限り、単騎待ち牌の品質で比較します。

```text
AcceptanceRemaining
→ AcceptanceTypeCount
→ [七対子テンパイ限定] ChiitoitsuWaitQuality
→ IishantenShape
→ ...
```

待ちの品質は `字牌 > 1/9 > 2/8 > 3/7 > 4/6 > 5` の固定順位です。同じ組同士は同品質で、スートや場風・自風・役牌かどうかは区別しません。生き枚数の比較が先なので、品質の低い待ちでも残枚数が多ければそちらを選びます。

適用するのは両候補とも七対子テンパイで、七対子を完成させる牌が一意に定まる場合だけです。通常形テンパイ、国士テンパイ、一向聴以上、副露形には広げません。`bot-scenario` の候補表示では `lost by: ChiitoitsuWaitQuality` として現れます。

## 1向聴: WeightedProspectiveValue

1向聴候補では、受け入れ牌を引いた後に進むテンパイの確定打点まで含めた `weighted prospective value` を最初に比較します。

```text
weighted prospective value
= Σ(1手目の物理牌 variant 残枚数 × Σ(最終和了牌の物理牌 variant 残枚数 × 支払い合計))
```

仮想ツモ牌も最終和了牌も赤5 / 黒5の物理牌 variant へ分けて別々に評価するため、赤5を引く枝では2手目の最良打牌そのものが変わり得ます。

リーチかダマかは押し引き・リーチ判断と同じ policy で決め、その baseline の支払い合計を使います。ダマ打点はロン和了を前提にした値なので、既存のフリテン判定で**ダマでロンできると確定した場合だけ**判断材料と確定値に使います。未来テンパイの恒常フリテンは「現在の自分の河 + 1手目の打牌 + 2手目の打牌」で判定でき、自分の河や履歴依存フリテンが分からない場合は非フリテンだと推測せず unknown のままにします。ロン可否が確定しない枝は、ダマ打点が高くてもダマと確定させず、待ち枚数だけを見る既存のリーチ判断へ委ねます。将来フリテンによる打点の割引や EV 補正は行いません。

これは期待得点でも和了率でもありません。本場・供託・ロン / ツモ確率・放銃率は含めません。点数計算の入力が足りない、ダマでロンできると確定しないなど、打点を確定できない枝がある候補は 0 点にせず値を持ちません。

打点軸を使うかどうかは候補ごとではなく、`Shanten` / `IsolatedTile` / `IsolatedHonor` まで同順位になる候補集合 (cohort) 単位で決めます。cohort の全候補で打点が確定している場合だけ軸を使い、1件でも確定しなければ cohort 全体で軸を無効化して `weighted tenpai wait` 以降へ落とします。比較ごとに軸を切り替えると順序が循環し、候補の列挙順で選択結果が変わってしまうためです。

## 1向聴: WeightedTenpaiWait

打点込みの比較が同値、または片側でも打点を確定できない場合は、受け入れ牌を引いた後にテンパイへ進む各 branch の待ちを重み付きで集約した `weighted tenpai wait` で比較します。単なる現在の受け入れ枚数だけでなく、テンパイ後に残る待ちも比較するための指標です。

これは期待得点や和了率ではありません。対象外または計算不要の候補は `-` です。

## 2向聴以上: WeightedNextAcceptance

2向聴以上では次の指標を表示します。

```text
weighted next acceptance
= Σ(first draw remaining × next acceptance)
```

既存 Acceptance の有効牌を1枚引いた後、既存 comparator が選ぶ次打牌後の受け入れを集計した2手先の牌効率 proxy です。これも期待値や和了率ではありません。

## lookahead

`bot-scenario --lookahead` は通常打牌候補ごとの2手先概要を追加します。`--verbose` と併用すると受け入れ牌ごとの詳細も表示します。

```bash
cargo run -p bot-scenario -- \
  --hand "234m455p789s1123z" \
  --draw "N" \
  --lookahead --verbose
```

受け入れ牌ごとの詳細は、仮想ツモ牌の物理牌 variant ごとに並びます。次打牌後がテンパイになる枝では最終待ちと、打牌選択が実際に使った打点 (`selection value`)、採用した baseline とロン可否、ダマ / リーチ両方の打点を表示します。打点は待ち牌種ごと・和了牌の赤5 / 黒5ごとの支払いと、その残枚数加重平均で、役なしや点数計算の入力不足は0点にせずそのまま区別します。

detailed diagnostics そのものは要求した場合だけ構築し、`act()` の通常経路では作りません。枝の評価は通常経路と同じ1本を共有するので、diagnostics の有無で最終 decision は変わりません。表示する `selection value` も打牌選択が使った値そのもので、diagnostics のために打点を求め直しません。

## selected と runner-up

候補の `selected: yes` は通常打牌 comparator の選択です。最終 action は Reach や防御 fallback によって変わる場合があります。最終結果は `Final decision` と `Summary` を確認してください。

`Summary.runner-up` は最終選択を除いた場合に次に選ばれる候補です。候補の比較理由と合わせて、どの比較軸で勝敗が決まったかを調査できます。出力形式は [Structured diagnostics](../diagnostics.md#summary-と-runner-up) を参照してください。
