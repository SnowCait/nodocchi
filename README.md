# のどっち

リーチ麻雀AI

## RiichiLab 接続

`riichilab-client` で RiichiLab に接続します。

```text
usage: riichilab-client [validate|ranked] [--agent normal|tsumogiri|shanten|menzen] [--log-file <PATH>] [--capture-file <PATH>]
```

### 接続モード

`validate` は validation 用 endpoint、`ranked` は ranked play 用 endpoint に接続します。argument を省略した場合は `validate` として扱います。ranked play には RiichiLab 上で active な bot が必要です。

```bash
cargo run -p riichilab-client --bin riichilab-client -- validate
cargo run -p riichilab-client --bin riichilab-client -- ranked
```

### Agent の指定

`--agent shanten` を指定すると、向聴ベースの `ShantenAgent` を使用します。`--agent menzen` を指定すると、`ShantenAgent` と同じ基本判断を使いつつ、チー・ポン・明槓など門前を崩す鳴きを行わない `MenzenAgent` を使用します。未指定の場合は `NormalAgent` を使用します。

```bash
--agent normal
--agent tsumogiri
--agent shanten
--agent menzen
```

### 実行例

validation の例:

```bash
RIICHILAB_BOT_TOKEN=...
cargo run -p riichilab-client --bin riichilab-client -- validate --agent shanten
```

ranked の例:

```bash
RIICHILAB_BOT_TOKEN=...
cargo run -p riichilab-client --bin riichilab-client -- ranked --agent shanten
```

`RIICHILAB_BOT_TOKEN` は secret として扱い、repository・README・log に残さないでください。
`ranked` は実戦 queue に入るため、validation と bot activation を確認してから実行してください。

### ログのファイル保存

`--log-file <PATH>` を指定すると、console 出力を維持したまま、同じ log を指定した file にも保存します。file 側は ANSI escape sequence を含まないため、そのまま検索・copy できます。log directory は自動生成しないため、file を開けない場合は起動時に error となります。

`ShantenAgent` の対局判断を後から調査する例:

```bash
RUST_LOG=info,bot_core::agent_decision=debug,bot_core::discard_selection=debug \
cargo run -p riichilab-client --bin riichilab-client -- \
  ranked \
  --agent shanten \
  --log-file logs/ranked.log
```

`RUST_LOG` の filter は console と file で共通です。上記のように target を追加すると、`agent decision` や `discard_selection` などの診断 log が console と `logs/ranked.log` の双方へ出力されます。

### request_action の capture

`--capture-file <PATH>` を指定すると、server から受信した `request_action` をそのまま保存します。保存した record は `bot-scenario` でそのまま再生でき、実戦で見つけた問題局面をオフラインで調査できます。

```bash
RIICHILAB_BOT_TOKEN=...
cargo run -p riichilab-client --bin riichilab-client -- \
  ranked \
  --agent shanten \
  --log-file logs/ranked.log \
  --capture-file logs/ranked-capture.jsonl
```

validation で試す場合:

```bash
RIICHILAB_BOT_TOKEN=...
cargo run -p riichilab-client --bin riichilab-client -- \
  validate \
  --agent shanten \
  --capture-file logs/validate-capture.jsonl
```

| 項目 | 内容 |
| --- | --- |
| 形式 | JSONL。`request_action` 1件が1行1 JSON object |
| 保存内容 | 受信した `request_action` の raw JSON そのもの。`request_id` / `possible_actions` / `observation` / `time` など受信した field をすべて保持する |
| 保存対象 | `request_action` のみ。`start_game` / `action_ack` / `end_game` などは保存しない |
| 単位 | client 1起動 = 1対局 = capture file 1つ |
| file | `--log-file` とは別 file。`observation` の base64 が大きいため通常 log には混ぜない |
| 未指定時 | capture 処理を一切行わない。clone・JSON 変換・file I/O ともに追加されない |

`GameContext` から `request_action` を逆生成せず、受信した JSON をそのまま1行として書きます。decode → 再 serialize による情報欠落が無いため、後から同じ局面を正確に再生できます。

capture file は「client 1起動 = ranked 1対局または validation 1対局」の単位で扱います。起動時に既存 file の内容へ追記せず、新しい session として file を置き換えます（既存 file があれば truncate します）。RiichiLab の `request_id` は対局内では一意ですが、対局を跨いだ一意性は保証されないため、1 file に複数対局を混ぜません。複数対局を残したい場合は、対局ごとに別 path を指定してください。

```bash
cargo run -p riichilab-client --bin riichilab-client -- \
  ranked --agent shanten \
  --capture-file "logs/ranked-capture-$(date +%Y%m%d-%H%M%S).jsonl"
```

1対局の中では、複数の `request_action` を同じ file へ順次追記します。JSONL なので、途中終了してもそこまでの record をそのまま利用できます。`grep` や `jq` で request_id を絞り込めます。

```bash
jq -r 'select(.request_id == 425)' logs/ranked-capture.jsonl
```

書き込みは通常 log と同じく non-blocking writer 経由で行い、`request_action` の deadline より capture I/O を優先しません。buffer が溢れた場合は応答を遅らせるより record を落とし、落ちた件数を warning log に出します。capture file を開けない場合は silent に無効化せず、起動時 error になります（log directory と同じく、directory は自動生成しません）。

