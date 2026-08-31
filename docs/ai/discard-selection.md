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

## 現在聴牌: CurrentTenpaiOffenseWeightedTotal

現在打牌の直後が聴牌になる候補同士では、既存の Reach / Damaten policy と手牌価値評価で求めた `current tenpai offense weighted total` を、現在の Acceptance より先に比較します。

```text
Shanten → existing pre-acceptance axes
→ [聴牌のみ] CurrentTenpaiOffenseWeightedTotal
→ AcceptanceRemaining → AcceptanceTypeCount → ...
```

値は生きた和了牌を赤5 / 黒5の physical variant まで分け、既存 `Payment.total()` を残枚数で重み付けした合計です。

```text
current tenpai offense weighted total
= Σ(生きた和了牌 physical variant の残枚数 × Payment.total())
```

平均打点ではありません。例えば「1枚 × 12,000点」は12,000、「6枚 × 3,900点」は23,400なので、後者を上位にします。また、これはロン / ツモ確率を掛けたEVではなく、本場・供託・点棒状況も含みません。

攻撃モードは既存 production policy と同じです。既にリーチ済みならReach、未リーチなら既存の `decide_reach_reason()` に従ってReachまたはDamatenを選びます。ダマ値は既存フリテン判定で `can_ron == Some(true)` と確定した場合だけ利用し、ロン可否unknown・役なし・点数計算不能などを0点とは扱いません。

軸の有効・無効は `Shanten` / `IsolatedTile` / `IsolatedHonor` まで同順位の候補集合 (cohort) 単位で決めます。cohortの全聴牌候補で値が確定した場合だけ使い、1件でもunknownならcohort全体で無効化して従来のAcceptance比較へ戻します。

評価対象は現在打牌直後の待ちと打点だけです。聴牌後に非和了牌を引いて別の待ちへ移る手変わりや、ダマ手変わりの2手先評価は行いません。既存の1向聴・2向聴以上の先読み軸も変更しません。

## 1向聴: ExpectedSelfTsumoValue

1向聴候補では、テンパイまでの経路を実際に引く確率と、テンパイ到達後に自分のツモで和了する期待支払いを掛けた `expected self-tsumo value` を最初に比較します。

```text
expected self-tsumo value
= Σ(その経路を引く確率 × テンパイ到達後の期待ツモ支払い)
```

対象の経路は次の2種類です。すぐ向聴数を下げてテンパイする枝と、一度だけ手変わりしてから次のツモでテンパイする枝を、同じ尺度で比べるための指標です。

```text
A. 1回目のツモが向聴数を下げる → 次打牌 → テンパイ
B. 1回目のツモが向聴数を維持する → 次打牌 → 1向聴
   → 2回目のツモが向聴数を下げる → 次打牌 → テンパイ
```

経路の確率は、現在打牌後に自分から見て未確認の物理牌を母数にした素直な確率です。自分が1枚確認するごとに母数が1減るだけで、相手の自摸で母数を減らしたり、巡目係数を掛けたりはしません。テンパイ到達後は毎巡の全ツモを探索せず、超幾何分布の閉形式で「残っている自分の自摸機会のうちに和了牌を引く確率」を求めます。

打点は既存の手牌価値をツモ和了 (`WinMethod::Tsumo`) の baseline で評価した支払い合計で、ロン baseline の値を流用しません。リーチかダマかは押し引き・リーチ判断と同じ policy で決めます。ツモ baseline で役が無い待ちはその牌で和了できないので、0点として加算せず和了できる待ちから外します。一発・海底・嶺上開花・裏ドラの上振れは既存 baseline と同じく加算しません。

**これは局収支 EV ではありません。** 自分のツモ和了だけを見た offense continuation value で、次のものは一切含めません。

- ロン和了 / 他家の和了率 / 放銃率
- 他家の鳴き / 将来の槓
- ダマのまま進めた場合の手変わり
- 本場 / 供託 / 点棒状況

手変わりは1回までで、2回続けて向聴数が下がらない枝はこの評価モデルの探索範囲外です (確定しない値ではなく、寄与 0 として扱います)。

