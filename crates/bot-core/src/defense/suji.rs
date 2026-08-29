use crate::action::LegalAction;
use crate::context::GameContext;
use bot_logic::TileType;

use super::hard_safety::{is_discarded_by_player, ron_capable_reached_players};
use super::wall::sequence_wait_routes;

/// スジ安全度。両側スジ / 片スジ / 無スジ の3段階。
///
/// 安全度は `Suji` > `HalfSuji` > `NoSuji` で、derive した `Ord` の順序と一致する。
///
/// - `Suji`:     対象牌のスジ本数ぶんすべての根拠牌が河にある(4/5/6 なら両側)。
/// - `HalfSuji`: 2本のスジを持つ 4/5/6 で、片側の根拠牌だけが河にある片スジ。
/// - `NoSuji`:   根拠牌が河に無い。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SujiSafetyRank {
    NoSuji,
    HalfSuji,
    Suji,
}

/// 指定 player の河に対するスジ安全度を求める pure helper。字牌は対象外で `None`。
///
/// 同一色のスジ根拠牌(1/2/3 と 7/8/9 は1本、4/5/6 は両側2本)が河に何本あるかで分類する。
/// すべて揃えば `Suji`、一部だけなら `HalfSuji`、無ければ `NoSuji`。
///
/// player が範囲外で河を取得できない場合は推測せず `NoSuji` として扱い、安全側へ倒さない。
pub fn suji_safety_rank_for(
    tile: TileType,
    player: usize,
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    let partners: Vec<TileType> = sequence_wait_routes(tile)
        .into_iter()
        .filter_map(|route| route.suji_partner)
        .collect();
    if partners.is_empty() {
        return Some(SujiSafetyRank::NoSuji);
    }
    let found = partners
        .iter()
        .filter(|&&partner| is_discarded_by_player(partner, player, context))
        .count();
    let rank = if found == partners.len() {
        SujiSafetyRank::Suji
    } else if found > 0 {
        SujiSafetyRank::HalfSuji
    } else {
        SujiSafetyRank::NoSuji
    };
    Some(rank)
}

/// 指定 player に対して完全なスジか判定する。`SujiSafetyRank::Suji` のときだけ `true`。
///
/// 片スジ (`HalfSuji`) と無スジ (`NoSuji`) はいずれも `false`。字牌や範囲外 player も `false`。
pub fn is_suji_for(tile: TileType, player: usize, context: &GameContext) -> bool {
    suji_safety_rank_for(tile, player, context) == Some(SujiSafetyRank::Suji)
}

/// いずれかのリーチ者に対して完全なスジか判定する。リーチ者がいなければ `false`。
pub fn is_suji_for_any_reached(tile: TileType, context: &GameContext) -> bool {
    suji_safety_rank_for_any_reached(tile, context) == Some(SujiSafetyRank::Suji)
}

/// 対象牌が現物のリーチ者を除いた、まだロンされ得る全リーチ者に対して完全なスジか判定する。
///
/// まだロンされ得るリーチ者が一人でも片スジ / 無スジなら `false`。リーチ者がいない場合と、
/// 全リーチ者に現物で集約対象が空になる場合も `false`。
pub fn is_suji_for_all_reached(tile: TileType, context: &GameContext) -> bool {
    suji_safety_rank_for_all_reached(tile, context) == Some(SujiSafetyRank::Suji)
}

// player 集合に対するスジ安全度の集約方法。
//
// 最も危険な評価 (最小値) は集合全員へ通したい防御に、最も安全な評価 (最大値) は誰か1人に対する
// スジがあるかの判定に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SujiAggregate {
    MostDangerous,
    Safest,
}

// player 集合の [`suji_safety_rank_for`] を集約する。字牌は対象外で None、集合が空なら NoSuji。
fn aggregate_suji_safety_rank(
    tile: TileType,
    players: &[usize],
    context: &GameContext,
    aggregate: SujiAggregate,
) -> Option<SujiSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    let ranks = players
        .iter()
        .filter_map(|&player| suji_safety_rank_for(tile, player, context));
    let rank = match aggregate {
        SujiAggregate::MostDangerous => ranks.min(),
        SujiAggregate::Safest => ranks.max(),
    };
    Some(rank.unwrap_or(SujiSafetyRank::NoSuji))
}

/// 指定 player 集合の河に対するスジ安全度。数牌なら `Some`、字牌なら `None`。
///
/// 各 player の [`suji_safety_rank_for`] の最小値(最も危険な評価)を採る。例えば player1 に
/// 対して `Suji`・player2 に対して `HalfSuji` なら全体は `HalfSuji`。集合が空なら `NoSuji` で、
/// 安全牌としては扱わない。
///
/// リーチ者向けの入口は [`suji_safety_rank_for_all_reached`] で、判定も集約もこの helper と
/// 共有する。
pub fn suji_safety_rank_for_players(
    tile: TileType,
    players: &[usize],
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    aggregate_suji_safety_rank(tile, players, context, SujiAggregate::MostDangerous)
}

/// いずれかのリーチ者の河に対するスジ安全度。数牌なら `Some`、字牌なら `None`。
///
/// 各リーチ者の [`suji_safety_rank_for`] の最大値(最も安全な評価)を採る。
/// リーチ者がいなければ `NoSuji`。
pub fn suji_safety_rank_for_any_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    aggregate_suji_safety_rank(
        tile,
        &context.reached_opponents(),
        context,
        SujiAggregate::Safest,
    )
}

/// 現物ではない全リーチ者の河に対するスジ安全度。数牌なら `Some`、字牌なら `None`。
///
/// 対象牌が現物のリーチ者を除外し、まだロンされ得るリーチ者の
/// [`suji_safety_rank_for`] の最小値(最も危険な評価)を採る。例えば player1 には現物、player2
/// には `Suji` なら全体は `Suji`。リーチ者がいない場合と全リーチ者に現物の場合は `NoSuji`
/// で、別のスジ安全性を捏造しない。
///
/// これが数牌防御におけるスジ評価の source of truth。任意の player 集合向けの
/// [`suji_safety_rank_for_players`] にリーチ者を渡す薄い wrapper。
pub fn suji_safety_rank_for_all_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    suji_safety_rank_for_players(tile, &ron_capable_reached_players(tile, context), context)
}

// 合法 Dahai のうち数牌のみを安全度の高い順(Suji → HalfSuji → NoSuji)に並べる。
// 同安全度は元の順序を保つ。スジ判定は現物ではない全リーチ者基準で、各リーチ者の rank の
// 最小値を使う。
pub fn suji_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<(&'a LegalAction, SujiSafetyRank)> {
    let mut ranked: Vec<(&'a LegalAction, SujiSafetyRank)> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => {
                suji_safety_rank_for_all_reached(tile.tile_type(), context)
                    .map(|rank| (action, rank))
            }
            _ => None,
        })
        .collect();
    ranked.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));
    ranked
}