capture file は実戦局面の発見・調査のための入口です。原因を特定してロジックを修正するときは、必要な局面を `bot-scenario` の JSON scenario へ落として恒久的な回帰 fixture にしてください。capture file 自体を fixture として大量に抱えることは想定していません。

## 地鳳接続

`chiihou-client` は起動時に地鳳 server の最新卓状態を取得して `gamestart` / `join` を自動送信し、その後 Nostr relay 経由で request を受信して、指定した Agent で打牌を選択して返信します。

### 実行方法

```bash
CHIIHOU_NSEC=nsec1... \
cargo run -p chiihou-client --bin chiihou-client -- \
  --channel hanchan \
  --agent shanten
```

server を上書きする場合:

```bash
CHIIHOU_NSEC=nsec1... \
cargo run -p chiihou-client --bin chiihou-client -- \
  --channel hanchan \
  --agent shanten \
  --server-npub npub1...
```

自動 next を有効化する場合:

```bash
CHIIHOU_NSEC=nsec1... \
cargo run -p chiihou-client --bin chiihou-client -- \
  --channel hanchan \
  --agent shanten \
  --auto-next
```

応答 publish 前に遅延を入れる場合:

```bash
CHIIHOU_NSEC=nsec1... \
RUST_LOG=info \
cargo run -p chiihou-client --bin chiihou-client -- \
  --channel hanchan \
  --agent shanten \
  --response-delay-ms 1000
```

自動 next と併用する場合:

```bash
CHIIHOU_NSEC=nsec1... \
RUST_LOG=info \
cargo run -p chiihou-client --bin chiihou-client -- \
  --channel hanchan \
  --agent shanten \
  --auto-next \
  --response-delay-ms 5000
```

### 引数

```text
usage: chiihou-client --channel <hanchan|tonpuu> [--agent normal|tsumogiri|shanten|menzen] [--server-npub <NPUB_OR_NPROFILE>] [--auto-next] [--response-delay-ms <MILLISECONDS>]
```

| 引数 | 必須 | 内容 |
| --- | --: | --- |
| `--channel` | 必須 | `hanchan` または `tonpuu` |
| `--agent` | 任意 | `normal`、`tsumogiri`、`shanten`、`menzen`。既定値は `normal` |
| `--server-npub` | 任意 | server の NIP-19 `npub` または `nprofile`。省略時は既定 server |
| `--auto-next` | 任意 | 局終了ごとに `next` を 1 回送信する。省略時は送信しない |
| `--response-delay-ms` | 任意 | 応答 event を publish する前の遅延（ミリ秒）。既定値は `0`（遅延なし） |

`--response-delay-ms` は、publish の集中を緩和するため、GET reply と自動 next の送信前に任意の遅延を設定できます。

既定 server:

```text
npub1j0ng5hmm7mf47r939zqkpepwekenj6uqhd5x555pn80utevvavjsfgqem2
```

### 自動参加動作

- 卓が存在しなければ `gamestart`
- 募集中なら `join`
- 対局中または next 待ちなら何も送信しない
- `gamestart` の送信者は 1 人目として登録されるため、追加の `join` は送らない
- command 送信後も同じ process で request を待ち受ける

### 環境変数

| 環境変数 | 必須 | 内容 |
| --- | --: | --- |
| `CHIIHOU_NSEC` | 必須 | AI の NIP-19 nsec |
| `RUST_LOG` | 任意 | logging filter。既定値は `info` |

### 注意事項

- `CHIIHOU_NSEC` は secret として扱い、repository・README・issue・PR・log に実値を残さないでください。
- shell history へ nsec を直接書く運用にも注意してください。
- `CHIIHOU_NSEC` は hex 秘密鍵を受け付けません。
- `--server-npub` は hex 公開鍵を受け付けません。
- event および filter 内部では hex へ正規化されます。
- 卓状態の取得に失敗した場合は推測で command を送信しません。
- `--server-npub` は既定 server を上書きする高度な用途向けです。
- command の受理確認や競合時の retry は未実装です。

## 局面解析

`bot-scenario` は、手牌・局面を入力して `ShantenAgent` の判断とその根拠をオフラインで確認する CLI です。実対局・WebSocket 接続は行いません。

### 簡易 CLI

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
| `--round-wind` | 任意 | 場風。`E` / `S` / `W` / `N` のみ |
| `--seat-wind` | 任意 | 自風。`E` / `S` / `W` / `N` のみ |
| `--allow-reach` | 任意 | リーチを合法手に加える |
| `--allow-hora` | 任意 | 和了を合法手に加える |
| `--allow-ryukyoku` | 任意 | 流局を合法手に加える |
| `--lookahead` | 任意 | 打牌候補ごとの2手先概要を追加表示する。`--verbose` と併用すると受け入れ牌ごとの詳細も出す |
| `--verbose` | 任意 | 通常打牌候補の詳細を追加表示する |

`player_id` / `oya` / `reached` / `discards` は簡易 CLI では指定できません。防御局面は JSON scenario を使用します。

