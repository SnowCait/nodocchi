//! `High` [`OpenHandThreatLevel`] の非リーチ副露相手に対する防御 safety の source of truth。
//!
//! 判定は既存 Defense の pure helper をそのまま共有し、字牌の見え枚数・壁・スジ・役牌価値を
//! 別実装しない。リーチ者向けの `*_for_all_reached` と違うのは対象 player 集合の決め方と、
//! 「現物相当」の根拠だけ。
//!
//! 現物相当の根拠は対象 player 自身の河 ([`is_discarded_by_player`]) だけで、
//! `post_reach_passed_tiles`(リーチ成立後に他家から切られて通った牌)は使わない。あちらは
//! リーチ固有の情報なので、非リーチ副露相手へは流用しない。
//!
//! 現時点では診断専用で、押し引き・防御 fallback の採用条件には接続していない。

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::defense::{
    HonorSafetyRank, OpponentHonorValue, SuitedSafetyRank, SujiSafetyRank, WallRank,
    honor_safety_rank, is_discarded_by_all_players, is_discarded_by_player,
    opponent_honor_value_for_players, suited_safety_rank_for_players, suji_safety_rank_for,
    suji_safety_rank_for_players, wall_rank,
};
use crate::open_hand_threat::{
    OpenHandThreatAssessment, OpenHandThreatLevel, classify_open_hand_threats,
};
use crate::threat::{PlayerThreatFacts, player_threat_facts_from_context};
use bot_logic::TileType;

/// [`OpenHandThreatLevel::High`] と分類された席を防御の target として集める pure helper。
///
/// 分類そのものは行わず、渡された classification をそのまま source of truth にする。配列の
/// index が席番号で、戻り値は席順。[`OpenHandThreatLevel::Present`] は今回の target にしない。
///
/// 自分の席・リーチ済みの席・`player_id` 不明の席は
/// [`OpenHandThreatAssessment::NotApplicable`] なので、level を持たず target にもならない。
pub fn high_open_hand_threat_players(assessments: &[OpenHandThreatAssessment; 4]) -> Vec<usize> {
    assessments
        .iter()
        .enumerate()
        .filter(|(_, assessment)| assessment.level() == Some(OpenHandThreatLevel::High))
        .map(|(player, _)| player)
        .collect()
}

/// 全4席分の facts から target を集める adapter。分類は [`classify_open_hand_threats`] が行う。
pub fn high_open_hand_threat_players_from_facts(facts: &[PlayerThreatFacts; 4]) -> Vec<usize> {
    high_open_hand_threat_players(&classify_open_hand_threats(facts))
}

/// `GameContext` から target を集める adapter。facts の構築も分類も既存経路を共有する。
pub fn high_open_hand_threat_players_from_context(context: &GameContext) -> Vec<usize> {
    high_open_hand_threat_players_from_facts(&player_threat_facts_from_context(context))
}

