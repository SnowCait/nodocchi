use crate::action::LegalAction;
use crate::context::{GameContext, seat_wind_for_player};
use bot_logic::TileType;

use super::hard_safety::ron_capable_reached_players;
use super::visible_count_of;

// 字牌の見え枚数に基づく安全度。見えているほど当たりにくいので安全度が高い。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HonorSafetyRank {
    NoVisible,
    OneVisible,
    TwoVisible,
    ThreeOrMoreVisible,
}

// 字牌の安全度を見え枚数から求める。字牌でなければ None。
pub fn honor_safety_rank(tile: TileType, context: &GameContext) -> Option<HonorSafetyRank> {
    if !tile.is_honor() {
        return None;
    }
    let rank = match visible_count_of(tile, context) {
        0 => HonorSafetyRank::NoVisible,
        1 => HonorSafetyRank::OneVisible,
        2 => HonorSafetyRank::TwoVisible,
        _ => HonorSafetyRank::ThreeOrMoreVisible,
    };
    Some(rank)
}

/// 対象リーチ者にとっての役牌価値。危険度は `GuestWind` < `SingleValueHonor` < `DoubleWind` で、
/// derive した `Ord` の順序と一致する。
///
/// これは防御上のヒューリスティックであって、「役牌ならルール上ロンされやすい」という意味では
/// ない。リーチ者はすでにリーチという役を持つため、客風でも待ち牌になり得る。相手が手牌に
/// 保持しやすいと考えられる役牌、特に連風牌を危険寄りに扱うための重み付けとして使う。
///
/// - `GuestWind`:        対象リーチ者にとって場風でも自風でもない風牌。
/// - `SingleValueHonor`: 三元牌、または場風だけ / 自風だけに該当する風牌。
/// - `DoubleWind`:       対象リーチ者にとって場風かつ自風の風牌(ダブ東・ダブ南等)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpponentHonorValue {
    GuestWind,
    SingleValueHonor,
    DoubleWind,
}

/// 指定 player にとっての役牌価値を求める pure helper。数牌は対象外で `None`。
///
/// 三元牌は常に `SingleValueHonor`。風牌は場風と player の自風の一致で分類し、特定の `E` / `S`
/// へはハードコードしない。
///
/// 場風または親が不明で風牌の分類を確定できない場合は推測せず `None` (unknown) を返す。
/// unknown を `GuestWind` として安全牌扱いしたり、根拠なく `DoubleWind` と決めつけたりしない。
pub fn opponent_honor_value_for(
    tile: TileType,
    player: usize,
    context: &GameContext,
) -> Option<OpponentHonorValue> {
    if tile.is_dragon() {
        return Some(OpponentHonorValue::SingleValueHonor);
    }
    if !tile.is_wind() {
        return None;
    }
    let round_wind = context.round_wind()?;
    let seat_wind = seat_wind_for_player(player, context.oya()?)?;

    let value = match (round_wind == tile, seat_wind == tile) {
        (true, true) => OpponentHonorValue::DoubleWind,
        (true, false) | (false, true) => OpponentHonorValue::SingleValueHonor,
        (false, false) => OpponentHonorValue::GuestWind,
    };
    Some(value)
}

/// 指定 player 集合に対する役牌価値のうち最も危険な評価。数牌は対象外で `None`。
///
/// 各 player の評価は [`opponent_honor_value_for`] そのもので、集約だけを行う。集合が空の場合、
/// 情報不足で誰の分も確定できない場合は `None` (unknown) で、unknown を安全側にも危険側にも
/// 倒さない。
///
/// ロンされない player (対象牌がその player 自身の河にある等) を外すかどうかは呼び出し側の
/// 責務で、ここでは渡された集合をそのまま集約する。リーチ者向けの入口は
/// [`opponent_honor_value_for_reached`]。
pub fn opponent_honor_value_for_players(
    tile: TileType,
    players: &[usize],
    context: &GameContext,
) -> Option<OpponentHonorValue> {
    players
        .iter()
        .filter_map(|&player| opponent_honor_value_for(tile, player, context))
        .max()
}