### JSON scenario

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
  "extra_visible_tiles": "",
  "legal_dahai": null,
  "allow_reach": false,
  "allow_hora": false,
  "allow_ryukyoku": false
}
```

`hand` / `draw` / `dora_indicators` / `round_wind` / `seat_wind` / `allow_*` は簡易 CLI の同名 option と同じです。JSON だけで指定できる field は次のとおりです。

| field | 内容 |
| --- | --- |
| `player_id` / `oya` | 自分の席 / 親の席。`0`..`3` |
| `reached` | 各 player のリーチ状態。要素数 4 |
| `discards` | 各 player の河。入力順のまま扱う。要素数 4 |
| `post_reach_passed` | 各 player のリーチ成立後に他家から切られて通った牌。要素数 4 |
| `melds` | 各 player の副露・暗槓。要素数 4。各面子は `kind` (`chi` / `pon` / `daiminkan` / `ankan` / `kakan`) と `tiles`、暗槓以外は鳴いた牌 `called_tile` を持つ |
| `extra_visible_tiles` | 副露牌など、他の field で表現していない見え牌 |
| `legal_dahai` | 打牌可能な牌とその順序 |

`hand` 以外は省略できます。省略した場合、河は空、`reached` は全員 `false`、`allow_*` は `false` として扱います。

現物は「対象リーチ者自身の河にある牌」と「そのリーチ成立後に他家から切られて通った牌」の両方です。後者は河から逆算できないため `post_reach_passed` で指定します。`post_reach_passed` は牌種だけを持つので、見え牌にも河にも影響しません。赤5は黒5と同じ牌種として扱います。

`post_reach_passed` はリーチ者専用の情報です。「リーチ後に通った」という根拠は非リーチの相手には成立しないため、非リーチ副露相手に対する防御（`OpenHand defense`）では使いません。リーチ者と `High` の副露相手が同時にいる複合 threat の防御（`Combined defense`）でも、`post_reach_passed` を安全根拠にできるのはリーチ者に対してだけです。

```bash
cargo run -p bot-scenario -- crates/bot-scenario/scenarios/post_reach_genbutsu.json
```

上の scenario は「player1 が 3p でリーチ → player2 が 4s でリーチ」の局面で、4s が player1 にも player2 にも現物になることを確認できます。

見え牌は手牌・ツモ牌・ドラ表示牌・河・`extra_visible_tiles` から自動的に扱われます。副露など追加で見えている牌は `extra_visible_tiles` に指定してください。

`seat_wind` は省略しても、`player_id` と `oya` があれば自動で決まります。明示した自風がその席と矛盾する場合は error です。

`legal_dahai` を指定すると、打牌可能な牌とその順序を明示できます。リーチ後のツモ切りのみの局面や、候補順に依存する判断の再現に利用できます。省略した場合は手牌とツモ牌から自動的に作られます。手牌に無い牌や、赤5と黒5が一致しない指定は error です。

### RiichiLab capture の再生

`riichilab-client --capture-file` で保存した `request_action` を、そのまま1件再生できます。

```bash
cargo run -p bot-scenario -- \
  --riichilab-capture logs/ranked-capture.jsonl \
  --request-id 425
```

| 引数 | 必須 | 内容 |
| --- | --: | --- |
| `--riichilab-capture` | 必須 | capture した JSONL の path |
| `--request-id` | 任意 | 再生する `request_id`。record が1件だけなら省略できる |

record が複数ある file で `--request-id` を省略した場合は、どれを再生するか推測せず error になります。`--hand` や JSON scenario とは併用できません。

`observation` の decode と `possible_actions` の変換は `riichilab-client` の実装をそのまま共有します。`bot-scenario` 側に Observation decoder は持ちません。出力の先頭に capture の出所を表示し、以降は JSON scenario と同じ structured diagnostics です。

```text
RiichiLab capture
  file: logs/ranked-capture.jsonl
  request_id: 425
  actor: None
  possible actions: 12
  legal actions: 12

Scenario
  hand: 1m 2m 3m 4m 5m 6m 5p 5p 7s 8s N P P
  draw: 6p
  ...
```

`post_reach_passed`（リーチ成立後に他家から切られて通った牌）は event 列から積み上げる情報で `observation` に含まれないため、replay では空になります。この情報を含めた検証は JSON scenario で行ってください。

非リーチ副露相手 (OpenHandThreat) の調査は次の流れです。

1. 実戦を capture する

    ```bash
    cargo run -p riichilab-client --bin riichilab-client -- \
      ranked --agent shanten \
      --log-file logs/ranked.log \
      --capture-file logs/ranked-capture.jsonl
    ```

2. 問題の `request_id` を特定する。`logs/ranked.log` の `action sent` や `action_ack` から、疑わしい局面の `request_id` を探す
3. その `request_id` を replay する

    ```bash
    cargo run -p bot-scenario -- \
      --riichilab-capture logs/ranked-capture.jsonl \
      --request-id 425
    ```

4. `Player threats` で相手の副露 facts（`reached` / `discards` / `open melds` / `meld kinds` / `meld dora`）と `open hand threat` を、`OpenHand defense` で `High` の相手に対する打牌ごとの safety を、`Combined defense` でリーチ者と `High` の相手が同時にいる場合の safety を、`Push/Pull` で `opponent reach count` と `reason` を、`Summary` で `selected` と `runner-up` を確認する

    ```text
    Push/Pull
      mode: Fold
      reason: TwoOrMoreShantenAgainstHighOpenHand
      opponent reach count: 0

    player 1
      opponent: yes
      reached: no
      discards: 9
      melds: 2
      open melds: 2
      meld kinds: Chi 1, Pon 1
      open hand threat: High
      open hand threat reason: TwoOrMoreOpenMeldsFromNineDiscards
    ```

5. 原因になった比較軸が分かったら、その局面を JSON scenario に落として回帰 fixture にする

capture file は調査の入口で、恒久的な回帰テストは既存の JSON scenario 側に置きます。

### 牌表記

MJAI 単牌表記と圧縮 MPSZ 表記の両方を受け付けます。空白区切りで両者を混在させても構いません。

```text
234m 5pr 67p E
```

| 表記 | 内容 |
| --- | --- |
| `1m`..`9m` / `1p`..`9p` / `1s`..`9s` | 数牌 |
| `E` `S` `W` `N` `P` `F` `C` | 字牌 |
| `5mr` `5pr` `5sr` | 赤5 |
| `234m455p789s1234z` | 圧縮 MPSZ。字牌は `1z`=`E` .. `7z`=`C` |
| `0m` `0p` `0s` | MPSZ の赤5。`0m`=`5mr` / `0p`=`5pr` / `0s`=`5sr`。`406m` は `4m 5mr 6m` |

曖昧な補正は行いません。`123` / `123x` / `8z` / `0z` / `5r` はいずれも error です。赤5は各色1枚しか存在しないため、`00m` のように同じ赤5を複数指定した場合も error です。

### 出力

入力した局面（`Scenario`）に続けて、最終的に選んだ打牌（`Final decision`）と、その根拠として通常打牌の候補比較（`Normal discard candidates`）・押し引き（`Push/Pull`）・リーチ（`Reach`）・防御（`Defense` / `Defense candidates` / `OpenHand defense` / `Combined defense`）を表示します。

```text
Final decision
  action: 1m
  source: DefenseFallback
  defense kind: Genbutsu