この軸は「現在打牌後に自分へ残っている自摸機会」が exact に分かる局面でだけ使います。山の残りツモ可能枚数を `floor(remaining_tiles / 4)` で自分の自摸回数へ直すだけで、巡目や河の枚数からの推測はしません。材料が揃わない局面では軸そのものを持たず、下の `weighted prospective value` 以降へ落とします。テンパイのツモ打点を確定できない枝が1つでもある候補も、0点にせず値を持ちません。

軸を使うかどうかは打点軸と同じく cohort 単位で決めます。

## 1向聴: WeightedProspectiveValue

`expected self-tsumo value` が同値、またはその軸を使えない場合は、受け入れ牌を引いた後に進むテンパイの確定打点まで含めた `weighted prospective value` を比較します。

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

仮想ツモが赤5 / 黒5に分かれる牌種では、物理牌ごとに次打牌を評価し、それぞれの残枚数で集約します。この指標に打点は含めないため、赤 / 黒で変わるのは残枚数の内訳だけです。1向聴の `weighted prospective value` はここでは使いません。

## lookahead

`bot-scenario --lookahead` は通常打牌候補ごとの2手先概要を追加します。`--verbose` と併用すると仮想ツモ牌ごとの詳細も表示します。

仮想ツモの対象は、現在打牌後に見え牌を反映して残っている牌のうち、向聴数が下がる牌 (既存 Acceptance そのもの) と向聴数を維持する牌です。向聴数が悪化する牌は対象外です。どちらの分類かは `transition` で確認できます。向聴数を維持する枝が打牌選択へ寄与するのは `expected self-tsumo value` を通じてだけで、`weighted tenpai wait` などの既存指標には寄与しません。

```bash
cargo run -p bot-scenario -- \
  --hand "234m455p789s1123z" \
  --draw "N" \
  --lookahead --verbose
```

仮想ツモ牌ごとの詳細は、仮想ツモ牌の物理牌 variant ごとに並びます。次打牌後がテンパイになる枝では最終待ちと、打牌選択が実際に使った打点 (`selection value`)、採用した baseline とロン可否、ダマ / リーチ両方の打点を表示します。打点は待ち牌種ごと・和了牌の赤5 / 黒5ごとの支払いと、その残枚数加重平均で、役なしや点数計算の入力不足は0点にせずそのまま区別します。

`--lookahead --verbose` では、現在打牌後が1向聴の候補について向聴数を維持する枝をもう1段追います。2手目の打牌後の1向聴が持つ既存 Acceptance を1枚引き、既存 comparator が選ぶ3手目の打牌後のテンパイまで表示します。深い枝は同じ表示を字下げだけ下げて並べ、その枝の合計を `downstream value`、候補ごとの合計を `same-shanten downstream value` に出します。

```text
same-shanten downstream value
= Σ(same-shanten ツモの残枚数
    × Σ(3手目へ進むツモの残枚数
        × Σ(最終和了牌の残枚数 × 支払い合計)))
```

これは期待値ではなく、平均へ正規化しない生の重み付き合計です。ロン / ツモ確率も放銃率も巡目も含めません。枝の深さが違うため `weighted prospective value` とは scale が違い、打牌選択はこの値を使いません。向聴数を維持する枝と下げる枝を確率で統合した尺度は `expected self-tsumo value` です。打点を確定できない枝が1つでもある候補は 0 点にせず値を持ちません。

detailed diagnostics そのものは要求した場合だけ構築し、`act()` の通常経路では作りません。枝の評価は通常経路と同じ1本を共有するので、diagnostics の有無で最終 decision は変わりません。表示する `selection value` も打牌選択が使った値そのもので、diagnostics のために打点を求め直しません。

## selected と runner-up

候補の `selected: yes` は通常打牌 comparator の選択です。最終 action は Reach や防御 fallback によって変わる場合があります。最終結果は `Final decision` と `Summary` を確認してください。

`Summary.runner-up` は最終選択を除いた場合に次に選ばれる候補です。候補の比較理由と合わせて、どの比較軸で勝敗が決まったかを調査できます。出力形式は [Structured diagnostics](../diagnostics.md#summary-と-runner-up) を参照してください。
