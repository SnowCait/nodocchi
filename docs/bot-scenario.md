# bot-scenario

`bot-scenario` は、手牌・局面を入力して `ShantenAgent` の判断と根拠をオフラインで確認する CLI です。実対局や WebSocket 接続は行いません。出力の読み方は [Structured diagnostics](diagnostics.md)、判断仕様は [麻雀 AI の概要](ai/overview.md) を参照してください。

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
| `--extra-visible-tiles` | 任意 | 他の option で表現していない見え牌 |
| `--remaining-tiles` | 任意 | 山の残りツモ可能枚数 |
| `--no-history-furiten` | 任意 | 同巡内フリテンでもリーチ後見逃しフリテンでもないことを明示 |
| `--allow-hora` | 任意 | 和了を合法手に加える |
| `--allow-ryukyoku` | 任意 | 九種九牌 (`LegalAction::Ryukyoku`) を合法手に加える |
| `--lookahead` | 任意 | 打牌候補ごとの2手先概要と、現在聴牌候補のダマ継続概要を追加。`--verbose` 併用時は受け入れ牌ごと・継続枝ごとの詳細も表示 |
| `--two-shanten-self-tsumo` | 任意 | 2向聴候補の ExpectedSelfTsumoValue を追加 (`--lookahead` を含む) |
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
```

明示した CLI option は baseline より優先されます。`--round-wind`、`--seat-wind`、`--player-id`、`--oya` を指定した場合はその値を使用します。自風は既存の局面解決規則に従い、`player_id` と `oya` が揃えば導出され、両者からの導出値と明示 `--seat-wind` が矛盾する場合は error です。

`--no-history-furiten` は baseline と結果上は同じですが、「現在は同巡内フリテンではなく、かつリーチ後見逃しフリテンでもない」と明示する shorthand です。いずれかが `true` の局面や、履歴フリテンを unknown のまま扱う局面は JSON scenario で指定します。

`--remaining-tiles` は JSON scenario の `remaining_tiles` と同じ意味で、山に残っているツモ可能な牌の枚数です。自分のツモを終えた後の枚数を渡します。self-tsumo continuation は残り自摸機会をこの枚数からだけ求め、巡目や河の枚数から推測しません。省略した局面では unknown のままで、その軸を使いません。

`--extra-visible-tiles` は JSON scenario の `extra_visible_tiles` と同じ意味で、手牌・ツモ牌・ドラ表示牌以外に見えている牌を加えます。加えた牌は受け入れ残枚数や待ちの残枚数へ反映されます。JSON scenario、RiichiLab capture、benchmark とは他の inline option と同じく併用できません。

```bash
cargo run -p bot-scenario -- \
  --hand "34599m235p345567s" \
  --extra-visible-tiles "11p 44p" \
  --summary-only
```

`--lookahead` は2手先概要に加えて、現在打牌後が聴牌になる候補の `Tenpai continuation` (現在聴牌 → 非和了ツモ → 最善打牌 → 再び聴牌) も表示します。待ちが変わる枝とツモ切りで元の待ちを維持する枝の両方を含みます。現時点では diagnostics 専用で打牌選択には接続しておらず、既にリーチしている局面と自分の席が分からない局面では表示しません。詳細は [打牌選択](ai/discard-selection.md#現在聴牌のダマ継続-diagnostics-only) を参照してください。

候補ごとの `self-tsumo comparison` (「今すぐリーチ」と「ダマで1巡継続」を同じ期待ツモ支払いで並べた比較) は、残り自摸機会が確定する局面でだけ値になります。`--remaining-tiles` で山の残枚数を渡さない局面では、自摸機会を推測せず `unknown` のままにします。

```bash
cargo run -p bot-scenario -- \
  --hand "340678m789p34789s" \
  --remaining-tiles 70 \
  --lookahead --verbose
