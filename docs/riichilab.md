# RiichiLab client

`riichilab-client` で RiichiLab に接続します。

```text
usage: riichilab-client [validate|ranked] [--agent normal|tsumogiri|shanten|menzen] [--log-file <PATH>] [--capture-file <PATH>]
```

## 接続モード

`validate` は validation 用 endpoint、`ranked` は ranked play 用 endpoint に接続します。argument を省略した場合は `validate` として扱います。ranked play には RiichiLab 上で active な bot が必要です。

```bash
cargo run -p riichilab-client --bin riichilab-client -- validate
cargo run -p riichilab-client --bin riichilab-client -- ranked
```

## Agent の指定

`--agent shanten` は向聴ベースの `ShantenAgent`、`--agent menzen` は同じ基本判断を使いながらチー・ポン・明槓など門前を崩す鳴きを行わない `MenzenAgent` を使用します。未指定時は `NormalAgent` です。

```text
--agent normal
--agent tsumogiri
--agent shanten
--agent menzen
```

validation の例:

```bash
RIICHILAB_BOT_TOKEN=... \
cargo run -p riichilab-client --bin riichilab-client -- \
  validate --agent shanten
```

ranked の例:

```bash
RIICHILAB_BOT_TOKEN=... \
cargo run -p riichilab-client --bin riichilab-client -- \
  ranked --agent shanten
```

`RIICHILAB_BOT_TOKEN` は secret として扱い、repository、文書、issue、PR、log に残さないでください。`ranked` は実戦 queue に入るため、validation と bot activation を確認してから実行してください。

## Logging

`--log-file <PATH>` を指定すると console 出力を維持したまま同じ log を file にも保存します。file 側は ANSI escape sequence を含みません。directory は自動生成しないため、file を開けない場合は起動時 error になります。

```bash
RUST_LOG=info,bot_core::agent_decision=debug,bot_core::discard_selection=debug \
cargo run -p riichilab-client --bin riichilab-client -- \
  ranked \
  --agent shanten \
  --log-file logs/ranked.log
```

`RUST_LOG` の filter は console と file で共通です。`agent decision` や `discard_selection` の診断 log も双方へ出力されます。

## request_action の capture

`--capture-file <PATH>` を指定すると、server から受信した `request_action` の raw JSON を保存します。保存した record は [`bot-scenario`](bot-scenario.md#riichilab-capture-の再生) で再生できます。

```bash
RIICHILAB_BOT_TOKEN=... \
cargo run -p riichilab-client --bin riichilab-client -- \
  ranked \
  --agent shanten \
  --log-file logs/ranked.log \
  --capture-file logs/ranked-capture.jsonl
```

validation でも同じ option を使用できます。

```bash
RIICHILAB_BOT_TOKEN=... \
cargo run -p riichilab-client --bin riichilab-client -- \
  validate \
  --agent shanten \
  --capture-file logs/validate-capture.jsonl
```

| 項目 | 内容 |
| --- | --- |
| 形式 | JSONL。`request_action` 1件が1行1 JSON object |
| 保存内容 | `request_id`、`possible_actions`、`observation`、`time` など受信した field を含む raw JSON |
| 保存対象 | `request_action` のみ。`start_game`、`action_ack`、`end_game` などは保存しない |
| 単位 | client 1起動 = 1対局 = capture file 1つ |
| file | `--log-file` とは別 file。大きな base64 observation を通常 log に混ぜない |
| 未指定時 | clone、JSON 変換、file I/O を含む capture 処理を行わない |

`GameContext` から逆生成せず受信 JSON をそのまま書くため、decode と再 serialize による情報欠落を避けられます。

### session semantics

capture file は client 1起動、つまり ranked 1対局または validation 1対局の単位です。起動時に既存 file を追記せず、新しい session として truncate します。`request_id` は対局内では一意でも対局を跨いだ一意性は保証されないため、1 file に複数対局を混ぜません。

複数対局を残す場合は対局ごとに別 path を指定します。

```bash
cargo run -p riichilab-client --bin riichilab-client -- \
  ranked --agent shanten \
  --capture-file "logs/ranked-capture-$(date +%Y%m%d-%H%M%S).jsonl"
```

1対局中の複数 record は順次追記されます。JSONL なので途中終了時も書き込み済み record を利用でき、`jq` などで絞り込めます。

```bash
jq -r 'select(.request_id == 425)' logs/ranked-capture.jsonl
```

書き込みは通常 log と同じ non-blocking writer を使い、request deadline より capture I/O を優先しません。buffer overflow 時は応答を遅らせる代わりに record を落とし、件数を warning log に出します。capture file を開けない場合は起動時 error です。

capture は実戦局面を発見・調査する入口です。原因を特定した後は必要な局面を `bot-scenario` の JSON scenario に落とし、恒久的な回帰 fixture としてください。詳しい使い分けは [bot-scenario の capture replay](bot-scenario.md#fixture-との使い分け) を参照してください。
