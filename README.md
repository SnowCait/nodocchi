# のどっち

リーチ麻雀AI

## RiichiLab 接続

`riichilab-client` で RiichiLab に接続します。

```text
usage: riichilab-client [validate|ranked] [--agent normal|tsumogiri|shanten]
```

### 接続モード

`validate` は validation 用 endpoint、`ranked` は ranked play 用 endpoint に接続します。argument を省略した場合は `validate` として扱います。ranked play には RiichiLab 上で active な bot が必要です。

```bash
cargo run -p riichilab-client --bin riichilab-client -- validate
cargo run -p riichilab-client --bin riichilab-client -- ranked
```

### Agent の指定

`--agent shanten` を指定すると、向聴ベースの `ShantenAgent` を使用します。未指定の場合は `NormalAgent` を使用します。

```bash
--agent normal
--agent tsumogiri
--agent shanten
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
usage: chiihou-client --channel <hanchan|tonpuu> [--agent normal|tsumogiri|shanten] [--server-npub <NPUB_OR_NPROFILE>] [--auto-next] [--response-delay-ms <MILLISECONDS>]
```

| 引数 | 必須 | 内容 |
| --- | --: | --- |
| `--channel` | 必須 | `hanchan` または `tonpuu` |
| `--agent` | 任意 | `normal`、`tsumogiri`、`shanten`。既定値は `normal` |
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

最終判断は必ず `ShantenAgent::diagnose()` の構造化診断から取得します。`bot-scenario` 側で打牌選択・押し引き・防御判断を再実装することはありません。

### 簡易 CLI

牌効率をすぐ確認する用途です。

```bash
cargo run -p bot-scenario -- \
  --hand "234m455p789s1123z" \
  --draw "N"
```

```text
usage:
  bot-scenario --hand <TILES> [--draw <TILE>] [--dora <TILES>] [--round-wind <WIND>]
               [--seat-wind <WIND>] [--allow-reach] [--allow-hora] [--allow-ryukyoku] [--verbose]
  bot-scenario <SCENARIO_JSON> [--verbose]
```

| 引数 | 必須 | 内容 |
| --- | --: | --- |
| `--hand` | 必須 | ツモ牌を除いた手牌 |
| `--draw` | 任意 | ツモ牌。2牌以上へ展開される文字列は error |
| `--dora` | 任意 | ドラ表示牌。ドラそのものではない |
| `--round-wind` | 任意 | 場風。`E` / `S` / `W` / `N` のみ |
| `--seat-wind` | 任意 | 自風。`E` / `S` / `W` / `N` のみ |
| `--allow-reach` | 任意 | `LegalAction::Reach` を追加する |
| `--allow-hora` | 任意 | `LegalAction::Hora` を追加する |
| `--allow-ryukyoku` | 任意 | `LegalAction::Ryukyoku` を追加する |
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

  "discards": [
    "",
    "1m 4m 7p E",
    "",
    ""
  ],

  "extra_visible_tiles": "",

  "legal_dahai": null,

  "allow_reach": false,
  "allow_hora": false,
  "allow_ryukyoku": false
}
```

`hand` 以外は省略可能で、省略時は `draw` / `round_wind` / `seat_wind` / `player_id` / `oya` / `legal_dahai` が `None`、`dora_indicators` / `extra_visible_tiles` / 各河が空、`reached` が `[false; 4]`、`allow_*` が `false` になります。`reached` と `discards` は指定する場合、四麻用に要素数 4 が必須です。

`visible_tiles` は直接指定できません。`hand` + `draw` + `dora_indicators` + 全 player の `discards` + `extra_visible_tiles` から、各 zone へ割り当て済みの物理牌をそのまま再利用して構築します。副露牌など schema で直接表現していない見え牌は `extra_visible_tiles` に指定します。

`seat_wind` は、省略かつ `player_id` と `oya` が両方ある場合に `(player_id + 4 - oya) % 4` から導出します。明示指定と導出結果が矛盾する場合は error です。

`legal_dahai` を指定すると、その順序がそのまま `LegalAction` 順になります。リーチ後のツモ切りのみ合法な局面や、StableOrder 依存の調査に使用します。省略時は `hand` → `draw` の入力順で合法 Dahai を自動生成し、同じ意味の action は重複させません（同一牌種の黒牌は代表1件、赤牌は代表1件）。`legal_dahai` は新しい牌を割り当てず、必ず `hand` + `draw` の物理牌へ対応付けます。手牌に無い牌や赤黒の不一致、同一意味の重複は error です。

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
| `0m` `0p` `0s` | MPSZ の赤5。`406m` は `4m 5mr 6m` |

曖昧な補正は行いません。`123` / `123x` / `8z` / `0z` / `00m` / `5r` / `10m` はいずれも error です。`10m` は「1m と 赤5m」とも「十萬」とも読めるため、`1` の直後の `0` は曖昧として拒否します。赤5を意図する場合は `1m 0m` と分けて入力してください。

### 出力

`Scenario` / `Final decision` / `Normal discard` / `Normal discard candidates` / `Push/Pull` / `Defense` / `Defense candidates` の section を表示します。

```text
Final decision
  action: 1m
  source: DefenseFallback
  defense kind: Genbutsu
```

通常打牌候補では、選択候補との比較結果を構造化診断の `comparison_reason` からそのまま `lost by:` として表示します。比較で決着しない場合は `lost by: StableOrder` になります。

構造化診断で評価されていない判断は推測せず、`not evaluated` と表示します。評価したが候補が無かった場合の `evaluated` / `selected: none` とは区別します。

## 公式ドキュメント

- RiichiLab Documentation: https://riichi.dev/docs
