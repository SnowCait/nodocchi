# bot-scenario

`bot-scenario` は、手牌・局面を入力して `ShantenAgent` の判断と根拠をオフラインで確認する CLI です。実対局や WebSocket 接続は行いません。出力の読み方は [Structured diagnostics](diagnostics.md)、判断仕様は [麻雀 AI の概要](ai/overview.md) を参照してください。

牌文字列の parse、physical tile の割り当て、scenario の validation、`GameContext` と `LegalAction` の構築は、platform 非依存の library crate [`bot-analysis`](../crates/bot-analysis/) にあります。`bot-scenario` は CLI 引数の解析・file I/O・RiichiLab capture の再生・出力の整形を担当し、局面構築は `bot-analysis` の `ScenarioSpec` → `Scenario::resolve()` → `Scenario` をそのまま使用します。

[Summary](#summary) が表示する値も `bot-analysis` が決めます。`Scenario` と production の `ShantenDecisionDiagnostic` から `AnalysisResult::from_decision()` が薄い構造化結果を作り、CLI はそれを文字列化するだけです。どの候補を比較対象にするか、どの防御 source を出すかといった選択は `bot-analysis` 側にあり、CLI は持ちません。

## 簡易 CLI

牌効率をすぐ確認する用途です。

```bash
cargo run -p bot-scenario -- \
  --hand "234m455p789s1123z" \
  --draw "N"
```

| 引数 | 必須 | 内容 |
| --- | --: | --- |
| `--hand` | 必須 | ツモ牌を除いた手牌 |
| `--draw` | 任意 | ツモ牌。2牌以上へ展開される文字列は error |
| `--dora` | 任意 | ドラ表示牌。ドラそのものではない |
| `--round-wind` | 任意 | 場風。`E` / `S` / `W` / `N` |
| `--seat-wind` | 任意 | 自風。`E` / `S` / `W` / `N` |
| `--player-id <0..3>` | 任意 | 自分の席 |
| `--oya <0..3>` | 任意 | 親の席 |
| `--discards-shimocha <TILES>` | 任意 | 下家の河。指定した牌列がその player の河そのもの |
| `--discards-toimen <TILES>` | 任意 | 対面の河 |
| `--discards-kamicha <TILES>` | 任意 | 上家の河 |
| `--riichi-shimocha [INDEX]` | 任意 | 下家をリーチ済みにする。`INDEX` は宣言牌が河の何枚目かで 1-based。省略時は宣言牌位置 unknown |
| `--riichi-toimen [INDEX]` | 任意 | 対面をリーチ済みにする |
| `--riichi-kamicha [INDEX]` | 任意 | 上家をリーチ済みにする |
| `--extra-visible-tiles` | 任意 | 他の option で表現していない見え牌 |
| `--remaining-tiles` | 任意 | 山の残りツモ可能枚数 |
| `--honba <COUNT>` | 任意 | 本場。省略時は unknown で、本場が必要な値は `0` 本と補完せず unavailable |
| `--reacher-riichi-facts <SPEC>` | 任意 | リーチ済みの席のリーチ状況依存役を明示する。`SPEC` は `double` / `ippatsu` の comma 区切り、または通常立直・一発なしの `none`。省略時はどちらも unknown |
| `--no-history-furiten` | 任意 | 同巡内フリテンでもリーチ後見逃しフリテンでもないことを明示 |
| `--allow-hora` | 任意 | 和了を合法手に加える |
| `--allow-ryukyoku` | 任意 | 九種九牌 (`LegalAction::Ryukyoku`) を合法手に加える |
| `--force-fold` | 任意 | 通常の押し引き判断とは無関係に、ベタ降りを仮定した場合の防御打牌を ForcedFold ranking の上位3候補 + 0-risk candidate 全件として表示する。他の診断 option と併用不可 (`--summary-only` / `--verbose` は併用可) |
| `--lookahead` | 任意 | 打牌候補ごとの2手先概要と、現在聴牌候補のダマ継続概要を追加。`--verbose` 併用時は受け入れ牌ごと・継続枝ごとの詳細も表示 |
| `--two-shanten-self-tsumo` | 任意 | 2向聴候補の ExpectedSelfTsumoValue を追加 (`--lookahead` を含む) |
| `--structural-expected-deal-in-loss` | 任意 | 通常打牌 selector が選んだ打牌について、単独リーチ相手への structural expected deal-in loss を追加。他家リーチがちょうど1人の局面だけが対象で、`--summary-only` / `--force-fold` と併用不可 |
| `--three-shanten-progress-self-tsumo` | 任意 | production が3向聴打牌比較に使う Progress-only self-tsumo 値を全合法3向聴候補について表示し、候補別時間・合計時間を追加。他の診断 option と併用不可 |
| `--three-shanten-continuation-comparison` | 任意 | 1向聴 continuation の枝を変えた2方式 (A: Progress + SameShanten / B: Progress のみ、B が production) で3向聴候補を評価し、値・時間・探索規模・選択打牌を比較。他の診断 option と併用不可 |
| `--iishanten-continuation-depth-comparison` | 任意 | 1向聴 continuation の手変わり回数を変えた2方式 (A: 1回まで = legacy shallow depth / B: 2回まで = 現行 production) で全1向聴候補の ExpectedSelfTsumoValue を評価し、値・順位・最初のツモ単位の内訳・時間・探索規模を比較。他の診断 option と併用不可 |
| `--iishanten-selection-depth-comparison` | 任意 | 同じ深度 A/B を production comparator を通した最終打牌選択として比較。A は legacy shallow depth (旧 production)、B は現行 production depth (SameShanten 2回まで + exact same-state memo)。他の診断 option と併用不可 |
| `--iishanten-selection-parallel-comparison` | 任意 | 同じ production B depth を逐次 / 候補単位並列で比較 (S / P2 / P4 / PA = `available_parallelism`)。候補の絞り込み・軸解決・comparator・選択打牌は既存 production selection をそのまま使う。PA が現行 production と同じ方式。他の診断 option と併用不可 |
| `--two-shanten-full-parallel-comparison` | 任意 | 2向聴のドラ差 gate を通った provisional 上位2候補の Full 追加評価を逐次 / 2並列で比較 (S / P2)。Progress cohort・上位2候補の選び方・gate・comparator・選択打牌は既存 production selection をそのまま使う。P2 が現行 production と同じ方式。他の診断 option と併用不可 |
| `--verbose` | 任意 | 通常打牌候補の詳細を追加 |

簡易 `--hand` CLI は、すぐに「何切る」を確認できるよう、option 未指定時に次の deterministic baseline を使用します。

```text
round wind = E
player_id = 0
oya = 1
reached = 全員 false
discards = 全員空
history_furiten.same_turn = false
history_furiten.riichi_missed_win = false
remaining_tiles = player_id / oya または明示 seat_wind と draw の状態から初巡相当値を導出
                  (相対席 option を使った場合は適用せず unknown)
```

明示した CLI option は baseline より優先されます。`--round-wind`、`--seat-wind`、`--player-id`、`--oya` を指定した場合はその値を使用します。自風は既存の局面解決規則に従い、`player_id` と `oya` が揃えば導出され、両者からの導出値と明示 `--seat-wind` が矛盾する場合は error です。

`--no-history-furiten` は baseline と結果上は同じですが、「現在は同巡内フリテンではなく、かつリーチ後見逃しフリテンでもない」と明示する shorthand です。いずれかが `true` の局面や、履歴フリテンを unknown のまま扱う局面は JSON scenario で指定します。

`--remaining-tiles` は JSON scenario の `remaining_tiles` と同じ意味で、山に残っているツモ可能な牌の枚数です。inline `--hand` で省略した場合は、player / dealer、または両者が揃わなければ明示した自風と、`--draw` の有無から初巡相当の枚数を補完します。明示した値はこの baseline より常に優先されます。[相対席 option](#相手の河とリーチ) を使った場合はこの baseline を適用せず、明示しない限り unknown です。JSON scenario の省略 field は従来どおり unknown です。

### 相手の河とリーチ

`--discards-shimocha` / `--discards-toimen` / `--discards-kamicha` は JSON scenario の `discards` と同じ意味で、指定した牌列がその player の河そのものです。「一部だけ観測できていて残りは unknown」という semantics は持ちません。自分の河を指定する option はありません。

`--riichi-shimocha` / `--riichi-toimen` / `--riichi-kamicha` はその席をリーチ済み (`reached = true`) にします。`INDEX` はリーチ宣言牌が河の何枚目かで、**河の1枚目を `1`** とする 1-based です。`GameContext` へ渡すときに 0-based へ変換します。

```bash
cargo run -p bot-scenario -- \
  --hand "234m455p789s1123z" --draw "N" \
  --discards-shimocha "1m 7p 4s 7p E" \
  --riichi-shimocha 4
```

この例では下家の河の4枚目の `7p` がリーチ宣言牌です。河に同じ牌種が複数あっても、index が宣言牌を一意に決めます。

`INDEX` を省略した `--riichi-shimocha` は「リーチ済みだが宣言牌位置は unknown」を意味します。宣言牌位置は河の末尾などから推測しません。

```bash
cargo run -p bot-scenario -- \
  --hand "234m455p789s1123z" \
  --riichi-toimen
```

相対席は `player_id` 基準です。

```text
shimocha = (player_id + 1) % 4
toimen   = (player_id + 2) % 4
kamicha  = (player_id + 3) % 4
```

`player_id` は inline baseline で補われるため通常は指定不要ですが、明示 `seat_wind` と `oya` だけを指定して `player_id` が unknown になる場合は相対席を解決できず error です。河の枚数を超える `INDEX`、`0`、リーチしていない席への `INDEX` も error です。

相対席 option を使った局面は初巡ではないため、`remaining_tiles` の初巡 baseline は適用しません。`--remaining-tiles` を明示すればその値を使い、明示しなければ unknown のままです。簡易 CLI では自分の河を含む局面全体を指定できず正確な山枚数を復元できないので、指定した河の枚数から推測することもしません。防御の exact hidden-hand model は `remaining_tiles` を実際の評価材料に使うため、不正確な値を既知の事実として渡しません。

`--extra-visible-tiles` は JSON scenario の `extra_visible_tiles` と同じ意味で、手牌・ツモ牌・ドラ表示牌以外に見えている牌を加えます。加えた牌は受け入れ残枚数や待ちの残枚数へ反映されます。JSON scenario、RiichiLab capture、benchmark とは他の inline option と同じく併用できません。

```bash
cargo run -p bot-scenario -- \
  --hand "34599m235p345567s" \
  --extra-visible-tiles "11p 44p" \
  --summary-only
```

`--lookahead` は2手先概要に加えて、現在打牌後が聴牌になる候補の `Tenpai continuation` (現在聴牌 → 非和了ツモ → 最善打牌 → 再び聴牌) も表示します。待ちが変わる枝とツモ切りで元の待ちを維持する枝の両方を含みます。現時点では diagnostics 専用で打牌選択には接続しておらず、既にリーチしている局面と自分の席が分からない局面では表示しません。詳細は [打牌選択](ai/discard-selection.md#現在聴牌のダマ継続-diagnostics-only) を参照してください。

候補ごとの `self-tsumo comparison` (「今すぐリーチ」と「ダマで1巡継続」を同じ期待ツモ支払いで並べた比較) は、残り自摸機会が確定する局面でだけ値になります。inline `--hand` では上記 baseline、JSON scenario では明示した `remaining_tiles` を使います。

```bash
cargo run -p bot-scenario -- \
  --hand "340678m789p34789s" \
  --remaining-tiles 70 \
  --lookahead --verbose
```

`--two-shanten-self-tsumo` は、打牌候補集合の最善向聴数が2向聴の場合に `Two-shanten expected self-tsumo value` を追加します。1向聴の `ExpectedSelfTsumoValue` と同じ尺度で2向聴候補の Full 値を並べる解析用 option です。production selection 自体は ForwardTargets 全候補を Progress-only で順位付け、上位2候補が strict に非同値かつ `discarded_dora_count` が異なる場合だけ Full で pairwise 再比較します。この option は選択用と別に全候補 Full 診断を構築します。探索は `2向聴 → (Progress / 一度だけの SameShanten) → 1向聴 → 既存の1向聴 continuation` まで進むため重くなります。inline `--hand` では残り自摸機会に baseline 値を使い、必要なら `--remaining-tiles` で上書きできます。詳細は [打牌選択](ai/discard-selection.md#2向聴-expectedselftsumovalue) を参照してください。

`--lookahead --verbose` が追う1向聴候補の same-shanten downstream とは対象も枝も別なので、互いに含みません。`--two-shanten-self-tsumo` 単独では downstream 探索は走らず、両方必要な場合は `--two-shanten-self-tsumo --verbose` を指定します。

```bash
cargo run -p bot-scenario -- \
  --hand "11258m234789p13s" \
  --draw "9s" \
  --two-shanten-self-tsumo
```

### 3向聴 Progress-only 診断

`--three-shanten-progress-self-tsumo` は、production の3向聴打牌比較が使う値をそのまま全候補分表示する診断 option です。値の evaluator は production と共通で、診断専用の実装は持ちません。3→2、2→1、1→0 のいずれも Progress のみを追います。1向聴を直接評価する通常の ExpectedSelfTsumoValue は変わらず SameShanten も追いますが、3向聴起点の continuation では追いません。次打牌の比較、確率、terminal scoring、Reach/Damaten も既存処理と共通で、unknown は `unknown` と表示します。

3→2ツモ後の次打牌は、production と共通の2向聴 comparatorで選びます。先行軸で敗退が確定した候補のProgress valueと、Progressで単独勝者が確定した場合の後続forward metricは遅延評価で省略します。同値/unknown時はcohort全体の後続軸を評価するため、全候補を先に評価した場合と値・選択は一致します。Shanten / isolated等の先行軸も既存どおりで、2向聴 Full gateは呼びません。比較cohortの値が未確定なら、その枝の値もunknownです。

```sh
cargo run --release -p bot-scenario -- \
  --hand '45m46899p1124579s' --dora-indicator E \
  --round-wind E --seat-wind N --player-id 0 --oya 1 --remaining-tiles 66 \
  --three-shanten-progress-self-tsumo
```

値は既存 self-tsumo value と同じ点数単位で小数6桁まで表示します。計測は通常診断の前に行い、入力構築を除く探索時間を表示します。候補間では既存 memo を共有するため、候補別時間には評価順の影響があります。cold 条件の比較には毎回新しいプロセスを使ってください。探索の枝を省略する近似はありません。

3向聴診断では同じ物理牌集合・見え牌・仮想河の2向聴 value、1向聴 continuation、次打牌評価も共有し、memo hit / miss数を表示します。continuationは未確認牌数と残り自摸機会も区別します。候補を浅い評価順で除外するpruningはありません。1向聴到達後も SameShanten を追っていた頃は全候補で秒単位のコストが残っていましたが、3向聴起点の continuation を Progress-only にしたことで軽くなっています。

production 側の比較規則は [打牌選択](ai/discard-selection.md#3向聴-progress-self-tsumo-value) を参照してください。`Normal discard candidates` の `three-shanten progress self-tsumo value` は打牌選択が実際に使った値そのもので、この軸で決着した場合の `lost by` は `ThreeShantenProgressSelfTsumoValue` です。

### 3向聴 continuation scope の A/B 比較

`--three-shanten-continuation-comparison` は、3向聴 Progress self-tsumo 評価が1向聴に到達した後どこまで枝を追うかだけを変えた2方式を、同じ局面で比較する診断 option です。

```text
A progress+same-shanten   3→2 Progress / 2→1 Progress / 1→0 Progress + SameShanten
B progress-only           3→2 Progress / 2→1 Progress / 1→0 Progress のみ
```

違いは1向聴 state で `DrawTransition::SameShanten` のツモを追うかどうかだけです。受け入れの列挙、残枚数、物理牌 variant、ツモ後の最良打牌の比較、テンパイ到達後の terminal scoring、Reach / Damaten、確率、残り自摸機会、unknown 伝播はすべて共通の primitive を通ります。production の3向聴軸は B と同じ Progress-only で、A は比較用に残している全枝方式です。1向聴を直接評価する通常経路はどちらの方式でも変わらず SameShanten を扱います。

```sh
cargo run --release -p bot-scenario -- \
  --hand '45m46899p1124579s' --dora-indicator E \
  --round-wind E --seat-wind N --player-id 0 --oya 1 --remaining-tiles 66 \
  --three-shanten-continuation-comparison
```

出力は方式ごとの候補別の値と時間、`Search size A -> B` の枝数・state 数・`next_discard` 呼び出し数・terminal scoring 数、そして両方式が選ぶ打牌です。A と B の値は起点が同じ3向聴でも枝集合が違うため、同じ量として比較しないでください。

向聴・受け入れ・一向聴形の memo は thread ごとに持つため、方式ごとの実測は必ず新しい thread で行います。同じ thread で続けて評価すると後から走った方式が暖まった memo を使ってしまうためで、この隔離により A → B / B → A のどちらの順でも実測が偏りません。thread を分けても探索する枝・評価値・選択は変わりません。

capture 全体で同じ比較を行う場合は `--compare-three-shanten-continuation` を使います。3向聴軸が発火した request だけを対象に、打牌選択1回の実測時間と3向聴 phase の実測時間 (total / min / p50 / p90 / max / speedup)、選択一致率、差分 request の上位候補を出します。

```sh
cargo run --release -p bot-scenario -- \
  --compare-three-shanten-continuation logs/20260909/ranked-capture-20260909-092911.jsonl
```

この option は他の scenario / 診断 option とは併用できません。

### 1向聴 continuation 深度の A/B 比較

`--iishanten-continuation-depth-comparison` は、1向聴 ExpectedSelfTsumoValue が手変わりのツモを何回まで許すかだけを変えた2方式を、同じ局面で比較する診断 option です。

```text
A legacy shallow depth   Progress / SameShanten → Progress (production 接続前の旧設定)
B production depth       A に SameShanten → SameShanten → Progress を追加 (現行 production)
```

違いは1向聴 state で `DrawTransition::SameShanten` を2回まで許すかどうかだけです。受け入れの列挙、残枚数、物理牌 variant、ツモ後の最良打牌の比較、テンパイ到達後の terminal scoring、Reach / Damaten、確率、残り自摸機会、unknown 伝播はどちらも共通の primitive を通ります。段数は2回で閉じていて、任意深度の再帰へは一般化しません。手変わりの枝の次打牌はその方式が集計する continuation で選ぶため、B では次打牌そのものが A と変わり得ます。

```sh
cargo run --release -p bot-scenario -- \
  --hand '34567899m5799p34s' --dora-indicator 3m \
  --round-wind E --seat-wind N --player-id 0 --oya 1 --remaining-tiles 66 \
  --iishanten-continuation-depth-comparison
```

出力は方式ごとの候補別の値・順位・時間、`Value A -> B` の候補別増分、`Top ExpectedSelfTsumoValue candidate`、`First-draw contribution A -> B` の最初のツモ1牌種単位の内訳、`Search size A -> B` の枝数・state 数・terminal scoring 数です。B の深度が現行 production の打牌選択が使う深度で、A は production 接続前の旧設定を比較 baseline として残したものです。経路の段数が違うため、A と B の値を同じ量として比較しないでください。この option は全1向聴候補を単独評価するだけで、production の cohort 絞り込みを通しません。

`Top ExpectedSelfTsumoValue candidate` と `same top ExpectedSelfTsumoValue candidate` は、**この軸単独の ranking の1位**であって production が選ぶ打牌ではありません。production は `Shanten → IsolatedTile → IsolatedHonor → ExpectedSelfTsumoValue` の順に既存 comparator を通し、pre-acceptance 軸まで同順位の cohort の中だけでこの値を比べ、その cohort に `unknown` が1件でもあれば軸ごと落とします ([打牌選択](ai/discard-selection.md#1向聴-expectedselftsumovalue) 参照)。この診断はその絞り込みも軸解決も持たず、全1向聴候補を値の高い順に並べるだけです。

方式ごとの実測は3向聴の A/B 比較と同じく新しい thread で行い、どちらも同じ cold な thread-local memo から始めます。この option は他の診断 option とは併用できません。

### 1向聴 selection 深度の A/B 比較

`--iishanten-selection-depth-comparison` は、同じ深度 A/B を **production の打牌 comparator を通した最終打牌選択として** 比較する診断 option です。

```text
A legacy shallow depth   Progress / SameShanten → Progress (production 接続前の旧設定)
B production depth       A に SameShanten → SameShanten → Progress を追加し、exact same-state memo を有効化 (現行 production)
```

`--iishanten-continuation-depth-comparison` が全1向聴候補の値を単独 ranking として並べるのに対し、この option は候補の絞り込み・unknown の軸解決・比較順・安定順序・最終選択まで既存 production selection の経路をそのまま通します。深く評価されるのは pre-acceptance 軸 (`Shanten → IsolatedTile → IsolatedHonor`) まで同順位の cohort だけなので、深度を上げたときの実際の selection cost はこちらでしか分かりません。

```sh
cargo run --release -p bot-scenario -- \
  --hand '34567899m5799p34s' --dora-indicator 3m \
  --round-wind E --seat-wind N --player-id 0 --oya 1 --remaining-tiles 66 \
  --iishanten-selection-depth-comparison
```

出力は方式ごとの選択打牌・cohort・候補別の値と比較理由・時間・探索規模・memo 利用数と、`Selection A -> B` / `ExpectedSelfTsumoValue A -> B` / `Cost A -> B` の差分です。方式ごとに run を2本取り、`elapsed` は探索規模の計上も phase timer も持たない計測 run から、cohort・値・stats は観測 run から取ります。どちらの run も新しい thread で行うため、先に走った run が後の run の thread-local memo を暖めません。

B は追加深度と exact same-state memo を一緒に有効にするため、A → B の elapsed 差は深度だけの差ではありません。同じ memo 条件へ揃えた深度だけの比較は `--iishanten-continuation-depth-comparison` が持ちます。

**production の打牌選択は B です。A は接続前の旧設定を比較 baseline として残しているだけです。** この option はどちらの深度も深い候補評価を逐次で行うので、`elapsed` は production の並列評価の latency ではありません。production が使う候補単位の並列評価は、同じ B depth の中で `--iishanten-selection-parallel-comparison` が比べます。並列評価は値を変えないため、ここに出る B の選択も候補別の値も production のものと一致します。他の診断 option とは併用できません。

### 1向聴 selection の候補単位並列比較

`--iishanten-selection-parallel-comparison` は、上記 production B depth の中で **深く評価する候補をどう分けるか** だけを変えた方式を比較する診断 option です。深度も comparator も値も枝も scoring semantics も変わりません。

```text
S   production と同じ B depth を逐次評価
P2  候補単位の並列、最大2 worker
P4  候補単位の並列、最大4 worker
PA  候補単位の並列、available_parallelism を上限 (現行 production と同じ方式)
```

並列にするのは、production の候補絞り込みが deep 評価対象として残した候補1件分の前方評価だけです。候補1件の前方集計値はその候補の打牌評価と探索設定だけで決まる純関数で、探索内の memo は同じ入力に同じ値を返す cache でしかありません。結果は候補 index へ書き戻すため thread の終了順にも worker 数にも依らず、cohort・候補ごとの `ExpectedSelfTsumoValue`・unknown 軸の解決・比較理由・選択打牌は全方式で bit-exact に一致します (`bit-exact with the sequential mode`)。worker 数は深く評価する候補数を超えません。

```sh
cargo run --release -p bot-scenario -- \
  --hand '34567899m5799p34s' --dora-indicator 3m \
  --round-wind E --seat-wind N --player-id 0 --oya 1 --remaining-tiles 66 \
  --iishanten-selection-parallel-comparison
```

worker はそれぞれ自分の探索基盤を持つため、逐次評価では候補間で共有できていた base 評価 memo・同一 state memo・thread-local の向聴 / 受け入れ memo を worker ごとに作り直します。wall-clock は縮む一方で総仕事量は増え得るので、出力には方式ごとの `elapsed` と speedup に加えて `Total work` として探索規模と memo hit / miss の増減も並べます。

**production の打牌選択は PA と同じ方式です。** 実際に使う worker 数は `min(available_parallelism, 深く評価する候補数)` で、`available_parallelism()` が取得できない環境と並列度1の環境では逐次評価へ落ちます。候補を分けるのは最善向聴数が1向聴の局面だけで、2向聴・3向聴の前方集計値は従来どおり逐次評価のままです。S / P2 / P4 は PA を比べるための baseline として残ります。他の診断 option とは併用できません。

### 2向聴 Full pair の並列比較

`--two-shanten-full-parallel-comparison` は、2向聴の production selection で **ドラ差 gate を通った provisional 上位2候補の Full 追加評価をどう実行するか** だけを変えた方式を比較する診断 option です。Progress-first も ForwardTargets cohort も上位2候補の選び方も Full gate も comparator も Full 値の意味も変わりません。

```text
S   gate を通った2候補を1本の LookaheadInputs で逐次評価
P2  その2候補を min(2, available_parallelism) worker で並列評価 (現行 production と同じ方式)
```

並列にするのは gate を通った2候補の Full 追加評価だけです。候補1件の Full 値はその候補の打牌評価と探索設定と、Progress 段で確定済みの寄与だけで決まる純関数で、探索内の memo は同じ入力に同じ値を返す cache でしかありません。結果は pair index へ書き戻すため thread の終了順にも worker 数にも依らず、2候補の Full `ExpectedSelfTsumoValue`・最終 selected index・選択打牌・比較理由は S / P2 で bit-exact に一致します (`bit-exact with the sequential mode`)。

```sh
cargo run --release -p bot-scenario -- \
  crates/bot-scenario/scenarios/two_shanten_dora_gate_chun.json \
  --two-shanten-full-parallel-comparison
```

worker はそれぞれ自分の探索基盤を持つため、逐次評価では Progress 段で暖まっていた base 評価 memo・構造評価 memo・thread-local の向聴 / 受け入れ memo を worker ごとに作り直します。wall-clock は縮む一方で総仕事量は増えるので、出力には `elapsed` と speedup に加えて `Total work` として探索規模と memo hit / miss の増減も並べます。

**production の打牌選択は P2 と同じ方式です。** Full 追加評価の対象は常に provisional 上位2候補だけなので、要求する worker 数の上限も `min(2, available_parallelism)` で、`available_parallelism()` が取得できない環境と並列度1の環境では逐次評価へ落ちます。ドラ差 gate が発火しない局面では Full 追加評価そのものが走らないので、thread も分けません。Progress cohort の評価は従来どおり逐次のままです。S は P2 を比べるための baseline として残ります。他の診断 option とは併用できません。

### --force-fold

`--force-fold` は、通常の押し引き判断とは無関係に「この局面でベタ降りすると仮定した場合の防御打牌」を確認する option です。防御候補をランキングし、上位候補と model risk・fold risk を並べます。

production bot の判断は変わりません。`ShantenAgent::act()` も `diagnose()` も `--force-fold` の有無で結果が変わらず、この option が `PushPullMode::Fold` を production decision へ注入することもありません。通常診断とは独立した hypothetical evaluation として、既存 Fold defense evaluator を直接実行します。

そのため通常打牌の選択・2手先探索・押し引き判定・Reach / Damaten 判断は走りません。防御 evaluator 自体が必要とする threat facts と exact defense facts は通常どおり使用します。

防御 evaluator も routing も candidate ordering の土台も production の Fold defense と同じもので、`--force-fold` 用の防御ロジックを別に持ちません。ranking だけは、そこへ ForcedFold 固有の [fold risk](#手牌内の同一牌枚数と-fold-risk) を重ねます。

| 相手の threat | 使用する防御 | `source` |
| --- | --- | --- |
| リーチ者のみ | リーチ者向け防御 fallback (共通現物 / exact ron-risk model / 字牌・壁・スジ等) | `DefenseFallback` |
| High OpenHandThreat の相手のみ | OpenHand 防御 fallback | `OpenHandDefenseFallback` |
| リーチ者と High OpenHandThreat の相手が同時 | 複合 threat 防御 fallback | `CombinedThreatDefenseFallback` |

threat の分類も既存 classification と同じ source of truth を使い、High 条件などをここで書き直しません。

[相対席 option](#相手の河とリーチ) と組み合わせて使えます。

```bash
cargo run -p bot-scenario -- \
  --hand "234m455p789s1123z" \
  --draw "N" \
  --discards-shimocha "1m 7p 4s 7p E" \
  --riichi-shimocha 4 \
  --remaining-tiles 42 \
  --force-fold \
  --summary-only
```

```text
Summary
  mode: ForcedFold
  source: DefenseFallback

  rank 1: E
    ron safe: yes
    reason: Genbutsu

  rank 2: S
    ron safe: no
    model risk: 1.68%
    evidence: 95487355974 / 5679785375284
    copies: 1
    fold risk: 1.68%

  rank 3: W
    ron safe: no
    model risk: 1.68%
    evidence: 95487355974 / 5679785375284
    copies: 1
    fold risk: 1.68%
```

OpenHand / 複合 threat では、その family の既存 category をそのまま出します。

```text
Summary
  mode: ForcedFold
  source: OpenHandDefenseFallback

  rank 1: 5m
    ron safe: yes
    reason: SafeAgainstAllTargets
```

`--summary-only` を付けない通常出力では、既存の `Defense` / `Defense candidates` / `OpenHand defense` / `Combined defense` section をそのまま表示するので、候補ごとの safety も確認できます。

#### Summary の防御候補 ranking

Summary には ForcedFold ranking の**上位3候補**と、そこに含まれない **0-risk candidate 全件**を表示します。候補が3件未満なら存在する候補だけ、0-risk candidate が4件以上ある場合は3件を超えてもすべて表示します。

順位は ForcedFold ranking 上の順位そのままで、0-risk candidate を追加表示するために付け替えません。ranking が

```text
1. E
2. N
3. 1m
4. 9p
5. 7s  <- 0-risk
```

なら Summary でも `7s` は `rank 5` として出ます。

ranking の土台は既存 production defense policy です。Reach / OpenHand / Combined のいずれも category precedence・exact `R/T` comparator・複数 target の worst-first lexicographic minimax・heuristic fallback 順序・tie-break・合法 action 順を既存 selector と共有し、表示側で独自の ranking や risk score を作りません。そのうえで exact `R/T` で並んだ段だけを ForcedFold 固有の [fold risk](#手牌内の同一牌枚数と-fold-risk) で並べ替えます。`rank 1` は `Forced fold` section の `selected action` と一致しますが、これは rank 1 用の特別処理ではなく、forced fold の答えが ranking の先頭そのものだからです。ordering の土台については [防御候補の ordering](ai/defense.md#防御候補の-ordering) を参照してください。

全候補が同じ表示経路を通ります。

- `ron safe`: 既存 policy 上、全 defense target からロンされないと確定している候補 (hard-safe) だけ `yes`。その場合は `reason` にその根拠 (`Genbutsu` / `SafeAgainstAllTargets` / `SafeAgainstAllThreats`) を出します。
- `model risk` / `evidence`: hard-safe ではない候補について、exact model が利用可能な場合の `R / T`。単独 target では1行、複数 target では player ごとに出します。**その牌を今1枚切った場合**の値で、手牌内の枚数で補正しません。
- `copies` / `fold risk`: `model risk` を出した候補について、手牌内の同一牌枚数と、それを織り込んだ ForcedFold の順位付け用 score ([手牌内の同一牌枚数と fold risk](#手牌内の同一牌枚数と-fold-risk))。
- `heuristic`: exact model が利用できない候補について、順位を決めた既存 heuristic の根拠。

hard-safe と exact `R == 0` は別の根拠なので、表示で潰しません。

```text
Summary
  mode: ForcedFold

  rank 1: E
    ron safe: yes
    reason: Genbutsu

  rank 2: N
    ron safe: no
    model risk: 0.00%
    evidence: 0 / 3812
    copies: 1
    fold risk: 0.00%

  rank 3: 1m
    ron safe: no
    model risk: 2.74%
    evidence: 104 / 3791
    copies: 1
    fold risk: 2.74%
```

`E` はルール上 hard-safe で、`N` は hard-safe ではないが exact hidden-hand model 上で `R == 0` です。0-risk かどうかは表示上の percentage ではなく integer evidence の `R == 0` で判定するので、`R = 1` / `T = 50000` のように表示が `0.00%` になる候補は 0-risk として扱いません。3枚以上見えた字牌も、それだけでは 0-risk になりません ([0-risk candidate の根拠](ai/defense.md#0-risk-candidate-の根拠))。

複数 target では player ごとの evidence を worst-first で出します。

```text
  rank 3: 9s
    ron safe: no
    model risk:
      player 1: 8.21% (123 / 1498)
      player 3: 2.34% (41 / 1750)
    copies: 2
    fold risk:
      player 1: 4.19%
      player 3: 1.18%
```

`model risk` の percentage は表示専用です。production selection も 0-risk 判定も既存の integer evidence の比較だけを使い、浮動小数点や percentage を comparator に使いません。合計・平均・加重平均・max だけの比較のような独自 risk score も作りません。ForcedFold の順位付けだけは次の `fold risk` を使います。

`R/T` は実際の放銃率ではなく、公開情報と整合する structural tenpai hidden-hand states のうちその牌で現在ロン可能な state の比率です ([`R/T` が表すもの](ai/defense.md#rt-が表すもの))。

#### 手牌内の同一牌枚数と fold risk

ベタ降りは1巡で終わらないので、同じ牌を複数枚持っている価値が順位に出ないと困ります。同一牌を複数枚持つ場合、その1枚目が通ったという事実によって、次巡以降に残りの同一牌を切るときの安全性が高まるからです。

そこで ForcedFold の順位付けだけは、手牌内の同一牌枚数 `copies` を織り込んだ `fold risk` を使います。

```text
fold_risk = 1 - (1 - model_risk) ^ (1 / copies)
```

「`copies` 巡ぶんを1回の `model_risk` でカバーできる」とみなして、この continuation value を簡易的に近似する heuristic です。**実際の放銃確率ではありません**。`copies == 1` では `model_risk` そのものなので、同じ牌を1枚ずつしか持たない局面の順位は従来どおりです。

`copies` は待ち判定上同一になる牌種単位で数えます。ロン牌としては赤5も黒5も同じ牌種なので、`0m` と `5m` を持っていれば `copies` は 2 です。自摸牌も手牌の一部として数えます。複数 target では target ごとに `fold risk` を出し、比較は `model risk` と同じ worst-first の辞書順で行います。

並べ替えるのは exact `R/T` で順位が決まった段の中だけです。hard-safe (`Genbutsu` / `SafeAgainstAllTargets` / `SafeAgainstAllThreats`)・同巡内通過・exact model が使えない heuristic の段は production ordering 上の位置のまま残るので、段の順序と段間の precedence は変わりません。適用先は Reach / OpenHand / Combined のどの exact 段でも同じで、防御 family で分けません。

##### 通過後の safety は target 種別で寿命が違う

この近似は、通過が次巡以降へどれだけ残るかを target 種別ごとに区別していません。実際の safety evidence の寿命は違います ([passed tile の区別](ai/defense.md#passed-tile-の区別))。

- **Reach**: 通れば `post_reach_passed` としてそのリーチ者への現物になります。リーチ者の手牌は変化しないので、この safety は局中継続します。
- **OpenHand**: 非リーチ相手の通過情報は、Reach と同じ永続的な hard-safe ではありません。`same_hand_passed` は「target の concealed hand が最後に変化して以降に通った」ことを前提とする safety evidence で、手出し・ツモ切りか判別できない打牌・鳴き・槓で失効します。production もこれを hard-safe とは扱わず、exact model の `R == 0` とも扱いません。

`fold risk` はこの違いを厳密にモデル化した値ではなく、あくまで ForcedFold ranking 用の heuristic です。通過後の防御状態そのものを評価する continuation value / lookahead は [issue #329](https://github.com/SnowCait/nodocchi/issues/329) で別途検討します。

`model risk` の意味も値も変えません。`fold risk` は ForcedFold の順位付け専用の score で、production の防御判断・押し引き判断・他の診断の ranking には一切使いません。`Forced fold` section の `selected action` は ForcedFold ranking の先頭なので、同じ牌を複数枚持つ局面では `Defense` section が出す production の `selected action` と別の牌になることがあります。

```bash
cargo run -p bot-scenario -- \
  --hand "3567m46888p12457s" \
  --draw "" \
  --discards-shimocha "2z 1p 2m 7z 3s 6s 5p 8m" \
  --riichi-shimocha 5 \
  --remaining-tiles 42 \
  --force-fold \
  --summary-only
```

```text
Summary
  mode: ForcedFold
  source: DefenseFallback

  rank 1: 8p
    ron safe: no
    model risk: 2.62%
    evidence: 64230213477 / 2452679059162
    copies: 3
    fold risk: 0.88%

  rank 2: 5m
    ron safe: no
    model risk: 2.35%
    evidence: 57655088883 / 2452679059162
    copies: 1
    fold risk: 2.35%

  rank 3: 1s
    ron safe: no
    model risk: 2.81%
    evidence: 68849401266 / 2452679059162
    copies: 1
    fold risk: 2.81%
```

1枚だけ切る `model risk` は `5m` のほうが低いままですが、手牌に3枚ある `8p` は1枚目が通れば次巡以降の `8p` の安全性が上がるぶんを `fold risk` が織り込むので、ベタ降りの打牌としては上位になります。

exact model が利用できない候補には存在しない percentage を作らず、順位を決めた既存 heuristic をそのまま出します。

```text
  rank 2: N
    ron safe: no
    model risk: unavailable
    heuristic: HonorSafety(ThreeOrMoreVisible)
```

#### `--summary-only` との関係

`--force-fold --summary-only` は `--force-fold` と**同じ防御評価・同じ ranked candidates・同じ Summary**を出します。違いは表示量だけで、`--summary-only` は `Scenario` / `Table state` / `Forced fold` / `Defense` / `Defense candidates` / `OpenHand defense` / `Combined defense` といった Summary 以外の詳細 section を省きます。

`--summary-only` は計算を省く option ではありません。exact `R == 0` の候補を全件検出するために必要な candidate risk evaluation も、共通現物 / hard-safe / same-hand passed で selection が決着した場合の exact ron-risk evidence の収集も、`--summary-only` でも行います ([防御候補の ordering](ai/defense.md#防御候補の-ordering))。

通常出力でも `--summary-only` でも exact model は1回しか走りません。ranking・Summary・詳細 diagnostics は同じ forced fold evaluation から得た candidate evidence を共有します。

#### unavailable になる場合

リーチ者も High OpenHandThreat の相手もいない局面では、防御対象がないので `unavailable` になります。通常打牌を「ベタ降り最善打牌」として返すことはありません。

```text
Summary
  mode: ForcedFold
  forced fold unavailable: no clear threat
```

合法な Dahai がなく防御打牌を選べない場合も同じく `unavailable` (`no defense discard`) です。

#### exact defense model に必要な fact

防御の exact hidden-hand model は `remaining_tiles` などの局面 fact を実際の評価材料に使います。[相対席 option](#相手の河とリーチ) を使った局面では `remaining_tiles` は明示しない限り unknown のままで、指定した河の枚数から山枚数を推測することはありません。exact model を使いたい場合は `--remaining-tiles` に正しい値を指定してください。必要な fact が足りない場合は、既存 evaluator の fallback semantics (字牌 / 壁 / スジ等) に従います。

自分の河や `melds`、`post_reach_passed` を含む正確な局面は JSON scenario または RiichiLab capture で指定します。`--force-fold` は通常の JSON scenario、inline scenario、RiichiLab capture のいずれでも使えます。

```bash
cargo run -p bot-scenario -- \
  scenarios/combined_threat_defense.json \
  --force-fold
```

benchmark / comparison 系の専用 mode や、通常打牌の追加診断を要求する option (`--lookahead`、`--two-shanten-self-tsumo`、各 cost 計測、各 A/B 比較) とは併用できません。

### --allow-ryukyoku

`--allow-ryukyoku` は九種九牌を**合法手として与える** option です。入力した手牌が九種九牌の成立条件 (么九牌9種以上) を満たすかどうかは判定しません。実対局と同じく、九種九牌が合法かどうかは入力側が source of truth で、nodocchi は成立条件を再判定しません。

合法手として与えたうえで、宣言するか続行するかは production の policy が決めます。

```bash
cargo run -p bot-scenario -- \
  --hand "158m158p5s123456z" \
  --draw "7z" \
  --allow-ryukyoku \
  --summary-only
```

```text
Summary
  choice 1: Ryukyoku
  choice 1 source: Ryukyoku

  ryukyoku: declare
  ryukyoku shanten: standard 8 / chiitoitsu 6 / kokushi 4
```

么九牌が10種あって国士3向聴になる手牌では、同じ option でも宣言せず続行します。

```bash
cargo run -p bot-scenario -- \
  --hand "158m15p15s123456z" \
  --draw "7z" \
  --allow-ryukyoku \
  --summary-only
```

```text
Summary
  choice 1: 8m
  choice 1 source: NormalDiscard

  ryukyoku: continue
  ryukyoku shanten: standard 8 / chiitoitsu 6 / kokushi 3
```

条件は [麻雀 AI の概要](ai/overview.md#九種九牌-ryukyoku)、出力の読み方は [Structured diagnostics](diagnostics.md#ryukyoku-九種九牌) を参照してください。

自分の河、`melds`、`post_reach_passed` などは簡易 CLI からは指定できません。より正確な実戦局面は JSON scenario または RiichiLab capture を使用してください。牌効率指標の意味は [打牌選択](ai/discard-selection.md) を参照してください。

### 入力モードごとの fact

| 入力モード | 省略・観測されない fact の扱い |
| --- | --- |
| inline `--hand` | 簡易「何切る」用の上記 baseline を使用 |
| JSON scenario | 省略 field は従来どおり unknown。必要な fact は JSON で明示 |
| RiichiLab capture | capture の observation から観測できる fact を使用し、復元できない履歴 fact は unknown |
| production AI | 実際の入力 facts を使用し、unknown を inline baseline で補完しない |

inline baseline は `bot-scenario` の入力補助であり、AI 本体が未知の局面情報を推測するルールではありません。正確な再現には JSON scenario、実戦観測の再生には RiichiLab capture を使用してください。

## Summary

`--summary-only` は Summary section だけを表示します。Summary は「何を選んだか」と「次点がなぜ負けたか」を短く確認するためのもので、候補ごとの metric 一覧は持ちません。

Summary の各行は `bot-analysis` の `AnalysisResult` が持つ値そのものです。CLI は enum を label へ、固定小数点を表示用の数値へ直すだけで、診断からの値の選択・判定のやり直しは行いません。`-` / `unknown` / `not evaluated` / `none` の使い分けも `AnalysisResult` が区別した状態をそのまま出したものです。

[`--force-fold`](#--force-fold) を指定した場合の Summary は通常判断の Summary ではないので、先頭に `mode: ForcedFold` を置き、choice 1 / 2 / 3 の比較も持ちません。代わりに production defense ordering の `rank` 付き候補を並べます ([Summary の防御候補 ranking](#summary-の防御候補-ranking))。

choice 2 / 3 が数値 comparator で負けた場合だけ、`lost by` の下へ比較値を1行追加します。

```text
  choice 1: 7p
  choice 1 source: NormalDiscard

  choice 2: 6s
  choice 2 source: NormalDiscard
  choice 2 lost by: WeightedNextAcceptanceRemaining
  choice 2 comparison: choice 1 428 > choice 2 396

  choice 3: W
  choice 3 source: NormalDiscard
  choice 3 lost by: WeightedNextAcceptanceRemaining
  choice 3 comparison: choice 2 396 > choice 3 384
```

`comparison:` の値は、その `lost by` を実際に決めた同一比較の winner と loser の値です。下位 choice は上位 choice を除いて再診断するため候補集合が変わり、候補集合単位で有効・無効が決まる軸もあります。順位ごとに別々の診断から値を混ぜず、決着した比較と同じ候補集合から両方の値を取ります。choice 3 が choice 2 に負けた比較なら、比較相手も choice 2 になります。

`StableOrder` や category / bool 系のように、決着した比較から両方の値を取得できない comparator では従来どおり `lost by` だけを表示します。候補ごとの metric 一覧は `Normal discard candidates` を参照してください。

## 牌表記

MJAI 単牌表記と圧縮 MPSZ 表記の両方を受け付けます。空白区切りで混在させることもできます。

```text
234m 5pr 67p E
```

| 表記 | 内容 |
| --- | --- |
| `1m`..`9m` / `1p`..`9p` / `1s`..`9s` | 数牌 |
| `E` `S` `W` `N` `P` `F` `C` | 字牌 |
| `5mr` `5pr` `5sr` | 赤5 |
| `234m455p789s1234z` | 圧縮 MPSZ。`1z`=`E` .. `7z`=`C` |
| `0m` `0p` `0s` | MPSZ の赤5。`406m` は `4m 5mr 6m` |

曖昧な補正は行いません。`123`、`123x`、`8z`、`0z`、`5r` は error です。赤5は各色1枚なので、`00m` のような重複指定も error です。

## JSON scenario

```bash
cargo run -p bot-scenario -- crates/bot-scenario/scenarios/defense.json
```

```json
{
  "hand": "234m455p789s1123z",
  "draw": "N",
  "dora_indicators": "3p",
  "round_wind": "E",
  "seat_wind": "S",
  "player_id": 0,
  "oya": 3,
  "reached": [false, true, false, false],
  "discards": ["", "1m 7p 4s 7p E", "", ""],
  "reach_discard_indices": [null, 4, null, null],
  "post_reach_passed": ["", "", "", ""],
  "history_furiten": {
    "same_turn": false,
    "riichi_missed_win": false
  },
  "extra_visible_tiles": "",
  "legal_dahai": null,
  "allow_hora": false,
  "allow_ryukyoku": false
}
```

`hand`、`draw`、`dora_indicators`、`round_wind`、`seat_wind`、`allow_*` は簡易 CLI の同名 option と同じ意味です。`allow_ryukyoku` も同じく九種九牌を合法手に加えるだけで、成立条件は判定しません。`hand` 以外は省略でき、河は空、`reached` は全員 `false`、`allow_*` は `false` になります。

### JSON field

| field | 内容 |
| --- | --- |
| `player_id` / `oya` | 自分の席 / 親の席。`0`..`3` |
| `reached` | 各 player のリーチ状態。要素数4 |
| `discards` | 各 player の河。入力順のまま扱う。要素数4 |
| `reach_discard_indices` | 各 player のリーチ宣言牌が河の何枚目か。河の1枚目を `1` とする 1-based で、要素数4。省略時と `null` は宣言牌位置 unknown |
| `post_reach_passed` | 各 player のリーチ成立後に他家から切られて通った牌。要素数4 |
| `temporary_passed` | 各 player の最後の手牌変化後に他家から切られて通った牌。要素数4。省略時 unknown |
| `history_furiten` | `same_turn` / `riichi_missed_win`。各値は省略時 unknown |
| `double_riichi` | `eligible` / `declared`。各値は省略時 unknown |
| `riichi_situation` | 各 player の `declared_double_riichi` / `ippatsu`。要素数4で、省略時と `null` は unknown |
| `melds` | 各 player の副露・暗槓。要素数4 |
| `extra_visible_tiles` | 他の field で表現していない見え牌 |
| `legal_dahai` | 打牌可能な牌と候補順 |
| `legal_ankan` | 合法な暗槓 |
| `remaining_tiles` / `honba` / `kyotaku_points` / `scores` / `kyoku` | table state |

### legal_dahai

`legal_dahai` は打牌可能な牌とその順序を明示します。リーチ後のツモ切りだけの局面や候補順に依存する判断の再現に利用できます。省略時は手牌とツモ牌から自動生成します。手牌に無い牌、赤5と黒5が一致しない指定、意味が重複する指定は error です。

### legal_ankan

`legal_ankan` は合法な暗槓を明示します。各要素は手牌とツモ牌から取る同じ牌種4枚で、`"E E E E"` のように書きます。

```json
{
  "hand": "123456789m1p111z",
  "draw": "E",
  "player_id": 0,
  "reached": [true, false, false, false],
  "legal_dahai": "E",
  "legal_ankan": ["E E E E"]
}
```

**暗槓が合法かどうかは入力側が source of truth** です。リーチ後に待ちが変わらないかどうかも含めて、ここへ書いた暗槓はそのまま合法手として渡します。局面そのもの (手牌・見え牌・副露) は変わりません。4枚でない指定、同じ牌種でない指定、手牌とツモ牌に無い指定は error です。

判断内訳は [Structured diagnostics](diagnostics.md#kan) の `Kan` section に出ます。

### melds

`melds` の各面子は次の field を持ちます。

| field | 内容 |
| --- | --- |
| `kind` | `chi` / `pon` / `daiminkan` / `ankan` / `kakan` |
| `tiles` | 面子を構成する物理牌 |
| `called_tile` | 鳴いた牌。`ankan` では指定しない |

副露牌は見え牌へ加わります。`extra_visible_tiles` は副露以外など、他の field で表現されない見え牌に使用します。`seat_wind` は `player_id` と `oya` があれば導出され、矛盾する明示値は error です。

### reach_discard_indices

`reach_discard_indices` は各 player のリーチ宣言牌が河の何枚目かを指定します。上の例では player 1 の河の4枚目の `7p` が宣言牌です。河に同じ牌種が複数あっても index が宣言牌を一意に決めます。

内部の `GameContext` は 0-based で保持し、変換は scenario の解決時に行います。`reached` の意味は変えないので、リーチ済みでも宣言牌位置が unknown な状態 (`null` や field 省略) を表現できます。unknown を河の末尾などから推測することはありません。

次の入力は error です。

- 要素数が4でない
- `0` や河の枚数を超える index
- `reached` が `false` の player への index

field ごと省略できるので、既存の JSON scenario はそのまま動作します。

RiichiLab capture の再生では、observation の `riichi_sutehais` (リーチ宣言時に切った物理牌 ID) と河を照合して同じ index を復元します。宣言牌が河に見つからないなど入力が矛盾する場合は推測せず unknown です。

### post_reach_passed

現物は対象リーチ者自身の河と、そのリーチ成立後に他家から切られて通った牌です。後者は河だけから逆算できないため `post_reach_passed` で指定します。牌種だけを保持し、見え牌や河には影響しません。赤5は黒5と同じ牌種です。

これはリーチ者専用の事実です。非リーチ相手の防御には使いません。詳しくは [防御におけるロン安全根拠](ai/defense.md#target-ごとのロン安全根拠) を参照してください。

```bash
cargo run -p bot-scenario -- crates/bot-scenario/scenarios/post_reach_genbutsu.json
```

### temporary_passed

`temporary_passed[player]` は、その player の最後のツモまたは鳴き・槓以降に、他家から切られてロンされず通った牌種です。赤5と黒5は同じ牌種として扱います。次のツモ、chi / pon / daiminkan / ankan / kakan で手牌が変化すると無効になります。

単一 observation からは復元できない履歴事実なので、JSON scenario では明示してください。field 省略は「安全牌なし」ではなく unknown です。リーチ者用の `post_reach_passed` とは寿命も意味も異なります。

例えば player 0 が 9m を切って通った直後に player 1 がツモると、打牌者自身には登録せず、player 1 の分はツモで消えるため、状態は `["", "", "9m", "9m"]` になります。

### history_furiten

`history_furiten.same_turn` は同巡内フリテン、`history_furiten.riichi_missed_win` はリーチ後のアガリ見逃しによる局中継続フリテンです。各値は `true` / `false` / 省略による unknown を区別します。

指定した値は**現在時点 (今回の打牌の前)** の facts です。ロン可否は恒常フリテンと合わせた総合値で、打牌後の評価時点へ補正してから判定します。`draw` を指定した局面は「自分のツモを経た打牌」になるため、`same_turn` が `true` でもその打牌後は解除されます。`draw` を省略した局面や鳴き後の局面では解除しません。unknown を `false` と推測しないので、軸を省略するとロン可否も unknown になります。規則は [フリテン](ai/furiten.md#総合ロン可否) を参照してください。

```bash
cargo run -p bot-scenario -- crates/bot-scenario/scenarios/history_furiten_same_turn.json
```

### double_riichi

`double_riichi.eligible` は「現在未リーチで、この局面で `Reach` を選ぶとダブル立直が確定するか」、`double_riichi.declared` は「宣言済みの自分のリーチがダブル立直だったか」です。各値は `true` / `false` / 省略による unknown を区別します。

未リーチの局面では `eligible` だけを、`reached[player_id]` が `true` の局面では `declared` だけを読みます。どちらもダブル立直と確定した場合だけダブル立直2翻で打点を評価し、確定できない場合は最低保証として通常立直1翻で評価します。`reached` だけからダブル立直は推測しません。

単一 observation からは復元できない履歴事実なので、JSON scenario では明示してください。RiichiLab の capture replay では `reach` event と宣言牌 `dahai` の時系列から自動的に復元します。

### riichi_situation

`riichi_situation.declared_double_riichi[player]` は「その player の宣言済みリーチがダブル立直だったか」、`riichi_situation.ippatsu[player]` は「今この打牌でその player にロンされた場合に一発が成立するか」です。どちらも player id 順の4要素で、`true` / `false` / `null` (と省略) による unknown を区別します。

```json
{
  "hand": "123456789m1235p",
  "draw": "9s",
  "reached": [false, true, false, false],
  "riichi_situation": {
    "declared_double_riichi": [null, false, null, null],
    "ippatsu": [null, false, null, null]
  }
}
```

未リーチの席に事実を置くことはできません (`reached[player]` が `false` の席へ `true` / `false` を指定すると error)。`reached` だけからは復元できない履歴事実なので、JSON scenario では明示してください。RiichiLab の capture replay では `reach` event・宣言牌 `dahai`・鳴き・ツモの時系列から自動的に復元します。

[単独リーチへの structural expected deal-in loss](diagnostics.md#単独リーチへの-structural-expected-deal-in-loss) はこの2つの事実を必要とし、確定できない場合は通常立直・一発なしと決め打たずに `unavailable` を出します。

### table state

| field | 意味 | 単位 | validation |
| --- | --- | --- | --- |
| `remaining_tiles` | 山の残りツモ可能枚数 | 枚 | 0以上の整数 |
| `honba` | 本場 | 本 | 0以上の整数 |
| `kyotaku_points` | 供託。リーチ棒の本数ではなく点数 | 点 | 0以上の整数 |
| `scores` | player id 順の現在持ち点。負数も指定可能 | 点 | 要素数4 |
| `kyoku` | 場風内の局。東1 / 南1 が `1` | 局 | `1`..`4` |

すべて省略でき、省略時は `0` や25000点で補完せず unknown とします。明示した `0` は観測済みの0として区別します。

```json
{
  "hand": "234m455p789s1123z",
  "draw": "N",
  "remaining_tiles": 42,
  "honba": 1,
  "kyotaku_points": 0,
  "scores": [25000, 24000, 26000, 25000],
  "kyoku": 2
}
```

`remaining_tiles` は `Call -> 打牌 -> 1向聴` における Pass / Call の
`ExpectedSelfTsumoValue` 比較で、流局までの残り自摸回数を求めるためにも使用します。現在1向聴の
比較に加え、現在2向聴から鳴いて1向聴になる候補の比較にも使います。後者は Pass の2向聴 Full
value と、Call 後の1向聴 value・比較結果・最良打牌を `Call` section と `Summary` に表示し、
`call higher` の場合は selected action も Call になります。各値は `Table state` diagnostics でも
確認できます。

## RiichiLab capture の再生

[`riichilab-client --capture-file`](riichilab.md#session-capture) で保存した [session capture](riichilab.md#record-envelope) の `request_action` を1件再生できます。

```bash
cargo run -p bot-scenario -- \
  --riichilab-capture logs/ranked-capture.jsonl \
  --request-id 425
```

| 引数 | 必須 | 内容 |
| --- | --: | --- |
| `--riichilab-capture` | 必須 | session capture JSONL の path |
| `--request-id` | 任意 | 再生する `request_id`。`request_action` が1件だけなら省略可能 |

再生対象は `direction` が `server` で `type` が `request_action` の record だけです。ただし session
record は先頭から順に処理し、server の `dahai` 等を live client と同じ validation state へ反映して、
対象 request の reaction source を復元します。client action や `action_ack` が同じ `request_id` を
持っていても、`--request-id` の対象件数には数えません。

複数の `request_action` を含む file で `--request-id` を省略すると、対象を推測せず error になります。record envelope 自体が壊れている行は skip せず error です。旧 capture 形式 (1行がそのまま `request_action` の raw JSON) は読みません。`--hand` や JSON scenario とは併用できません。

`observation` decoder、`possible_actions` 変換、reaction source の validation state は
`riichilab-client` の実装を共有します。先頭に capture の出所を表示し、以降は JSON scenario と同じ
[structured diagnostics](diagnostics.md) です。

単一 request の observation だけでは次を復元できません。

- event 列から積み上げる `post_reach_passed` は空
- 履歴依存フリテンは unknown

`scores`、`honba`、`kyotaku`、`kyoku` は observation から復元します。`remaining_tiles` は observation に field がありませんが、見えている牌 (全員の河・副露・自分の手牌) から復元します。RiichiLab live client と Chiihou における履歴依存フリテンの違いは [フリテン](ai/furiten.md#入力経路ごとの-known--unknown) を参照してください。

## RiichiLab capture の production latency 計測

session capture 内の `request_action` を全件再生し、復元した局面に対して production と同じ `ShantenAgent::act()` を実行して、その decision latency を request 単位で計測します。同じ capture corpus を revision 間で実行すれば、p50 / p95 / p99 / max や3秒超の件数を同じ方法で比較できます。

計測に使う `GameContext` は replay と同じ経路で、`observation` と capture の server event 列から
復元します。reaction source は event 列から反映しますが、live client が積み上げる
`post_reach_passed`、`temporary_passed`、`same_hand_passed`、履歴依存フリテンは引き続き含まれないため、
capture replay の入力は live client の入力と完全一致しません。復元できない事実は
[RiichiLab capture の再生](#riichilab-capture-の再生) と同じで、入力経路ごとの known / unknown は
[フリテン](ai/furiten.md#入力経路ごとの-known--unknown) を参照してください。revision 間の比較では
同じ capture corpus から同じ入力を復元するので、相対比較の基盤としては有効です。

性能比較は release build で行います。debug build の値は最適化後の decision latency と対応しません。

```bash
cargo build --release -p bot-scenario

./target/release/bot-scenario \
  --benchmark-riichilab-capture \
  logs/game-001.jsonl \
  logs/game-002.jsonl
```

| 引数 | 必須 | 内容 |
| --- | --: | --- |
| `--benchmark-riichilab-capture` | 必須 | session capture JSONL の path。以降に続く path も同じ run の入力として扱う |
| `--benchmark-json` | 任意 | 集計と request ごとの結果を JSON で保存する path |

shell の glob 展開で複数 file を1回の run にまとめられます。`--riichilab-capture`、`--request-id`、`--hand`、JSON scenario、`--lookahead`、`--verbose`、`--summary-only` とは併用できません。

malformed な record や decode できない `observation` は黙って読み飛ばさず、その時点で error になります。

### 計測区間

timer に含むのは、復元済みの `GameContext` と合法手に対する production `ShantenAgent::act()` だけです。

| | 内容 |
| --- | --- |
| 含む | production `ShantenAgent::act()` |
| 含まない | capture file の読み込み、JSON parse、`observation` decode、`GameContext` 構築、合法手構築、出力整形、file I/O、集計 |

計測のために診断 (`--lookahead` / `--verbose` 相当) は構築しません。各 request は1回だけ実行します。同じ request を繰り返す microbenchmark ではありません。

#### phase 別の内訳

request ごとに、production の判断経路をそのまま3つの phase へ分けて計測します。判断を再実行せず、通った経路の経過時間をその場で計上するだけなので、選択結果は計測の有無で変わりません。

| phase | 内容 |
| --- | --- |
| `early` | Hora / Ryukyoku / 鳴きなど、通常打牌選択より前 |
| `normal_discard` | 通常打牌選択の全体 |
| `post_discard` | 通常打牌選択より後の押し引き / Reach / 防御 / 最終 action 選択 |

Hora などで早期 return した request は、到達しなかった phase が 0 のままになります。phase 別の集計や percentile は出しません。

`normal_discard` はさらに内部処理別へ分けます。区切りは通常打牌選択の既存の責務境界そのままで、探索も scoring も比較も変えません。

| subphase | 内容 |
| --- | --- |
| `base` | 合法打牌候補の生成と、向聴 / 受け入れなどの基本評価 |
| `forward` | 通常の forward lookahead と、その探索済み枝からの集計 |
| `two_shanten_self_tsumo` | production comparator の2向聴 Progress-first 評価と、ドラ差 gate 対象 pair の Full 追加評価。total と実際の評価区切りごとの時間を表示する。gate 対象 pair の Full 追加評価は並列に走るため、候補別時間の合計は total を超え得る |
| `three_shanten_self_tsumo` | production comparator の3向聴 Progress-only self-tsumo 評価 |
| `finalize` | 残りの補助評価 (現在聴牌候補の待ち / 打点 / ツモ期待値) と候補比較・最終打牌の確定 |

5つの合計は同じ request の `normal_discard` を超えません。2向聴 EV を実行しない request では `two_shanten_self_tsumo` は 0、候補 timing は空のままです。3向聴 Progress 評価を実行しない request では `three_shanten_self_tsumo` は 0 のままです。通常打牌選択を通らなかった request では全 subphase が 0 のままです。

`early` は鳴き判断だけを内訳として持ちます。反応 turn の latency 調査用の計測で、鳴き policy そのものは変えません。

| subphase | 内容 |
| --- | --- |
| `call` | 鳴き判断全体の壁時計。最初の候補評価から最終候補の選択まで |
| `call_candidates` | 鳴き候補ごとの評価の合計。候補別に kind / 鳴いた牌 / consumed / elapsed と、そのうちの鳴き後の打牌選択 (`post_call_discard`) を表示する |
| `call_pass` | 1向聴 Call / Pass 比較のために1回だけ評価する Pass 側 ExpectedSelfTsumoValue |
| `call_two_shanten_pass` | 2向聴 Call / Pass 比較のために1回だけ評価する Pass 側の2向聴 Full ExpectedSelfTsumoValue |
| `call_remaining` | 候補評価と Pass 評価を除いた残りの鳴き policy 処理 (比較・採用候補の選択など) |

Call / Pass 比較が発火する request では、Call 側の候補評価 group と Pass 側の継続評価を別 thread で重ねます。どちらの elapsed もその評価が実際に走っていた時間なので、`call_candidates` / `call_pass` / `call_two_shanten_pass` / `call_remaining` の合計は壁時計である `call` を超え得ます (その場合 `call_remaining` は 0 になります)。重ねなかった request では従来どおり合計が `call` に一致します。`call` は同じ request の `early` を超えません。合法な Chi / Pon が無い request では全て 0、候補 timing は空のままです。Call / Pass 比較が発火しない request では `call_pass` も `call_two_shanten_pass` も 0 のままです。どちらを評価するかは現在の向聴数が決めるので、同じ request で両方が 0 を超えることはありません。同じ `tile` / `consumed` の重複候補も除かず、合法 action の順にそれぞれ1件ずつ並びます。`early` の残りと `post_discard` の内部は細分化していません。

`DecisionPhaseDurations` / `NormalDiscardPhaseDurations` は scalar のみの `Copy` な DTO です。可変長の評価区切り別 timing は別に保持し、`act_with_phase_timing()` の結果から `two_shanten_self_tsumo_candidates()` で `(TileType, Duration)` の iterator として、1向聴の深い前方評価の候補別 timing は `iishanten_forward_candidates()` で `IishantenForwardCandidateDuration` の slice として、鳴き候補別 timing は `call_candidates()` で `CallCandidateDuration` の slice として読み取れます。ドラ差 gate を通った上位2候補は Progress と Full 追加評価の区切りが別々記録されるため、同じ牌種が2回現れます。Full 追加評価の2件は並列に走るため、それぞれの elapsed は worker が独立に計った実測で、候補 timing の合計は `two_shanten_self_tsumo` phase を超え得ます。

候補ごとに複数の値を持つ `IishantenForwardCandidateDuration` と `CallCandidateDuration` は bot-core の public API です。打牌と実測だけの2向聴候補は `(TileType, Duration)` として読めれば足りるので、その内部型は公開しません。

`forward` はさらに前方集計値の内部処理別へ分けます。こちらも既存の処理境界そのままで、探索する枝も scoring も集計も変えません。

| subphase | 内容 |
| --- | --- |
| `lookahead_search` | 通常 lookahead の仮想ツモ枝探索。ツモ後の次打牌評価と、その枝が使う将来打点の scoring を含む |
| `weighted_aggregation` | 探索済みの枝からの重み付き集計 (WeightedNextAcceptance / weighted tenpai wait) |
| `self_tsumo_continuation` | 探索済みの通常 lookahead 枝からの1向聴 self-tsumo continuation 集計 |

3つの合計は同じ request の `forward` を超えません。この意味は従来から変わりません。計上するのは計測 thread が実際に通った区切りだけです。前方集計値の入力を組み立てる時間はどの内訳にも入りません。前方集計値を計算しない局面 (テンパイ、最善向聴を維持する候補が1件など) では 0 のままです。`self_tsumo_continuation` は2向聴 EV ではありません。

production が1向聴候補を候補単位で並行に評価した request では、計測 thread が phase の区切りを1つも通らないため、3つとも従来どおり 0 のままです。その request の内訳は下の `forward_candidates` から読みます。候補の内訳をこの3つへ足し込んで、wall-clock phase の内訳から「並行に評価した候補の実時間の合計」へ意味を変えることはしていません。

`forward` はさらに、production が深い前方評価を実際に行った1向聴候補ごとの実測 (`forward_candidates`) も持ちます。取るのは時間だけで、探索規模や memo の利用数は持ちません。

| 項目 | 内容 |
| --- | --- |
| `discard` | その候補の打牌 |
| `elapsed` | その候補を評価していた実時間 |
| `lookahead_search` / `weighted_aggregation` / `self_tsumo_continuation` | `elapsed` の内訳。`forward` の subphase と同じ区切り |

並ぶのは production が実際に深く評価した候補 (既存の候補絞り込み `forward_target_mask` が残した cohort) だけです。深い評価の対象にならなかった候補は前方評価そのものを通らないため、1件も混ざりません。順序は production の候補順 (合法打牌の評価順) そのままで、並行に評価した request でも thread の終了順には依らず、同じ index の候補へ書き戻した結果と一致します。最善向聴数が1向聴でない request と、前方集計値を計算しない request では空のままです。

候補の `elapsed` と、その内訳の3つは、いずれもその候補を評価していた実時間です。production の1向聴候補は候補単位で並行に評価するため、候補の `elapsed` の合計も、候補の内訳の合計も、壁時計である `forward` を超え得ます。これは2向聴 Full 追加評価や Call / Pass の重なりと同じ semantics です。

| 値 | 意味 |
| --- | --- |
| `forward` (`normal_discard_forward_ns`) | production phase の壁時計 |
| `forward_candidates[*].elapsed` | その候補を評価していた worker の実時間 |
| `forward_candidates[*].lookahead_search` など | その候補の中での実時間 |

合計を壁時計として読まないでください。逆に、phase 側の `lookahead_search` / `weighted_aggregation` / `self_tsumo_continuation` は従来の wall-clock 内訳のままで、候補側の値とは別物です。

この計測は benchmark でだけ有効にします。通常の RiichiLab client は候補単位の計測を有効にしないため、候補評価は計測を入れる前と同じ helper をそのまま通り、時計も候補の計測器も作りません。探索規模の counter はこの計測では扱いません。必要な場合は `--iishanten-selection-parallel-comparison` の観測 run から読みます。

### 出力

```text
RiichiLab production latency benchmark
  captures: 12
  requests: 742
  total: 136528.000 ms
  mean: 184.000 ms
  p50: 72.000 ms
  p90: 510.000 ms
  p95: 820.000 ms
  p99: 1810.000 ms
  max: 2470.000 ms
  > 500 ms: 83
  > 1 s: 21
  > 2 s: 3
  > 3 s: 0

Slowest requests
  2470.000 ms  logs/game-003.jsonl  request_id=425  early=0.012 ms (call=0.000 ms call_candidates=0.000 ms count=0 [] call_pass=0.000 ms call_two_shanten_pass=0.000 ms call_remaining=0.000 ms)  normal_discard=2401.000 ms (base=30.000 ms forward=951.000 ms [lookahead_search=0.000 ms weighted_aggregation=0.000 ms self_tsumo_continuation=0.000 ms] forward_candidates=2 [3m=620.000 ms [lookahead_search=590.000 ms weighted_aggregation=18.000 ms self_tsumo_continuation=12.000 ms] 9s=601.000 ms [lookahead_search=572.000 ms weighted_aggregation=17.000 ms self_tsumo_continuation=12.000 ms]] two_shanten_self_tsumo=1400.000 ms candidates=2 [5m=720.000 ms 8m=670.000 ms] three_shanten_self_tsumo=0.000 ms finalize=20.000 ms)  post_discard=68.988 ms  selected=9s
  1146.000 ms  logs/game-011.jsonl  request_id=279  early=1144.933 ms (call=1140.000 ms call_candidates=877.000 ms count=2 [Chi(3m<-2m,4m)=440.000 ms post_call_discard=438.000 ms Chi(3m<-2m,4m)=437.000 ms post_call_discard=435.000 ms] call_pass=262.000 ms call_two_shanten_pass=0.000 ms call_remaining=1.000 ms)  normal_discard=0.000 ms (base=0.000 ms forward=0.000 ms [lookahead_search=0.000 ms weighted_aggregation=0.000 ms self_tsumo_continuation=0.000 ms] forward_candidates=0 [] two_shanten_self_tsumo=0.000 ms candidates=0 [] three_shanten_self_tsumo=0.000 ms finalize=0.000 ms)  post_discard=0.000 ms  selected=None
  2310.000 ms  logs/game-008.jsonl  request_id=317  early=0.010 ms (call=0.000 ms call_candidates=0.000 ms count=0 [] call_pass=0.000 ms call_two_shanten_pass=0.000 ms call_remaining=0.000 ms)  normal_discard=2200.000 ms (base=28.000 ms forward=2152.000 ms [lookahead_search=2100.000 ms weighted_aggregation=32.000 ms self_tsumo_continuation=20.000 ms] forward_candidates=0 [] two_shanten_self_tsumo=0.000 ms candidates=0 [] three_shanten_self_tsumo=0.000 ms finalize=20.000 ms)  post_discard=109.990 ms  selected=5p
```

percentile は nearest-rank です。昇順に並べた `n` 件について順位 `ceil(p / 100 * n)` の値をそのまま採用し、補間しません。threshold の件数は閾値を厳密に超えた request だけを数えます。`selected` は計測した production decision そのものです。

`Slowest requests` は elapsed 降順に最大20件表示します。`early` / `normal_discard` / `post_discard` は同じ request の phase 別内訳で、`early` と `normal_discard` の括弧内はその内訳、`forward` の角括弧内はさらにその内訳です。`early` の括弧内には鳴き判断の total・候補別 timing・Pass 側 timing・残りを表示します。2向聴 EV は total の後に、実際に評価した `ForwardTargets` の候補数と `discard=elapsed` を表示し、その後に3向聴 Progress 評価の total を表示します。`candidates` の件数と `discard=elapsed` には、ドラ差 gate を通った上位2候補の Full 追加評価が Progress の区切りとは別に並びます。この2件は並列に走る独立した実測なので、`candidates` の合計は `two_shanten_self_tsumo` を超え得ます。同じ局面は `--riichilab-capture` と `--request-id` で再調査できます。

```bash
./target/release/bot-scenario \
  --riichilab-capture logs/game-003.jsonl \
  --request-id 425
```

### machine-readable output

`--benchmark-json` は集計と request ごとの結果を JSON で保存します。時間は ns です。

```json
{
  "summary": {
    "captures": 12,
    "requests": 742,
    "total_ns": 136528000000,
    "mean_ns": 184000000,
    "p50_ns": 72000000,
    "p90_ns": 510000000,
    "p95_ns": 820000000,
    "p99_ns": 1810000000,
    "max_ns": 2470000000,
    "over_500ms": 83,
    "over_1s": 21,
    "over_2s": 3,
    "over_3s": 0
  },
  "requests": [
    {
      "capture": "logs/game-003.jsonl",
      "request_id": 425,
      "actor": 0,
      "elapsed_ns": 2470000000,
      "early_ns": 12000,
      "call_ns": 0,
      "call_candidates_ns": 0,
      "call_pass_iishanten_self_tsumo_ns": 0,
      "call_pass_two_shanten_self_tsumo_ns": 0,
      "call_pass_three_shanten_self_tsumo_ns": 0,
      "call_remaining_ns": 0,
      "call_candidate_count": 0,
      "call_candidates": [],
      "normal_discard_ns": 2401000000,
      "normal_discard_base_ns": 30000000,
      "normal_discard_forward_ns": 951000000,
      "forward_lookahead_search_ns": 0,
      "forward_weighted_aggregation_ns": 0,
      "forward_self_tsumo_ns": 0,
      "iishanten_forward_candidate_count": 2,
      "iishanten_forward_candidates": [
        {
          "discard": "3m",
          "elapsed_ns": 620000000,
          "lookahead_search_ns": 590000000,
          "weighted_aggregation_ns": 18000000,
          "self_tsumo_continuation_ns": 12000000
        },
        {
          "discard": "9s",
          "elapsed_ns": 601000000,
          "lookahead_search_ns": 572000000,
          "weighted_aggregation_ns": 17000000,
          "self_tsumo_continuation_ns": 12000000
        }
      ],
      "two_shanten_self_tsumo_ns": 1400000000,
      "two_shanten_self_tsumo_candidate_count": 2,
      "two_shanten_self_tsumo_candidates": [
        { "discard": "5m", "elapsed_ns": 720000000 },
        { "discard": "8m", "elapsed_ns": 670000000 }
      ],
      "three_shanten_self_tsumo_ns": 0,
      "normal_discard_finalize_ns": 20000000,
      "post_discard_ns": 68988000,
      "selected": "9s"
    }
  ]
}
```

`requests` は計測順、つまり capture の指定順と file 内の `request_action` record 順です。`early_ns` / `normal_discard_ns` / `post_discard_ns` は phase 別の内訳で、合計は `elapsed_ns` を超えません。`normal_discard_base_ns` / `normal_discard_forward_ns` / `two_shanten_self_tsumo_ns` / `three_shanten_self_tsumo_ns` / `normal_discard_finalize_ns` は `normal_discard_ns` の内訳で、合計は `normal_discard_ns` を超えません。`forward_lookahead_search_ns` / `forward_weighted_aggregation_ns` / `forward_self_tsumo_ns` は `normal_discard_forward_ns` の wall-clock の内訳で、合計は `normal_discard_forward_ns` を超えません。この意味は従来から変わりません。production が1向聴候補を候補単位で並行に評価した request では、計測 thread が phase の区切りを通らないため従来どおり 0 のままで、その request の内訳は `iishanten_forward_candidates` から読みます。`iishanten_forward_candidates` は production が深い前方評価を実際に行った1向聴候補だけを production の候補順そのままで持ち、その件数を `iishanten_forward_candidate_count` にも出します。深く評価されなかった候補は1件も混ざりません。候補ごとに `discard` / `elapsed_ns` と、その内訳の `lookahead_search_ns` / `weighted_aggregation_ns` / `self_tsumo_continuation_ns` を持ちます。候補単位で並行に評価するため、候補の `elapsed_ns` の合計も、候補の `lookahead_search_ns` などの合計も、phase の wall-clock である `normal_discard_forward_ns` を超え得ます。合計を wall-clock として読まないでください。候補側の `lookahead_search_ns` などはその候補の中での実時間で、request 単位の `forward_lookahead_search_ns` とは別物です。最善向聴数が1向聴でない request では 0 と空 array のままです。`two_shanten_self_tsumo_candidates` は production が実際に評価した `ForwardTargets` だけを評価順に持ち、その件数を `two_shanten_self_tsumo_candidate_count` にも出します。Progress 候補は逐次評価しますが、Full gate を通った上位2候補の Full 追加評価は並列に走るため、候補別時間の合計は phase の wall-clock である `two_shanten_self_tsumo_ns` を超え得ます。`call_ns` は `early_ns` の内訳です。Call 側の候補評価 group と Pass 側の継続評価は重ねて走るため、`call_candidates_ns` / `call_pass_iishanten_self_tsumo_ns` / `call_pass_two_shanten_self_tsumo_ns` / `call_pass_three_shanten_self_tsumo_ns` / `call_remaining_ns` の合計は壁時計である `call_ns` を超え得ます。重ねなかった request では合計が `call_ns` に一致します。`call_pass_two_shanten_self_tsumo_ns` は現在2向聴の Call / Pass 比較が評価した Pass 側の2向聴 Full、`call_pass_three_shanten_self_tsumo_ns` は現在3向聴の Call / Pass 比較が評価した Pass 側の3向聴 Progress-only で、読み方はどちらも `call_pass_iishanten_self_tsumo_ns` と同じです。どれを評価するかは現在の向聴数が決めるので、同じ request で複数が 0 を超えることはありません。`call_candidates` は production が実際に評価した鳴き候補だけを評価順に持ち、候補ごとに `kind` / `tile` / `consumed` / `elapsed_ns` / `post_call_discard_selection_ns` を持ちます。件数は `call_candidate_count` にも出します。鳴き候補が無い request では 0 と空 array のままです。

CI の共有 runner は実行時間が安定しないため、CI では集計や percentile の correctness だけを test し、実測値を pass / fail の threshold にはしません。実性能値は release build を実環境で実行して取得します。

## fixture との使い分け

capture は実戦局面を見つけて調べる入口、JSON scenario は恒久的な回帰 fixture です。

1. `riichilab-client` で対局を capture する
2. capture の client action と `action_ack`、または log の `action sent` から問題の `request_id` を特定する
3. `bot-scenario --riichilab-capture ... --request-id ...` で再生する
4. diagnostics から判断経路を確認する
5. 原因が分かったら局面を JSON scenario に落として回帰 fixture にする

既存 fixture は [`crates/bot-scenario/scenarios/`](../crates/bot-scenario/scenarios/) にあります。副露 threat の段階比較には `open_hand_*.json`、複合 threat には `combined_threat_defense.json` などを使用します。`open_hand_value_pon_and_chi.json` は現在、通常役牌1翻だけの2副露なので `Present` です。正確な境界条件は production tests を source of truth としてください。 局面構築そのものの回帰 fixture は [`crates/bot-analysis/scenarios/`](../crates/bot-analysis/scenarios/) にあります。