```

防御 fallback は、リーチ者向け (`source: DefenseFallback` + `defense kind`)、非リーチ副露相手向け (`source: OpenHandDefenseFallback` + `open hand defense category`)、両者が同時にいる複合 threat 向け (`source: CombinedThreatDefenseFallback` + `combined defense category`) を別の経路として区別します。どれで最終 action を選んだかは `Final decision` / `Summary` / `bot_core::agent_decision` のログから判別できます。

```text
Final decision
  action: 5m
  source: OpenHandDefenseFallback
  open hand defense category: DiscardedByAllTargets
```

`Reach` は、通常打牌で選んだ打牌を切った後のテンパイ形に基づく判断です。選んだ打牌・打牌後の向聴・ツモ和了できる待ちの残枚数と種類数・恒常フリテン・ロン可否・理由を表示します。リーチを検討するのは押し引きが `Push` の場合だけで、それ以外は `not evaluated` になります。

```bash
cargo run -p bot-scenario -- crates/bot-scenario/scenarios/reach_tanki_wait.json
```

```text
Reach
  evaluated
  decision: no
  reason: InsufficientLiveWait
  selected discard: N
  shanten: 0
  live wait: 3 remaining / 1 types
  permanent furiten: no
  ron: yes
  tenpai waits: 5s
  live tenpai waits: 5s
  discarded waits: none
```

上の scenario は打 北 で 5s 単騎テンパイになる局面です。打牌前の14枚をそのまま見ると受け入れは `5s` と `北` の6枚ありますが、実際に切る打牌を決めた後の待ちは 5s の3枚だけなので、リーチせずダマに進みます。

通常打牌候補では、選ばれなかった理由も表示されます。その判断処理を通らなかった場合は `not evaluated` と表示されます。

`Player threats` は、局面から観測できる player ごとの事実と、そこから求めた副露相手の暫定 classification を表示する section です。リーチ・親・自風・河の枚数 (`discards`)・副露数 (`melds` / `open melds` / `kans`) と `MeldKind` の内訳、副露ごとの牌・公開かどうか・槓かどうか・ドラ・赤ドラ・役牌になり得るかを出します。

```bash
cargo run -p bot-scenario -- crates/bot-scenario/scenarios/opponent_threat.json
```

```text
player 1
  opponent: yes
  reached: no
  dealer: no
  seat wind: S
  discards: 0
  melds: 2
  open melds: 2
  kans: 0
  meld kinds: Chi 1, Pon 1
  meld dora: 2
  meld red dora: 1
  open meld dora: 2
  open meld red dora: 1
  open confirmed value honor: 1
  open hand threat: High
  open hand threat reason: TwoOrMoreWithValueHonor
  meld 1: Pon P P P
    open: yes
    kan: no
    dora: 0
    red dora: 0
    dragon: yes
    round wind: no
    seat wind: no
  meld 2: Chi 4m 5mr 6m
    open: yes
    kan: no
    dora: 2
    red dora: 1
