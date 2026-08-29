mod diagnostic;
mod hard_safety;
mod honor;
mod suited;
mod suji;
mod wall;

#[cfg(test)]
mod tests;

use crate::action::{LegalAction, prefer_black_five_for_action};
use crate::context::GameContext;
use bot_logic::TileType;

pub use diagnostic::{
    DefenseCandidateDiagnostic, DefenseDecisionDiagnostic, DefenseFallbackDiagnostic,
    log_defense_fallback_decision,
};
pub use hard_safety::{
    genbutsu_dahai_actions_for_all_reached, is_discarded_by_all_players, is_discarded_by_player,
    is_genbutsu_for, is_genbutsu_for_all_reached, select_genbutsu_fallback_action,
};
pub use honor::{
    HonorSafetyRank, OpponentHonorValue, honor_dahai_actions_by_safety,
    honor_dahai_actions_by_safety_with, honor_safety_rank, opponent_honor_value_for,
    opponent_honor_value_for_players, opponent_honor_value_for_reached,
    select_honor_safety_fallback_action,
};
pub use suited::{
    SuitedSafetyRank, select_suited_safety_fallback_action, suited_dahai_actions_by_safety,
    suited_dahai_actions_by_safety_with, suited_safety_outweighs_honor,
    suited_safety_rank_for_all_reached, suited_safety_rank_for_any_reached,
    suited_safety_rank_for_players,
};
pub use suji::{
    SujiSafetyRank, is_suji_for, is_suji_for_all_reached, is_suji_for_any_reached,
    suji_dahai_actions_by_safety, suji_safety_rank_for, suji_safety_rank_for_all_reached,
    suji_safety_rank_for_any_reached, suji_safety_rank_for_players,
};
pub use wall::{WallRank, is_no_chance, is_one_chance, wall_rank, wall_tile_types_by_rank};

// visible_tiles 中で同じ TileType の枚数を数える。赤5も通常5と同じ TileType として数える。
pub fn visible_count_of(tile: TileType, context: &GameContext) -> u8 {
    context
        .visible_tiles()
        .iter()
        .filter(|visible| visible.tile_type() == tile)
        .count() as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenseFallbackKind {
    Genbutsu,
    HonorSafety(HonorSafetyRank),
    SuitedSafety(SuitedSafetyRank),
}

// 他家リーチ中の防御 fallback を優先順位付きで選ぶ。
// 全リーチ者への共通現物を最優先にし、その候補が無い場合は最上位の字牌・数牌候補を限定的に
// 横断比較して、選ばれた種別を添えて返す。
//
// 現物は黒5対応済みの select_genbutsu_fallback_action をそのまま利用する。字牌 safety と
// 数牌 safety は DefenseFallbackKind に載せる rank を候補列から一度に得るため、候補列を直接
// 使ったうえで prefer_black_five_for_action で黒5へ正規化する(冪等)。字牌に赤5は無いので
// 字牌側の正規化は実質 no-op。いずれも牌種選択・種別・安全度 rank は変えない。
pub fn select_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<(&'a LegalAction, DefenseFallbackKind)> {
    if let Some(action) = select_genbutsu_fallback_action(context, legal_actions) {
        return Some((action, DefenseFallbackKind::Genbutsu));
    }

    if context.any_opponent_reached() {
        let honor = honor_dahai_actions_by_safety(legal_actions, context)
            .into_iter()
            .next();
        let suited = suited_dahai_actions_by_safety(legal_actions, context)
            .into_iter()
            .find(|(_, rank)| *rank != SuitedSafetyRank::NoSafety);

        if let (Some((honor_action, honor_rank)), Some((suited_action, suited_rank))) =
            (honor, suited)
            && let LegalAction::Dahai { tile: honor_tile } = honor_action
            && suited_safety_outweighs_honor(
                honor_rank,
                opponent_honor_value_for_reached(honor_tile.tile_type(), context),
                suited_rank,
            )
        {
            let action = prefer_black_five_for_action(legal_actions, suited_action);
            return Some((action, DefenseFallbackKind::SuitedSafety(suited_rank)));
        }

        if let Some((action, rank)) = honor {
            let action = prefer_black_five_for_action(legal_actions, action);
            return Some((action, DefenseFallbackKind::HonorSafety(rank)));
        }

        if let Some((action, rank)) = suited {
            let action = prefer_black_five_for_action(legal_actions, action);
            return Some((action, DefenseFallbackKind::SuitedSafety(rank)));
        }
    }

    None
}

// 防御 fallback の action だけを返す薄い wrapper。
pub fn select_defense_fallback_action<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<&'a LegalAction> {
    select_defense_fallback_action_with_kind(context, legal_actions).map(|(action, _)| action)
}
