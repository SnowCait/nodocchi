# のどっち

リーチ麻雀AI

## RiichiLab 接続

`riichilab-client` で RiichiLab に接続します。

### Agent の指定

`MAHJONG_AGENT=shanten` を指定すると、向聴ベースの `ShantenAgent` を使用します。未指定の場合は `NormalAgent` を使用します。

```bash
MAHJONG_AGENT=shanten
```

### Endpoint の指定

validation では `/ws/validate`、ranked では `/ws/ranked` を指定します。未指定の場合は `/ws/validate` に接続します。ranked play には RiichiLab 上で active な bot が必要です。

```bash
RIICHILAB_ENDPOINT=wss://game.riichi.dev/ws/validate
RIICHILAB_ENDPOINT=wss://game.riichi.dev/ws/ranked
```

### 実行例

validation の例:

```bash
RIICHILAB_BOT_TOKEN=...
MAHJONG_AGENT=shanten
RIICHILAB_ENDPOINT=wss://game.riichi.dev/ws/validate
cargo run -p riichilab-client --bin validate
```

ranked の例:

```bash
RIICHILAB_BOT_TOKEN=...
MAHJONG_AGENT=shanten
RIICHILAB_ENDPOINT=wss://game.riichi.dev/ws/ranked
cargo run -p riichilab-client --bin validate
```

`RIICHILAB_BOT_TOKEN` は secret として扱い、repository・README・log に残さないでください。
ranked endpoint は実戦 queue に入るため、validation と bot activation を確認してから実行してください。

## 公式ドキュメント

- RiichiLab Documentation: https://riichi.dev/docs
