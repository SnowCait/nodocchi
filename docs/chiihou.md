# 地鳳 client

`chiihou-client` は起動時に地鳳 server の最新卓状態を取得して `gamestart` / `join` を自動送信し、その後 Nostr relay 経由で request を受信して指定した Agent で返信します。

## 起動方法

```bash
CHIIHOU_NSEC=nsec1... \
cargo run -p chiihou-client --bin chiihou-client -- \
  --channel hanchan \
  --agent shanten
```

```text
usage: chiihou-client --channel <hanchan|tonpuu> [--agent normal|tsumogiri|shanten|menzen] [--server-npub <NPUB_OR_NPROFILE>] [--auto-next] [--response-delay-ms <MILLISECONDS>]
```

| 引数 | 必須 | 内容 |
| --- | --: | --- |
| `--channel` | 必須 | `hanchan` または `tonpuu` |
| `--agent` | 任意 | `normal`、`tsumogiri`、`shanten`、`menzen`。既定値は `normal` |
| `--server-npub` | 任意 | server の NIP-19 `npub` または `nprofile`。省略時は既定 server |
| `--auto-next` | 任意 | 局終了ごとに `next` を1回送信する |
| `--response-delay-ms` | 任意 | GET reply と自動 next の publish 前に入れる遅延。ミリ秒、既定値 `0` |

既定 server:

```text
npub1j0ng5hmm7mf47r939zqkpepwekenj6uqhd5x555pn80utevvavjsfgqem2
```

server を上書きする例:

```bash
CHIIHOU_NSEC=nsec1... \
cargo run -p chiihou-client --bin chiihou-client -- \
  --channel hanchan \
  --agent shanten \
  --server-npub npub1...
```

自動 next と応答遅延を有効化する例:

```bash
CHIIHOU_NSEC=nsec1... \
RUST_LOG=info \
cargo run -p chiihou-client --bin chiihou-client -- \
  --channel hanchan \
  --agent shanten \
  --auto-next \
  --response-delay-ms 5000
```

## 自動参加動作

- 卓が存在しなければ `gamestart`
- 募集中なら `join`
- 対局中または next 待ちなら何も送信しない
- `gamestart` の送信者は1人目として登録されるため追加の `join` は送らない
- command 送信後も同じ process で request を待ち受ける

## 環境変数

| 環境変数 | 必須 | 内容 |
| --- | --: | --- |
| `CHIIHOU_NSEC` | 必須 | AI の NIP-19 nsec |
| `RUST_LOG` | 任意 | logging filter。既定値は `info` |

## 注意事項

- `CHIIHOU_NSEC` は secret として扱い、repository、文書、issue、PR、log に実値を残さないでください。
- shell history へ nsec を直接書く運用にも注意してください。
- `CHIIHOU_NSEC` は hex 秘密鍵を受け付けません。
- `--server-npub` は hex 公開鍵を受け付けません。event と filter 内部では hex へ正規化されます。
- 卓状態の取得に失敗した場合は推測で command を送信しません。
- `--server-npub` は既定 server を上書きする高度な用途向けです。
- command の受理確認や競合時の retry は未実装です。
- 履歴依存フリテンの known / unknown は [フリテン](ai/furiten.md#入力経路ごとの-known--unknown) を参照してください。
