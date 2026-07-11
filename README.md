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

`--response-delay-ms` は、publish の集中を緩和するため、GET reply と自動 next の送信前に任意の遅延を設定できます。`--auto-next` とは独立しており、遅延を指定しても自動 next は有効になりません。

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

## 公式ドキュメント

- RiichiLab Documentation: https://riichi.dev/docs
