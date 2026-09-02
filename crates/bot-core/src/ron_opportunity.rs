//! 現在聴牌の Ron opportunity を structural facts として並べる診断層。
//!
//! Reach / Damaten を比べる材料のうち、Ron 側は「その待ちで和了した場合の支払い」しか無かった。
//! その前段として、この層は「今リーチした場合、現在の待ち牌が公開情報上どの程度安全に見えるか」
//! を既存 Defense helper の観測値そのままで持つ。
//!
//! ```text
//! 待ちごと
//!   live copies          既存の受け入れが持つ残枚数
//!   if Reach             リーチ宣言が公開された場合の公開 safety evidence
//!   if Damaten           テンパイ宣言が公開されないという事実だけ
//! 局面全体
//!   external threats     既存 classification が持つリーチ者と High OpenHand target
//! ```
//!
//! # 確率ではない
//!
//! ここが持つのは公開情報から観測できる structural facts だけで、確率は1つも持たない。
//!
//! ```text
//! 持たない: Ron probability / discard probability / deal-in probability
//! 持たない: 「スジなら何%」のような係数、Reach / Damaten のロン率補正
//! 持たない: self-tsumo と Ron を統合した EV、winner、新しい should_reach
//! ```
//!
//! nodocchi はまだ「他家がその牌を切る確率」の模型を持たない。`GameContext` は player ごとの
//! 河・副露・リーチ状態を持つが、河は [`TileId`](bot_logic::TileId) の列で各牌の
//! 手出し / ツモ切りも持たないため、相手別の打牌確率もこの層では作らない。
//!
//! # Defense の exact `R/T` は使わない
//!
//! [`RonRiskEvidence`](crate::defense::RonRiskEvidence) 等の exact model は
//!
//! ```text
//! 自分が牌 x を切った場合、相手が x でロン可能な hidden-hand state の structural weight
//! ```
//!
//! を表す。ここで欲しいのは
//!
//! ```text
//! 自分が x 待ちでテンパイしている場合、他家から見て x がどう見えるか
//! ```
//!
//! で意味が違うため、`R/T` を Ron opportunity へ持ち込まない。放銃率でもロン確率でもない値を
//! ロン確率の代用にしない。
//!
//! # 評価時点は打牌後の公開状態
//!
//! 公開 safety は、通常打牌 selection が選んだ打牌を河へ置いた**直後**の公開状態に対して評価する。
//!
//! ```text
//! 現在の GameContext
//! + selected discard を自分の河へ移す (GameContext::after_own_discard)
//! ↓
//! 打牌後の公開状態
//! ↓
//! 既存 Defense helper (is_genbutsu_for / suited_safety_evidence_for_players /
//!                      honor_safety_rank / visible_count_of)
//! ```
//!
//! 待ちとロン可否 ([`TenpaiWaitAvailability`]) も打牌後の状態なので、両者の評価時点が揃う。
//! 宣言牌が作るスジと現物は、この打牌後の河を既存 helper が読むことでそのまま反映される。この層に
//! 「宣言牌が 1s ならスジを1本足す」のような safety rule は書かない。
//!
//! 見え枚数 (壁・字牌の見え枚数) は打牌で変わらない。`visible_tiles` は自分の手牌を既に含むので、
//! 同じ物理牌が手牌から河へ移っても枚数は同じになる。同じ post-discard 状態を既存 helper へ通した
//! 結果そのものであり、打牌前の値を別に持っているわけではない。
//!
//! # ロンできない待ちは unavailable
//!
//! 目的が Ron opportunity なので、実際にロンできない局面では 0 として扱わず診断ごと `None` に
//! する。フリテンでも公開 safety 自体は計算できるが、ロン不能な待ちを確率候補のように並べない。
//!
//! ```text
//! can_ron = Some(true)  → Ron opportunity あり
//! can_ron = Some(false) → unavailable (フリテン)
//! can_ron = None        → unavailable (ロン可否 unknown を非フリテンと推測しない)
//! reach illegal         → 待ちは並べるが reach public safety は unavailable
//! ```
//!
//! # diagnostics only
//!
//! production の Reach / Damaten 判断も打牌選択も変更しない。構築するのは診断が有効な経路だけで、
//! この診断のために safety 評価も threat 分類も追加探索も production へ入れない。External threats は
//! 押し引きが既に構築した classification を借りるだけで、分類し直さない。