/// 全リーチ者に対する役牌価値のうち最も危険な評価。数牌は対象外で `None`。
///
/// 対象牌が現物のリーチ者からはロンされないので、そのリーチ者は集約対象から除外する。
/// リーチ者がいない場合、全リーチ者に対して現物の場合、情報不足で誰の分も確定できない場合は
/// `None` (unknown)。unknown を安全側にも危険側にも倒さない。
///
/// これが字牌防御における役牌価値の source of truth。集約自体は
/// [`opponent_honor_value_for_players`] と共有する。
pub fn opponent_honor_value_for_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<OpponentHonorValue> {
    opponent_honor_value_for_players(tile, &ron_capable_reached_players(tile, context), context)
}

pub(super) type RankedHonorCandidate<'a> =
    (&'a LegalAction, HonorSafetyRank, Option<OpponentHonorValue>);

pub(super) fn sort_group_by_opponent_honor_value(group: &mut [RankedHonorCandidate<'_>]) {
    for run in group.split_mut(|candidate| candidate.2.is_none()) {
        run.sort_by_key(|candidate| candidate.2);
    }
}

/// 合法 Dahai のうち字牌のみを 見え枚数の安全度 → 役牌価値 → 元の順序 で並べる共有実装。
///
/// 役牌価値の求め方だけを呼び出し側から差し替えられるようにしてある。見え枚数の安全度
/// ([`honor_safety_rank`]) と、同 rank 内を役牌価値の切りやすい順
/// (`GuestWind` → `SingleValueHonor` → `DoubleWind`) に並べる sorting はここが唯一の実装で、
/// 対象 player 集合ごとにコピーしない。unknown の役牌価値は推測せず、その位置の元の順序を保つ。
///
/// リーチ者向けの入口は [`honor_dahai_actions_by_safety`]。非リーチ副露相手向けの入口は
/// [`open_hand_honor_dahai_actions_by_safety`](crate::open_hand_defense::open_hand_honor_dahai_actions_by_safety)。
pub fn honor_dahai_actions_by_safety_with<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
    opponent_honor_value: impl Fn(TileType) -> Option<OpponentHonorValue>,
) -> Vec<(&'a LegalAction, HonorSafetyRank)> {
    let mut ranked: Vec<RankedHonorCandidate<'a>> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => {
                let tile_type = tile.tile_type();
                honor_safety_rank(tile_type, context)
                    .map(|rank| (action, rank, opponent_honor_value(tile_type)))
            }
            _ => None,
        })
        .collect();
    ranked.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));

    let mut start = 0;
    while start < ranked.len() {
        let rank = ranked[start].1;
        let end = ranked[start..]
            .iter()
            .position(|candidate| candidate.1 != rank)
            .map_or(ranked.len(), |offset| start + offset);
        sort_group_by_opponent_honor_value(&mut ranked[start..end]);
        start = end;
    }

    ranked
        .into_iter()
        .map(|(action, rank, _)| (action, rank))
        .collect()
}

// 合法 Dahai のうち字牌のみを 見え枚数の安全度 → 役牌価値 → 元の順序 で並べる。
// 役牌価値は全リーチ者基準で、現物のリーチ者を除いた最も危険な評価を使う。
pub fn honor_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<(&'a LegalAction, HonorSafetyRank)> {
    honor_dahai_actions_by_safety_with(legal_actions, context, |tile| {
        opponent_honor_value_for_reached(tile, context)
    })
}

// 最も安全度の高い字牌 Dahai を fallback として選ぶ。候補がなければ None。
pub fn select_honor_safety_fallback_action<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Option<&'a LegalAction> {
    honor_dahai_actions_by_safety(legal_actions, context)
        .into_iter()
        .next()
        .map(|(action, _)| action)
}