```

`player_id` が不明でも席を除外せず常に4席分を表示し、自分か他家か (`opponent`)・親か (`dealer`) ・自風が確定できない場合は推測せず `unknown` / `None` と表示します。暗槓は fixed meld として `melds` と `kans` に数えますが `open melds` には数えません。`discards` はその player が河へ切った枚数そのもので、「河2段目」のような表示上の区切りからは導出しません。

`meld dora` / `meld red dora` は暗槓を含む fixed meld 全体の集計です。`open meld dora` / `open meld red dora` / `open confirmed value honor` は公開された副露だけの集計で、暗槓のドラ・赤ドラ・役牌は含みません。

#### 副露相手の暫定 OpenHandThreat

`open hand threat` は、非リーチで副露している他家に対する暫定 classification です。観測 facts だけを入力にした暫定 heuristic であり、テンパイ確率・放銃率・推定打点を表すものではありません。

| level | 条件 |
| --- | --- |
| `High` | 下の暫定条件のいずれかを満たす |
| `Present` | `High` ではないが `open melds` が1つ以上ある |
| `None` | `open melds` が0。暗槓だけの相手もここ |

`High` の暫定条件は次のいずれかです。

- `open melds` が3つ以上 (`ThreeOrMoreOpenMelds`)
- `open melds` が2つ以上かつ確定役牌の副露がある (`TwoOrMoreWithValueHonor`)
- `open melds` が2つ以上かつ公開副露内のドラが2枚以上 (`TwoOrMoreWithDora`)
- 親が `open melds` を2つ以上持つ (`DealerWithTwoOrMoreOpenMelds`)
- `open melds` が2つ以上かつ `discards` が9枚以上 (`TwoOrMoreOpenMeldsFromNineDiscards`)
- `open melds` が1つ以上かつ `discards` が12枚以上 (`OpenMeldFromTwelveDiscards`)

局進行の threshold (2副露以上 + 河9枚以上、1副露以上 + 河12枚以上) は、中盤以降の副露相手を警戒するための現在の暫定値です。テンパイ確率を計算した結果ではなく、実戦の regression test に基づいて将来調整します。複数の条件を同時に満たす場合、`open hand threat reason` には上の並び順で最初に一致した条件だけを表示します。level 自体はどの条件を満たしても `High` です。

自分の席・リーチ済みの席・`player_id` 不明で自分かどうか確定できない席は分類の対象外で、`not applicable (SelfSeat)` / `not applicable (Reached)` / `not applicable (UnknownSeat)` と表示します。リーチ者の危険度は既存のリーチ情報が source of truth なので、OpenHandThreat とは二重に適用しません。席が不明な相手を他家と推測して `Present` / `High` にすることも、逆に危険度なしと確定させることもしません。

#### High OpenHandThreat の押し引き

他家リーチ者が0人で `open hand threat: High` の相手が1人以上いる局面は、`decide_push_pull()` の新しい policy の対象になります。同じ classification を押し引きと `OpenHand defense` の両方が参照し、High 条件をそれぞれで書き直しません。

| 自分の状態 | mode | reason |
| --- | --- | --- |
| 攻撃評価を作れない | `Neutral` | `MissingOffenseAgainstHighOpenHand` |
| テンパイ (向聴 <= 0) | `Push` | `TenpaiAgainstHighOpenHand` |
| 一向聴 / 受け入れ8枚以上・2種類以上 | `Neutral` | `StrongIishantenAgainstHighOpenHand` |
| 一向聴 / 完全形・受け入れ6枚以上・2種類以上 | `Neutral` | `CompleteIishantenAgainstHighOpenHand` |
| 一向聴 / 自分が親・受け入れ7枚以上・2種類以上 | `Neutral` | `DealerIishantenAgainstHighOpenHand` |
| 一向聴 / 自分が子・簡易打点 proxy 4以上 | `Neutral` | `HighValueIishantenAgainstHighOpenHand` |
| 上記以外の一向聴 | `Fold` | `IishantenAgainstHighOpenHand` |
| 二向聴以上 | `Fold` | `TwoOrMoreShantenAgainstHighOpenHand` |

一向聴の threshold は既存のリーチ policy と同じ pure helper を共有しており、リーチ局面の境界は変えていません。テンパイなら High の副露相手がいても押し、情報不足 (攻撃評価なし) を理由に強制 Fold にはしません。`Push` の場合は Reach → 通常打牌 → 防御 fallback、`Neutral` は通常打牌 → 防御 fallback という既存の順序をそのまま使うため、High というだけで安全牌を通常打牌より優先することはありません。安全牌を優先するのは `Fold` の場合だけです。

`open hand threat: Present` の相手は行動を変えません。`High` の相手がいない局面は従来どおり `NoOpponentReach` → `Push` です。

他家リーチ者が1人以上いる局面には、この policy を適用しません。リーチ者と `High` の副露相手が同時にいる局面は、次の複合 threat policy の対象になります。

#### 非リーチ副露相手への OpenHand defense safety

`OpenHand defense` は、`open hand threat: High` の非リーチ副露相手に対する防御 safety の section です。target は `Player threats` の classification をそのまま source of truth にして選び、防御側で危険度を分類し直しません。`High` の相手だけが target で、`Present` / `None` の相手・自分の席・リーチ済みの席・`player_id` 不明の席は target にしません。

```bash
cargo run -p bot-scenario -- crates/bot-scenario/scenarios/open_hand_defense.json
```

```text
OpenHand defense
  targets: 1, 3
  selected action: 5m
  selected category: DiscardedByAllTargets

5m
  selected: yes
  discarded by all targets: yes
  discarded by target[1]: yes
  discarded by target[3]: yes
  honor safety: -
  opponent honor value: -
  wall: NoWall
  suji safety[1]: NoSuji
  suji safety[3]: HalfSuji
  suji safety: NoSuji
  suited safety: NoSafety
  category: DiscardedByAllTargets