```

`--two-shanten-self-tsumo` は、打牌候補集合の最善向聴数が2向聴の場合に `Two-shanten expected self-tsumo value` を追加します。1向聴の `ExpectedSelfTsumoValue` と同じ尺度で2向聴候補を並べる diagnostics 専用の値で、打牌選択には接続していません。探索は `2向聴 → (Progress / 一度だけの SameShanten) → 1向聴 → 既存の1向聴 continuation` まで進むため、他のどの診断よりも重くなります。残り自摸機会が要るので `--remaining-tiles` と併用してください。詳細は [打牌選択](ai/discard-selection.md#2向聴-expectedselftsumovalue-diagnostics-only) を参照してください。

```bash
cargo run -p bot-scenario -- \
  --hand "11258m234789p13s" \
  --draw "9s" \
  --remaining-tiles 66 \
  --two-shanten-self-tsumo
```

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

`reached` と `discards` は簡易 CLI からは指定できません。防御を含む局面や正確な実戦局面は JSON scenario または RiichiLab capture を使用してください。牌効率指標の意味は [打牌選択](ai/discard-selection.md) を参照してください。

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
  "discards": ["", "1m 4m 7p E", "", ""],
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
| `post_reach_passed` | 各 player のリーチ成立後に他家から切られて通った牌。要素数4 |
| `temporary_passed` | 各 player の最後の手牌変化後に他家から切られて通った牌。要素数4。省略時 unknown |
| `history_furiten` | `same_turn` / `riichi_missed_win`。各値は省略時 unknown |
| `melds` | 各 player の副露・暗槓。要素数4 |
| `extra_visible_tiles` | 他の field で表現していない見え牌 |
| `legal_dahai` | 打牌可能な牌と候補順 |
| `remaining_tiles` / `honba` / `kyotaku_points` / `scores` / `kyoku` | table state |

### legal_dahai

`legal_dahai` は打牌可能な牌とその順序を明示します。リーチ後のツモ切りだけの局面や候補順に依存する判断の再現に利用できます。省略時は手牌とツモ牌から自動生成します。手牌に無い牌、赤5と黒5が一致しない指定、意味が重複する指定は error です。

### melds

`melds` の各面子は次の field を持ちます。

| field | 内容 |
| --- | --- |
| `kind` | `chi` / `pon` / `daiminkan` / `ankan` / `kakan` |
| `tiles` | 面子を構成する物理牌 |
| `called_tile` | 鳴いた牌。`ankan` では指定しない |

副露牌は見え牌へ加わります。`extra_visible_tiles` は副露以外など、他の field で表現されない見え牌に使用します。`seat_wind` は `player_id` と `oya` があれば導出され、矛盾する明示値は error です。

### post_reach_passed

現物は対象リーチ者自身の河と、そのリーチ成立後に他家から切られて通った牌です。後者は河だけから逆算できないため `post_reach_passed` で指定します。牌種だけを保持し、見え牌や河には影響しません。赤5は黒5と同じ牌種です。

これはリーチ者専用の事実です。非リーチ副露相手の防御には使いません。詳しくは [防御におけるロン安全根拠](ai/defense.md#target-ごとのロン安全根拠) を参照してください。

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
`ExpectedSelfTsumoValue` 比較で、流局までの残り自摸回数を求めるためにも使用します。各値は
`Table state` diagnostics でも確認できます。

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
| `forward` | 打牌選択が使う前方集計値 (lookahead / ExpectedSelfTsumoValue / WeightedNextAcceptance など) |
| `finalize` | 残りの補助評価 (現在聴牌候補の待ち / 打点 / ツモ期待値) と候補比較・最終打牌の確定 |

3つの合計は同じ request の `normal_discard` を超えません。通常打牌選択を通らなかった request では 0 のままです。`early` / `post_discard` の内部は細分化していません。

`forward` はさらに前方集計値の内部処理別へ分けます。こちらも既存の処理境界そのままで、探索する枝も scoring も集計も変えません。

| subphase | 内容 |
| --- | --- |
| `search` | 仮想ツモ枝の探索。ツモ後の次打牌評価と、その枝が使う将来打点の scoring を含む |
| `aggregate` | 探索済みの枝からの重み付き集計 (WeightedNextAcceptance / weighted tenpai wait) |
| `self_tsumo` | 探索済みの枝からの ExpectedSelfTsumoValue の集計 |

3つの合計は同じ request の `forward` を超えません。前方集計値の入力を組み立てる時間はどの内訳にも入りません。前方集計値を計算しない局面 (テンパイ、最善向聴を維持する候補が1件など) では 0 のままです。

この計測は benchmark でだけ有効にします。通常の RiichiLab client は計測しません。

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
  2470.000 ms  logs/game-003.jsonl  request_id=425  early=0.012 ms  normal_discard=2401.000 ms (base=30.000 ms forward=2351.000 ms [search=2300.000 ms aggregate=31.000 ms self_tsumo=20.000 ms] finalize=20.000 ms)  post_discard=68.988 ms  selected=9s
  2310.000 ms  logs/game-008.jsonl  request_id=317  early=0.010 ms  normal_discard=2200.000 ms (base=28.000 ms forward=2152.000 ms [search=2100.000 ms aggregate=32.000 ms self_tsumo=20.000 ms] finalize=20.000 ms)  post_discard=109.990 ms  selected=5p
```

