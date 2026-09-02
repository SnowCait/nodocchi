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

評価対象は現在打牌直後の待ちと打点だけです。聴牌後に非和了牌を引いて別の待ちへ移る手変わりや、ダマ手変わりの2手先評価は行いません。既存の1向聴・2向聴以上の先読み軸も変更しません。ダマで継続した場合の次の1巡は [現在聴牌のダマ継続 (diagnostics only)](#現在聴牌のダマ継続-diagnostics-only) で観測できますが、この比較には接続していません。

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

## 現在聴牌のダマ継続 (diagnostics only)

`bot-scenario --lookahead` は、現在打牌後が聴牌になる候補について次の枝を `Tenpai continuation` に表示します。

```text
現在聴牌 → 非和了牌を1枚ツモ → 既存 selector の最善打牌 → 再び聴牌
```

枝には、待ちが実際に変わるもの (手変わり) だけでなく、**ツモった牌をそのまま切って元の聴牌・元の待ちを維持するものも含みます**。「今すぐリーチする」と「ダマで継続する」を比べるには、次の1巡で待ちが変わらない場合の価値も同じ枝集合の中に必要になるためです。枝を待ちの変化で分類 (据え置き / 待ち改善 / 打点改善) することはまだ行いません。

**現時点では diagnostics 専用で、打牌選択には接続していません。** `CurrentTenpaiOffenseWeightedTotal` の比較にも、リーチ / ダマ判断にも、押し引きにも使いません。継続 bonus も係数も threshold も持ちません。`CurrentTenpaiOffenseWeightedTotal` や `weighted prospective value` は単位の違う値なので、下の self-tsumo 比較と直接並べることはしません。

枝はすべて既存基盤そのものです。

- 非和了ツモは既存2手先評価の枝を、その牌でダマのまま実際にツモ和了できるかで振り分けたものです。残枚数は見え牌を差し引いたうえで赤5 / 黒5の物理牌 variant まで分け、振り分けも variant 単位で行います。`DrawTransition` の意味そのものは変えません。

  | 仮想ツモ | 扱い |
  | --- | --- |
  | `SameShanten` | 非和了ツモ。継続枝の候補 |
  | `Progress` でダマツモに役がある | 実際に和了する牌。継続枝には入れない |
  | `Progress` だがダマツモでは役なし | 実戦上は非和了ツモ。継続枝の候補 |
  | `Progress` で役の有無を確定できない | 和了枝か継続枝か決められないので、その候補の集計値を持たない |

  構造上は和了形になる牌でも、副露手では役が無くてツモ和了できないことがあります。その牌は実際には和了できず、引いた後に打牌してテンパイを続けられるので継続枝として扱います。役の有無は既存の Damaten Tsumo scoring の結論そのままで、この層が役や翻数を判定し直すことはありません。門前手のツモ和了には必ず門前清自摸和が付くため、この振り分けで継続枝が増えるのは副露手だけです。
- 次打牌は既存 comparator が選んだ `next discard` そのもので、向聴・受け入れ・赤5・ドラ・形・将来打点のどの比較もやり直しません。
- 継続後のテンパイの待ち・攻撃モード・打点は既存の将来打点評価 (`ProductionProspectiveValuator`) が返した値そのものです。

horizon は「1ツモ → 1打牌 → 次の聴牌」で必ず打ち切り、2回目の非和了ツモは追いません。最善打牌後が聴牌に戻らない枝は継続成立として扱いません。

対象は自分が未リーチと確定している局面だけです。既にリーチしていればダマで継続する選択肢が無いので探索せず、自分の席が分からず未リーチかどうかを判断できない局面でも未リーチだとは推測せず探索しません。

### 「今すぐリーチ」と「1巡 defer」の counterfactual 比較

同じ候補について、次の4つを [`expected self-tsumo value`](#1向聴-expectedselftsumovalue) と同じ確率模型・同じ単位 (期待ツモ支払い) で並べます。

```text
U0 = 現在打牌後の unknown physical tiles
n  = 現在打牌後に残っている自分の自摸機会

reach now
  = 現在聴牌を forced Reach の Tsumo baseline で評価した TenpaiTsumoValue を、
    残り自摸機会 n 全体の閉形式へ通した期待支払い

defer → production
defer → forced Reach
defer → forced Damaten
  = 共通の、最初の1自摸で現在待ちを引く Damaten Tsumo
  + Σ(共通の非和了牌 variant の経路確率 × mode 別 terminal tenpai の期待支払い)
```

`reach now` は「今リーチして手変わりせず、現在の待ちのまま残り自摸機会を使い切る」という仮定の値で、production が現在ダマを選ぶかどうかとは無関係な forced Reach baseline です。現在局面でリーチできるかは production のリーチ判断と同じく実際の合法手 (`LegalAction::Reach`) だけが source of truth で、合法手にリーチが無ければ値を作らず unavailable にします。局面から合法条件を組み立て直しません。

継続後の未来テンパイのリーチ / ダマだけは現在の合法手を流用できないため、既存の将来テンパイ判定 (`is_reach_legal()` を将来テンパイの材料で評価) がそのまま持ちます。

```text
現在の reach now     → 実際の LegalAction::Reach
継続後の未来テンパイ → 既存の将来テンパイ Reach 判定
```

従来 `damaten continuation` と表示していた値は、**将来も強制ダマにする値ではありません**。「今はリーチせず1巡待つ」ものの、terminal tenpai の mode は既存 `decide_reach_reason()` が選ぶ production policy でした。この production continuation は意味を変えず `defer → production` として残し、今回 `defer → forced Reach` と `defer → forced Damaten` を counterfactual として分離しました。

3つの defer は、最初のツモ、非和了牌の物理 variant、既存 selector が選んだ `next discard`、`SelfTsumoPath::immediate()` をすべて共有します。切り替えるのは同じ terminal tenpai に適用する Reach / Damaten Tsumo baseline だけです。`defer → forced Reach` は既存の将来 Reach legality が合法とした枝だけを Reach baseline で評価し、違法な枝は 0 点ではなく unavailable にします。`defer → forced Damaten` は Ron の役有無ではなく既存 Damaten Tsumo baseline を使い、副露手で Tsumo が役なしになる physical variant は既存 semantics どおり成功待ちに含めません。

手変わりは1回だけです。terminal tenpai へ到達した後は既存の閉形式で残り自摸機会全体を評価するので、継続後の unknown tiles と自摸機会は経路の semantics どおり `U0 - 1` / `n - 1` になります。ダマツモで実際に和了できる現在待ちは、3 mode 共通の最初の1自摸として1回だけ構築し、継続枝には入れないため二重計上しません。役が無くて和了できない牌は逆に即ツモ側から外れ、継続枝側だけが数えます。

`reach now` と `defer → forced Reach` を比べることで、既存 Reach / Damaten threshold から独立して「今すぐリーチするか、1巡だけ手変わりを見るか」を観測できます。これは診断値であり、production Reach policy は変更していません。

材料が揃わない場合は 0 点ではなく値を持ちません。山の残枚数が分からず自摸機会を確定できない局面、ツモ打点を確定できない現在聴牌、terminal tenpai のツモ打点が確定しない継続枝はどれも `None` です。

**この比較も diagnostics 専用で、どれを選ぶかの結論は持ちません。** winner も `should_reach` も作らず、リーチ判断にも打牌選択にも接続していません。Ron probability は含まず、self-tsumo と Ron baseline の aggregate も作りません。

## Reach / Damaten の判断材料 (diagnostics only)

上の self-tsumo 比較と、production の Reach / Damaten 判断が使っているロン側の材料は、`Reach / Damaten comparison` で1か所にまとめて観測できます。局面ごとに別の section を行き来せずに、選んだ打牌1件分の判断材料を並べて確認するための表示です。

```text
Reach / Damaten comparison
  discard                          通常打牌 selection が実際に選んだ打牌
  production                       既存 Reach 判断の reason と採否
  self-tsumo                       reach now / 3つの defer counterfactual の期待ツモ支払い
  Ron                              reach legal / 2つの Ron baseline / ロン可否 / ダマ verdict
```

self-tsumo は選んだ打牌に対応する `Tenpai continuation` の候補1件の比較そのもので、Ron 側の production facts (reason・ロン可否・ダマ打点) は既存のリーチ判断そのものです。統合のために探索も集計もやり直しません。新しく点数計算するのは下の `reach baseline` だけで、これは production 判断が評価しない観測値です。

### 2つの軸は足しません

self-tsumo と Ron baseline は**単位の違う別の軸**です。

| 軸 | 意味 | 確率を含むか |
| --- | --- | --- |
| self-tsumo | 残り自摸機会でツモ和了する期待支払い | 自摸確率を含む |
| Ron baseline | その待ちでロン和了した場合の支払い | **ロンの発生確率を含まない** |

nodocchi はまだ「他家がその牌を切る確率」の模型を持たないため、Ron baseline を期待値へ変換できません。したがって `reach now self-tsumo + reach ron baseline` のような合計も、係数で重み付けした正規化 score も作りません。ron probability・ron EV・EV 係数・threshold のどれも追加していません。**現時点の値は完全な EV ではなく、将来 Ron 発生確率の模型を導入するまでその状態が続きます。**

### Ron baseline

- `reach baseline` は今リーチしてその待ちでロン和了した場合の最低保証打点です。既存のリーチ baseline (`reach_baseline_context()`) をそのまま使うので、リーチ1翻を含み、一発・裏ドラ・河底のような上振れは加算しません (裏ドラは未観測ではなく「0枚と確定」として扱います)。集約も押し引きの攻撃打点と同じ残枚数加重で、赤5 / 黒5は別 variant のまま残します。実際にリーチできる局面 (合法手に `LegalAction::Reach` がある) かつ既存 Ron availability (`TenpaiWaitAvailability::can_ron()`) が `Some(true)` の場合だけ評価し、フリテンとロン可否 unknown では `unavailable` にします。
- `damaten baseline` はダマのままロン和了した場合の打点で、既存のリーチ / ダマ判断が評価したダマ打点診断そのものです ([手牌価値](hand-value.md) を参照)。ダマでロンできない場合とロン可否が unknown の場合は既存 semantics どおり評価せず `unavailable` にします。**0 点としては扱いません。**

`reach baseline` の評価は**診断経路だけ**で行います。通常の `act()` はこの層を通らないので、完成手 (`TenpaiCompletedHands`) の組み立ても hand-value evaluation も production には入りません。完成手は待ちごとの解析を丸ごと所有する重い値なので、診断のために production の打牌選択へ持ち回らせません。リーチ判断がダマ打点のために組み立てた集合があればその所有権をそのまま受け取り、無い経路でだけ選んだ打牌1件について既存 helper で1回組み立てます (待ちは既存の受け入れから求めるので、向聴も受け入れも計算し直しません)。

フリテンでロンできない局面でも、Tsumo 側の `reach now` と3つの defer counterfactual はそれぞれの既存入力に従う独立した軸として評価します。逆に、ツモれることを理由に `reach baseline` を含む Ron 側の値を確定させることもしません。

### Ron opportunity (structural facts only)

`reach baseline` / `damaten baseline` は「その待ちで和了した場合の支払い」でしかなく、他家がその待ち牌を切る確率を持ちません。その前段として、`Ron` 節の `opportunity` に**待ち牌が公開情報上どう見えるか**を並べます。

```text
Ron
  reach baseline: ...
  can ron: yes
  damaten baseline: ...
  opportunity (structural facts, no ron probability)
    wait 4s
      live copies: 3
      if Reach
        declaration visible: yes
        genbutsu: no
        suited safety
          suji / wall / combined
      if Damaten
        declaration visible: no
    external threats
      reached opponents: 0
      high open-hand targets: 0
```

**これは Ron 確率ではありません。** 追加したのは既存の公開情報から観測できる structural facts だけで、ron probability・discard probability・deal-in probability・「スジなら何%」のような係数・Reach / Damaten のロン率補正はどれも持ちません。self-tsumo と Ron baseline を統合した EV も、winner も、新しい `should_reach` も作りません。

- **live copies** は選んだ打牌後の既存受け入れが持つ残枚数そのものです。見え牌を別経路で数え直しません。残枚数 0 の牌種は待ちとして並べません。赤5 / 黒5は同じ牌種として1件にまとめ、structural safety を共有します。物理 variant ごとの打点は従来どおり Ron baseline 側が別々に持ちます。
- **reach public safety** は「自分が今リーチを宣言した場合、その待ち牌が他家から見てどう見えるか」の evidence です。現物は既存の hard-safety helper (`is_genbutsu_for()`)、数牌は既存 [`SuitedSafetyEvidence`](defense.md#suji--halfsuji) (スジ + 壁 + 既存の統合 rank)、字牌は既存 [`HonorSafety`](defense.md#honorsafety) の rank と見え枚数をそのまま載せます。新しい safety rank も係数も作らず、Defense selection の comparator も呼びません。
- **Damaten** 側には Reach と同じ safety rank を付けません。`declaration visible: no` という事実だけです。これは「ダマなら安全牌評価が無効」という意味ではなく、**他家がこちらの待ちに対する防御を開始する公開トリガーが無い**という事実を表します。
- **external threats** は既存 classification の観測値です。リーチ者は `GameContext::reached_opponents()`、High OpenHand target は既存 [`OpenHandThreat`](push-pull.md#openhandthreat) の分類そのままで、threat を分類し直すことも確率へ変換することもしません。

#### 評価時点は打牌後の公開状態

reach public safety は、**通常打牌 selection が選んだ打牌を河へ置いた直後の公開状態**に対して既存 Defense helper を適用します。

```text
現在の GameContext
+ selected discard を自分の河へ移す (GameContext::after_own_discard)
↓
打牌後の公開状態
↓
既存 Defense helper (is_genbutsu_for() / suited_safety_evidence_for_players() /
                     honor_safety_rank() / visible_count_of())
```

待ちとロン可否 (`TenpaiWaitAvailability`) も打牌後の状態なので、両者の評価時点が揃います。リーチ宣言牌が作るスジと現物は、この打牌後の河を既存 helper が読むことでそのまま反映されます。「宣言牌が 1s ならスジを1本足す」のような safety rule を診断側に書くことはしません。

見え枚数 (壁・字牌の見え枚数) は打牌で変わりません。`visible_tiles` は自分の手牌を既に含むので、同じ物理牌が手牌から河へ移っても枚数が変わらないためです。値は同じ打牌後の状態を既存 helper へ通した結果そのもので、打牌前の値を別に保持しているわけではありません。

切る物理牌は通常打牌 selection が選んだ合法 `Dahai` そのもので、どの牌を切るかを別経路で推測しません。projection は元の `GameContext` を書き換えず、診断が有効な経路でだけ組み立てます。リーチが合法でない局面と、選んだ打牌が分からない局面では打牌前の状態で代用せず `unavailable` にします。

#### Defense の exact `R/T` は使いません

[リーチ者ごとの exact ron risk](defense.md#リーチ者ごとの-exact-ron-risk) (`RonRiskEvidence` / `ron_capable_weight` / `tenpai_weight` / `R/T`) はこの診断に入れません。exact model が表すのは

```text
自分が牌 x を切った場合、相手が x でロン可能な hidden-hand state の structural weight
```

で、Ron opportunity が欲しい

```text
自分が x 待ちでテンパイしている場合、他家から見て x がどう見えるか
```

とは意味が違うためです。`R/T` は実放銃率でもロン確率でも opponent behavior probability でもないので、ロン確率の代用にもしません。

#### 相手別の打牌確率はまだありません

`GameContext` は player ごとの河・副露・リーチ状態を持ちますが、empirical discard model は持ちません。河は `TileId` の列で、core context 上では各河牌の手出し / ツモ切りも保持していません。したがって「opponent 1 が 4s を切る確率」のような opponent behavior probability はまだ存在せず、この層でも作りません。self-tsumo と Ron baseline を統合した EV も、この模型が入るまで作りません。

#### ロンできない待ちは unavailable

目的が Ron opportunity なので、実際にロンできない局面では 0 として扱わず評価しないままにします。

| 局面 | Ron opportunity |
| --- | --- |
| `can_ron = Some(true)` | 待ちごとの facts を並べる |
| `can_ron = Some(false)` (フリテン) | `unavailable` |
| `can_ron = None` (ロン可否 unknown) | `unavailable` |
| リーチが合法でない | 待ちは並べるが `if Reach` は `unavailable` |

フリテンでも公開 safety 自体は計算できますが、ロン不能な待ちを確率候補のように並べないことを優先します。既存の `TenpaiWaitAvailability` / フリテン semantics と矛盾する値は作りません。

**この統合診断も diagnostics 専用です。** production の Reach / Damaten 判断 (`decide_reach_reason()`) は変更しておらず、統合診断はその結論を観測値として載せるだけです。winner も新しい `should_reach` も持たず、構築の有無は最終 action を変えません。Ron opportunity のために safety 評価も threat 分類も追加探索も production 経路へ入れず、押し引きが既に構築した classification を借りるだけにします。

## selected と runner-up

候補の `selected: yes` は通常打牌 comparator の選択です。最終 action は Reach や防御 fallback によって変わる場合があります。最終結果は `Final decision` と `Summary` を確認してください。

`Summary.runner-up` は最終選択を除いた場合に次に選ばれる候補です。候補の比較理由と合わせて、どの比較軸で勝敗が決まったかを調査できます。出力形式は [Structured diagnostics](../diagnostics.md#summary-と-runner-up) を参照してください。