```

| 行 | 内容 |
| --- | --- |
| `targets` | `High` の相手の席。いなければ `none` で候補も出さない |
| `selected action` / `selected category` | 実際に採用した OpenHand 防御 fallback。採用しなかった場合は `selected: none` |
| `selected` | その候補が採用されたか |
| `discarded by target[n]` | その相手自身の河に同じ牌種があるか |
| `discarded by all targets` | 全 target 自身の河にあるか。target が0人なら `no` |
| `honor safety` | 字牌の見え枚数による安全度。既存 Defense と同じ4段階 |
| `opponent honor value` | まだロンされ得る target にとっての役牌価値。最も危険な値を採る |
| `wall` | 順子待ち経路の壁 / ワンチャンス。見え牌由来で target に依らない |
| `suji safety[n]` | その相手の河に対するスジ安全度。その相手単独の評価 |
| `suji safety` | まだロンされ得る target 全体のスジ安全度。最も危険な rank を採る |
| `suited safety` | 壁とスジを統合した数牌の安全度 |
| `category` | `DiscardedByAllTargets` → `HonorSafety` → `SuitedSafety` の大分類 |

M リーグ公式ルールでは「自己の捨て牌にアガリ形を構成できる牌がある聴牌」がフリテンで、フリテン時はツモアガリのみです（<https://m-league.jp/about/>）。そのため対象 player 自身の河にある牌は、リーチの有無によらずその player からのロンについて安全根拠として使えます。`category` の第一分類 `DiscardedByAllTargets` はこの根拠だけを使います。

一方、`post_reach_passed`（リーチ成立後に他家から切られて通った牌）はリーチ者専用で、非リーチ副露相手には使いません。リーチ者向けの現物 (`Defense` の `genbutsu`) が本人の河と `post_reach_passed` の両方を含むのに対し、`OpenHand defense` は本人の河だけを見ます。「本人の河」と「リーチ後に通った牌」を混ぜないため、第一分類の名前も `Genbutsu` とは分けています。

字牌の見え枚数・役牌価値・壁・スジは既存 Defense と同じ判定を共有し、副露相手用に別実装を持ちません。違うのは対象 player 集合の決め方だけで、複数 target の集約はリーチ者向けと同じく最も危険な評価（スジは最小 rank、役牌価値は最大値）を採ります。場風や親が不明で風牌を確定できない場合は `-` のままにし、客風とは推測しません。

target ごとに評価が変わる `opponent honor value` / `suji safety` / `suited safety` は、その牌でまだロンされ得る target だけを集約します。`discarded by target[n]: yes` の相手はフリテンでその牌をロンできないため、その相手の無スジや役牌価値を全体の危険度に持ち込みません。除外根拠は本人の河だけで、`post_reach_passed` は使いません。`suji safety[n]` はその相手単独の評価なので、除外された相手の値は集約後の `suji safety` と一致しないことがあります。全 target が河に切っている牌は集約対象が0人になりますが、`suji safety` を `Suji` とは扱わず、安全根拠は `category: DiscardedByAllTargets` が表します。

押し引きが `Fold` になった場合は、この safety から `DiscardedByAllTargets` → `HonorSafety` → `SuitedSafety` の順で fallback を選び、通常打牌より優先します。字牌は既存 Defense と同じ見え枚数の安全度で並べ、同じ rank 内は `opponent honor value` の切りやすい順 (`GuestWind` → `SingleValueHonor` → `DoubleWind`) にします。数牌は既存 Defense と同じ `NoChance` → `OneChance` → `Suji` → `HalfSuji` の順で、`NoSafety` しか無い場合は fallback として選びません。fallback を1件も選べない場合だけ通常打牌に戻ります。

選択そのものは production の selector が source of truth で、この section はその結果を `selected` に写すだけです。`act()` と `diagnose()` は同じ selector を共有するため、`Final decision` の `action` と `selected action` は必ず一致します。

`Push` / `Neutral` では順序を変えないため、`High` の相手がいても安全牌が通常打牌より優先されることはありません。リーチ者だけがいる局面ではこの fallback に切り替えず、既存のリーチ者向け防御 fallback (`Defense`) をそのまま使います。リーチ者と `High` の相手が同時にいる複合 threat 局面では、`Defense` でも `OpenHand defense` でもなく `Combined defense` を使います。

#### RiichiThreat + High OpenHandThreat の複合 threat

他家リーチ者が1人以上いて、かつ `open hand threat: High` の相手も1人以上いる局面は複合 threat として扱います。判定条件は `opponent_reach_count >= 1` かつ `High` の相手が1人以上いることだけで、`open hand threat: Present` の相手は複合 threat に含めません。`High` の条件は `Player threats` の classification がそのまま source of truth です。

押し引きは、単独の子リーチより強い pressure として判定します。

| 自分の状態 | mode | reason |
| --- | --- | --- |
| 攻撃評価を作れない | `Neutral` | `MissingOffenseAgainstCombinedThreat` |
| テンパイ (向聴 <= 0) | `Neutral` | `TenpaiAgainstCombinedThreat` |
| 一向聴 | `Fold` | `IishantenAgainstCombinedThreat` |
| 二向聴以上 | `Fold` | `TwoOrMoreShantenAgainstCombinedThreat` |

単独の子リーチだけならテンパイは `Push` ですが、複合 threat では押しません。ただしテンパイから即 `Fold` にもせず `Neutral` に留め、リーチだけを抑制して通常打牌は維持します。情報不足 (攻撃評価なし) を理由に強制 `Fold` にもしません。

一向聴では、単独の子リーチや `High` の副露相手単独に対する限定補正 (強い一向聴・完全一向聴・自分が親・簡易高打点) を適用しません。これらは片方だけの threat に対する補正として維持し、複合 threat には持ち込みません。

判定順は 複合 threat → リーチのみ → 非リーチ副露相手のみ です。リーチ者だけの局面、`High` の副露相手だけの局面、`Present` だけの局面、threat が無い局面の判定は変わりません。

#### 複合 threat の Combined defense safety

`Combined defense` は、複合 threat 局面の防御 safety の section です。target はリーチ情報と `Player threats` の classification をそのまま source of truth にして選び、防御側でリーチ者も `High` の相手も判定し直しません。複合 threat ではない局面は `targets: none` で候補も出さず、防御は既存の `Defense` / `OpenHand defense` が担当します。

```bash
cargo run -p bot-scenario -- crates/bot-scenario/scenarios/combined_threat_defense.json
```

```text
Combined defense
  targets: 1(Riichi), 3(HighOpenHand)
  selected action: 5m
  selected category: SafeAgainstAllThreats

