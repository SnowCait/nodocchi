use crate::action::{LegalAction, prefer_black_five_for_action};
use crate::context::GameContext;
use bot_logic::TileType;

use super::hard_safety::ron_capable_reached_players;
use super::honor::{HonorSafetyRank, OpponentHonorValue};
use super::suji::{SujiSafetyRank, suji_safety_rank_for_any_reached, suji_safety_rank_for_players};
use super::wall::{WallRank, wall_rank};

// 数牌の防御 fallback 用の安全度。壁 / スジを統合して分類する。
// 安全度は NoChance > OneChance > Suji > HalfSuji > NoSafety。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SuitedSafetyRank {
    NoSafety,
    HalfSuji,
    Suji,
    OneChance,
    NoChance,
}

// スジ安全度を数牌防御の安全度へ写す。壁が無い場合の分類にだけ使う。
fn suited_safety_rank_from_suji(rank: SujiSafetyRank) -> SuitedSafetyRank {
    match rank {
        SujiSafetyRank::Suji => SuitedSafetyRank::Suji,
        SujiSafetyRank::HalfSuji => SuitedSafetyRank::HalfSuji,
        SujiSafetyRank::NoSuji => SuitedSafetyRank::NoSafety,
    }
}

// いずれかのリーチ者の河に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で None。
pub fn suited_safety_rank_for_any_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    let rank = match wall_rank(tile, context) {
        WallRank::NoChance => SuitedSafetyRank::NoChance,
        WallRank::OneChance => SuitedSafetyRank::OneChance,
        WallRank::NoWall => suji_safety_rank_for_any_reached(tile, context)
            .map_or(SuitedSafetyRank::NoSafety, suited_safety_rank_from_suji),
    };
    Some(rank)
}

/// 指定 player 集合の河に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で `None`。
///
/// 壁評価はスジ評価より優先する。壁は見え牌由来で対象 player に依らないため [`wall_rank`] を
/// そのまま使い、スジ評価は [`suji_safety_rank_for_players`] の最小値(最も危険な評価)を使う。
/// 集合が空なら壁が無い限り `NoSafety`。
pub fn suited_safety_rank_for_players(
    tile: TileType,
    players: &[usize],
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    let rank = match wall_rank(tile, context) {
        WallRank::NoChance => SuitedSafetyRank::NoChance,
        WallRank::OneChance => SuitedSafetyRank::OneChance,
        WallRank::NoWall => suji_safety_rank_for_players(tile, players, context)
            .map_or(SuitedSafetyRank::NoSafety, suited_safety_rank_from_suji),
    };
    Some(rank)
}

// 現物ではない全リーチ者に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で None。
// 壁評価はスジ評価より優先する。スジ評価は現物ではない全リーチ者に対する rank の最小値を使う。
pub fn suited_safety_rank_for_all_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    suited_safety_rank_for_players(tile, &ron_capable_reached_players(tile, context), context)
}

/// 合法 Dahai のうち数牌のみを安全度の高い順
/// (`NoChance` → `OneChance` → `Suji` → `HalfSuji` → `NoSafety`) に並べる共有実装。
///
/// 数牌の安全度の求め方だけを呼び出し側から差し替えられるようにしてある。同安全度は元の順序を
/// 保つ。並べ替えはここが唯一の実装で、対象 player 集合ごとにコピーしない。
///
/// リーチ者向けの入口は [`suited_dahai_actions_by_safety`]。非リーチ副露相手向けの入口は
/// [`open_hand_suited_dahai_actions_by_safety`](crate::open_hand_defense::open_hand_suited_dahai_actions_by_safety)。
pub fn suited_dahai_actions_by_safety_with<'a>(
    legal_actions: &'a [LegalAction],
    suited_safety_rank: impl Fn(TileType) -> Option<SuitedSafetyRank>,
) -> Vec<(&'a LegalAction, SuitedSafetyRank)> {
    let mut ranked: Vec<(&'a LegalAction, SuitedSafetyRank)> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => {
                suited_safety_rank(tile.tile_type()).map(|rank| (action, rank))
            }
            _ => None,
        })
        .collect();
    ranked.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));
    ranked
}

// 合法 Dahai のうち数牌のみを安全度の高い順
// (NoChance → OneChance → Suji → HalfSuji → NoSafety)に並べる。
// 同安全度は元の順序を保つ。スジ判定は現物ではない全リーチ者基準。
pub fn suited_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<(&'a LegalAction, SuitedSafetyRank)> {
    suited_dahai_actions_by_safety_with(legal_actions, |tile| {
        suited_safety_rank_for_all_reached(tile, context)
    })
}

// 他家リーチ中に、最も安全度の高い数牌 Dahai を fallback として選ぶ。
// 他家リーチがない、または NoSafety しか候補がなければ None。NoSafety は選ばない。
//
// 安全度順と安定順序で牌種を決めた後、その同一牌種内では prefer_black_five_for_action で
// 黒5を優先する。安全度 rank や合法 action 間の牌種順は変えず、物理牌だけを黒牌へ正規化する。
pub fn select_suited_safety_fallback_action<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Option<&'a LegalAction> {
    if !context.any_opponent_reached() {
        return None;
    }
    let chosen = suited_dahai_actions_by_safety(legal_actions, context)
        .into_iter()
        .find(|(_, rank)| *rank != SuitedSafetyRank::NoSafety)?
        .0;
    Some(prefer_black_five_for_action(legal_actions, chosen))
}

/// 最上位の字牌候補より数牌候補を優先する、限定的な横断比較。
///
/// 字牌・数牌全体の包括的な順位ではなく、1枚見えの連風牌が完全スジ以上の数牌より無条件に
/// 優先されていたケースだけを補正する。2枚以上見えた字牌や客風など、既存の明確に安全な字牌の
/// 優先順位は変えない。
pub fn suited_safety_outweighs_honor(
    honor_rank: HonorSafetyRank,
    opponent_honor_value: Option<OpponentHonorValue>,
    suited_rank: SuitedSafetyRank,
) -> bool {
    honor_rank == HonorSafetyRank::OneVisible
        && opponent_honor_value == Some(OpponentHonorValue::DoubleWind)
        && suited_rank >= SuitedSafetyRank::Suji
}