/// 全 target 自身の河にある牌か判定する。target が0人なら `false`。
///
/// 判定は [`is_discarded_by_player`] の集約 ([`is_discarded_by_all_players`]) で、
/// `post_reach_passed_tiles` は見ない。他家が切っただけで通った牌も恒常的な安全牌にしない。
pub fn is_discarded_by_all_open_hand_threats(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> bool {
    is_discarded_by_all_players(tile, targets, context)
}

// 対象牌でまだロンされ得る target。target ごとに評価が変わる safety はこの集合だけを集約する。
//
// 対象牌が自身の河にある target はフリテンでその牌をロンできないため、その target の評価が
// 全体の安全度を悪化させないよう除外する。除外根拠は本人の河 ([`is_discarded_by_player`]) だけ
// で、`post_reach_passed_tiles` は使わない。
//
// 全 target が対象牌を河に切っている場合は空になる。空集合は「安全と確定した」ではなく
// 「target ごとの評価が無い」で、その場合の安全根拠は
// [`OpenHandDefenseCategory::DiscardedByAllTargets`] が表す。
fn ron_capable_targets(tile: TileType, targets: &[usize], context: &GameContext) -> Vec<usize> {
    targets
        .iter()
        .copied()
        .filter(|&player| !is_discarded_by_player(tile, player, context))
        .collect()
}

/// target に対する役牌価値のうち最も危険な評価。数牌は対象外で `None`。
///
/// 対象牌が自身の河にある target からはロンされないので、その target は集約対象から除外する
/// ([`ron_capable_targets`])。target がいない場合、全 target 自身の河にある場合、情報不足で
/// 誰の分も確定できない場合は `None` (unknown)。情報不足を `GuestWind` と推測しない。
///
/// 集約は既存 Defense と同じ [`opponent_honor_value_for_players`]。
pub fn opponent_honor_value_for_open_hand_threats(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> Option<OpponentHonorValue> {
    opponent_honor_value_for_players(tile, &ron_capable_targets(tile, targets, context), context)
}

/// target の河に対するスジ安全度。数牌なら `Some`、字牌なら `None`。
///
/// 対象牌が自身の河にある target は役牌価値と同じく集約対象から除外する
/// ([`ron_capable_targets`])。その target からはロンされないので、その河のスジが無いことを
/// 全体の危険度に持ち込まない。
///
/// 残った target の [`suji_safety_rank_for`] の最小値(最も危険な評価)を採る。target が0人の
/// 場合と全 target 自身の河にある場合は `NoSuji` で、スジがあるとは扱わない。判定も集約も既存
/// Defense の [`suji_safety_rank_for_players`] と共有する。
pub fn suji_safety_rank_for_open_hand_threats(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    suji_safety_rank_for_players(tile, &ron_capable_targets(tile, targets, context), context)
}

/// target に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で `None`。
///
/// 壁は見え牌由来で target に依らないため既存 [`wall_rank`] をそのまま使い、スジは
/// [`suji_safety_rank_for_open_hand_threats`] と同じ [`ron_capable_targets`] だけを集約する。
/// 分類は既存 Defense の [`suited_safety_rank_for_players`] と共有する。
pub fn suited_safety_rank_for_open_hand_threats(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    suited_safety_rank_for_players(tile, &ron_capable_targets(tile, targets, context), context)
}

/// target に対する防御候補の大分類。
///
/// 優先順位は既存 Defense ([`DefenseFallbackKind`](crate::defense::DefenseFallbackKind)) に
/// 合わせて `DiscardedByAllTargets` → `HonorSafety` → `SuitedSafety`。
///
/// 第一分類を `Genbutsu` と呼ばないのは、リーチ者向けの現物が `post_reach_passed_tiles` まで
/// 含むのに対し、こちらは対象 player 自身の河だけを根拠にするため。「本人の河」と「リーチ後に
/// 通った牌」の意味を混ぜない。
///
/// 現時点では診断専用で、この順位で action を選ぶことはしない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenHandDefenseCategory {
    DiscardedByAllTargets,
    HonorSafety(HonorSafetyRank),
    SuitedSafety(SuitedSafetyRank),
}

/// 牌1つ分の防御候補の大分類を求める pure helper。
///
/// 字牌でも数牌でもない牌は無いため、実際に `None` になるのは分類できない場合だけ。
pub fn open_hand_defense_category(
    tile: TileType,
    targets: &[usize],
    context: &GameContext,
) -> Option<OpenHandDefenseCategory> {
    if is_discarded_by_all_open_hand_threats(tile, targets, context) {
        return Some(OpenHandDefenseCategory::DiscardedByAllTargets);
    }
    if let Some(rank) = honor_safety_rank(tile, context) {
        return Some(OpenHandDefenseCategory::HonorSafety(rank));
    }
    suited_safety_rank_for_open_hand_threats(tile, targets, context)
        .map(OpenHandDefenseCategory::SuitedSafety)
}

/// target 1人に対する safety。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenHandDefenseTargetSafety {
    /// target の席。
    pub player: usize,
    /// この target 自身の河に同じ牌種があるか ([`is_discarded_by_player`])。
    /// `post_reach_passed_tiles` は含まない。
    pub discarded_by_target: bool,
    /// この target の河に対する [`suji_safety_rank_for`]。字牌では `None`。
    ///
    /// この target 単独の評価そのもので、`discarded_by_target` による除外は行わない。集約側
    /// ([`OpenHandDefenseCandidateDiagnostic::suji_safety_rank`]) は
    /// `discarded_by_target` が `true` の target を外すため、両者の値は一致しないことがある。
    pub suji_safety_rank: Option<SujiSafetyRank>,
}

/// 合法 Dahai 1件ごとの、High OpenHandThreat 相手に対する防御評価。
///
/// production で使う pure な safety helper の結果をそのまま持つ解析用データで、表示のために
/// safety を計算し直さない。これ自体が action 選択を行うこともない。
///
/// 数牌では `wall_rank` / `suji_safety_rank` / `suited_safety_rank` が `Some`、字牌では
/// `honor_safety_rank` が `Some` になり、無関係なフィールドは `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenHandDefenseCandidateDiagnostic {
    /// 対象の合法 Dahai。物理牌(赤5 / 黒5)の区別を保持する。
    pub action: LegalAction,
    /// `action` の牌種。
    pub tile: TileType,
    /// target ごとの safety。席順で、target が0人なら空。
    pub targets: Vec<OpenHandDefenseTargetSafety>,
    /// 全 target 自身の河にあるか。target が0人なら `false`。
    pub discarded_by_all_targets: bool,
    pub honor_safety_rank: Option<HonorSafetyRank>,
    /// target に対する [`opponent_honor_value_for_open_hand_threats`] の結果。数牌では `None`。
    pub opponent_honor_value: Option<OpponentHonorValue>,
    pub wall_rank: Option<WallRank>,
    /// target に対する [`suji_safety_rank_for_open_hand_threats`] の結果そのもの。
    ///
    /// 壁と統合する前の純粋なスジ評価なので、`suited_safety_rank` が壁由来の
    /// `OneChance` / `NoChance` になっている場合でも `HalfSuji` と `NoSuji` を区別できる。
    /// 対象牌が自身の河にある target は集約対象から外れるため、`targets` の
    /// `suji_safety_rank` をそのまま最小値へ潰した値とは一致しないことがある。
    pub suji_safety_rank: Option<SujiSafetyRank>,
    pub suited_safety_rank: Option<SuitedSafetyRank>,
    /// [`open_hand_defense_category`] による大分類。
    pub category: Option<OpenHandDefenseCategory>,
}