use bot_logic::{EffectiveAcceptance, TenpaiWaitAvailability, TileId, TileType};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::defense::{
    HonorSafetyRank, SuitedSafetyEvidence, honor_safety_rank, is_genbutsu_for,
    suited_safety_evidence_for_players, visible_count_of,
};
use crate::open_hand_defense::high_open_hand_threat_players;
use crate::open_hand_threat::OpenHandThreatAssessment;

// リーチ宣言は他家へ公開される。ダマでは公開されない。どちらも局面に依らない構造上の事実。
const REACH_DECLARATION_VISIBLE: bool = true;
const DAMATEN_DECLARATION_VISIBLE: bool = false;

/// 現在聴牌の Ron opportunity を表す structural facts。
///
/// ロン確率もその代用値も持たない。実際にロンできる (`can_ron() == Some(true)`) 局面だけで構築し、
/// フリテンとロン可否 unknown では診断ごと `None` になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RonOpportunityDiagnostic {
    /// 生きている待ち牌種ごとの facts。残枚数 0 の牌種は含まない。
    pub waits: Vec<RonOpportunityWaitDiagnostic>,
    /// 局面全体の他家 threat。既存 classification そのもので、確率へは変換しない。
    pub external_threats: RonOpportunityExternalThreats,
}

/// 生きている待ち牌1種の structural facts。
///
/// 赤5 / 黒5は同じ [`TileType`] として1件にまとめ、物理 variant ごとの打点とは分ける。打点の
/// variant 分割は Ron baseline 側が持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RonOpportunityWaitDiagnostic {
    pub tile: TileType,
    /// 見え牌を差し引いた残枚数。既存の打牌後受け入れが持つ値そのもの。
    pub live_copies: u8,
    /// 今リーチを宣言した場合の公開 safety evidence。
    ///
    /// リーチが合法でない場合と、打牌後の公開状態を組み立てられない場合 (選んだ打牌が無い・
    /// 自分の席を特定できない) は `None` (unavailable)。
    pub reach_public_safety: Option<ReachPublicSafetyEvidence>,
    /// ダマのまま続けた場合に、他家へこちらのテンパイが宣言されるか。常に `false`。
    ///
    /// 「ダマなら安全牌評価が無効」という意味ではなく、他家がこちらの待ちに対する防御を始める
    /// 公開トリガーが無い、という事実だけを表す。ダマ側には Reach と同じ safety rank を付けない。
    pub damaten_declaration_visible: bool,
}

/// 自分がリーチを宣言した場合に、その待ち牌が他家から見て公開情報上どう見えるかの evidence。
///
/// 他家が実際にその牌を切る確率ではない。宣言牌を河へ置いた直後の公開状態へ既存 Defense helper を
/// 適用した観測値そのもので、新しい safety rank も係数も作らない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachPublicSafetyEvidence {
    /// リーチ宣言が他家へ公開されるか。リーチなので常に `true`。
    pub declaration_visible: bool,
    /// 宣言牌まで含めた自分の河から見て、その待ち牌が現物になるか。
    ///
    /// 判定は既存 hard-safety helper ([`is_genbutsu_for`]) を打牌後の公開状態へ通すだけで、
    /// 現物判定をここで実装し直さない。
    ///
    /// 現在の和了可能待ちが自分の河にあれば恒常フリテンになるので、この診断が構築される局面
    /// では `false` になる。待ちとロン可否も同じ打牌後の状態なので、既存 furiten semantics と
    /// 矛盾する値にはならない。
    pub genbutsu: bool,
    /// 数牌の場合の既存 Defense evidence。字牌では `None`。
    ///
    /// 壁は見え牌由来、スジは自分の河由来で、どちらも打牌後の公開状態へ通した既存
    /// [`suited_safety_evidence_for_players`] そのもの。宣言牌が作るスジもこの河から出る。
    /// Defense selection の comparator は呼ばない。
    pub suited: Option<SuitedSafetyEvidence>,
    /// 字牌の場合の既存 Defense evidence。数牌では `None`。
    pub honor: Option<HonorPublicSafetyEvidence>,
}

