use crate::action::{LegalAction, prefer_black_five_for_action};
use crate::context::GameContext;
use bot_logic::TileType;

/// 指定 player 自身の河に同じ牌種があるか判定する pure helper。
///
/// M リーグ公式ルールでは「自己の捨て牌にアガリ形を構成できる牌がある聴牌」がフリテンで、
/// フリテン時はツモアガリのみになる。そのため対象 player 自身の河にある牌は、その player からの
/// ロンについてリーチの有無によらず安全根拠として使える。
///
/// 「リーチ成立後に他家から切られて通った牌」(`GameContext::is_post_reach_passed`) は含まない。
/// あちらはリーチ後にその player が見逃していない、というリーチ固有の情報なので、非リーチ相手へ
/// 流用しない。両方を含む従来の現物判定は [`is_genbutsu_for`]。
///
/// 赤5は黒5と同じ牌種として扱う。範囲外の player は河を取得できないので `false`。
pub fn is_discarded_by_player(tile: TileType, player: usize, context: &GameContext) -> bool {
    context
        .discards_of(player)
        .is_some_and(|discards| discards.iter().any(|t| t.tile_type() == tile))
}

/// 指定 player 集合の全員自身の河にある牌か判定する pure helper。
///
/// 判定は [`is_discarded_by_player`] だけを使い、`post_reach_passed_tiles` は見ない。
/// 集合が空なら `false` で、対象がいないことを安全側へ倒さない。
pub fn is_discarded_by_all_players(
    tile: TileType,
    players: &[usize],
    context: &GameContext,
) -> bool {
    !players.is_empty()
        && players
            .iter()
            .all(|&player| is_discarded_by_player(tile, player, context))
}

/// 指定リーチ者にとっての現物か判定する。
///
/// 現物は「対象 player 自身の河にある牌 ([`is_discarded_by_player`])」または「そのリーチ成立後に
/// 他家から切られて通った牌」。後者はリーチ固有の情報なので、非リーチ相手の防御には使わない。
///
/// discards は防御・現物判定用、visible_tiles は枚数補正用なので用途を分ける。
pub fn is_genbutsu_for(tile: TileType, player: usize, context: &GameContext) -> bool {
    is_discarded_by_player(tile, player, context) || context.is_post_reach_passed(tile, player)
}

// 全リーチ者に共通する現物か判定する。リーチ者がいなければ false。
pub fn is_genbutsu_for_all_reached(tile: TileType, context: &GameContext) -> bool {
    let reached = context.reached_opponents();
    if reached.is_empty() {
        return false;
    }
    reached
        .iter()
        .all(|&player| is_genbutsu_for(tile, player, context))
}

// 合法 Dahai の中から全リーチ者に共通する現物候補を、元の順序を保ったまま抽出する。
pub fn genbutsu_dahai_actions_for_all_reached<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<&'a LegalAction> {
    legal_actions
        .iter()
        .filter(|action| match action {
            LegalAction::Dahai { tile } => is_genbutsu_for_all_reached(tile.tile_type(), context),
            _ => false,
        })
        .collect()
}

// 他家リーチ中に、合法 Dahai の中から全リーチ者に共通する現物を fallback として選ぶ。
// 他家リーチがない、または共通現物がなければ None。合法 action からのみ選ぶ。
//
// 最初の共通現物で牌種を決めた後、その同一牌種内では prefer_black_five_for_action で黒5を
// 優先する。牌種の選択や合法 action 間の牌種順は変えず、物理牌だけを黒牌へ正規化する。
pub fn select_genbutsu_fallback_action<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<&'a LegalAction> {
    if !context.any_opponent_reached() {
        return None;
    }
    let chosen = genbutsu_dahai_actions_for_all_reached(legal_actions, context)
        .into_iter()
        .next()?;
    Some(prefer_black_five_for_action(legal_actions, chosen))
}

// 対象牌でまだロンされ得るリーチ者。target ごとに評価が変わる safety はこの集合だけを集約する。
//
// 除外根拠はリーチ専用の現物判定 ([`is_genbutsu_for`]) で、本人の河と
// `post_reach_passed_tiles` の両方を使う。全リーチ者に現物なら空になり、その場合の安全根拠は
// [`is_genbutsu_for_all_reached`] と [`DefenseFallbackKind::Genbutsu`] が表す。
pub(super) fn ron_capable_reached_players(tile: TileType, context: &GameContext) -> Vec<usize> {
    context
        .reached_opponents()
        .into_iter()
        .filter(|&player| !is_genbutsu_for(tile, player, context))
        .collect()
}