impl OpenHandDefenseCandidateDiagnostic {
    /// 合法 Dahai 1件から防御評価を構築する pure helper。Dahai 以外の action では `None`。
    pub fn for_dahai_action(
        context: &GameContext,
        action: &LegalAction,
        targets: &[usize],
    ) -> Option<Self> {
        let LegalAction::Dahai { tile } = action else {
            return None;
        };
        let tile_type = tile.tile_type();
        let suited_tile = (!tile_type.is_honor()).then_some(tile_type);

        Some(Self {
            action: action.clone(),
            tile: tile_type,
            targets: targets
                .iter()
                .map(|&player| OpenHandDefenseTargetSafety {
                    player,
                    discarded_by_target: is_discarded_by_player(tile_type, player, context),
                    suji_safety_rank: suji_safety_rank_for(tile_type, player, context),
                })
                .collect(),
            discarded_by_all_targets: is_discarded_by_all_open_hand_threats(
                tile_type, targets, context,
            ),
            honor_safety_rank: honor_safety_rank(tile_type, context),
            opponent_honor_value: opponent_honor_value_for_open_hand_threats(
                tile_type, targets, context,
            ),
            wall_rank: suited_tile.map(|tile| wall_rank(tile, context)),
            suji_safety_rank: suji_safety_rank_for_open_hand_threats(tile_type, targets, context),
            suited_safety_rank: suited_safety_rank_for_open_hand_threats(
                tile_type, targets, context,
            ),
            category: open_hand_defense_category(tile_type, targets, context),
        })
    }

    /// 合法 action のうち Dahai だけを、元の順序を保って防御評価へ変換する。
    pub fn for_legal_actions(
        context: &GameContext,
        legal_actions: &[LegalAction],
        targets: &[usize],
    ) -> Vec<Self> {
        legal_actions
            .iter()
            .filter_map(|action| Self::for_dahai_action(context, action, targets))
            .collect()
    }
}

/// High OpenHandThreat 相手に対する防御 safety の構造化診断。
///
/// `targets` が空の局面は「OpenHand Defense target なし」で、候補評価も作らない。target がいない
/// ことを safety の値で表さないための区別。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenHandDefenseDiagnostic {
    /// [`OpenHandThreatLevel::High`] の target 席。席順。
    pub targets: Vec<usize>,
    /// target が1人以上いる場合の全合法 Dahai の防御評価。target が0人なら空。
    pub candidates: Vec<OpenHandDefenseCandidateDiagnostic>,
}

impl OpenHandDefenseDiagnostic {
    /// 構築済みの classification から診断を作る pure helper。
    ///
    /// 分類を作り直さないので、`Player threats` が持つ classification と target が必ず一致する。
    pub fn from_assessments(
        context: &GameContext,
        legal_actions: &[LegalAction],
        assessments: &[OpenHandThreatAssessment; 4],
    ) -> Self {
        let targets = high_open_hand_threat_players(assessments);
        let candidates = if targets.is_empty() {
            Vec::new()
        } else {
            OpenHandDefenseCandidateDiagnostic::for_legal_actions(context, legal_actions, &targets)
        };
        Self {
            targets,
            candidates,
        }
    }

    /// `GameContext` から診断を作る adapter。分類は [`classify_open_hand_threats`] が行う。
    pub fn from_context(context: &GameContext, legal_actions: &[LegalAction]) -> Self {
        Self::from_assessments(
            context,
            legal_actions,
            &classify_open_hand_threats(&player_threat_facts_from_context(context)),
        )
    }

