# bot-scenario

`bot-scenario` は、手牌・局面を入力して `ShantenAgent` の判断と根拠をオフラインで確認する CLI です。実対局や WebSocket 接続は行いません。出力の読み方は [Structured diagnostics](diagnostics.md)、判断仕様は [麻雀 AI の概要](ai/overview.md) を参照してください。

## 簡易 CLI

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
| `--round-wind` | 任意 | 場風。`E` / `S` / `W` / `N` |
| `--seat-wind` | 任意 | 自風。`E` / `S` / `W` / `N` |
| `--allow-hora` | 任意 | 和了を合法手に加える |
| `--allow-ryukyoku` | 任意 | 流局を合法手に加える |
| `--lookahead` | 任意 | 打牌候補ごとの2手先概要を追加。`--verbose` 併用時は受け入れ牌ごとの詳細も表示 |
| `--verbose` | 任意 | 通常打牌候補の詳細を追加 |

`player_id`、`oya`、`reached`、`discards` は簡易 CLI では指定できません。防御を含む局面は JSON scenario を使用します。牌効率指標の意味は [打牌選択](ai/discard-selection.md) を参照してください。

## 牌表記

MJAI 単牌表記と圧縮 MPSZ 表記の両方を受け付けます。空白区切りで混在させることもできます。

```text
234m 5pr 67p E
```

| 表記 | 内容 |
| --- | --- |
| `1m`..`9m` / `1p`..`9p` / `1s`..`9s` | 数牌 |
| `E` `S` `W` `N` `P` `F` `C` | 字牌 |
| `5mr` `5pr` `5sr` | 赤5 |
| `234m455p789s1234z` | 圧縮 MPSZ。`1z`=`E` .. `7z`=`C` |
| `0m` `0p` `0s` | MPSZ の赤5。`406m` は `4m 5mr 6m` |

曖昧な補正は行いません。`123`、`123x`、`8z`、`0z`、`5r` は error です。赤5は各色1枚なので、`00m` のような重複指定も error です。

