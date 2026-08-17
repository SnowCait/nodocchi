# 防御

防御は threat の種類に応じて3つの経路を持ちます。共通の字牌・壁・スジ helper を共有し、target の決め方とロン安全の根拠を分けます。Push/Pull が `Fold` の場合に防御 fallback を通常打牌より優先します。

## Riichi Defense

他家リーチ者に対する既存の `Defense` です。

- Genbutsu
- HonorSafety
- wall / one-chance
- Suji / HalfSuji

### Genbutsu

リーチ者本人の河と、リーチ成立後に他家から切られて通った `post_reach_passed` の両方を現物とします。複数リーチでは全 target に対する安全性を集約します。

### HonorSafety

字牌は見え枚数で安全度を分類します。同じ安全 rank 内では相手にとっての役牌価値を使い、`GuestWind` → `SingleValueHonor` → `DoubleWind` の切りやすい順で比較します。不明な場風・自風を推測しません。

### Suji / HalfSuji

相手の河に基づいて数牌のスジ安全度を評価します。端寄りの片側だけが通る場合を `HalfSuji`、両側の根拠が揃う場合を `Suji` として区別します。

### wall / one-chance

見え牌から順子待ち経路を評価し、`NoChance` / `OneChance` などの wall rank を作ります。wall は target に依存しません。数牌では wall とスジを共有 helper で統合します。

## OpenHand Defense

`open hand threat: High` の非リーチ副露相手だけを target にします。classification は [OpenHandThreat](push-pull.md#openhandthreat) を共有し、Defense 側で High 条件を再実装しません。`Present` / `None`、自分、リーチ済み、player id 不明の席は target 外です。

候補の大分類は次の順です。

1. `SafeAgainstAllTargets`
2. `HonorSafety`
3. `SuitedSafety`

第一分類 `SafeAgainstAllTargets` は、本人の河または現在有効な一時通過牌によって全 target にロンされない牌です。「全 target 自身の河にある」という意味ではありません。字牌・役牌価値・壁・スジは Riichi Defense と同じ helper を共有します。複数 target の集約では、まだその牌でロン可能な相手のうち最も危険な評価を採ります。

数牌は `NoChance` → `OneChance` → `Suji` → `HalfSuji` の順で fallback を探し、`NoSafety` だけなら選びません。選べる防御候補がない場合は通常打牌へ戻ります。

## Combined Defense

リーチ者と High OpenHandThreat が同時に存在する複合 threat で使います。target には種類 `Riichi` / `HighOpenHand` を保持し、全 target にロン安全なら `SafeAgainstAllThreats` とします。

候補の大分類は次の順です。

1. `SafeAgainstAllThreats`
2. `HonorSafety`
3. `SuitedSafety`

ロン安全な target はその牌をロンできないため、その相手の無スジや役牌価値を集約から除きます。wall は見え牌由来なので全 target で共有します。

## target ごとのロン安全根拠

ここは3経路で混同しない重要な差です。

| target | ロン安全の根拠 |
| --- | --- |
| `Riichi` | 本人の河 + `post_reach_passed` |
| `HighOpenHand` | 本人の河 + 現在有効な `temporary_passed` |

`post_reach_passed` は「リーチ成立後に通った」というリーチ固有の事実で、リーチ者の手牌が変化しないため局中継続します。`temporary_passed` は非リーチを含む各 player について「最後の手牌変化後に通った」事実で、対象 player の次のツモ、chi / pon / daiminkan / ankan / kakan で消えます。両者は寿命が異なる別 state で、前者を非リーチ副露相手へ流用しません。

入力方法は [bot-scenario の post_reach_passed](../bot-scenario.md#post_reach_passed) と [temporary_passed](../bot-scenario.md#temporary_passed)、出力の読み方は [Structured diagnostics](../diagnostics.md#combined-defense) を参照してください。

## fallback と source of truth

selection は production selector が source of truth です。diagnostics は同じ selector の結果を `selected` として表示し、`act()` と `diagnose()` で別の防御ロジックを持ちません。`Push` では通常打牌の優先順を変えず、`Fold` のときだけ該当 threat 用 fallback を先に試します。