percentile は nearest-rank です。昇順に並べた `n` 件について順位 `ceil(p / 100 * n)` の値をそのまま採用し、補間しません。threshold の件数は閾値を厳密に超えた request だけを数えます。`selected` は計測した production decision そのものです。

`Slowest requests` は elapsed 降順に最大20件表示します。`early` / `normal_discard` / `post_discard` は同じ request の phase 別内訳で、`normal_discard` の括弧内はその内訳、`forward` の角括弧内はさらにその内訳です。同じ局面は `--riichilab-capture` と `--request-id` で再調査できます。

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
      "normal_discard_ns": 2401000000,
      "normal_discard_base_ns": 30000000,
      "normal_discard_forward_ns": 2351000000,
      "forward_candidate_search_ns": 2300000000,
      "forward_weighted_aggregation_ns": 31000000,
      "forward_self_tsumo_ns": 20000000,
      "normal_discard_finalize_ns": 20000000,
      "post_discard_ns": 68988000,
      "selected": "9s"
    }
  ]
}
```

`requests` は計測順、つまり capture の指定順と file 内の `request_action` record 順です。`early_ns` / `normal_discard_ns` / `post_discard_ns` は phase 別の内訳で、合計は `elapsed_ns` を超えません。`normal_discard_base_ns` / `normal_discard_forward_ns` / `normal_discard_finalize_ns` は `normal_discard_ns` の内訳で、合計は `normal_discard_ns` を超えません。`forward_candidate_search_ns` / `forward_weighted_aggregation_ns` / `forward_self_tsumo_ns` は `normal_discard_forward_ns` の内訳で、合計は `normal_discard_forward_ns` を超えません。

CI の共有 runner は実行時間が安定しないため、CI では集計や percentile の correctness だけを test し、実測値を pass / fail の threshold にはしません。実性能値は release build を実環境で実行して取得します。

## fixture との使い分け

capture は実戦局面を見つけて調べる入口、JSON scenario は恒久的な回帰 fixture です。

1. `riichilab-client` で対局を capture する
2. capture の client action と `action_ack`、または log の `action sent` から問題の `request_id` を特定する
3. `bot-scenario --riichilab-capture ... --request-id ...` で再生する
4. diagnostics から判断経路を確認する
5. 原因が分かったら局面を JSON scenario に落として回帰 fixture にする

既存 fixture は [`crates/bot-scenario/scenarios/`](../crates/bot-scenario/scenarios/) にあります。副露 threat の段階比較には `open_hand_*.json`、複合 threat には `combined_threat_defense.json` などを使用します。`open_hand_value_pon_and_chi.json` は現在、通常役牌1翻だけの2副露なので `Present` です。正確な境界条件は production tests を source of truth としてください。
