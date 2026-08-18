# のどっち

リーチ麻雀 AI。

RiichiLab と地鳳へ接続する client、向聴数を基に打牌・リーチ・押し引き・防御を判断する `ShantenAgent`、局面をオフラインで再現して判断根拠を確認する `bot-scenario` を提供します。

## 主な機能

- [RiichiLab 接続](docs/riichilab.md)
- [地鳳接続](docs/chiihou.md)
- [`ShantenAgent` を中心とした麻雀 AI](docs/ai/overview.md)
- [`bot-scenario` による局面解析](docs/bot-scenario.md)
- [RiichiLab の request capture / replay](docs/bot-scenario.md#riichilab-capture-の再生)
- [Structured diagnostics](docs/diagnostics.md)

## Quick Start

局面を直接指定して `ShantenAgent` の判断を確認します。

```bash
cargo run -p bot-scenario -- \
  --hand "234m455p789s1123z" \
  --draw "N"
```

RiichiLab の validation endpoint へ接続する例です。token は secret として扱い、repository や log に残さないでください。

```bash
RIICHILAB_BOT_TOKEN=... \
cargo run -p riichilab-client --bin riichilab-client -- \
  validate --agent shanten
```

地鳳へ接続する例です。`CHIIHOU_NSEC` も secret として扱ってください。

```bash
CHIIHOU_NSEC=nsec1... \
cargo run -p chiihou-client --bin chiihou-client -- \
  --channel hanchan --agent shanten
```

## Documentation

- [RiichiLab client](docs/riichilab.md)
- [地鳳 client](docs/chiihou.md)
- [bot-scenario](docs/bot-scenario.md)
- [Structured diagnostics](docs/diagnostics.md)
- [麻雀 AI の概要](docs/ai/overview.md)
- [打牌選択](docs/ai/discard-selection.md)
- [押し引きと threat](docs/ai/push-pull.md)
- [防御](docs/ai/defense.md)
- [フリテン](docs/ai/furiten.md)
- [手牌評価](docs/ai/hand-value.md)

## Development

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## 公式ドキュメント

- [RiichiLab Documentation](https://riichi.dev/docs)