    /// High OpenHandThreat の相手がいるか。
    pub fn has_target(&self) -> bool {
        !self.targets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meld::{Meld, MeldKind};
    use crate::open_hand_threat::{OpenHandThreatDecision, OpenHandThreatExclusion};
    use crate::threat::player_threat_facts_from_context;
    use bot_logic::TileId;

    // 自分は player 0、親は player 1、場風は東で固定する。
    const SELF_PLAYER: usize = 0;
    const DEALER_PLAYER: usize = 1;

    fn tile_type(mjai: &str) -> TileType {
        TileType::from_mjai_type_str(mjai).unwrap()
    }

    // 牌種から物理牌を作る。copy で同じ牌種の別の物理牌になる。
    fn tile_copy(mjai: &str, copy: u8) -> TileId {
        TileId::new(tile_type(mjai).raw() * 4 + copy).unwrap()
    }

    fn tile(mjai: &str) -> TileId {
        tile_copy(mjai, 0)
    }

    fn tiles(mjai: &str) -> Vec<TileId> {
        mjai.split_whitespace().map(tile).collect()
    }

    // 牌種のみを指定した Chi。ドラも役牌も含まない。
    fn chi() -> Meld {
        Meld::new(MeldKind::Chi, tiles("1s 2s 3s"), Some(tile("1s")))
    }

    // 白の Pon。風情報が無くても確定役牌。
    fn value_pon() -> Meld {
        Meld::new(
            MeldKind::Pon,
            (0..3).map(|copy| tile_copy("P", copy)).collect(),
            Some(tile("P")),
        )
    }

    fn ankan() -> Meld {
        Meld::new(
            MeldKind::Ankan,
            (0..4).map(|copy| tile_copy("F", copy)).collect(),
            None,
        )
    }

    // 副露を count 個持つ席を作る。High 条件の「3副露以上」を満たすかどうかを count で決める。
    fn open_melds(count: usize) -> Vec<Meld> {
        (0..count).map(|_| chi()).collect()
    }

    #[derive(Debug, Clone, Default)]
    struct ContextSpec {
        player_id: Option<u8>,
        oya: Option<u8>,
        round_wind: Option<TileType>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
        melds: [Vec<Meld>; 4],
        visible_tiles: Vec<TileId>,
        post_reach_passed: [Vec<TileType>; 4],
    }

    impl ContextSpec {
        // 自分 player 0 / 親 player 1 / 場風 東 の既定。副露も河も無い。
        fn new() -> Self {
            Self {
                player_id: Some(SELF_PLAYER as u8),
                oya: Some(DEALER_PLAYER as u8),
                round_wind: Some(tile_type("E")),
                ..Self::default()
            }
        }

        fn melds_of(mut self, player: usize, melds: Vec<Meld>) -> Self {
            self.melds[player] = melds;
            self
        }

        fn discards_of(mut self, player: usize, mjai: &str) -> Self {
            self.discards[player] = tiles(mjai);
            self
        }

        fn reached(mut self, player: usize) -> Self {
            self.reached[player] = true;
            self
        }

        fn visible(mut self, tiles: Vec<TileId>) -> Self {
            self.visible_tiles = tiles;
            self
        }

        fn post_reach_passed(mut self, player: usize, mjai: &str) -> Self {
            self.post_reach_passed[player] = mjai
                .split_whitespace()
                .map(tile_type)
                .collect::<Vec<TileType>>();
            self
        }

        fn build(self) -> GameContext {
            GameContext::from_parts_with_melds(
                None,
                vec![],
                vec![],
                self.round_wind,
                None,
                self.visible_tiles,
                self.player_id,
                self.oya,
                self.discards,
                self.reached,
                self.melds,
            )
            .with_post_reach_passed_tiles(self.post_reach_passed)
        }
    }

    fn assessments(context: &GameContext) -> [OpenHandThreatAssessment; 4] {
        classify_open_hand_threats(&player_threat_facts_from_context(context))
    }

    fn targets(context: &GameContext) -> Vec<usize> {
        high_open_hand_threat_players_from_context(context)
    }

    // 3副露の非リーチ他家を player 3 に持つ局面。player 3 だけが High target になる。
    fn single_target_context() -> GameContext {
        ContextSpec::new().melds_of(3, open_melds(3)).build()
    }

    // 3副露の非リーチ他家を player 2 と player 3 に持つ局面。
    fn two_target_context() -> GameContext {
        ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .build()
    }

    // ---- target の決定 ----

    #[test]
    fn a_high_open_hand_threat_is_a_target() {
        let context = single_target_context();
        assert_eq!(targets(&context), vec![3]);
        assert_eq!(
            high_open_hand_threat_players(&assessments(&context)),
            vec![3]
        );
    }

    #[test]
    fn a_present_open_hand_threat_is_not_a_target() {
        // 1副露だけの相手は Present で、今回の防御 target にしない。
        let context = ContextSpec::new().melds_of(3, open_melds(1)).build();

        assert_eq!(
            assessments(&context)[3].level(),
            Some(OpenHandThreatLevel::Present)
        );
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn a_reached_player_is_not_a_target() {
        // 副露しているリーチ者は OpenHandThreat の対象外なので防御 target にもならない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .reached(3)
            .build();

        assert_eq!(
            assessments(&context)[3],
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::Reached)
        );
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn the_self_seat_is_not_a_target() {
        let context = ContextSpec::new()
            .melds_of(SELF_PLAYER, open_melds(3))
            .build();

        assert_eq!(
            assessments(&context)[SELF_PLAYER],
            OpenHandThreatAssessment::NotApplicable(OpenHandThreatExclusion::SelfSeat)
        );
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn an_unknown_player_id_makes_no_target() {
        // player_id 不明の席を他家と推測して防御 target にしない。
        let mut spec = ContextSpec::new().melds_of(3, open_melds(3));
        spec.player_id = None;
        let context = spec.build();

        for player in 0..4 {
            assert_eq!(
                assessments(&context)[player].exclusion(),
                Some(OpenHandThreatExclusion::UnknownSeat),
                "player {player}"
            );
        }
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn an_ankan_only_player_is_not_a_target() {
        // 暗槓は open meld ではないので OpenHandThreat が None になり、target にならない。
        let context = ContextSpec::new().melds_of(3, vec![ankan()]).build();

        assert_eq!(
            assessments(&context)[3],
            OpenHandThreatAssessment::Classified(OpenHandThreatDecision {
                level: OpenHandThreatLevel::None,
                reason: crate::open_hand_threat::OpenHandThreatReason::NoOpenMeld,
            })
        );
        assert!(targets(&context).is_empty());
    }

    #[test]
    fn every_high_player_becomes_a_target_in_seat_order() {
        let context = ContextSpec::new()
            .melds_of(1, open_melds(3))
            .melds_of(2, open_melds(1))
            .melds_of(3, open_melds(3))
            .build();

        assert_eq!(targets(&context), vec![1, 3]);
    }

    #[test]
    fn the_targets_match_the_shared_classification() {
        // 防御側で分類し直さず、classification をそのまま source of truth にする。
        let context = two_target_context();
        let facts = player_threat_facts_from_context(&context);

        assert_eq!(
            high_open_hand_threat_players_from_facts(&facts),
            targets(&context)
        );
        assert_eq!(
            high_open_hand_threat_players(&classify_open_hand_threats(&facts)),
            targets(&context)
        );
    }

    // ---- 本人の河 (現物相当) ----

    #[test]
    fn a_tile_in_the_targets_river_is_river_safe() {
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "4s")
            .build();
        let targets = targets(&context);

        assert!(is_discarded_by_player(tile_type("4s"), 3, &context));
        assert!(is_discarded_by_all_open_hand_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
    }

    #[test]
    fn a_tile_only_in_another_players_river_is_not_river_safe() {
        // 他家が切っただけの牌は、対象 player 自身の河ではないので安全根拠にしない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(2, "4s")
            .build();
        let targets = targets(&context);

        assert_eq!(targets, vec![3]);
        assert!(!is_discarded_by_player(tile_type("4s"), 3, &context));
        assert!(!is_discarded_by_all_open_hand_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
    }

    #[test]
    fn a_post_reach_passed_tile_is_not_river_safe_for_an_open_hand_target() {
        // post_reach_passed_tiles はリーチ固有の情報で、非リーチ副露相手には流用しない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .post_reach_passed(3, "4s")
            .build();
        let targets = targets(&context);

        assert!(context.is_post_reach_passed(tile_type("4s"), 3));
        assert!(!is_discarded_by_player(tile_type("4s"), 3, &context));
        assert!(!is_discarded_by_all_open_hand_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
    }

    #[test]
    fn a_tile_in_every_targets_river_is_river_safe_for_all() {
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "4s 5s")
            .discards_of(3, "4s 6s")
            .build();
        let targets = targets(&context);

        assert_eq!(targets, vec![2, 3]);
        assert!(is_discarded_by_all_open_hand_threats(
            tile_type("4s"),
            &targets,
            &context
        ));
        // 片方の河にしか無い牌は全 target には通らない。
        assert!(!is_discarded_by_all_open_hand_threats(
            tile_type("5s"),
            &targets,
            &context
        ));
        assert!(!is_discarded_by_all_open_hand_threats(
            tile_type("6s"),
            &targets,
            &context
        ));
    }

    #[test]
    fn river_safe_for_all_is_false_without_targets() {
        // target が0人の局面を「全員に通る」と扱わない。
        let context = ContextSpec::new().discards_of(3, "4s").build();

        assert!(targets(&context).is_empty());
        assert!(!is_discarded_by_all_open_hand_threats(
            tile_type("4s"),
            &targets(&context),
            &context
        ));
    }

    #[test]
    fn a_red_five_is_river_safe_as_the_same_tile_type() {
        // 河の黒5と赤5は同じ牌種として扱う。
        let mut spec = ContextSpec::new().melds_of(3, open_melds(3));
        spec.discards[3] = vec![tile_copy("5s", 1)];
        let context = spec.build();

        assert!(is_discarded_by_all_open_hand_threats(
            tile_type("5s"),
            &targets(&context),
            &context
        ));
    }

    // ---- 字牌 safety ----

    #[test]
    fn the_honor_safety_rank_matches_the_existing_defense_helper() {
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .visible(vec![tile_copy("N", 0), tile_copy("N", 1)])
            .build();
        let candidate = candidate(&context, "N");

        assert_eq!(
            candidate.honor_safety_rank,
            Some(HonorSafetyRank::TwoVisible)
        );
        assert_eq!(
            candidate.honor_safety_rank,
            honor_safety_rank(tile_type("N"), &context)
        );
    }

    #[test]
    fn the_most_dangerous_opponent_honor_value_of_the_targets_wins() {
        // 場風 東。player 1 は親で自風も東なのでダブ東、player 3 の自風は北。
        let context = ContextSpec::new()
            .melds_of(DEALER_PLAYER, open_melds(3))
            .melds_of(3, open_melds(3))
            .build();
        let targets = targets(&context);

        assert_eq!(targets, vec![DEALER_PLAYER, 3]);
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("E"), &targets, &context),
            Some(OpponentHonorValue::DoubleWind)
        );
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("E"), &[3], &context),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn a_target_with_the_tile_in_its_river_is_excluded_from_the_honor_value() {
        // 自身の河にある牌ではロンされないので、その target は役牌価値の集約対象から外す。
        // 東は player 1 にとってダブ東、player 3 にとっては場風だけの役牌。
        let context = ContextSpec::new()
            .melds_of(DEALER_PLAYER, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(DEALER_PLAYER, "E")
            .build();

        assert_eq!(
            opponent_honor_value_for_open_hand_threats(
                tile_type("E"),
                &targets(&context),
                &context
            ),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn a_wind_that_is_neither_the_round_wind_nor_a_seat_wind_is_a_guest_wind() {
        // 場風 東、player 3 の自風は西。南は player 3 にとって客風。
        let context = ContextSpec::new().melds_of(3, open_melds(3)).build();

        assert_eq!(context.seat_wind_of(3), Some(tile_type("W")));
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(
                tile_type("S"),
                &targets(&context),
                &context
            ),
            Some(OpponentHonorValue::GuestWind)
        );
    }

    #[test]
    fn an_unknown_wind_is_not_guessed_as_a_guest_wind() {
        // 場風か親が不明な風牌は推測せず unknown のままにする。
        let mut spec = ContextSpec::new().melds_of(3, open_melds(3));
        spec.round_wind = None;
        let no_round_wind = spec.build();
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(
                tile_type("N"),
                &targets(&no_round_wind),
                &no_round_wind
            ),
            None
        );

        let mut spec = ContextSpec::new().melds_of(3, open_melds(3));
        spec.oya = None;
        let no_oya = spec.build();
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("N"), &targets(&no_oya), &no_oya),
            None
        );

        // 三元牌は風情報が無くても確定する。
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("P"), &targets(&no_oya), &no_oya),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn the_honor_value_is_unknown_without_targets() {
        let context = ContextSpec::new().build();

        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("E"), &[], &context),
            None
        );
    }