/// 字牌の待ちについての公開 safety evidence。
///
/// 打牌後の公開状態へ通した既存の字牌 safety ([`honor_safety_rank`]) と見え枚数
/// ([`visible_count_of`]) そのもので、役牌価値のような別軸はここでは持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HonorPublicSafetyEvidence {
    pub rank: HonorSafetyRank,
    pub visible_count: u8,
}

/// Ron opportunity に影響し得る他家 threat の structural facts。
///
/// 他家が別の threat に対して降りている可能性は、相手の打牌 simulation ではなく既存
/// classification の観測値だけで表す。確率へは変換しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RonOpportunityExternalThreats {
    /// 他家リーチ者の席。[`GameContext::reached_opponents`] そのもの。
    pub reached_opponents: Vec<usize>,
    /// High OpenHandThreat と分類された席。既存 classification そのもので、分類し直さない。
    pub high_open_hand_targets: Vec<usize>,
}

impl RonOpportunityExternalThreats {
    pub fn reached_opponent_count(&self) -> usize {
        self.reached_opponents.len()
    }

    pub fn high_open_hand_target_count(&self) -> usize {
        self.high_open_hand_targets.len()
    }
}

/// Ron opportunity 診断を組み立てるための材料。
///
/// どれも既に構築済みの値の借用で、この診断のために計算し直すものは無い。
pub(crate) struct RonOpportunityInputs<'a> {
    pub context: &'a GameContext,
    /// 現在局面の合法手に [`LegalAction::Reach`](crate::action::LegalAction::Reach) があるか。
    /// Reach / Damaten comparison と同じ fact を共有する。
    pub reach_legal: bool,
    /// 選んだ打牌後のテンパイの待ちとロン可否。既存診断そのもの。
    pub wait: &'a TenpaiWaitAvailability,
    /// 選んだ打牌後の受け入れ。待ち牌種ごとの残枚数の source of truth で、見え牌を数え直さない。
    pub acceptance: &'a EffectiveAcceptance,
    /// 通常打牌 selection が選んだ合法 Dahai。Reach / Damaten comparison が載せるものと同じ
    /// action で、どの牌を切るかをここで推測しない。
    ///
    /// 公開 safety を打牌後の状態で評価するために、この物理牌を自分の河へ移した projection を
    /// 作る。赤5 / 黒5の区別も selection が選んだ物理牌そのままにする。
    pub selected_discard: Option<&'a LegalAction>,
    /// 押し引きが既に構築した全4席分の OpenHandThreat classification。
    pub open_hand_threats: &'a [OpenHandThreatAssessment; 4],
}

/// 現在の待ちについて Ron opportunity の structural facts を組み立てる。
///
/// 実際にロンできる局面だけで構築する。フリテンとロン可否 unknown では 0 として扱わず `None`
/// (unavailable) にする。呼ぶのは診断が有効な経路だけで、production の判断は変わらない。
pub(crate) fn diagnose_ron_opportunity(
    inputs: RonOpportunityInputs<'_>,
) -> Option<RonOpportunityDiagnostic> {
    let RonOpportunityInputs {
        context,
        reach_legal,
        wait,
        acceptance,
        selected_discard,
        open_hand_threats,
    } = inputs;

    if wait.can_ron() != Some(true) {
        return None;
    }

    // 公開 safety の評価時点を打牌後へ揃えるための projection。リーチが合法でない局面では
    // 組み立てない。
    let public_state = reach_legal
        .then(|| reach_declaration_public_state(context, selected_discard?))
        .flatten();

    let waits = wait
        .live_waits
        .iter()
        .filter_map(|&tile| {
            Some(RonOpportunityWaitDiagnostic {
                tile,
                live_copies: live_copies_of(tile, acceptance)?,
                reach_public_safety: public_state
                    .as_ref()
                    .and_then(|public| reach_public_safety(tile, public)),
                damaten_declaration_visible: DAMATEN_DECLARATION_VISIBLE,
            })
        })
        .collect();

    Some(RonOpportunityDiagnostic {
        waits,
        external_threats: RonOpportunityExternalThreats {
            reached_opponents: context.reached_opponents(),
            high_open_hand_targets: high_open_hand_threat_players(open_hand_threats),
        },
    })
}

