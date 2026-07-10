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

`chiihou-client` は Nostr relay 経由で地鳳の request を受信し、指定した Agent で打牌を選択して返信します。

### 実行方法

```bash
CHIIHOU_NSEC=nsec1... \
cargo run -p chiihou-client --bin chiihou-client -- \
  --server-npub npub1... \
  --channel hanchan \
  --agent shanten
```

### 引数

```text
usage: chiihou-client --server-npub <NPUB> --channel <hanchan|tonpuu> [--agent normal|tsumogiri|shanten]
```

| 引数 | 必須 | 内容 |
| --- | --: | --- |
| `--server-npub` | 必須 | 地鳳 server の NIP-19 npub |
| `--channel` | 必須 | `hanchan` または `tonpuu` |
| `--agent` | 任意 | `normal`、`tsumogiri`、`shanten`。既定値は `normal` |

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
- 現時点の binary は request の購読と返信のみ行い、`join` / `gamestart` はまだ送信しません。

## 公式ドキュメント

- RiichiLab Documentation: https://riichi.dev/docs
