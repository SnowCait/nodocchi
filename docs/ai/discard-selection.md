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

## 現在聴牌: Ron / self-tsumo の cohort 比較

現在打牌の直後が聴牌になる候補同士では、候補集合 (cohort) の恒常フリテン状態に応じて、現在の Acceptance より先に使う値軸を切り替えます。

```text
Shanten → existing pre-acceptance axes
→ [聴牌のみ]
   全候補 PermanentFuriten::No
     : CurrentTenpaiOffenseWeightedTotal
   全候補 PermanentFuriten::Yes かつ全候補 base Reach かつ timing 込みの値が全件確定
     : CurrentTenpaiContinuationSelfTsumoValue
   その他の PermanentFuriten::Yes を含む known cohort
     : CurrentTenpaiExpectedSelfTsumoValue
→ AcceptanceRemaining → AcceptanceTypeCount → ...
```

全候補が非フリテンの場合は、従来どおり既存の Reach / Damaten policy と手牌価値評価で求めた `current tenpai offense weighted total` を使います。値は生きた和了牌を赤5 / 黒5の physical variant まで分け、既存 `Payment.total()` を残枚数で重み付けした合計です。

```text
current tenpai offense weighted total
= Σ(生きた和了牌 physical variant の残枚数 × Payment.total())
```

平均打点ではありません。例えば「1枚 × 12,000点」は12,000、「6枚 × 3,900点」は23,400なので、後者を上位にします。また、これはロン / ツモ確率を掛けたEVではなく、本場・供託・点棒状況も含みません。

cohort の全候補で `PermanentFuriten` が `Yes` / `No` のどちらかに確定し、少なくとも1候補が `Yes` の場合は、Ron の生値を候補間で比較せず `current tenpai expected self-tsumo value` を共通軸にします。全候補が恒常フリテンの場合も、恒常フリテン / 非フリテンが混在する場合も同じです。

```text
current tenpai expected self-tsumo value
= P(残り自摸機会内にツモ和了) × ツモ和了時の期待 Payment.total()
```

この値は既存 `TenpaiTsumoValue::expected_payment()` の固定小数点尺度そのもので、打牌後の `SelfTsumoFacts`、live physical variant、Tsumo scoring を再利用します。production の base Reach / Damaten policy が選んだ mode に対応する Tsumo baseline を使い、現在の待ちのまま残り自摸機会を使い切る値です。候補ごとの `ReachTimingDecision` を織り込むのは、次に述べる限定 cohort だけです。

Ron probability を持っていないため、mixed cohort でも `ExpectedSelfTsumoValue` に Ron opportunity や Ron weighted total を加算しません。これは自分のツモ和了だけを見る self-tsumo-only offense continuation value であり、局収支EVではありません。非フリテン同士では従来の Ron-based 軸を維持し、フリテンとの共通比較が必要な cohort だけ、全候補で意味が同じ self-tsumo 尺度を使う暫定 policy です。

攻撃モードは既存 production policy と同じです。既にリーチ済みならReach、未リーチなら既存の `decide_reach_reason()` に従ってReachまたはDamatenを選びます。ダマ値は既存フリテン判定で `can_ron == Some(true)` と確定した場合だけ利用し、ロン可否unknown・役なし・点数計算不能などを0点とは扱いません。

軸の有効・無効は `Shanten` / `IsolatedTile` / `IsolatedHonor` まで同順位の cohort 単位で決めます。`PermanentFuriten::Unknown` が1件でもあれば新しい furiten-aware self-tsumo 軸を使いません。また self-tsumo value が1件でもunknownなら、値を0とせず cohort 全体で軸を無効化します。どちらも既存の次軸へ fallback し、pairwise に軸の有無を変えません。

`CurrentTenpaiExpectedSelfTsumoValue` の評価対象は現在打牌直後の待ちと打点だけです。聴牌後に非和了牌を引いて別の待ちへ移る手変わりや、ダマ手変わりの2手先評価は行いません。既存の1向聴・2向聴以上の先読み軸も変更しません。

