# のどっち

リーチ麻雀AI

## RiichiLab 接続

`riichilab-client` で RiichiLab に接続します。

```text
usage: riichilab-client [validate|ranked] [--agent normal|tsumogiri|shanten|menzen]
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
| `extra_visible_tiles` | 副露牌など、他の field で表現していない見え牌 |
| `legal_dahai` | 打牌可能な牌とその順序 |

`hand` 以外は省略できます。省略した場合、河は空、`reached` は全員 `false`、`allow_*` は `false` として扱います。

見え牌は手牌・ツモ牌・ドラ表示牌・河・`extra_visible_tiles` から自動的に扱われます。副露など追加で見えている牌は `extra_visible_tiles` に指定してください。

`seat_wind` は省略しても、`player_id` と `oya` があれば自動で決まります。明示した自風がその席と矛盾する場合は error です。

`legal_dahai` を指定すると、打牌可能な牌とその順序を明示できます。リーチ後のツモ切りのみの局面や、候補順に依存する判断の再現に利用できます。省略した場合は手牌とツモ牌から自動的に作られます。手牌に無い牌や、赤5と黒5が一致しない指定は error です。

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

入力した局面（`Scenario`）に続けて、最終的に選んだ打牌（`Final decision`）と、その根拠として通常打牌の候補比較（`Normal discard candidates`）・押し引き（`Push/Pull`）・防御（`Defense` / `Defense candidates`）を表示します。

```text
Final decision
  action: 1m
  source: DefenseFallback
  defense kind: Genbutsu
```

通常打牌候補では、選ばれなかった理由も表示されます。その判断処理を通らなかった場合は `not evaluated` と表示されます。

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

## 公式ドキュメント

- RiichiLab Documentation: https://riichi.dev/docs