// 待ち牌種の残枚数。既存の打牌後受け入れが持つ値そのままで、見え牌を別経路で数え直さない。
// 受け入れに無い牌種と残枚数 0 の牌種は待ちとして並べない。
fn live_copies_of(tile: TileType, acceptance: &EffectiveAcceptance) -> Option<u8> {
    acceptance
        .tiles
        .iter()
        .find(|accepted| accepted.tile == tile)
        .map(|accepted| accepted.remaining)
        .filter(|&remaining| remaining > 0)
}

// リーチ宣言牌を自分の河へ置いた直後の公開状態。既存 projection をそのまま使い、河の組み立てを
// ここで書き直さない。選んだ action が Dahai でない場合と自分の席を特定できない場合は推測せず
// None にして、公開 safety を unavailable にする。
fn reach_declaration_public_state(
    context: &GameContext,
    selected_discard: &LegalAction,
) -> Option<GameContext> {
    let LegalAction::Dahai { tile } = selected_discard else {
        return None;
    };
    let discard: TileId = *tile;
    context.after_own_discard(discard)
}

// リーチを宣言した場合の公開 safety evidence。`public` は宣言牌を河へ置いた後の公開状態で、
// 現物もスジもその河から出る。
//
// スジは他家が自分の河から読む情報なので、対象 player 集合は自分の席だけにする。壁は見え牌由来で
// 対象 player に依らない。どちらも既存 helper の結果そのままで、rank も係数もここでは作らない。
fn reach_public_safety(tile: TileType, public: &GameContext) -> Option<ReachPublicSafetyEvidence> {
    let own_seat = usize::from(public.player_id()?);

    Some(ReachPublicSafetyEvidence {
        declaration_visible: REACH_DECLARATION_VISIBLE,
        genbutsu: is_genbutsu_for(tile, own_seat, public),
        suited: suited_safety_evidence_for_players(tile, &[own_seat], public),
        honor: honor_safety_rank(tile, public).map(|rank| HonorPublicSafetyEvidence {
            rank,
            visible_count: visible_count_of(tile, public),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use bot_logic::{
        FixedMeldCount, HistoryFuritenFacts, OwnDiscards, TileCounts,
        calculate_acceptance_with_fixed_melds_and_visible_tiles, structural_acceptance_tile_types,
        tenpai_wait_availability,
    };

    use crate::defense::{SujiSafetyRank, WallRank, suited_safety_rank_for_players, wall_rank};
    use crate::meld::{Meld, MeldKind};
    use crate::open_hand_defense::high_open_hand_threat_players_from_context;
    use crate::open_hand_threat::classify_open_hand_threats;
    use crate::threat::player_threat_facts_from_context;

    // 打牌後の門前13枚。123m 456m 789m 123p + 単騎牌で、単騎牌がそのまま待ちになる。
    const SUITED_WAIT_HAND: [&str; 13] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4s",
    ];
    const HONOR_WAIT_HAND: [&str; 13] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "E",
    ];
    const RED_WAIT_HAND: [&str; 13] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
    ];

    // 待ちにもスジにも壁にも関係しない打牌。
    const NEUTRAL_DISCARD: &str = "9p";
    // 4s のスジ根拠になる打牌。宣言後の河へ入って初めて片スジになる。
    const SUJI_PARTNER_DISCARD: &str = "1s";

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).expect("牌種として読める")
    }

    // 同じ物理牌を手牌・河・副露で使い回さないよう、1枚ずつ払い出す。
    struct TileIdSource {
        used: [bool; TileId::COUNT],
    }

    impl TileIdSource {
        fn new() -> Self {
            Self {
                used: [false; TileId::COUNT],
            }
        }

        fn tiles(&mut self, strings: &[&str]) -> Vec<TileId> {
            strings.iter().map(|s| self.tile(s)).collect()
        }

        fn tile(&mut self, s: &str) -> TileId {
            let id = TileId::copies(tile(s))
                .find(|id| !id.is_red() && !self.used[id.index()])
                .expect("同じ物理牌を使い回していない");
            self.used[id.index()] = true;
            id
        }
    }

    struct CaseSpec<'a> {
        /// 打牌後の門前13枚。そのままテンパイ形になる。
        hand: &'a [&'a str],
        /// 通常打牌 selection が選ぶ打牌。ツモ牌として持ち、これを切って `hand` のテンパイになる。
        discard: &'a str,
        /// 打牌前の自分の河。
        own_discards: &'a [&'a str],
        /// リーチしている他家の席。
        reached_opponents: &'a [usize],
        /// 他家の副露。High OpenHandThreat の分類に使う。
        opponent_melds: &'a [(usize, &'a [&'a str])],
    }

    impl Default for CaseSpec<'_> {
        fn default() -> Self {
            Self {
                hand: &SUITED_WAIT_HAND,
                discard: NEUTRAL_DISCARD,
                own_discards: &[],
                reached_opponents: &[],
                opponent_melds: &[],
            }
        }
    }

    struct Case {
        /// 打牌前の局面。
        context: GameContext,
        /// 打牌を河へ置いた直後の公開状態。期待値の source of truth。
        public: GameContext,
        selected_discard: LegalAction,
        wait: TenpaiWaitAvailability,
        acceptance: EffectiveAcceptance,
    }

    impl CaseSpec<'_> {
        fn build(&self) -> Case {
            let mut source = TileIdSource::new();
            let hand_tiles = source.tiles(self.hand);
            let drawn_tile = source.tile(self.discard);
            let own_discards = source.tiles(self.own_discards);
            let melds: Vec<(usize, Vec<TileId>)> = self
                .opponent_melds
                .iter()
                .map(|(player, meld)| (*player, source.tiles(meld)))
                .collect();

            let visible: Vec<TileId> = hand_tiles
                .iter()
                .chain(std::iter::once(&drawn_tile))
                .chain(own_discards.iter())
                .chain(melds.iter().flat_map(|(_, meld)| meld.iter()))
                .copied()
                .collect();

            let mut discards: [Vec<TileId>; 4] = Default::default();
            discards[0] = own_discards;
            let mut player_melds: [Vec<Meld>; 4] = Default::default();
            for (player, meld) in melds {
                let called_tile = meld[0];
                player_melds[player].push(Meld::new(MeldKind::Pon, meld, Some(called_tile)));
            }
            let mut reached = [false; 4];
            for &player in self.reached_opponents {
                reached[player] = true;
            }

            let context = GameContext::from_parts_with_melds(
                Some(drawn_tile),
                hand_tiles.clone(),
                Vec::new(),
                Some(tile("E")),
                Some(tile("S")),
                visible,
                Some(0),
                Some(3),
                discards,
                reached,
                player_melds,
            )
            .with_history_furiten_facts(HistoryFuritenFacts {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            });
            let public = context
                .after_own_discard(drawn_tile)
                .expect("自分の席が分かっている");

            // 待ちとロン可否も打牌後の状態で求め、公開 safety と評価時点を揃える。
            let counts = TileCounts::from_tiles(hand_tiles.iter().copied());
            let acceptance = calculate_acceptance_with_fixed_melds_and_visible_tiles(
                &counts,
                FixedMeldCount::NONE,
                context.visible_tiles(),
            );
            let wait = tenpai_wait_availability(
                &acceptance,
                &structural_acceptance_tile_types(&counts),
                &OwnDiscards::from_optional_river(public.own_discards()),
                context.history_furiten_after_own_discard(),
            )
            .expect("テンパイしている");

            Case {
                context,
                public,
                selected_discard: LegalAction::Dahai { tile: drawn_tile },
                wait,
                acceptance,
            }
        }
    }

    impl Case {
        fn diagnose(&self, reach_legal: bool) -> Option<RonOpportunityDiagnostic> {
            self.diagnose_with(reach_legal, Some(&self.selected_discard))
        }

        fn diagnose_with(
            &self,
            reach_legal: bool,
            selected_discard: Option<&LegalAction>,
        ) -> Option<RonOpportunityDiagnostic> {
            diagnose_ron_opportunity(RonOpportunityInputs {
                context: &self.context,
                reach_legal,
                wait: &self.wait,
                acceptance: &self.acceptance,
                selected_discard,
                open_hand_threats: &classify_open_hand_threats(&player_threat_facts_from_context(
                    &self.context,
                )),
            })
        }

        fn opportunity(&self) -> RonOpportunityDiagnostic {
            self.diagnose(true).expect("Ron opportunity を構築している")
        }

        fn safety(&self, tile: TileType) -> ReachPublicSafetyEvidence {
            wait_of(&self.opportunity(), tile)
                .reach_public_safety
                .expect("リーチが合法なら公開 safety を評価する")
        }
    }

    fn wait_of(
        opportunity: &RonOpportunityDiagnostic,
        tile: TileType,
    ) -> RonOpportunityWaitDiagnostic {
        *opportunity
            .waits
            .iter()
            .find(|wait| wait.tile == tile)
            .unwrap_or_else(|| panic!("{} の待ちがある", tile.to_mjai_string()))
    }

    // ---- 待ちの範囲 ----

    #[test]
    fn only_the_live_waits_of_the_current_tenpai_are_listed() {
        let case = CaseSpec::default().build();
        let opportunity = case.opportunity();

        assert_eq!(case.wait.live_waits, [tile("4s")]);
        assert_eq!(
            opportunity
                .waits
                .iter()
                .map(|wait| wait.tile)
                .collect::<Vec<_>>(),
            case.wait.live_waits
        );
    }

    #[test]
    fn a_wait_without_live_copies_is_not_listed() {
        // 待ちの 4s が4枚とも見えている局面。ツモ側の受け入れから外れる牌は並べない。
        let case = CaseSpec {
            own_discards: &["4s", "4s", "4s"],
            ..CaseSpec::default()
        }
        .build();

        assert!(case.wait.live_waits.is_empty());
        // 自分の河に待ちがあるのでフリテンになり、Ron opportunity ごと unavailable。
        assert_eq!(case.wait.can_ron(), Some(false));
        assert_eq!(case.diagnose(true), None);
    }

    #[test]
    fn the_live_copies_come_from_the_existing_acceptance() {
        // 見え牌を別経路で数え直さず、打牌後受け入れが持つ残枚数そのものを載せる。
        let case = CaseSpec {
            own_discards: &["4p"],
            ..CaseSpec::default()
        }
        .build();
        let opportunity = case.opportunity();

        assert_eq!(
            opportunity
                .waits
                .iter()
                .map(|wait| (wait.tile, wait.live_copies))
                .collect::<Vec<_>>(),
            case.acceptance
                .tiles
                .iter()
                .map(|accepted| (accepted.tile, accepted.remaining))
                .collect::<Vec<_>>()
        );
        assert_eq!(wait_of(&opportunity, tile("4s")).live_copies, 3);
    }

    // ---- 評価時点 ----

    #[test]
    fn the_reach_declaration_tile_is_reflected_in_the_public_suji() {
        // 宣言牌の 1s は 4s のスジ根拠になる。宣言前の河には無いので、打牌前の状態で評価すると
        // 片スジを取り落とす。
        let case = CaseSpec {
            discard: SUJI_PARTNER_DISCARD,
            ..CaseSpec::default()
        }
        .build();
        let evidence = case
            .safety(tile("4s"))
            .suited
            .expect("数牌の evidence がある");

        // 期待値は打牌後の公開状態へ既存 helper を通した結果そのもの。
        assert_eq!(
            Some(evidence),
            suited_safety_evidence_for_players(tile("4s"), &[0], &case.public)
        );
        // 打牌前の状態とは違う評価になる。
        assert_ne!(
            Some(evidence),
            suited_safety_evidence_for_players(tile("4s"), &[0], &case.context)
        );
        assert_eq!(evidence.suji_rank, SujiSafetyRank::HalfSuji);
        assert_eq!(
            suited_safety_evidence_for_players(tile("4s"), &[0], &case.context)
                .map(|before| before.suji_rank),
            Some(SujiSafetyRank::NoSuji)
        );
    }

    #[test]
    fn the_public_visible_count_uses_the_post_discard_state() {
        // 壁と字牌の見え枚数も打牌後の公開状態で評価する。宣言牌は既に自分の手牌として見えて
        // いるので、河へ移っても見え枚数は変わらない。
        let case = CaseSpec {
            discard: "3s",
            ..CaseSpec::default()
        }
        .build();
        let evidence = case
            .safety(tile("4s"))
            .suited
            .expect("数牌の evidence がある");

        assert_eq!(
            Some(evidence),
            suited_safety_evidence_for_players(tile("4s"), &[0], &case.public)
        );
        assert_eq!(evidence.wall_rank, wall_rank(tile("4s"), &case.public));
        assert_eq!(
            visible_count_of(tile("3s"), &case.public),
            visible_count_of(tile("3s"), &case.context)
        );
    }

    // ---- 公開 safety ----

    #[test]
    fn a_suited_wait_reuses_the_existing_suited_evidence() {
        // 自分の河に 1s があるので、他家から見て 4s は片スジ。既存 evidence そのものを載せる。
        let case = CaseSpec {
            own_discards: &["1s"],
            ..CaseSpec::default()
        }
        .build();
        let safety = case.safety(tile("4s"));

        let evidence = safety.suited.expect("数牌の evidence がある");
        assert_eq!(
            Some(evidence),
            suited_safety_evidence_for_players(tile("4s"), &[0], &case.public)
        );
        assert_eq!(evidence.suji_rank, SujiSafetyRank::HalfSuji);
        assert_eq!(evidence.wall_rank, WallRank::NoWall);
        assert_eq!(
            Some(evidence.legacy_rank()),
            suited_safety_rank_for_players(tile("4s"), &[0], &case.public)
        );
        assert_eq!(safety.honor, None);
    }

    #[test]
    fn an_honor_wait_reuses_the_existing_honor_safety() {
        // 東を捨てた恒常フリテンでは Ron opportunity ごと unavailable。
        let furiten = CaseSpec {
            hand: &HONOR_WAIT_HAND,
            own_discards: &["E"],
            ..CaseSpec::default()
        }
        .build();
        assert_eq!(furiten.diagnose(true), None);

        let case = CaseSpec {
            hand: &HONOR_WAIT_HAND,
            ..CaseSpec::default()
        }
        .build();
        let safety = case.safety(tile("E"));

        let honor = safety.honor.expect("字牌の evidence がある");
        assert_eq!(Some(honor.rank), honor_safety_rank(tile("E"), &case.public));
        assert_eq!(honor.rank, HonorSafetyRank::OneVisible);
        assert_eq!(
            honor.visible_count,
            visible_count_of(tile("E"), &case.public)
        );
        assert_eq!(safety.suited, None);
    }

    #[test]
    fn the_genbutsu_fact_matches_the_existing_hard_safety_helper() {
        let case = CaseSpec::default().build();
        let safety = case.safety(tile("4s"));

        assert!(!safety.genbutsu);
        assert_eq!(
            safety.genbutsu,
            is_genbutsu_for(tile("4s"), 0, &case.public)
        );
        assert!(safety.declaration_visible);
    }

    #[test]
    fn damaten_only_carries_the_declaration_fact() {
        // ダマには Reach と同じ safety rank を付けず、宣言が公開されないという事実だけを持つ。
        let case = CaseSpec::default().build();

        for wait in &case.opportunity().waits {
            assert!(!wait.damaten_declaration_visible);
        }
    }

    #[test]
    fn an_illegal_reach_keeps_the_reach_public_safety_unavailable() {
        let case = CaseSpec::default().build();
        let opportunity = case.diagnose(false).expect("ダマ側は評価できる");

        for wait in &opportunity.waits {
            assert_eq!(wait.reach_public_safety, None);
            assert!(!wait.damaten_declaration_visible);
            assert!(wait.live_copies > 0);
        }
    }

    #[test]
    fn a_missing_selected_discard_keeps_the_reach_public_safety_unavailable() {
        // どの牌を切るか分からなければ打牌後の公開状態を組み立てられない。打牌前の状態で
        // 代用せず unavailable にする。
        let case = CaseSpec::default().build();
        let opportunity = case.diagnose_with(true, None).expect("ダマ側は評価できる");

        for wait in &opportunity.waits {
            assert_eq!(wait.reach_public_safety, None);
        }
    }

    // ---- 赤5 / 黒5 ----

    #[test]
    fn the_red_and_black_five_share_one_structural_safety_evidence() {
        // 5s 単騎。赤5も黒5も同じ牌種なので、structural safety は1件を共有する。
        let case = CaseSpec {
            hand: &RED_WAIT_HAND,
            ..CaseSpec::default()
        }
        .build();
        let opportunity = case.opportunity();

        assert_eq!(opportunity.waits.len(), 1);
        let wait = opportunity.waits[0];
        assert_eq!(wait.tile, tile("5s"));
        assert_eq!(wait.live_copies, 3);
        assert_eq!(
            wait.reach_public_safety
                .and_then(|safety| safety.suited)
                .map(|evidence| evidence.legacy_rank()),
            suited_safety_rank_for_players(tile("5s"), &[0], &case.public)
        );
    }

    // ---- Ron availability ----

    #[test]
    fn a_furiten_tenpai_has_no_ron_opportunity() {
        let case = CaseSpec {
            own_discards: &["4s"],
            ..CaseSpec::default()
        }
        .build();

        assert_eq!(case.wait.can_ron(), Some(false));
        assert_eq!(case.diagnose(true), None);
    }

    #[test]
    fn an_unknown_ron_availability_is_not_guessed() {
        let case = CaseSpec::default().build();
        let wait = TenpaiWaitAvailability {
            history_furiten: HistoryFuritenFacts::default(),
            ..case.wait.clone()
        };

        assert_eq!(wait.can_ron(), None);
        assert_eq!(
            diagnose_ron_opportunity(RonOpportunityInputs {
                context: &case.context,
                reach_legal: true,
                wait: &wait,
                acceptance: &case.acceptance,
                selected_discard: Some(&case.selected_discard),
                open_hand_threats: &classify_open_hand_threats(&player_threat_facts_from_context(
                    &case.context
                )),
            }),
            None
        );
    }

    // ---- Defense exact model と混ぜない ----

    #[test]
    fn the_reach_public_safety_does_not_depend_on_the_defense_exact_model() {
        // Defense の exact R/T は「自分が牌 x を切った場合に相手が x でロン可能な hidden-hand
        // state の weight」で、こちらの待ちが他家からどう見えるかとは意味が違う。他家リーチの
        // 有無で公開 safety evidence を変えない。
        let base = CaseSpec::default().build();
        let reached = CaseSpec {
            reached_opponents: &[1],
            ..CaseSpec::default()
        }
        .build();

        assert_eq!(base.safety(tile("4s")), reached.safety(tile("4s")));
        // リーチ者の存在は external threats にだけ現れる。
        assert_eq!(
            reached.opportunity().external_threats.reached_opponents,
            vec![1]
        );
    }

    // ---- 持たない値 ----

    #[test]
    fn the_ron_opportunity_only_carries_structural_facts() {
        // 確率・score・winner を持たない。そういうフィールドが増えるとこの構築が壊れる。
        let opportunity = RonOpportunityDiagnostic {
            waits: vec![RonOpportunityWaitDiagnostic {
                tile: tile("4s"),
                live_copies: 3,
                reach_public_safety: Some(ReachPublicSafetyEvidence {
                    declaration_visible: true,
                    genbutsu: false,
                    suited: Some(SuitedSafetyEvidence {
                        wall_rank: WallRank::NoWall,
                        suji_rank: SujiSafetyRank::NoSuji,
                    }),
                    honor: None,
                }),
                damaten_declaration_visible: false,
            }],
            external_threats: RonOpportunityExternalThreats {
                reached_opponents: Vec::new(),
                high_open_hand_targets: Vec::new(),
            },
        };

        assert_eq!(opportunity.external_threats.reached_opponent_count(), 0);
        assert_eq!(
            opportunity.external_threats.high_open_hand_target_count(),
            0
        );
    }

    // ---- 他家 threat ----

    #[test]
    fn the_external_reach_threat_matches_the_context() {
        let case = CaseSpec {
            reached_opponents: &[1, 3],
            ..CaseSpec::default()
        }
        .build();
        let threats = case.opportunity().external_threats;

        assert_eq!(threats.reached_opponents, case.context.reached_opponents());
        assert_eq!(threats.reached_opponents, vec![1, 3]);
        assert_eq!(threats.reached_opponent_count(), 2);
    }

    #[test]
    fn the_high_open_hand_targets_match_the_existing_classifier() {
        // 3副露で High になる相手。分類は既存 classifier が source of truth。
        let case = CaseSpec {
            opponent_melds: &[
                (2, &["W", "W", "W"]),
                (2, &["N", "N", "N"]),
                (2, &["P", "P", "P"]),
            ],
            ..CaseSpec::default()
        }
        .build();
        let threats = case.opportunity().external_threats;

        assert_eq!(
            threats.high_open_hand_targets,
            high_open_hand_threat_players_from_context(&case.context)
        );
        assert_eq!(threats.high_open_hand_targets, vec![2]);
        assert_eq!(threats.high_open_hand_target_count(), 1);
        assert!(threats.reached_opponents.is_empty());
    }
}