## JSON scenario

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
  "history_furiten": {
    "same_turn": false,
    "riichi_missed_win": false
  },
  "extra_visible_tiles": "",
  "legal_dahai": null,
  "allow_hora": false,
  "allow_ryukyoku": false
}
```

`hand`、`draw`、`dora_indicators`、`round_wind`、`seat_wind`、`allow_*` は簡易 CLI の同名 option と同じ意味です。`hand` 以外は省略でき、河は空、`reached` は全員 `false`、`allow_*` は `false` になります。

### JSON field

| field | 内容 |
| --- | --- |
| `player_id` / `oya` | 自分の席 / 親の席。`0`..`3` |
| `reached` | 各 player のリーチ状態。要素数4 |
| `discards` | 各 player の河。入力順のまま扱う。要素数4 |
| `post_reach_passed` | 各 player のリーチ成立後に他家から切られて通った牌。要素数4 |
| `temporary_passed` | 各 player の最後の手牌変化後に他家から切られて通った牌。要素数4。省略時 unknown |
| `history_furiten` | `same_turn` / `riichi_missed_win`。各値は省略時 unknown |
| `melds` | 各 player の副露・暗槓。要素数4 |
| `extra_visible_tiles` | 他の field で表現していない見え牌 |
| `legal_dahai` | 打牌可能な牌と候補順 |
| `remaining_tiles` / `honba` / `kyotaku_points` / `scores` / `kyoku` | table state |

### legal_dahai

`legal_dahai` は打牌可能な牌とその順序を明示します。リーチ後のツモ切りだけの局面や候補順に依存する判断の再現に利用できます。省略時は手牌とツモ牌から自動生成します。手牌に無い牌、赤5と黒5が一致しない指定、意味が重複する指定は error です。

### melds

`melds` の各面子は次の field を持ちます。

| field | 内容 |
| --- | --- |
| `kind` | `chi` / `pon` / `daiminkan` / `ankan` / `kakan` |
| `tiles` | 面子を構成する物理牌 |
| `called_tile` | 鳴いた牌。`ankan` では指定しない |

副露牌は見え牌へ加わります。`extra_visible_tiles` は副露以外など、他の field で表現されない見え牌に使用します。`seat_wind` は `player_id` と `oya` があれば導出され、矛盾する明示値は error です。

### post_reach_passed

現物は対象リーチ者自身の河と、そのリーチ成立後に他家から切られて通った牌です。後者は河だけから逆算できないため `post_reach_passed` で指定します。牌種だけを保持し、見え牌や河には影響しません。赤5は黒5と同じ牌種です。

これはリーチ者専用の事実です。非リーチ副露相手の防御には使いません。詳しくは [防御におけるロン安全根拠](ai/defense.md#target-ごとのロン安全根拠) を参照してください。

```bash
cargo run -p bot-scenario -- crates/bot-scenario/scenarios/post_reach_genbutsu.json
```

### temporary_passed

`temporary_passed[player]` は、その player の最後のツモまたは鳴き・槓以降に、他家から切られてロンされず通った牌種です。赤5と黒5は同じ牌種として扱います。次のツモ、chi / pon / daiminkan / ankan / kakan で手牌が変化すると無効になります。

単一 observation からは復元できない履歴事実なので、JSON scenario では明示してください。field 省略は「安全牌なし」ではなく unknown です。リーチ者用の `post_reach_passed` とは寿命も意味も異なります。

例えば player 0 が 9m を切って通った直後に player 1 がツモると、打牌者自身には登録せず、player 1 の分はツモで消えるため、状態は `["", "", "9m", "9m"]` になります。

### history_furiten

`history_furiten.same_turn` は同巡内フリテン、`history_furiten.riichi_missed_win` はリーチ後のアガリ見逃しによる局中継続フリテンです。各値は `true` / `false` / 省略による unknown を区別します。

指定した値は**現在時点 (今回の打牌の前)** の facts です。ロン可否は恒常フリテンと合わせた総合値で、打牌後の評価時点へ補正してから判定します。`draw` を指定した局面は「自分のツモを経た打牌」になるため、`same_turn` が `true` でもその打牌後は解除されます。`draw` を省略した局面や鳴き後の局面では解除しません。unknown を `false` と推測しないので、軸を省略するとロン可否も unknown になります。規則は [フリテン](ai/furiten.md#総合ロン可否) を参照してください。

```bash
cargo run -p bot-scenario -- crates/bot-scenario/scenarios/history_furiten_same_turn.json
```

### table state

| field | 意味 | 単位 | validation |
| --- | --- | --- | --- |
| `remaining_tiles` | 山の残りツモ可能枚数 | 枚 | 0以上の整数 |
| `honba` | 本場 | 本 | 0以上の整数 |
| `kyotaku_points` | 供託。リーチ棒の本数ではなく点数 | 点 | 0以上の整数 |
| `scores` | player id 順の現在持ち点。負数も指定可能 | 点 | 要素数4 |
| `kyoku` | 場風内の局。東1 / 南1 が `1` | 局 | `1`..`4` |

すべて省略でき、省略時は `0` や25000点で補完せず unknown とします。明示した `0` は観測済みの0として区別します。

```json
{
  "hand": "234m455p789s1123z",
  "draw": "N",
  "remaining_tiles": 42,
  "honba": 1,
  "kyotaku_points": 0,
  "scores": [25000, 24000, 26000, 25000],
  "kyoku": 2
}
```

現在これらは AI の打牌、押し引き、リーチ、防御、鳴きには使用せず、`Table state` diagnostics で確認するための観測事実です。

## RiichiLab capture の再生

[`riichilab-client --capture-file`](riichilab.md#request_action-の-capture) で保存した `request_action` を1件再生できます。

```bash
cargo run -p bot-scenario -- \
  --riichilab-capture logs/ranked-capture.jsonl \
  --request-id 425
```

| 引数 | 必須 | 内容 |
| --- | --: | --- |
| `--riichilab-capture` | 必須 | capture JSONL の path |
| `--request-id` | 任意 | 再生する `request_id`。record が1件だけなら省略可能 |

複数 record の file で `--request-id` を省略すると、対象を推測せず error になります。`--hand` や JSON scenario とは併用できません。

`observation` decoder と `possible_actions` 変換は `riichilab-client` の実装を共有します。先頭に capture の出所を表示し、以降は JSON scenario と同じ [structured diagnostics](diagnostics.md) です。

単一 request の observation だけでは次を復元できません。

- event 列から積み上げる `post_reach_passed` は空
- 履歴依存フリテンは unknown

`scores`、`honba`、`kyotaku`、`kyoku` は observation から復元します。`remaining_tiles` は observation に field がありませんが、見えている牌 (全員の河・副露・自分の手牌) から復元します。RiichiLab live client と Chiihou における履歴依存フリテンの違いは [フリテン](ai/furiten.md#入力経路ごとの-known--unknown) を参照してください。

## fixture との使い分け

capture は実戦局面を見つけて調べる入口、JSON scenario は恒久的な回帰 fixture です。

1. `riichilab-client` で対局を capture する
2. log の `action sent` / `action_ack` から問題の `request_id` を特定する
3. `bot-scenario --riichilab-capture ... --request-id ...` で再生する
4. diagnostics から判断経路を確認する
5. 原因が分かったら局面を JSON scenario に落として回帰 fixture にする

既存 fixture は [`crates/bot-scenario/scenarios/`](../crates/bot-scenario/scenarios/) にあります。副露 threat の段階比較には `open_hand_*.json`、複合 threat には `combined_threat_defense.json` などを使用します。`open_hand_value_pon_and_chi.json` は現在、通常役牌1翻だけの2副露なので `Present` です。正確な境界条件は production tests を source of truth としてください。
