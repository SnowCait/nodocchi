//! 九種九牌 (Nine terminals draw) を宣言するか続行するかの policy 層。
//!
//! この層が持つのは「`LegalAction::Ryukyoku` が合法な局面で宣言するか続行するか」だけで、
//! 九種九牌の成立条件そのものは判定しない。
//!
//! ```text
//! server が LegalAction::Ryukyoku を提示
//! → 九種九牌は合法とみなす
//! → nodocchi は宣言するか続行するかだけ判断
//! ```
//!
//! # source of truth
//!
//! | 材料 | source of truth |
//! | --- | --- |
//! | 九種九牌の合法性 | 入力側 (server / scenario) が渡す [`LegalAction::Ryukyoku`](crate::action::LegalAction::Ryukyoku) |
//! | 通常手・七対子・国士の向聴数 | [`calculate_shanten`] |
//!
//! 么九牌の種類数を数え直すことも、国士専用の向聴計算を持つこともしない。
//!
//! # 続行条件
//!
//! ```text
//! standard shanten <= 2
//! OR chiitoitsu shanten <= 2
//! OR kokushi shanten <= 3
//! ```
//!
//! いずれも満たさない場合は宣言する。3条件は同格で、複数同時に成立しても優先順位は付けない。
//! 点棒状況・親子・受け入れ枚数は判断材料にしない。
//!
//! # 手牌を評価できない場合
//!
//! 現在の自摸後 concealed hand を組み立てられない context では、向聴数を推測して続行せず
//! 従来どおり宣言する。向聴数は `unknown` ([`RyukyokuDecisionDiagnostic::shanten`] が `None`)
//! のまま保持し、0や適当な値で埋めない。

use bot_logic::{Shanten, TileCounts, calculate_shanten};

use crate::context::GameContext;

/// 九種九牌を宣言せず続行する通常手の向聴数。inclusive。
pub const RYUKYOKU_CONTINUE_STANDARD_SHANTEN: i8 = 2;

/// 九種九牌を宣言せず続行する七対子の向聴数。inclusive。
pub const RYUKYOKU_CONTINUE_CHIITOITSU_SHANTEN: i8 = 2;

/// 九種九牌を宣言せず続行する国士無双の向聴数。inclusive。
///
/// 九種九牌が成立する手牌はそのまま国士の材料になるため、通常手・七対子より1向聴だけ広く
/// 取る。これは意図した policy で、係数から導いた値ではない。
pub const RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN: i8 = 3;

/// 判断対象になる自摸後 concealed hand の枚数 [枚]。
///
/// 九種九牌は副露を挟まない自分のツモ番でしか宣言できないので、`hand_tiles` 13枚 +
/// `drawn_tile` 1枚以外の局面は評価対象にしない。
const RYUKYOKU_HAND_TILE_COUNT: usize = 14;

/// 九種九牌を宣言するか続行するかの結論。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RyukyokuVerdict {
    /// 九種九牌を宣言する。
    Declare,
    /// 九種九牌を宣言せず、既存の打牌判断へ進む。
    Continue,
}

/// 九種九牌判断の構造化診断。
///
/// 契約:
///
/// - [`LegalAction::Ryukyoku`](crate::action::LegalAction::Ryukyoku) が合法だった局面でだけ
///   作られる。合法性はこの層で再判定しない。
/// - `verdict` は `ShantenAgent::act()` が実際に通った経路そのもので、診断用の別判断ロジック
///   は持たない。
/// - 3種類の向聴数は同じ [`calculate_shanten`] の結果で、判断に使った値そのもの。
/// - 手牌を評価できなかった場合は `shanten` が `None` になり、向聴数を推測して埋めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RyukyokuDecisionDiagnostic {
    /// 現在の自摸後 concealed hand の向聴数。手牌を組み立てられない場合は `None`。
    pub shanten: Option<Shanten>,
    pub verdict: RyukyokuVerdict,
}

impl RyukyokuDecisionDiagnostic {
    /// 通常手の向聴数。手牌を評価できなかった場合は `None`。
    pub fn standard_shanten(&self) -> Option<i8> {
        self.shanten.map(|shanten| shanten.standard)
    }

    /// 七対子の向聴数。手牌を評価できなかった場合は `None`。
    pub fn chiitoitsu_shanten(&self) -> Option<i8> {
        self.shanten.map(|shanten| shanten.chiitoitsu)
    }

    /// 国士無双の向聴数。手牌を評価できなかった場合は `None`。
    pub fn kokushi_shanten(&self) -> Option<i8> {
        self.shanten.map(|shanten| shanten.kokushi)
    }

