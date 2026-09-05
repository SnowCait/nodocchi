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

`RUST_LOG` を指定せずに起動した場合、console は従来どおり `info` だけを表示します。`--log-file <PATH>` を指定すると、console を静かに保ったまま file 側へ判断調査用 preset を自動適用します。

| 出力先 | `RUST_LOG` 未指定時の filter |
| --- | --- |
| console | 全体 `info` |
| log file | 全体 `info`、agent decision `debug`、push/pull `debug`、通常打牌候補比較 `trace`、防御候補比較 `trace` |

通常の実戦調査では、次のように `--log-file` を指定するだけで後から判断根拠を確認できます。

```bash
cargo run -p riichilab-client --bin riichilab-client -- \
  ranked \
  --agent shanten \
  --log-file logs/ranked.log
```

investigation preset の target は既存 instrumentation の `bot_core::agent_decision=debug`、`bot_core::push_pull=debug`、`bot_core::discard_selection=trace`、`bot_core::defense=trace` です。`bot_core` 全体の trace は有効にしません。

判断ログは production selection が既に計算した exact evidence を記録します。現物などで production decision が exact model を必要としなかった場合は、ログのためだけに追加評価せず、R/T fields を unknown のまま残します。

`RUST_LOG` を明示した場合は特殊調査用の override として扱い、console と file の両方へその filter をそのまま適用します。この場合、investigation preset は暗黙に追加しません。

```bash
RUST_LOG=bot_core::push_pull=trace \
cargo run -p riichilab-client --bin riichilab-client -- \
  ranked \
  --agent shanten \
  --log-file logs/ranked.log
```

file は既存どおり追記で、ANSI escape sequence を含みません。directory は自動生成しないため、file を開けない場合は起動時 error になります。

`--log-file logs/ranked.log` を指定した場合、同じ directory の `logs/ranked-slow.log` は slow request が初めて発生した時点で生成します。slow request が1件も発生しなかった起動では作成しません。この file には `slow request_action response` だけを記録し、investigation file filter や `RUST_LOG` には依存しません。正常終了までに slow request が1件以上あった場合は、件数と slow log の path を console へ WARN で1回通知します。slow log の生成は最初の slow request 時に行うため、開けなかった場合は起動時 error にはならず、console へ WARN を1回出力して slow log への記録だけを止めます。

### 送受信 action の照合

送信 action と server が適用した結果は、`RUST_LOG` を指定せず `--log-file` だけで照合できます。

| log | 内容 |
| --- | --- |
| `action sent` | `request_id`、`request_action_id`、`actor`、`action_type`、`tile`、`tsumogiri` に加え、`response` へ WebSocket へ送信した JSON payload そのもの |
| `meld applied` | server から受信した chi / pon / daiminkan の `actor`、`target`、`pai`、`consumed` |

`response` は送信直前の serialize 結果をそのまま記録するため、Chi / Pon / Daiminkan では `pai` と `consumed` を exact に確認できます。`MjaiAction` からの再構築ではありません。

```text
action sent request_id=Some(131) request_action_id=131 actor=Some(1) action_type="chi" tile=None tsumogiri=None response={"type":"chi","actor":1,"pai":"7p","consumed":["6p","8p"],"request_id":131}
meld applied actor=1 target=0 pai="7p" consumed=["5p", "6p"]
```

この2行を比較すると、送信した副露と server が実際に適用した副露が一致しているかを log だけで判定できます。

## Session capture

