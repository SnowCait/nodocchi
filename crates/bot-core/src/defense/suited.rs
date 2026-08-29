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

/// 数牌の防御根拠を壁とスジの両方について保持する evidence。
///
/// [`SuitedSafetyRank`] は壁がある時点でスジ評価を捨ててしまうため、`OneChance` + `Suji` と
/// `OneChance` + `NoSuji` を区別できない。こちらは両方の根拠をそのまま持つ。
///
/// 壁は見え牌由来で対象 player に依らず、スジは対象 player 集合ごとに変わる。どの player 集合に
/// 対する evidence かは構築側 ([`suited_safety_evidence_for_players`]) が決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuitedSafetyEvidence {
    pub wall_rank: WallRank,
    pub suji_rank: SujiSafetyRank,
}

impl SuitedSafetyEvidence {
    /// evidence を既存の [`SuitedSafetyRank`] へ写す互換用 mapping。
    ///
    /// 壁があればスジ評価を捨てて壁の分類だけを返す、という既存 production の挙動をそのまま
    /// 再現するためだけのもので、安全度の policy として設計された変換ではない。壁とスジを
    /// 同時に評価する comparator は別途導入する。
    pub fn legacy_rank(self) -> SuitedSafetyRank {
        match self.wall_rank {
            WallRank::NoChance => SuitedSafetyRank::NoChance,
            WallRank::OneChance => SuitedSafetyRank::OneChance,
            WallRank::NoWall => match self.suji_rank {
                SujiSafetyRank::Suji => SuitedSafetyRank::Suji,
                SujiSafetyRank::HalfSuji => SuitedSafetyRank::HalfSuji,
                SujiSafetyRank::NoSuji => SuitedSafetyRank::NoSafety,
            },
        }
    }
}

/// 指定 player 集合に対する数牌の防御 evidence。字牌は対象外で `None`。
///
/// 壁とスジの判定規則はここで再実装せず、[`wall_rank`] と [`suji_safety_rank_for_players`] の
/// 結果を組み合わせるだけにする。集合が空ならスジ評価は `NoSuji`。
///
/// リーチ / OpenHand / Combined はいずれも「まだロンされ得る player 集合」を決めたうえで
/// この helper を通す。evidence の意味を経路ごとに変えない。
pub fn suited_safety_evidence_for_players(
    tile: TileType,
    players: &[usize],
    context: &GameContext,
) -> Option<SuitedSafetyEvidence> {
    let suji_rank = suji_safety_rank_for_players(tile, players, context)?;
    Some(SuitedSafetyEvidence {
        wall_rank: wall_rank(tile, context),
        suji_rank,
    })
}

/// 現物ではない全リーチ者に対する数牌の防御 evidence。字牌は対象外で `None`。
///
/// 対象牌が現物のリーチ者を [`ron_capable_reached_players`] で除外し、まだロンされ得る
/// リーチ者だけを [`suited_safety_evidence_for_players`] へ渡す薄い adapter。
pub fn suited_safety_evidence_for_all_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SuitedSafetyEvidence> {
    suited_safety_evidence_for_players(tile, &ron_capable_reached_players(tile, context), context)
}

/// いずれかのリーチ者に対する数牌の防御 evidence。字牌は対象外で `None`。
///
/// スジ評価だけは [`suji_safety_rank_for_any_reached`] の最も安全な評価を使う。`*_for_all_reached`
/// とは集約の向きが違うので、こちらの evidence を全リーチ者向けの安全根拠として使わない。
pub fn suited_safety_evidence_for_any_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SuitedSafetyEvidence> {
    let suji_rank = suji_safety_rank_for_any_reached(tile, context)?;
    Some(SuitedSafetyEvidence {
        wall_rank: wall_rank(tile, context),
        suji_rank,
    })
}

// いずれかのリーチ者の河に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で None。
pub fn suited_safety_rank_for_any_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    suited_safety_evidence_for_any_reached(tile, context).map(SuitedSafetyEvidence::legacy_rank)
}

/// 指定 player 集合の河に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で `None`。
///
/// [`suited_safety_evidence_for_players`] の evidence を
/// [`SuitedSafetyEvidence::legacy_rank`] で潰す薄い wrapper。壁評価はスジ評価より優先し、
/// 集合が空なら壁が無い限り `NoSafety`。
pub fn suited_safety_rank_for_players(
    tile: TileType,
    players: &[usize],
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    suited_safety_evidence_for_players(tile, players, context)
        .map(SuitedSafetyEvidence::legacy_rank)
}

// 現物ではない全リーチ者に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で None。
// 壁評価はスジ評価より優先する。スジ評価は現物ではない全リーチ者に対する rank の最小値を使う。
pub fn suited_safety_rank_for_all_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    suited_safety_evidence_for_all_reached(tile, context).map(SuitedSafetyEvidence::legacy_rank)
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