    // ---- 数牌 safety ----

    #[test]
    fn the_suji_rank_of_a_single_target_matches_the_existing_defense_helper() {
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "2m")
            .build();
        let targets = targets(&context);

        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            suji_safety_rank_for(tile_type("5m"), 3, &context)
        );
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::HalfSuji)
        );
    }

    #[test]
    fn the_most_dangerous_suji_rank_of_the_targets_wins() {
        // player 2 に対しては両側スジ、player 3 に対しては片スジ。集約は最も危険な HalfSuji。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "2m 8m")
            .discards_of(3, "2m")
            .build();
        let targets = targets(&context);

        assert_eq!(
            suji_safety_rank_for(tile_type("5m"), 2, &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suji_safety_rank_for(tile_type("5m"), 3, &context),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::HalfSuji)
        );
    }

    #[test]
    fn a_target_with_the_tile_in_its_river_does_not_lower_the_suji_rank() {
        // player 2 は 5m を河に切っているのでフリテンでロンできない。その無スジを集約に
        // 持ち込まず、ロンされ得る player 3 の両側スジがそのまま全体の評価になる。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "5m")
            .discards_of(3, "2m 8m")
            .build();
        let targets = targets(&context);

        assert_eq!(targets, vec![2, 3]);
        assert!(is_discarded_by_player(tile_type("5m"), 2, &context));
        assert_eq!(
            suji_safety_rank_for(tile_type("5m"), 2, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suji_safety_rank_for(tile_type("5m"), 3, &context),
            Some(SujiSafetyRank::Suji)
        );

        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::Suji)
        );
    }

    #[test]
    fn a_remaining_target_without_a_suji_still_lowers_the_suji_rank() {
        // player 2 は 5m を河に切っているが、player 3 にはまだロンされ得るので無スジのまま。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "5m")
            .build();
        let targets = targets(&context);

        assert_eq!(targets, vec![2, 3]);
        assert!(!is_discarded_by_player(tile_type("5m"), 3, &context));
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::NoSafety)
        );
    }

    #[test]
    fn a_tile_in_every_targets_river_keeps_the_first_category() {
        // 全 target が河に切っている牌は集約対象が0人になるが、安全根拠は本人の河の分類が持つ。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "5m")
            .discards_of(3, "5m")
            .build();
        let targets = targets(&context);

        assert!(is_discarded_by_all_open_hand_threats(
            tile_type("5m"),
            &targets,
            &context
        ));
        assert_eq!(
            open_hand_defense_category(tile_type("5m"), &targets, &context),
            Some(OpenHandDefenseCategory::DiscardedByAllTargets)
        );
        // 集約対象が0人でも「スジがある」とは扱わない。
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::NoSafety)
        );
    }

    #[test]
    fn a_post_reach_passed_tile_does_not_exclude_a_target_from_the_aggregate() {
        // post_reach_passed_tiles は非リーチ副露相手の除外根拠にしない。player 2 は 5m を
        // 河に切っていないので、その無スジも役牌価値もそのまま集約へ入る。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(3, "2m 8m")
            .post_reach_passed(2, "5m E")
            .build();
        let targets = targets(&context);

        assert!(context.is_post_reach_passed(tile_type("5m"), 2));
        assert!(!is_discarded_by_player(tile_type("5m"), 2, &context));
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::NoSafety)
        );
        // 役牌価値も同じ除外規則を共有する。
        assert_eq!(
            opponent_honor_value_for_open_hand_threats(tile_type("E"), &targets, &context),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn the_suji_rank_without_targets_is_no_suji() {
        let context = ContextSpec::new().discards_of(3, "2m 8m").build();

        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("5m"), &[], &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suji_safety_rank_for_open_hand_threats(tile_type("N"), &[], &context),
            None
        );
    }

    #[test]
    fn the_wall_rank_matches_the_existing_defense_helper() {
        // 6m が4枚見えていれば 8m の順子待ち経路は残らない。壁は見え牌由来で target に依らない。
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .visible((0..4).map(|copy| tile_copy("6m", copy)).collect())
            .build();
        let candidate = candidate(&context, "8m");

        assert_eq!(candidate.wall_rank, Some(WallRank::NoChance));
        assert_eq!(
            candidate.wall_rank,
            Some(wall_rank(tile_type("8m"), &context))
        );
        assert_eq!(
            candidate.suited_safety_rank,
            Some(SuitedSafetyRank::NoChance)
        );
    }

    #[test]
    fn the_suited_safety_rank_prefers_the_wall_over_the_suji() {
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "2m 8m")
            .build();
        let targets = targets(&context);

        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("5m"), &targets, &context),
            Some(SuitedSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_open_hand_threats(tile_type("N"), &targets, &context),
            None
        );
    }

    // ---- 候補診断 ----

    fn dahai(mjai: &str) -> LegalAction {
        LegalAction::Dahai { tile: tile(mjai) }
    }

    fn candidate(context: &GameContext, mjai: &str) -> OpenHandDefenseCandidateDiagnostic {
        OpenHandDefenseCandidateDiagnostic::for_dahai_action(
            context,
            &dahai(mjai),
            &high_open_hand_threat_players_from_context(context),
        )
        .expect("Dahai の候補診断")
    }

    #[test]
    fn a_candidate_carries_the_per_target_safety() {
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "5m 2m 8m")
            .discards_of(3, "2m")
            .build();
        let candidate = candidate(&context, "5m");

        assert_eq!(
            candidate.targets,
            vec![
                OpenHandDefenseTargetSafety {
                    player: 2,
                    discarded_by_target: true,
                    suji_safety_rank: Some(SujiSafetyRank::Suji),
                },
                OpenHandDefenseTargetSafety {
                    player: 3,
                    discarded_by_target: false,
                    suji_safety_rank: Some(SujiSafetyRank::HalfSuji),
                },
            ]
        );
        assert!(!candidate.discarded_by_all_targets);
        assert_eq!(candidate.suji_safety_rank, Some(SujiSafetyRank::HalfSuji));
        assert_eq!(
            candidate.suited_safety_rank,
            Some(SuitedSafetyRank::HalfSuji)
        );
        assert_eq!(
            candidate.category,
            Some(OpenHandDefenseCategory::SuitedSafety(
                SuitedSafetyRank::HalfSuji
            ))
        );
    }

    #[test]
    fn a_candidate_reports_the_same_values_as_the_pure_helpers() {
        // 表示のために safety を計算し直さず、production の pure helper の値をそのまま載せる。
        let context = ContextSpec::new()
            .melds_of(2, open_melds(3))
            .melds_of(3, open_melds(3))
            .discards_of(2, "2m")
            .visible(vec![tile_copy("N", 0)])
            .build();
        let targets = targets(&context);

        for mjai in ["5m", "N"] {
            let candidate = candidate(&context, mjai);
            let tile = tile_type(mjai);

            assert_eq!(
                candidate.discarded_by_all_targets,
                is_discarded_by_all_open_hand_threats(tile, &targets, &context),
                "{mjai}"
            );
            assert_eq!(
                candidate.honor_safety_rank,
                honor_safety_rank(tile, &context),
                "{mjai}"
            );
            assert_eq!(
                candidate.opponent_honor_value,
                opponent_honor_value_for_open_hand_threats(tile, &targets, &context),
                "{mjai}"
            );
            assert_eq!(
                candidate.suji_safety_rank,
                suji_safety_rank_for_open_hand_threats(tile, &targets, &context),
                "{mjai}"
            );
            assert_eq!(
                candidate.suited_safety_rank,
                suited_safety_rank_for_open_hand_threats(tile, &targets, &context),
                "{mjai}"
            );
            assert_eq!(
                candidate.category,
                open_hand_defense_category(tile, &targets, &context),
                "{mjai}"
            );
        }
    }

    #[test]
    fn the_category_prefers_the_targets_river_over_the_honor_and_suited_safety() {
        let context = ContextSpec::new()
            .melds_of(3, open_melds(3))
            .discards_of(3, "N 5m")
            .build();
        let targets = targets(&context);

        assert_eq!(
            open_hand_defense_category(tile_type("N"), &targets, &context),
            Some(OpenHandDefenseCategory::DiscardedByAllTargets)
        );
        assert_eq!(
            open_hand_defense_category(tile_type("5m"), &targets, &context),
            Some(OpenHandDefenseCategory::DiscardedByAllTargets)
        );
        assert_eq!(
            open_hand_defense_category(tile_type("W"), &targets, &context),
            Some(OpenHandDefenseCategory::HonorSafety(
                HonorSafetyRank::NoVisible
            ))
        );
        assert_eq!(
            open_hand_defense_category(tile_type("3m"), &targets, &context),
            Some(OpenHandDefenseCategory::SuitedSafety(
                SuitedSafetyRank::NoSafety
            ))
        );
    }

    // ---- 局面診断 ----

    #[test]
    fn the_diagnostic_keeps_the_legal_dahai_order_and_skips_other_actions() {
        let context = single_target_context();
        let legal_actions = vec![
            LegalAction::Reach,
            dahai("N"),
            dahai("5m"),
            LegalAction::Hora,
        ];
        let diagnostic = OpenHandDefenseDiagnostic::from_context(&context, &legal_actions);

        assert!(diagnostic.has_target());
        assert_eq!(diagnostic.targets, vec![3]);
        assert_eq!(
            diagnostic
                .candidates
                .iter()
                .map(|candidate| candidate.tile)
                .collect::<Vec<TileType>>(),
            vec![tile_type("N"), tile_type("5m")]
        );
    }

    #[test]
    fn the_diagnostic_has_no_candidates_without_targets() {
        // Present しかいない局面を「target なし」と分かる形にする。
        let context = ContextSpec::new().melds_of(3, open_melds(1)).build();
        let diagnostic = OpenHandDefenseDiagnostic::from_context(&context, &[dahai("N")]);

        assert!(!diagnostic.has_target());
        assert!(diagnostic.targets.is_empty());
        assert!(diagnostic.candidates.is_empty());
    }

    #[test]
    fn the_diagnostic_shares_the_given_classification() {
        let context = two_target_context();
        let legal_actions = vec![dahai("N")];

        assert_eq!(
            OpenHandDefenseDiagnostic::from_assessments(
                &context,
                &legal_actions,
                &assessments(&context)
            ),
            OpenHandDefenseDiagnostic::from_context(&context, &legal_actions)
        );
    }

    #[test]
    fn a_value_honor_meld_target_is_selected_from_the_classification() {
        // 2副露 + 確定役牌の High 条件でも、target の決め方は classification のまま。
        let context = ContextSpec::new()
            .melds_of(3, vec![value_pon(), chi()])
            .build();

        assert_eq!(
            assessments(&context)[3].level(),
            Some(OpenHandThreatLevel::High)
        );
        assert_eq!(targets(&context), vec![3]);
    }
}