5m
  selected: yes
  safe against all threats: yes
  ron safe[1 Riichi]: yes
  ron safe[3 HighOpenHand]: yes
  honor safety: -
  opponent honor value: -
  wall: NoWall
  suji safety[1]: NoSuji
  suji safety[3]: HalfSuji
  suji safety: NoSuji
  suited safety: NoSafety
  category: SafeAgainstAllThreats
```

| 行 | 内容 |
| --- | --- |
| `targets` | 複合 threat の target。席と種類 (`Riichi` / `HighOpenHand`)。複合 threat でなければ `none` |
| `selected action` / `selected category` | 実際に採用した複合 threat 用の防御 fallback。採用しなかった場合は `selected: none` |
| `ron safe[n kind]` | その target にその牌でロンされないと言えるか |
| `safe against all threats` | 全 target に対してロン安全か。target が0人なら `no` |
| `honor safety` | 字牌の見え枚数による安全度。既存 Defense と同じ4段階 |
| `opponent honor value` | まだロンされ得る target に対する役牌価値のうち最も危険な評価 |
| `wall` / `suji safety[n]` / `suji safety` / `suited safety` | 壁・target ごとのスジ・その集約・数牌 safety |
| `category` | `SafeAgainstAllThreats` → `HonorSafety` → `SuitedSafety` の大分類 |

ロン安全の根拠は target の種類ごとに違います。リーチ者は既存の現物判定 (本人の河 + `post_reach_passed`) を使い、`High` の副露相手は本人の河だけを使います。`post_reach_passed` はリーチ固有の情報なので、副露相手には絶対に流用しません。上の例の `9m` は `post_reach_passed[1]` にあるためリーチ者には通りますが、player 3 の河には無いので `safe against all threats: no` になります。

第一分類の `SafeAgainstAllThreats` は、リーチ現物と副露相手本人の河が混ざった集合です。根拠が違うので、既存の `Genbutsu` (リーチ者向け) や `DiscardedByAllTargets` (副露相手向け) とは別の名前にしています。

字牌の見え枚数・役牌価値・壁・スジは既存 Defense と同じ判定を共有し、複合 threat 用に別実装を持ちません。target ごとに評価が変わる `opponent honor value` / `suji safety` / `suited safety` は、その牌でまだロンされ得る target だけを集約します。`ron safe[n kind]: yes` の相手はその牌でロンできないため、その相手の無スジや役牌価値を全体の危険度に持ち込みません。全 target がロン安全な牌は集約対象が0人になりますが、`suji safety` を `Suji` とは扱わず、安全根拠は `category: SafeAgainstAllThreats` が表します。壁は見え牌由来で target に依らないため、既存の `wall_rank` をそのまま共有します。

押し引きが `Fold` になった場合は、この safety から `SafeAgainstAllThreats` → `HonorSafety` → `SuitedSafety` の順で fallback を選び、通常打牌より優先します。字牌は既存 Defense と同じ見え枚数の安全度で並べ、同じ rank 内は `opponent honor value` の切りやすい順 (`GuestWind` → `SingleValueHonor` → `DoubleWind`) にします。数牌は既存 Defense と同じ `NoChance` → `OneChance` → `Suji` → `HalfSuji` の順で、`NoSafety` しか無い場合は fallback として選びません。fallback を1件も選べない場合だけ通常打牌に戻ります。同じ牌種の赤5 / 黒5では既存どおり黒5を優先します。

選択そのものは production の selector が source of truth で、この section はその結果を `selected` に写すだけです。`act()` と `diagnose()` は同じ selector を共有するため、`Final decision` の `action` と `selected action` は必ず一致します。`Neutral` では順序を変えないため、複合 threat でも安全牌が通常打牌より優先されることはありません。

出力の最後には `Summary` を表示します。詳細出力が長くても、ターミナル最下部だけで最終選択と次点候補を確認できます。

```text
Summary
  selected: 7s
  source: DefenseFallback
  selected detail: SuitedSafety(Suji)
  runner-up: 4p
  runner-up source: DefenseFallback
  runner-up detail: SuitedSafety(HalfSuji)