    /// 九種九牌を宣言すると判断したか。
    pub fn should_declare(&self) -> bool {
        self.verdict == RyukyokuVerdict::Declare
    }
}

/// 九種九牌が合法な局面で、宣言するか続行するかを決める。
///
/// 判断は現在の自摸後 concealed hand の向聴数だけで行う。条件は
/// [`continues_with_shanten`] が source of truth。
pub fn evaluate_ryukyoku_decision(ctx: &GameContext) -> RyukyokuDecisionDiagnostic {
    let shanten = current_hand_counts(ctx).map(|counts| calculate_shanten(&counts));
    let verdict = match shanten {
        Some(shanten) if continues_with_shanten(shanten) => RyukyokuVerdict::Continue,
        _ => RyukyokuVerdict::Declare,
    };

    RyukyokuDecisionDiagnostic { shanten, verdict }
}

/// 向聴数から続行できるかを判定する pure helper。
///
/// 3条件は同格の OR で、どれか1つでも満たせば続行する。
pub fn continues_with_shanten(shanten: Shanten) -> bool {
    shanten.standard <= RYUKYOKU_CONTINUE_STANDARD_SHANTEN
        || shanten.chiitoitsu <= RYUKYOKU_CONTINUE_CHIITOITSU_SHANTEN
        || shanten.kokushi <= RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN
}