`--capture-file <PATH>` を指定すると、1対局の protocol payload を JSONL で保存します。server から受信したイベントと、nodocchi が WebSocket へ送信した action の双方向を、client が観測した時系列順に記録します。保存した record は [`bot-scenario`](bot-scenario.md#riichilab-capture-の再生) で再生できます。

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
| 形式 | JSONL。1行1 record の direction envelope |
| 単位 | client 1起動 = 1対局 = capture file 1つ |
| 保存対象 (server) | `MjaiEvent` が扱う受信イベントすべて。`start_game`、`start_kyoku`、`tsumo`、`dahai`、`chi`、`pon`、`daiminkan`、`ankan`、`kakan`、`reach`、`hora`、`ryukyoku`、`end_kyoku`、`request_action`、`action_ack`、`end_game`、`validation_result` |
| 保存対象 (client) | WebSocket へ送信した action payload |
| 保存内容 | 受信 / 送信した protocol payload そのもの |
| 順序 | client が観測した event / action の時系列順。`request_id` で並べ替えない |
| file | `--log-file` とは別 file。大きな base64 observation を通常 log に混ぜない |
| 未指定時 | envelope 構築、clone、file I/O を含む capture 処理を行わない |

### record envelope

各行は `version`、`direction`、`event` を持つ envelope です。`event` は受信 / 送信した payload そのもので、`GameContext` や diagnostic からの再構築ではありません。

```json
{"version":1,"direction":"server","event":{"type":"request_action","request_id":123,"possible_actions":[{"type":"dahai","pai":"1m","tsumogiri":false}],"observation":"..."}}
{"version":1,"direction":"client","event":{"type":"reach","actor":1,"request_id":123}}
{"version":1,"direction":"server","event":{"type":"action_ack","request_id":123,"status":"accepted"}}
{"version":1,"direction":"server","event":{"type":"reach","actor":1}}
{"version":1,"direction":"server","event":{"type":"dahai","actor":1,"pai":"1m","tsumogiri":true}}
```

`direction` は `server` (受信) と `client` (送信) を区別します。`dahai`、`reach`、`hora` などの `type` は双方に現れるため、`type` だけから向きを推測しません。

client record は通常 log の `action sent` の `response=` と同じ、送信直前に serialize した payload です。`MjaiAction` からの再構築ではないため、Chi / Pon の `pai` と `consumed`、`request_id` は送信した値と完全に一致します。

`version` は capture schema の version です。旧形式 (1行がそのまま `request_action` の raw JSON) との後方互換は意図的に持たず、envelope の無い行は読みません。

### session semantics

capture file は client 1起動、つまり ranked 1対局または validation 1対局の単位です。起動時に既存 file を追記せず、新しい session として truncate します。`request_id` は対局内では一意でも対局を跨いだ一意性は保証されないため、1 file に複数対局を混ぜません。

複数対局を残す場合は対局ごとに別 path を指定します。

```bash
cargo run -p riichilab-client --bin riichilab-client -- \
  ranked --agent shanten \
  --capture-file "logs/ranked-capture-$(date +%Y%m%d-%H%M%S).jsonl"
```

対局ごとに分けた capture file は、まとめて [production latency 計測](bot-scenario.md#riichilab-capture-の-production-latency-計測) の入力にできます。

1対局中の record は順次追記されます。JSONL なので途中終了時も書き込み済み record を利用でき、`jq` などで絞り込めます。

```bash
jq -c 'select(.direction == "server" and .event.type == "request_action" and .event.request_id == 425)' logs/ranked-capture.jsonl
jq -c 'select(.direction == "client")' logs/ranked-capture.jsonl
```

書き込みは通常 log と同じ non-blocking writer を使い、request deadline より capture I/O を優先しません。buffer overflow 時は応答を遅らせる代わりに record を落とし、件数を warning log に出します。capture file を開けない場合は起動時 error です。

### 局の時系列を追う

同じ file に request と応答と結果が並ぶため、1局を次の時系列として追えます。

```text
start_kyoku
  → server request_action
  → client action
  → server action_ack
  → server reach / dahai / 副露
  → server hora / ryukyoku
end_kyoku
```

capture record には `ShantenDiagnostic`、`RonOpportunityDiagnostic`、`ReachDamatenComparisonDiagnostic` などの bot-core diagnostic を埋め込みません。これらは同じ `request_action` の observation から replay / offline analyzer 側で再計算します。capture は protocol の観測記録に限定し、agent の diagnostic とは密結合させません。

この双方向 capture により、[Ron opportunity](ai/discard-selection.md#ron-opportunity-structural-facts-only) の structural facts と、実戦でその後に発生した opponent の `dahai` / `hora` / `ryukyoku` を offline で対応付けられます。Ron probability の推定や dataset 化は今後の課題で、現時点では実装していません。

capture は実戦局面を発見・調査する入口です。原因を特定した後は必要な局面を `bot-scenario` の JSON scenario に落とし、恒久的な回帰 fixture としてください。詳しい使い分けは [bot-scenario の capture replay](bot-scenario.md#fixture-との使い分け) を参照してください。