```

次点 (`runner-up`) は、最終選択を除いた場合に次に選ばれる候補です。次点が存在しない場合は `runner-up: -` と表示します。

### 副露相手の比較用 scenario

`scenarios/open_hand_*.json` は、非リーチの副露相手だけがいる局面を段階的に並べた scenario 群です。正解 action を決めるための scenario ではなく、同じ自分の局面に対して相手の観測 facts だけを変えたときの現行判断を見比べるための fixture です。

| scenario | 相手の副露 |
| --- | --- |
| `open_hand_baseline.json` | なし。基準局面 |
| `open_hand_chi.json` | Chi 1副露 |
| `open_hand_value_pon.json` | 役牌 (白) Pon 1副露 |
| `open_hand_two_melds.json` | 役牌・ドラを含まない副露 2つ |
| `open_hand_value_pon_and_chi.json` | 役牌 Pon + 通常副露 |
| `open_hand_dora_melds.json` | ドラと赤ドラが見えている副露 2つ |
| `open_hand_two_melds_nine_discards.json` | 役牌・ドラを含まない副露 2つ + 河9枚 |
| `open_hand_chi_twelve_discards.json` | Chi 1副露 + 河12枚 |
| `open_hand_three_melds.json` | 役牌・ドラを含まない3副露 |
| `open_hand_three_melds_value_dora.json` | 役牌 Pon とドラを含む3副露 |
| `open_hand_dealer_value_pon.json` | 親の役牌 Pon |
| `open_hand_ankan.json` | 暗槓のみ。`open melds` は 0 |

自分の手牌・ツモ牌・合法 Dahai・河・ドラ表示牌は共通なので、出力を diff すると相手の副露 facts の差だけを取り出せます。

```bash
diff \
  <(cargo run -q -p bot-scenario -- crates/bot-scenario/scenarios/open_hand_baseline.json) \
  <(cargo run -q -p bot-scenario -- crates/bot-scenario/scenarios/open_hand_value_pon.json)
```

`open_hand_weak_*.json` は、副露なし・役牌 Pon・3副露の3局面を弱い自分の手 (二向聴) で並べたものです。副露相手の facts と自分の攻撃力を後から組み合わせられるよう、`Push/Pull` の offense だけが違う組を用意しています。

さらに、押し引きが分かれる一向聴の組を最小限だけ用意しています。どちらも「副露なし」と「3副露 (`High`)」の対で、自分の手牌・合法 Dahai は組の中で共通です。

| scenario | 自分の手 | `High` のときの `Push/Pull` |
| --- | --- | --- |
| `open_hand_iishanten_baseline.json` / `open_hand_iishanten_three_melds.json` | 強い一向聴 (受け入れ8枚以上・2種類以上) | `Neutral` / `StrongIishantenAgainstHighOpenHand` |
| `open_hand_weak_iishanten_baseline.json` / `open_hand_weak_iishanten_three_melds.json` | 弱い一向聴 (受け入れ7枚・2種類) | `Fold` / `IishantenAgainstHighOpenHand` |

弱い一向聴の組は `extra_visible_tiles` で受け入れ牌をほぼ見え牌にして、強い一向聴の threshold に届かない形へ固定しています。

副露牌は見え牌に加わるため受け入れが変わり得ますが、この scenario 群は相手の副露牌が自分の受け入れ牌種と重ならない局面に揃えてあるため、`Normal discard` と offense は一致します。

各 scenario の `open hand threat` は、副露なしが `None`、1副露が `Present`、`open_hand_value_pon_and_chi.json` / `open_hand_dora_melds.json` / 3副露の各 scenario が `High` になります。`open_hand_two_melds_nine_discards.json` と `open_hand_chi_twelve_discards.json` だけは相手の河が進んでおり、局進行の threshold で `High` になります。他の scenario は相手の河が空なので、局進行の threshold は `High` の理由になりません。

`High` の scenario では `OpenHand defense` に target と合法 Dahai ごとの safety が出ます。`Present` / `None` だけの scenario は `targets: none` です。`open_hand_defense.json` は、`High` の相手が2人・`Present` の相手が1人いる局面で、本人の河・字牌 safety・壁・スジの出方をまとめて見るための fixture です。

`combined_threat_defense.json` は、player 1 がリーチ・player 3 が3副露の `High`・player 2 が `Present` という複合 threat の fixture です。`Combined defense` で target の種類ごとのロン安全・集約・大分類を見比べられます。リーチ者にだけ通る `post_reach_passed` の牌 (`9m`) と、両方の河にある牌 (`5m`) の違いもこの fixture で確認できます。

押し引きは `High` の scenario でだけ分かれます。テンパイの scenario 群は `TenpaiAgainstHighOpenHand` → `Push`、強い一向聴は `StrongIishantenAgainstHighOpenHand` → `Neutral`、弱い一向聴は `IishantenAgainstHighOpenHand` → `Fold`、二向聴の `open_hand_weak_*.json` は `TwoOrMoreShantenAgainstHighOpenHand` → `Fold` です。`Present` / `None` だけの scenario はどれも従来どおり `NoOpponentReach` → `Push` になります。

`Fold` になる scenario では、`Final decision` の `source` が `OpenHandDefenseFallback` になり、通常打牌より OpenHand 防御 fallback が優先されます。`open_hand_defense.json` は本人の河 (`DiscardedByAllTargets`)、`open_hand_weak_iishanten_three_melds.json` は字牌 (`HonorSafety`) が選ばれる例です。

## 公式ドキュメント

- RiichiLab Documentation: https://riichi.dev/docs