// 現在の自摸後 concealed hand を `hand_tiles` + `drawn_tile` から組み立てる。
//
// `drawn_tile` は `hand_tiles` に含まれない契約なので、ここで二重に加えないよう1枚だけ足す。
// 自摸牌が無い context、14枚にならない context、同じ牌種が5枚以上ある context は正しい手牌を
// 復元できないので `None` を返し、別の手牌で代用しない。
fn current_hand_counts(ctx: &GameContext) -> Option<TileCounts> {
    let drawn_tile = ctx.drawn_tile()?;
    let hand_tiles = ctx.hand_tiles();
    if hand_tiles.len() + 1 != RYUKYOKU_HAND_TILE_COUNT {
        return None;
    }

    let mut counts = TileCounts::new();
    for tile in hand_tiles.iter().copied().chain([drawn_tile]) {
        counts.try_add(tile.tile_type()).ok()?;
    }
    Some(counts)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use bot_logic::{TileId, TileType, chiitoitsu_shanten, kokushi_shanten, standard_shanten};

    /// 九種九牌が成立する代表形。么九牌10種・対子なしで国士3向聴。
    pub(crate) const KOKUSHI_THREE_HAND: [&str; 14] = [
        "E", "S", "W", "N", "P", "F", "C", "1m", "1p", "1s", "5m", "5p", "5s", "8m",
    ];

    /// 九種九牌が成立する代表形。么九牌9種・対子なしで国士4向聴、通常手8向聴、七対子6向聴。
    pub(crate) const KOKUSHI_FOUR_HAND: [&str; 14] = [
        "E", "S", "W", "N", "C", "1m", "4m", "7m", "1p", "4p", "7p", "1s", "4s", "9s",
    ];

    /// 通常手2向聴。七対子6向聴・国士8向聴で、続行するのは通常手の条件だけによる。
    pub(crate) const STANDARD_TWO_HAND: [&str; 14] = [
        "1m", "2m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "2p", "5s", "E", "S", "W",
    ];

    /// 通常手3向聴。七対子6向聴・国士6向聴で、どの条件も満たさない。
    pub(crate) const STANDARD_THREE_HAND: [&str; 14] = [
        "1m", "2m", "3m", "4p", "5p", "6p", "7s", "8s", "E", "S", "W", "N", "P", "F",
    ];

    /// 七対子2向聴。通常手3向聴・国士9向聴で、続行するのは七対子の条件だけによる。
    pub(crate) const CHIITOITSU_TWO_HAND: [&str; 14] = [
        "3m", "3m", "6p", "6p", "3s", "3s", "8s", "8s", "1m", "4m", "9p", "2s", "E", "C",
    ];

    /// 七対子3向聴。通常手4向聴・国士9向聴で、どの条件も満たさない。
    pub(crate) const CHIITOITSU_THREE_HAND: [&str; 14] = [
        "3m", "3m", "6p", "6p", "3s", "3s", "1m", "4m", "9p", "2s", "E", "C", "7m", "2p",
    ];

    /// 牌種文字列から物理牌を作る。同じ牌種は別の物理牌を割り当てる。
    pub(crate) fn hand_tiles(hand: &[&str]) -> Vec<TileId> {
        let mut used = [0usize; TileType::COUNT];
        hand.iter()
            .map(|tile| {
                let tile_type = TileType::from_mjai_type_str(tile).expect("牌種を解釈できる");
                let copy = &mut used[tile_type.index()];
                let tile = TileId::copies(tile_type)
                    .nth(*copy)
                    .expect("同じ牌種は4枚まで");
                *copy += 1;
                tile
            })
            .collect()
    }

    /// 自摸後14枚のうち最後の1枚を drawn_tile として渡す。手牌側に自摸牌を含めない。
    pub(crate) fn context_from_hand(hand: &[&str]) -> GameContext {
        let mut tiles = hand_tiles(hand);
        let drawn_tile = tiles.pop().expect("手牌が空でない");
        GameContext::from_parts(Some(drawn_tile), tiles)
    }

    fn decision(hand: &[&str]) -> RyukyokuDecisionDiagnostic {
        evaluate_ryukyoku_decision(&context_from_hand(hand))
    }

    // 既存の向聴計算をそのまま呼び、test 側で向聴数を組み立て直さない。
    fn shanten_of(hand: &[&str]) -> Shanten {
        let counts = TileCounts::from_tiles(hand_tiles(hand));
        Shanten {
            standard: standard_shanten(&counts),
            chiitoitsu: chiitoitsu_shanten(&counts),
            kokushi: kokushi_shanten(&counts),
        }
    }

    #[test]
    fn the_diagnostic_reports_the_existing_shanten_of_the_current_hand() {
        let decision = decision(&KOKUSHI_THREE_HAND);
        let shanten = shanten_of(&KOKUSHI_THREE_HAND);

        assert_eq!(decision.shanten, Some(shanten));
        assert_eq!(decision.standard_shanten(), Some(shanten.standard));
        assert_eq!(decision.chiitoitsu_shanten(), Some(shanten.chiitoitsu));
        assert_eq!(decision.kokushi_shanten(), Some(shanten.kokushi));
    }

    #[test]
    fn continues_on_a_kokushi_three_shanten_hand() {
        let decision = decision(&KOKUSHI_THREE_HAND);

        assert_eq!(
            decision.kokushi_shanten(),
            Some(RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN)
        );
        assert_eq!(decision.verdict, RyukyokuVerdict::Continue);
    }

    #[test]
    fn declares_on_a_kokushi_four_shanten_hand() {
        let decision = decision(&KOKUSHI_FOUR_HAND);
        let shanten = shanten_of(&KOKUSHI_FOUR_HAND);

        assert_eq!(shanten.kokushi, RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN + 1);
        assert!(shanten.standard > RYUKYOKU_CONTINUE_STANDARD_SHANTEN);
        assert!(shanten.chiitoitsu > RYUKYOKU_CONTINUE_CHIITOITSU_SHANTEN);
        assert_eq!(decision.verdict, RyukyokuVerdict::Declare);
    }

    #[test]
    fn continues_on_a_standard_two_shanten_hand() {
        let decision = decision(&STANDARD_TWO_HAND);
        let shanten = shanten_of(&STANDARD_TWO_HAND);

        assert_eq!(shanten.standard, RYUKYOKU_CONTINUE_STANDARD_SHANTEN);
        assert!(shanten.chiitoitsu > RYUKYOKU_CONTINUE_CHIITOITSU_SHANTEN);
        assert!(shanten.kokushi > RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN);
        assert_eq!(decision.verdict, RyukyokuVerdict::Continue);
    }

    #[test]
    fn declares_on_a_standard_three_shanten_hand() {
        let decision = decision(&STANDARD_THREE_HAND);
        let shanten = shanten_of(&STANDARD_THREE_HAND);

        assert_eq!(shanten.standard, RYUKYOKU_CONTINUE_STANDARD_SHANTEN + 1);
        assert!(shanten.chiitoitsu > RYUKYOKU_CONTINUE_CHIITOITSU_SHANTEN);
        assert!(shanten.kokushi > RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN);
        assert_eq!(decision.verdict, RyukyokuVerdict::Declare);
    }

    #[test]
    fn continues_on_a_chiitoitsu_two_shanten_hand() {
        let decision = decision(&CHIITOITSU_TWO_HAND);
        let shanten = shanten_of(&CHIITOITSU_TWO_HAND);

        assert_eq!(shanten.chiitoitsu, RYUKYOKU_CONTINUE_CHIITOITSU_SHANTEN);
        assert!(shanten.standard > RYUKYOKU_CONTINUE_STANDARD_SHANTEN);
        assert!(shanten.kokushi > RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN);
        assert_eq!(decision.verdict, RyukyokuVerdict::Continue);
    }

    #[test]
    fn declares_on_a_chiitoitsu_three_shanten_hand() {
        let decision = decision(&CHIITOITSU_THREE_HAND);
        let shanten = shanten_of(&CHIITOITSU_THREE_HAND);

        assert_eq!(shanten.chiitoitsu, RYUKYOKU_CONTINUE_CHIITOITSU_SHANTEN + 1);
        assert!(shanten.standard > RYUKYOKU_CONTINUE_STANDARD_SHANTEN);
        assert!(shanten.kokushi > RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN);
        assert_eq!(decision.verdict, RyukyokuVerdict::Declare);
    }

    #[test]
    fn any_single_condition_is_enough_to_continue() {
        for shanten in [
            Shanten {
                standard: RYUKYOKU_CONTINUE_STANDARD_SHANTEN,
                chiitoitsu: 6,
                kokushi: 9,
            },
            Shanten {
                standard: 8,
                chiitoitsu: RYUKYOKU_CONTINUE_CHIITOITSU_SHANTEN,
                kokushi: 9,
            },
            Shanten {
                standard: 8,
                chiitoitsu: 6,
                kokushi: RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN,
            },
        ] {
            assert!(continues_with_shanten(shanten), "{shanten:?}");
        }

        assert!(!continues_with_shanten(Shanten {
            standard: RYUKYOKU_CONTINUE_STANDARD_SHANTEN + 1,
            chiitoitsu: RYUKYOKU_CONTINUE_CHIITOITSU_SHANTEN + 1,
            kokushi: RYUKYOKU_CONTINUE_KOKUSHI_SHANTEN + 1,
        }));
    }

    #[test]
    fn declares_without_a_drawn_tile() {
        let mut tiles = hand_tiles(&KOKUSHI_THREE_HAND);
        tiles.pop();
        let ctx = GameContext::from_parts(None, tiles);
        let decision = evaluate_ryukyoku_decision(&ctx);

        assert_eq!(decision.shanten, None);
        assert_eq!(decision.standard_shanten(), None);
        assert_eq!(decision.chiitoitsu_shanten(), None);
        assert_eq!(decision.kokushi_shanten(), None);
        assert_eq!(decision.verdict, RyukyokuVerdict::Declare);
    }

    #[test]
    fn declares_when_the_hand_is_not_fourteen_tiles_after_the_draw() {
        for hand_size in [0, 12, 14] {
            let tiles = hand_tiles(&KOKUSHI_THREE_HAND)[..hand_size].to_vec();
            let drawn_tile = hand_tiles(&["1m"])[0];
            let ctx = GameContext::from_parts(Some(drawn_tile), tiles);
            let decision = evaluate_ryukyoku_decision(&ctx);

            assert_eq!(decision.shanten, None, "hand size {hand_size}");
            assert_eq!(
                decision.verdict,
                RyukyokuVerdict::Declare,
                "hand size {hand_size}"
            );
        }
    }

    #[test]
    fn declares_when_a_tile_type_appears_five_times() {
        let mut tiles = hand_tiles(&["1m", "1m", "1m", "1m"]);
        tiles.push(tiles[0]);
        tiles.extend(hand_tiles(&[
            "2m", "3m", "4m", "6m", "7m", "8m", "2p", "3p",
        ]));
        let drawn_tile = hand_tiles(&["4p"])[0];
        assert_eq!(tiles.len() + 1, RYUKYOKU_HAND_TILE_COUNT);

        let decision =
            evaluate_ryukyoku_decision(&GameContext::from_parts(Some(drawn_tile), tiles));

        assert_eq!(decision.shanten, None);
        assert_eq!(decision.verdict, RyukyokuVerdict::Declare);
    }

    #[test]
    fn the_drawn_tile_is_counted_once() {
        let ctx = context_from_hand(&KOKUSHI_THREE_HAND);
        let counts = current_hand_counts(&ctx).expect("手牌を組み立てられる");
        let total: u8 = TileType::all().map(|tile| counts.count(tile)).sum();

        assert_eq!(usize::from(total), RYUKYOKU_HAND_TILE_COUNT);
        assert_eq!(
            counts,
            TileCounts::from_tiles(hand_tiles(&KOKUSHI_THREE_HAND))
        );
    }
}