### 恒常フリテン cohort の timing 込み self-tsumo 軸

cohort が次をすべて満たす場合だけ、現在の待ちのままの値ではなく `current tenpai continuation self-tsumo value` を使います。1つでも満たさない cohort では、この軸のために継続評価そのものを行いません。

```text
CurrentTenpaiFuritenCohort::AllPermanentFuriten
+ cohort の全候補で既存 base Reach / Damaten policy がリーチを選ぶ
+ 全候補で reach now / defer → forced Reach の比較が確定し、timing 込みの値が Some
```

「base policy がリーチを選ぶ」は実際のリーチ判断と同じ結論です。押し引きの `TenpaiOffenseMode` (`decide_reach_reason()`) だけでなく、その前段の categorical rule (`selects_named_yakuman_damaten()`) も通します。恒常フリテンの named 役満候補は `TenpaiOffenseMode` ではリーチでも base policy はダマ (`NamedYakumanDamaten`) を選ぶので、continuation axis の対象にしません。役満判定は既存 Tsumo scoring の結論そのままで、候補比較のために待ちも完成手も作り直しません。

候補値は既存 [リーチ timing](#リーチ-timing) policy (`decide_permanent_furiten_reach_timing()`) が選んだ側の self-tsumo value そのものです。

```text
ReachNow    → reach now
DeferReach  → defer → forced Reach
```

比較不能 (`SelfTsumoComparisonUnknown`) の候補は 0 点にせず値を持ちません。1件でも確定しなければ cohort 全体でこの軸を外し、現在の待ちのままの `CurrentTenpaiExpectedSelfTsumoValue` へ戻します。新しい threshold も係数も持たず、Ron probability も Ron EV も導入しません。

全候補が恒常フリテンで、ロンできる候補が1件も無いからこそ、self-tsumo だけで比較が閉じます。`MixedKnown` cohort は `No` 側にだけロン機会があるため対象外で、従来どおり `CurrentTenpaiExpectedSelfTsumoValue` のままです。base policy がダマを選んだ候補 (`HighValueDamaten` / `NamedYakumanDamaten`) を1件でも含む cohort も対象外で、「片方は timing 込み・片方は現在待ち」という非対称な比較は作りません。そのような cohort は全体で既存 `CurrentTenpaiExpectedSelfTsumoValue` へ落とし、選ばれた候補の base reason と Reach timing も従来どおりです。

比較に使う値も、選択済み候補へ後段で適用する Reach timing も、同じ既存の継続評価と同じ timing policy を通します。policy を複製しないので、候補比較が `DeferReach` を選んだ値と、その候補が選ばれた後の Reach timing の結論は一致します。同じ候補の2手先評価を consumer ごとに繰り返さないよう、経路ごとに source を1つへ寄せています。

```text
通常 discard selection
  gate を通った候補だけ 1-candidate continuation helper
  → Reach timing → 候補比較 → 選択済み候補の timing をそのまま後段のリーチ判断へ

--lookahead 診断
  既存 all-candidate TenpaiContinuationDiagnostic
  → 対象候補の self_tsumo を抽出 → 同じ Reach timing policy → 候補比較
```

後段のリーチ判断は、候補比較が同じ gate で timing を評価済みならその結論をそのまま使い、評価していない場合 (現在聴牌候補が1件・cohort が対象外) だけ従来どおり選択済み1候補を評価します。`--lookahead` では既存の全候補継続診断が selection 用 continuation の source になるので、同じ候補について個別の2手先評価を追加で走らせません。診断の有無で選択された打牌・比較理由・Reach timing は変わりません。

残り自摸機会が確定しない局面 (`remaining_tiles` unknown) では self-tsumo 確率模型の材料が揃わず、継続比較のどの値も確定しません。同じ材料から決まる `current tenpai expected self-tsumo value` も確定せず必ず既存軸へ落ちるので、exact fact だけで分かるこの場合は2手先評価そのものを走らせません。

ダマで継続した場合の次の1巡そのものは [現在聴牌のダマ継続 (diagnostics only)](#現在聴牌のダマ継続-diagnostics-only) で観測できます。

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

### 鳴き後も1向聴になる Call

現在1向聴から Chi / Pon と合法打牌を経ても1向聴の候補は、上記と同じ
`ExpectedSelfTsumoValue` で Pass と比較します。鳴き後の全合法打牌候補は喰い替え禁止牌を先に
除外し、通常打牌と同じ forward metric と comparator で選びます。terminal scoring は今回の
副露を含む hand state から fixed meld count・門前・将来リーチ可否・完成手を一方向に導出します。

Pass は架空の現在打牌を作らず、「action 済みで次の自摸を待つ1向聴 state」から通常打牌後と
同じ Progress / SameShanten 探索へ入ります。流局までの horizon は観測済みの reaction 元 player
から Pass 後の最初の自摸位置を求めて揃えます。reaction 元または残り山が unknown なら値も
unknown のままです。

Call が Pass より厳密に高い場合だけ鳴き、同値・どちらか unknown では鳴きません。倍率、固定点、
受け入れ threshold、Chi / Pon 別補正はありません。既存の「Call → 即テンパイ」policy は先に
評価され、成立した候補を従来どおり優先します。

### 2向聴から1向聴になる Call の観測

現在2向聴から Chi / Pon と合法打牌を経て1向聴になる候補は、diagnostics だけで Call / Pass の
self-tsumo value を観測します。Call 側は上記と同じ鳴き後候補生成・喰い替え制約・forward metric・
既存 comparator を通り、最良打牌後の1向聴 continuation へ接続します。Pass 側は reaction 元から
求めた同じ流局 horizon で、次の自摸を待つ2向聴 state の既存 Full evaluation を使います。

Progress-only は2向聴を維持する SameShanten 枝を含まず、完全な1向聴 continuation である Call
側との比較では Call に有利になるため、この観測には使いません。Full は対象候補が存在する場合に
Pass state 1件だけを評価して全候補で共有し、通常打牌候補の Full 全探索は行いません。

この比較は observation-only です。`CallHigher` でも production の `eligible` / reason / action
selection は変えず、diagnostics を無効にした通常の action では追加探索も行いません。鳴き後の
最良打牌が2向聴のままの候補、Kan、他家リーチなど既存 policy 境界で評価を打ち切る候補は対象外です。

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

## 2向聴: ExpectedSelfTsumoValue

2向聴の候補も、1向聴と同じ self-tsumo 尺度へ揃えて観測できます。

```text
two-shanten expected self-tsumo value
= Σ(その経路を引く確率 × テンパイ到達後の期待ツモ支払い)
```

対象の経路は次の2種類で、最初のツモで2向聴を維持する枝は1回だけ許します。

```text
A. 2向聴 → Progress → 最良打牌 → 1向聴 → 1向聴の ExpectedSelfTsumoValue
B. 2向聴 → SameShanten → 最良打牌 → 2向聴 → Progress → 最良打牌 → 1向聴
   → 1向聴の ExpectedSelfTsumoValue
```

確率も期待支払いも1向聴と同じ閉形式で、接続先は1向聴の `ExpectedSelfTsumoValue` そのものです。
向聴・受け入れ・見え牌・赤5・打牌比較・将来打点はどれも既存 layer が source of truth で、この
軸のために係数も threshold も pruning も追加しません。2回目の SameShanten はこの評価モデルの
探索範囲外で、確定しない値ではなく寄与 0 として扱います。

production の通常打牌では、現在打牌後が2向聴で、Shanten / IsolatedTile /
IsolatedHonor まで同順位の ForwardTargets cohort 全候補をまず A の Progress-only 値で
順位付けます。provisional 1位と2位の Progress-only 値が strict に異なり、かつ既存の
`discarded_dora_count` が異なる場合だけ、その2候補に B を追加した Full 値で pairwise
再比較します。Progress-only が同値の場合は B を計算せず、後続 comparator と stable
order に委ねます。赤5だけで gate は広げません。

Progress-only の cohort 配列と Full の2候補 pair は別々に既存 comparator へ渡します。
Full 値を上位2候補だけに入れた配列で全候補を比較することはありません。
cohort 全候補の Progress-only 値を確定できない場合はその軸を無効にし、
[WeightedNextAcceptance](#2向聴以上-weightednextacceptance) 以下へ戻ります。
起点の向聴数が違うため、1向聴の `ExpectedSelfTsumoValue` とは別 field で保持し、
1向聴候補と2向聴候補の間で比較しません。3向聴以上、押し引き、リーチ判断にも使いません。

材料 (ツモ打点と残り自摸機会) が揃わない局面と、到達したテンパイのツモ打点を1つでも確定できない
候補は、0点で補完せず値を持ちません。`bot-scenario --two-shanten-self-tsumo` で表示できます。

### 実行コストの計測

Full 軸は探索が最も深いため、production は上記の gate 対象 pair 以外では
SameShanten 枝を評価しません。ForwardTargets 全候補 Full の実行コストは
`bot-scenario --two-shanten-self-tsumo-cost` で、Progress-only は
`--two-shanten-progress-self-tsumo-cost` で分けて計測できます。

```bash
cargo run --release -p bot-scenario -- \
  --hand "11258m234789p13s" \
  --draw "9s" \
  --remaining-tiles 66 \
  --two-shanten-self-tsumo-cost forward-targets
```

`--two-shanten-self-tsumo-cost all` は全2向聴候補、`forward-targets` は production の比較対象
だけを評価し、候補ごとの期待値と実測時間を表示します。範囲の絞り込みは打牌選択が使う前方評価の
絞り込みそのもので、残った候補の値は全候補で求めた場合と一致します。計測用の入口も同じ探索を使います。

`--two-shanten-progress-self-tsumo-cost forward-targets` は同じ cohort について、
A の Progress 枝だけの値と実測時間を表示します。候補ごとの値は既存の
`awaiting_draw_two_shanten_progress_self_tsumo_value()` と同じ枝と集計を通ります。
この option は計測専用で、production comparator と打牌選択は変更しません。

計測より前に深い探索を走らせると向聴・受け入れの memo が温まり、後続の計測が本来より速く
見えてしまいます。そのため `--lookahead` / `--verbose` / `--two-shanten-self-tsumo` /
`--summary-only` とは同時に指定できません。

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

**`Tenpai continuation` 節が並べる全候補分の継続枝は diagnostics 専用です。** base の Reach / ダマ判断にも押し引きにも使わず、継続 bonus も係数も threshold も持ちません。`CurrentTenpaiOffenseWeightedTotal` と `CurrentTenpaiExpectedSelfTsumoValue` は、この枝集合を含まない現在の待ちだけの軸です。production がこの継続 semantics を使うのは、選択済み1候補にだけ適用する [リーチ timing](#リーチ-timing) と、[恒常フリテン cohort の timing 込み self-tsumo 軸](#恒常フリテン-cohort-の-timing-込み-self-tsumo-軸) の2つで、どちらも全候補分の詳細診断ではなく既存の1候補継続 helper を通ります。Ron の生値や `weighted prospective value` は単位の違う値なので、self-tsumo expected value と加算しません。

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

`reach now` と `defer → forced Reach` を比べることで、既存 Reach / Damaten threshold から独立して「今すぐリーチするか、1巡だけ手変わりを見るか」を観測できます。

材料が揃わない場合は 0 点ではなく値を持ちません。山の残枚数が分からず自摸機会を確定できない局面、ツモ打点を確定できない現在聴牌、terminal tenpai のツモ打点が確定しない継続枝はどれも `None` です。

**全候補分のこの比較 (`Tenpai continuation` 節) は diagnostics 専用で、どれを選ぶかの結論は持ちません。** winner も `should_reach` も作らず、打牌選択にも押し引きにも接続していません。Ron probability は含まず、self-tsumo と Ron baseline の aggregate も作りません。production が使うのは、次に述べる限定条件下の [リーチ timing](#リーチ-timing) だけです。

## base Reach / Damaten policy の categorical rule

base policy (`ReachDecisionReason`) がダマを選ぶ理由のうち、ダマ打点 threshold による判断と恒常フリテンの named 役満は別の事実に基づきます。

```text
HighValueDamaten
= 通常のダマ打点 threshold heuristic

NamedYakumanDamaten
= PermanentFuriten::Yes で
  全 live Tsumo variant が named yakuman と確定した場合の categorical policy
```

`HighValueDamaten` は「全ての生きた待ちがダマで役ありかつ `Payment.total()` が threshold 以上」という打点の大小の heuristic です。`NamedYakumanDamaten` は打点の大小ではなく、ロンできない聴牌のツモ和了が既存 scoring 上**名前の付いた役満で確定している**という別の事実だけに基づく categorical rule なので、`HighValueDamaten` へは統合しません。

条件は次をすべて満たす場合だけです。

```text
選んだ打牌後がテンパイ
PermanentFuriten::Yes
live wait > 0
生きた physical variant が1件以上あり、その全ての Tsumo HandValue が確定かつ named 役満
```

役満かどうかは既存 scoring (`TenpaiCompletedHands` → `evaluate_tenpai_hand_value()` → `HandValue::is_yakuman()`) だけが source of truth です。国士無双の向聴・役満名の列挙・牌姿からの独自判定・点数 threshold からの推測はどれも行いません。Tsumo baseline も共通化済みの既存 Tsumo scoring をそのまま使い、リーチを宣言しない場合のモード (Damaten) で評価します。

次はどれも対象外で、従来どおりの base policy になります。

```text
数え役満 (名前の付いた役満ではない)
一部の live variant だけ named 役満
役なし variant を含む
scoring unknown
live remaining == 0 の variant (判定に含めない)
PermanentFuriten::No / PermanentFuriten::Unknown
same-turn furiten / リーチ後見逃しフリテンだけの局面
```

**数え役満はこの特例に入れません。** 点数が役満相当でも名前の付いた役満ではないので、既存のダマ打点 threshold の結論になります。非フリテンの named 役満も従来どおり `HighValueDamaten` のままで、四暗刻シャンポンのように「ツモなら役満・ロンなら役満ではない」可能性がある非フリテン局面の扱いも変えていません。

`NamedYakumanDamaten` は base policy がダマを選ぶ理由なので、[リーチ timing](#リーチ-timing) は評価しません (`timing` は `None`)。`DeferReach` ではなく、通常打牌 selection が選んだ Dahai をそのまま行います。この categorical rule を適用するのはリーチ action の判断だけで、押し引きの攻撃モードと現在聴牌 / 将来テンパイの selection value は変更していません。

## リーチ timing

`ReachDecisionReason` (base Reach / Damaten policy) と `ReachTimingDecision` は**別の層**です。

```text
ReachDecisionReason    Reach か Damaten かを決める base policy
ReachTimingDecision    base policy が Reach の場合に、そのリーチを今回宣言するか
```

`ReachTimingDecision` は base policy がリーチを選んだ場合だけ適用します。base policy がダマを選んだ聴牌 (`HighValueDamaten` など)、リーチが合法でない局面、打牌後がテンパイでない局面では timing 判断そのものを行わず (`timing` は `None`)、`ReachTimingDecision` に `Damaten` は含まれません。base の `reason` を timing の理由で上書きすることもありません。

`DeferReach` の意味は次の1つだけです。

```text
今回の request では Reach を宣言しない
→ 通常 discard selection が既に選んだ Dahai を行う
→ 状態を記憶せず、次の局面で通常 policy を評価し直す
```

**「必ず1巡待ち、その後必ずリーチする」という production state ではありません。** persistent flag も turn counter も残り巡数も持ちません。RiichiLab では Reach 宣言と Dahai が別 response なので ([RiichiLab client](../riichilab.md) の capture 例を参照)、`DeferReach` は「今回は Reach response を送らず Dahai を行う」という production behavior そのものです。

判断材料に使う `one draw → forced Reach` は上の counterfactual の**評価 horizon** であって、production action の約束ではありません。実際の対局では次の自分のツモまでに他家和了・他家リーチ・副露機会・局終了が起こり得ます。production は次の request で必ず現在局面から評価し直します。

### production 接続対象は2つの限定経路だけ

共通して、次をすべて満たす場合だけ timing evaluation へ進みます。

```text
- 合法手に LegalAction::Reach がある
- 通常 discard selection が打牌を選んでいる
- その打牌後がテンパイ
- 生きた待ちが1枚以上ある
- base decide_reach_reason() が Reach を選んだ
```

その後、次のどちらかだけを対象にします。

1. `PermanentFuriten::Yes` が確定した恒常フリテン聴牌
2. 次を**すべて**満たす非フリテン悪形の暫定 heuristic

```text
PermanentFuriten::No
can_ron == Some(true)
live wait は1種類だけ
live copies は1〜3枚
待ち牌は么九牌ではない (`TileType::is_yaochu() == false`、つまり2〜8の中張牌)
Reach 宣言牌を河へ置いた後の public safety が非現物かつ NoWall かつ NoSuji
reached opponents = 0
High OpenHand targets = 0
```

恒常フリテンは既存の `PermanentFuriten` だけが source of truth です。`can_ron() == Some(false)` だけで恒常フリテンだと推測しません。`PermanentFuriten::Unknown` と履歴依存フリテンだけの局面はどちらの経路にも入れず、従来どおり `ReachNow` を維持します。

非フリテン全般を self-tsumo だけで比較する policy ではありません。非フリテンなら今リーチした最初の1巡からロン機会が生まれますが、nodocchi はまだ「他家がその牌を切る確率」の模型を持たないためです。今回の接続は、明らかな悪形を上記の structural facts で狭く限定する**暫定 policy**です。

`genbutsu == false` かつ `wall_rank == NoWall` かつ `suji_rank == NoSuji` は「ロン確率が高い」という意味ではありません。Reach 後に既存 public safety evidence 上の現物・壁・スジによる安全根拠が無いことだけを確認する structural gate です。`SuitedSafetyEvidence::legacy_rank()` には依存せず、各 evidence を数値係数へ変換しません。[Ron opportunity](#ron-opportunity-structural-facts-only) の Defense exact `R/T` も流用せず、Ron probability / discard probability は追加していません。

么九牌かどうかは既存 `TileType::is_yaochu()` だけを source of truth にします。標準形と七対子の2〜8単騎は対象になり得ますが、1 / 9単騎・字牌単騎・待ちがすべて么九牌の国士無双は対象外です。七対子や国士を hand family / shanten の特殊分岐で判定しません。

恒常フリテンなら現在の待ちでロンできないので self-tsumo counterfactual だけで比較が閉じます。非フリテン悪形はそうではないため、この限定 heuristic を恒常フリテンの reason と区別して診断します。

### 選択済み1候補だけを評価します

production 経路は次の順です。

```text
通常 discard selection
↓
selected discard 確定
↓
base Reach policy (decide_reach_reason)
↓
PermanentFuriten::Yes
または限定した非フリテン悪形の structural gate
↓
selected candidate 1件だけ
  reach now
  vs
  defer one draw → forced Reach
```

`--lookahead` の全候補継続診断は production では構築しません。gate を通った局面で評価するのは通常打牌 selection が選んだ1候補だけで、枝の分類も次打牌も打点も既存の `TenpaiContinuation` / `ProspectiveTenpaiValue` / `SelfTsumoPath` / 既存 selector / 既存 scoring をそのまま通ります (`selected_tenpai_self_tsumo_comparison()`)。counterfactual の semantics も上の diagnostics と同一で、2回目の手変わり探索も、terminal mode への `decide_reach_reason()` の再注入も行いません。診断の有無で production action が変わることもありません。

[恒常フリテン cohort の timing 込み self-tsumo 軸](#恒常フリテン-cohort-の-timing-込み-self-tsumo-軸) が同じ候補の timing を既に評価している場合は、その結論をそのまま使います。同じ候補・同じ gate・同じ policy なので結論は変わらず、2手先評価を繰り返さないだけです。

### 比較するのは大小だけです

```text
defer forced Reach >  reach now  → DeferReach
defer forced Reach <= reach now  → ReachNow
```

同値は `ReachNow` です。点差・割合・待ち枚数のような arbitrary threshold は持たず、実戦統計も Ron probability も使いません。

どちらかを確定できない場合 (山の残枚数 unknown、`reach now` のツモ打点 unknown、terminal のツモ打点 unknown、将来 Reach legality を解決できないなど) は **0 点として扱わず**、比較不能として既存 base Reach をそのまま維持します。

### 押し引きとは別の層です

timing は、既存 base Reach / Damaten policy と既存押し引きが攻撃継続を許した**後**に、最終的に今回リーチを宣言するかだけを決める層です。

```text
base offense mode      押し引きが攻撃打点を求めるときの Reach / Damaten (decide_reach_reason)
current Reach timing   今回の request で Reach を宣言するか (ReachTimingDecision)
```

この2つは別概念で、`DeferReach` のために push/pull threshold も `TenpaiOffenseValue` threshold も Defense selection も変更していません。押し引きが `Fold` を選んだ局面ではリーチ判断そのものを行わないため、防御 fallback や鳴き・和了・流局が採用した action を timing が上書きすることもありません。

### 表示

`Reach` 節に `timing` を、`Summary` に `reach: deferred` と `reach timing reason` を出します。timing evaluation の対象外は評価しなかったことだけを軽量に出し、self-tsumo 比較の値は production でも構築しません。

```text
Reach
  base decision: yes
  base reason: Eligible
  ...
  timing
    decision: DeferReach
    reason: PermanentFuritenSelfTsumo
    reach now: 980.401
    defer one draw
      forced Reach: 1076.190
```

恒常フリテン経路は `PermanentFuritenSelfTsumo`、非フリテン悪形の暫定 heuristic は `NonFuritenBadWaitHeuristic` と表示し、両者を混同しません。

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

切る物理牌は通常打牌 selection が選んだ合法 `Dahai` そのもので、どの牌を切るかを別経路で推測しません。projection は元の `GameContext` を書き換えません。統合診断のほか、上記の非フリテン悪形 heuristic が他の cheap gate をすべて通過した selected wait 1件だけも、同じ pure helper を共有します。リーチが合法でない局面と、選んだ打牌が分からない局面では打牌前の状態で代用せず `unavailable` にします。

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

**`RonOpportunityDiagnostic` 全体は diagnostics 専用です。** production の base Reach / Damaten 判断 (`decide_reach_reason()`) は変更しておらず、統合診断はその結論を観測値として載せるだけです。winner も新しい `should_reach` も持ちません。

非フリテン悪形の暫定 timing heuristic は診断全体を構築せず、selected wait 1件の Reach public safety だけを同じ pure helper から取得します。High OpenHand target も押し引きが既に構築した classification を借り、分類し直しません。この gate を通過した場合にだけ既存 `selected_tenpai_self_tsumo_comparison()` を評価します。

## selected と runner-up

候補の `selected: yes` は通常打牌 comparator の選択です。最終 action は Reach や防御 fallback によって変わる場合があります。最終結果は `Final decision` と `Summary` を確認してください。

`Summary.runner-up` は最終選択を除いた場合に次に選ばれる候補です。候補の比較理由と合わせて、どの比較軸で勝敗が決まったかを調査できます。出力形式は [Structured diagnostics](../diagnostics.md#summary-と-runner-up) を参照してください。
