//! 現在聴牌をダマで継続した場合の、次の1巡分を観測するための診断層。
//!
//! ```text
//! 現在聴牌 → 非和了牌を1枚ツモ → 既存 selector の最善打牌 → 再び聴牌
//! ```
//!
//! という枝だけを取り出す。探索も打牌選択も打点計算もこの層は持たず、既存の2手先評価
//! ([`LookaheadDiagnostic`]) が構築済みの枝と、その枝を評価済みの将来打点
//! ([`ProspectiveLookaheadDiagnostic`]) を絞り込むだけである。
//!
//! # 継続枝に含めるもの
//!
//! 待ちが実際に変わる枝 (手変わり) だけでなく、ツモった牌をそのまま切って元の聴牌・元の待ちを
//! 維持する枝も継続枝として扱う。「今すぐリーチする」と「ダマで継続する」を比べるには、次の
//! 1巡で待ちが変わらない場合の価値も同じ枝集合の中に必要になるためである。枝を待ちの変化で
//! 分類 (据え置き / 待ち改善 / 打点改善) することは、この層ではまだ行わない。
//!
//! # 枝の出どころ
//!
//! 非和了ツモは既存2手先評価の [`DrawTransition::SameShanten`] そのものである。現在打牌後が
//! 聴牌の候補では、和了牌は向聴数を下げるので [`DrawTransition::Progress`] に分類され、向聴数を
//! 維持する牌 = 非和了牌になる。継続枝からの和了牌の除外は、この既存分類をそのまま使うだけで、
//! この層が和了牌を判定し直すことはない。
//!
//! 次打牌も既存の打牌評価と既存 comparator が選んだ [`DrawVariantLookaheadDiagnostic::next_discard`]
//! そのもので、向聴・受け入れ・赤5・ドラ・形・将来打点のどの比較もここで作り直さない。その
//! 打牌後が再び聴牌の枝だけを継続成立として扱い、聴牌に戻らない枝は含めない。
//!
//! horizon は「1ツモ → 1打牌 → 次の聴牌」で必ず打ち切る。2回目の非和了ツモは追わない。
//!
//! # 対象局面
//!
//! 自分が未リーチと確定している局面の、現在打牌後が聴牌の候補だけを対象にする。既にリーチ
//! していればダマで継続する選択肢が無いので探索対象にせず、自分の席が分からず未リーチかどうかを
//! 判断できない局面でも未リーチだと推測しない。どちらも [`TenpaiContinuationDiagnostic`] を
//! 構築しない (`None`)。
//!
//! # 打牌選択への接続
//!
//! 現時点では diagnostics 専用で、打牌選択には接続していない。
//! Σ(残枚数 × 継続後の打点) のような集計値も、現在聴牌の
//! [`OffenseValue`](crate::offense_value::OffenseValue) と比較する係数も threshold も持たない。
//! 継続枝と現在聴牌の攻撃打点はまだ同じ確率模型・同じ horizon に揃っていないため、単純に
//! 足したり比べたりできる量ではない。

use bot_logic::{
    DiscardEvaluation, DiscardLookaheadDiagnostic, DrawLookaheadDiagnostic, DrawTransition,
    DrawVariantLookaheadDiagnostic, EffectiveAcceptanceTile, LookaheadDiagnostic, TileId, TileType,
};

use crate::context::GameContext;
use crate::offense_value::TenpaiOffenseMode;
use crate::prospective_value::{
    ProspectiveDiscardValue, ProspectiveDrawValue, ProspectiveDrawVariantValue,
    ProspectiveLookaheadDiagnostic, ProspectiveTenpaiValue, ProspectiveWaitValue,
};

// テンパイの向聴数。
const TENPAI_SHANTEN: i8 = 0;

/// 現在聴牌候補をダマで継続した場合の診断。
///
/// `candidates` は現在打牌後が聴牌の候補だけを、既存2手先評価と同じ順序で並べる。打牌選択にも
/// 押し引きにもリーチ判断にも使わない解析専用の情報で、構築の有無は選択結果を変えない。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TenpaiContinuationDiagnostic {
    pub candidates: Vec<TenpaiContinuationCandidate>,
}

impl TenpaiContinuationDiagnostic {
    pub fn candidate(&self, discard: TileType) -> Option<&TenpaiContinuationCandidate> {
        self.candidates
            .iter()
            .find(|candidate| candidate.discard == discard)
    }
}

/// 現在聴牌候補1件分の継続枝。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenpaiContinuationCandidate {
    /// 現在の打牌候補の牌種。この打牌後が現在聴牌になる。
    pub discard: TileType,
    /// 現在聴牌の待ち。既存打牌評価の受け入れそのもので、この診断のために求め直さない。
    pub current_wait: Vec<EffectiveAcceptanceTile>,
    /// 継続が成立した枝。非和了ツモの物理牌 variant 単位で並ぶ。
    ///
    /// 待ちが変わる枝と、ツモ切りで元の待ちを維持する枝の両方を含む。
    pub branches: Vec<TenpaiContinuationBranch>,
}

impl TenpaiContinuationCandidate {
    /// 現在聴牌の待ちの残枚数合計。
    pub fn current_wait_remaining(&self) -> u32 {
        self.current_wait
            .iter()
            .map(|tile| u32::from(tile.remaining))
            .sum()
    }

    /// 継続が成立した枝の残枚数合計。
    ///
    /// 期待値でも和了率でもなく、成立枝の物理牌が何枚あるかを数えただけの観測値。継続後の
    /// 打点で重み付けした集計値はこの段階では作らない。
    pub fn branch_remaining(&self) -> u32 {
        self.branches
            .iter()
            .map(|branch| u32::from(branch.remaining()))
            .sum()
    }
}

/// 成立した継続枝1件。
///
/// 枝の中身は既存 prospective diagnostic の物理牌 variant ([`ProspectiveDrawVariantValue`])
/// そのもので、この型はそれが「どの非和了ツモ牌種の枝か」を添えるだけ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenpaiContinuationBranch {
    /// 非和了ツモの牌種。
    pub draw: TileType,
    /// その牌種全体の残枚数。`variant` はこのうち赤5 / 黒5どちらか一方の枚数を持つ。
    pub draw_remaining: u8,
    /// 物理牌 variant 単位の枝。次打牌・将来打点は既存診断が持つ値そのもの。
    pub variant: ProspectiveDrawVariantValue,
}

impl TenpaiContinuationBranch {
    /// 仮想的にツモった物理牌。赤5と黒5は別の枝になる。
    pub fn drawn_tile(&self) -> TileId {
        self.variant.drawn_tile
    }

    /// この物理牌 variant の残枚数。見え牌を差し引いた枚数そのもの。
    pub fn remaining(&self) -> u8 {
        self.variant.remaining
    }

    /// 既存 selector が選んだ次打牌。ツモ切りで元の待ちを維持する枝ではツモ牌と同じ牌種になる。
    pub fn next_discard(&self) -> Option<TileType> {
        self.variant.next_discard
    }

    /// 継続後のテンパイを production のリーチ判断が評価した攻撃モード。
    ///
    /// 自分の席が分からず既リーチかどうかを判断できない枝は
    /// [`TenpaiOffenseMode::Unknown`]、テンパイを評価できなかった枝は `None`。
    pub fn offense_mode(&self) -> Option<TenpaiOffenseMode> {
        self.evaluated().map(|tenpai| tenpai.mode)
    }

    /// 継続後の待ちと残枚数。評価できなかった枝は空。
    pub fn waits(&self) -> &[ProspectiveWaitValue] {
        self.evaluated().map_or(&[], ProspectiveTenpaiValue::waits)
    }

    /// 継続後の待ちの残枚数合計。
    pub fn wait_remaining(&self) -> u32 {
        self.waits()
            .iter()
            .map(|wait| u32::from(wait.remaining))
            .sum()
    }

    /// 継続後の待ちの牌種数。
    pub fn wait_type_count(&self) -> usize {
        self.waits().len()
    }

    /// production の将来打点。確定できない枝は `None`。
    ///
    /// 既存の [`ProductionProspectiveValuator`](crate::prospective_value) が返した値そのもので、
    /// 表示や診断のために求め直さない。
    pub fn prospective_value(&self) -> Option<u64> {
        self.variant.selection_value
    }

    fn evaluated(&self) -> Option<&ProspectiveTenpaiValue> {
        self.variant.outcome.evaluated()
    }
}

/// 構築済みの2手先評価とその将来打点から、現在聴牌候補の継続枝を絞り込む。
///
/// 探索も打牌評価も点数計算も行わず、既に構築済みの枝を選び直すだけ。`evaluations` /
/// `lookahead` / `value` は同じ候補集合から作った同じ順序のものを渡す。対応しない場合は推測せず
/// `None` にする。
///
/// 自分が未リーチと確定していない局面 (既リーチ・自分の席が不明) では `None`。
pub(crate) fn diagnose_tenpai_continuation(
    context: &GameContext,
    evaluations: &[DiscardEvaluation],
    lookahead: &LookaheadDiagnostic,
    value: &ProspectiveLookaheadDiagnostic,
) -> Option<TenpaiContinuationDiagnostic> {
    if context.own_reached() != Some(false) {
        return None;
    }
    if lookahead.candidates.len() != evaluations.len()
        || value.candidates.len() != evaluations.len()
    {
        return None;
    }

    Some(TenpaiContinuationDiagnostic {
        candidates: evaluations
            .iter()
            .zip(&lookahead.candidates)
            .zip(&value.candidates)
            .filter(|((evaluation, candidate), value)| {
                evaluation.min_shanten_after_discard() == TENPAI_SHANTEN
                    && candidate.discard == evaluation.discard
                    && value.discard == evaluation.discard
            })
            .map(|((evaluation, candidate), value)| {
                candidate_continuation(evaluation, candidate, value)
            })
            .collect(),
    })
}

// 現在聴牌候補1件分の継続枝。非和了ツモは既存分類 (same-shanten) そのもので、和了牌の枝
// (Progress) はここへ入らない。
fn candidate_continuation(
    evaluation: &DiscardEvaluation,
    candidate: &DiscardLookaheadDiagnostic,
    value: &ProspectiveDiscardValue,
) -> TenpaiContinuationCandidate {
    TenpaiContinuationCandidate {
        discard: evaluation.discard,
        current_wait: evaluation.acceptance_after_discard.tiles.clone(),
        branches: candidate
            .draws
            .iter()
            .zip(&value.draws)
            .filter(|(draw, value)| {
                draw.transition == DrawTransition::SameShanten && draw.draw == value.draw
            })
            .flat_map(|(draw, value)| draw_branches(draw, value))
            .collect(),
    }
}

// 非和了ツモ1牌種分の枝。既存2手先評価が構造 (次打牌後が聴牌か) を、既存将来打点が値を持つ。
fn draw_branches<'a>(
    draw: &'a DrawLookaheadDiagnostic,
    value: &'a ProspectiveDrawValue,
) -> impl Iterator<Item = TenpaiContinuationBranch> + 'a {
    draw.variants
        .iter()
        .zip(&value.variants)
        .filter(|(variant, value)| {
            variant.drawn_tile == value.drawn_tile && continues_tenpai(variant)
        })
        .map(|(_, value)| TenpaiContinuationBranch {
            draw: draw.draw,
            draw_remaining: draw.remaining,
            variant: value.clone(),
        })
}

// 最善打牌後が再び聴牌になった枝だけが継続成立。打牌候補が1件も無い枝と、聴牌に戻らない枝は
// 成立として扱わない。
fn continues_tenpai(variant: &DrawVariantLookaheadDiagnostic) -> bool {
    variant.next_min_shanten() == Some(TENPAI_SHANTEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::LazyLock;

    use bot_logic::HistoryFuritenFacts;

    use crate::action::LegalAction;
    use crate::discard_selection::{
        DiscardActionSelectionWithDiagnostic, LookaheadDiagnosticScope,
        select_discard_action_with_diagnostic,
    };

    // 123m 456m 789m 123p 東 の門前13枚に南をツモった単騎テンパイ。打 E で南単騎、打 S で東単騎
    // になり、どちらの現在聴牌からも、非和了ツモ1枚とその後の最善打牌でダマ継続できる。
    const HAND: [&str; 13] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "E",
    ];
    const DRAW: &str = "S";

    // 同じ牌種の赤5 / 黒5を取り違えないよう、物理牌を1枚ずつ払い出す。
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
            let red = s.ends_with('r');
            let id = TileId::copies(tile(s))
                .find(|id| id.is_red() == red && !self.used[id.index()])
                .expect("同じ物理牌を使い回していない");
            self.used[id.index()] = true;
            id
        }
    }

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s.trim_end_matches('r')).expect("牌種として読める")
    }

    struct CaseSpec<'a> {
        extra_visible: &'a [&'a str],
        own_reached: bool,
        /// 自分の席。`None` では既リーチかどうかを判断できない。
        player_id: Option<u8>,
        scope: LookaheadDiagnosticScope,
    }

    impl Default for CaseSpec<'_> {
        fn default() -> Self {
            Self {
                extra_visible: &[],
                own_reached: false,
                player_id: Some(0),
                scope: LookaheadDiagnosticScope::Lookahead,
            }
        }
    }

    impl CaseSpec<'_> {
        fn build(self) -> DiscardActionSelectionWithDiagnostic {
            let mut source = TileIdSource::new();
            let hand_tiles = source.tiles(&HAND);
            let drawn_tile = source.tile(DRAW);
            let extra_visible = source.tiles(self.extra_visible);

            let visible: Vec<TileId> = hand_tiles
                .iter()
                .chain([&drawn_tile])
                .chain(extra_visible.iter())
                .copied()
                .collect();
            let actions: Vec<LegalAction> = hand_tiles
                .iter()
                .chain([&drawn_tile])
                .map(|&tile| LegalAction::Dahai { tile })
                .collect();

            let mut reached = [false; 4];
            if let Some(player_id) = self.player_id {
                reached[usize::from(player_id)] = self.own_reached;
            }

            let context = GameContext::from_parts_with_table_state(
                Some(drawn_tile),
                hand_tiles,
                Vec::new(),
                Some(tile("E")),
                Some(tile("S")),
                visible,
                self.player_id,
                Some(3),
                Default::default(),
                reached,
            )
            .with_history_furiten_facts(HistoryFuritenFacts {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            });

            select_discard_action_with_diagnostic(&context, &actions, self.scope)
        }
    }

    // 2手先探索は重いので、同じ局面を使う複数のテストで構築結果を共有する。
    static CASE: LazyLock<DiscardActionSelectionWithDiagnostic> =
        LazyLock::new(|| CaseSpec::default().build());
    static NO_LOOKAHEAD: LazyLock<DiscardActionSelectionWithDiagnostic> = LazyLock::new(|| {
        CaseSpec {
            scope: LookaheadDiagnosticScope::None,
            ..CaseSpec::default()
        }
        .build()
    });

    fn continuation(
        selection: &DiscardActionSelectionWithDiagnostic,
    ) -> &TenpaiContinuationDiagnostic {
        selection
            .tenpai_continuation
            .as_ref()
            .expect("継続診断が構築されている")
    }

    // 打 E (南単騎テンパイ) の継続枝。
    fn discard_east(
        selection: &DiscardActionSelectionWithDiagnostic,
    ) -> &TenpaiContinuationCandidate {
        continuation(selection)
            .candidate(tile("E"))
            .expect("打 E の現在聴牌候補がある")
    }

    fn branch<'a>(
        candidate: &'a TenpaiContinuationCandidate,
        drawn: &str,
    ) -> &'a TenpaiContinuationBranch {
        let drawn_tile = tile(drawn);
        let red = drawn.ends_with('r');
        candidate
            .branches
            .iter()
            .find(|branch| {
                branch.drawn_tile().tile_type() == drawn_tile && branch.drawn_tile().is_red() == red
            })
            .unwrap_or_else(|| panic!("{drawn} の継続枝がある"))
    }

    fn wait_tiles(branch: &TenpaiContinuationBranch) -> Vec<TileType> {
        branch
            .waits()
            .iter()
            .map(|wait| wait.winning_tile)
            .collect()
    }

    #[test]
    fn only_current_tenpai_candidates_are_searched() {
        let selection = &*CASE;
        let discards: Vec<TileType> = continuation(selection)
            .candidates
            .iter()
            .map(|candidate| candidate.discard)
            .collect();

        // 打 E / 打 S だけが聴牌で、他の打牌は1向聴以上。
        assert_eq!(discards, vec![tile("E"), tile("S")]);
    }

    #[test]
    fn non_winning_draw_reaches_a_new_tenpai() {
        let selection = &*CASE;
        let candidate = discard_east(selection);

        // 現在は南単騎。1m を引くと南を切って三面張へ待ちが変わる。
        assert_eq!(
            candidate
                .current_wait
                .iter()
                .map(|wait| wait.tile)
                .collect::<Vec<_>>(),
            vec![tile("S")]
        );
        assert_eq!(candidate.current_wait_remaining(), 3);

        let branch = branch(candidate, "1m");
        assert_eq!(branch.next_discard(), Some(tile("S")));
        assert_eq!(wait_tiles(branch), vec![tile("1m"), tile("4m"), tile("7m")]);
        assert_eq!(branch.wait_type_count(), 3);
        assert_eq!(branch.wait_remaining(), 8);
        assert_eq!(branch.offense_mode(), Some(TenpaiOffenseMode::Reach));
        assert!(branch.prospective_value().is_some());
    }

    #[test]
    fn tsumogiri_that_holds_the_wait_is_a_continuation_branch() {
        // ツモ切りで元の待ちを維持する枝も継続枝に含める。「今すぐリーチ」と「ダマ継続」を
        // 比べるには待ちが変わらない場合の価値も必要なので、待ちが変わる枝だけへは絞らない。
        let candidate = discard_east(&CASE);
        let branch = branch(candidate, "2m");

        assert_eq!(branch.next_discard(), Some(tile("2m")));
        assert_eq!(wait_tiles(branch), vec![tile("S")]);
        assert_eq!(branch.wait_remaining(), candidate.current_wait_remaining());
    }

    #[test]
    fn current_winning_tile_is_not_a_continuation_branch() {
        let selection = &*CASE;
        let candidate = discard_east(selection);

        // 南は現在の和了牌なので、継続枝には現れない。
        assert!(
            !candidate
                .branches
                .iter()
                .any(|branch| branch.draw == tile("S"))
        );
        // 既存2手先評価では向聴数を下げる枝 (Progress) として残っている。
        let lookahead = selection.lookahead.as_ref().expect("2手先診断がある");
        let draw = lookahead
            .candidate(tile("E"))
            .expect("打 E の2手先評価がある")
            .draw(tile("S"))
            .expect("南の枝がある");
        assert_eq!(draw.transition, DrawTransition::Progress);
    }

    #[test]
    fn every_branch_returns_to_tenpai_after_the_next_discard() {
        let selection = &*CASE;
        let lookahead = selection.lookahead.as_ref().expect("2手先診断がある");

        for candidate in &continuation(selection).candidates {
            let draws = lookahead
                .candidate(candidate.discard)
                .expect("同じ候補の2手先評価がある");
            for branch in &candidate.branches {
                let variant = draws
                    .draw(branch.draw)
                    .expect("同じ牌種の枝がある")
                    .variant(branch.drawn_tile())
                    .expect("同じ物理牌の枝がある");
                assert_eq!(variant.next_min_shanten(), Some(TENPAI_SHANTEN));
            }
        }
    }

    #[test]
    fn seen_tiles_reduce_the_draw_variant_remaining() {
        let visible = CaseSpec {
            extra_visible: &["1m", "1m"],
            ..CaseSpec::default()
        }
        .build();

        // 手牌の1枚に加えて2枚が見えているので、1m の継続枝は残り1枚になる。
        assert_eq!(branch(discard_east(&CASE), "1m").remaining(), 3);
        let seen = branch(discard_east(&visible), "1m");
        assert_eq!(seen.remaining(), 1);
        assert_eq!(seen.draw_remaining, 1);
    }

    #[test]
    fn all_seen_draws_have_no_branch() {
        let visible = CaseSpec {
            extra_visible: &["1m", "1m", "1m"],
            ..CaseSpec::default()
        }
        .build();

        assert!(
            !discard_east(&visible)
                .branches
                .iter()
                .any(|branch| branch.draw == tile("1m"))
        );
    }

    #[test]
    fn red_and_black_five_stay_separate_branches() {
        let selection = &*CASE;
        let candidate = discard_east(selection);

        let red = branch(candidate, "5mr");
        let black = branch(candidate, "5m");
        assert_eq!(red.remaining(), 1);
        assert_eq!(black.remaining(), 2);
        assert_eq!(red.draw_remaining, 3);
        assert_eq!(black.draw_remaining, 3);

        // 待ちは同じでも、赤5を手牌へ残す枝の方が打点が高い。
        assert_eq!(wait_tiles(red), wait_tiles(black));
        assert!(red.prospective_value() > black.prospective_value());
    }

    #[test]
    fn branches_reuse_the_existing_lookahead_next_discard() {
        let selection = &*CASE;
        let lookahead = selection.lookahead.as_ref().expect("2手先診断がある");

        for candidate in &continuation(selection).candidates {
            let draws = lookahead
                .candidate(candidate.discard)
                .expect("同じ候補の2手先評価がある");
            for branch in &candidate.branches {
                let draw = draws.draw(branch.draw).expect("同じ牌種の枝がある");
                let variant = draw
                    .variant(branch.drawn_tile())
                    .expect("同じ物理牌の枝がある");

                assert_eq!(draw.transition, DrawTransition::SameShanten);
                assert_eq!(branch.draw_remaining, draw.remaining);
                assert_eq!(branch.remaining(), variant.remaining);
                assert_eq!(branch.next_discard(), variant.next_discard_tile());
                assert_eq!(branch.prospective_value(), variant.prospective_value);
            }
        }
    }

    #[test]
    fn branches_reuse_the_existing_prospective_value() {
        let selection = &*CASE;
        let value = selection.lookahead_value.as_ref().expect("将来打点がある");

        for candidate in &continuation(selection).candidates {
            let draws = value
                .candidate(candidate.discard)
                .expect("同じ候補の将来打点がある");
            for branch in &candidate.branches {
                let variant = draws
                    .draw(branch.draw)
                    .expect("同じ牌種の枝がある")
                    .variant(branch.drawn_tile())
                    .expect("同じ物理牌の枝がある");
                assert_eq!(&branch.variant, variant);
            }
        }
    }

    #[test]
    fn reached_hand_is_not_searched() {
        let reached = CaseSpec {
            own_reached: true,
            ..CaseSpec::default()
        }
        .build();

        assert!(reached.tenpai_continuation.is_none());
        // 2手先診断そのものは既存どおり構築する。継続診断だけを行わない。
        assert!(reached.lookahead.is_some());
    }

    #[test]
    fn unknown_own_reach_is_not_assumed_to_be_not_reached() {
        let unknown_seat = CaseSpec {
            player_id: None,
            ..CaseSpec::default()
        }
        .build();

        assert!(unknown_seat.tenpai_continuation.is_none());
    }

    #[test]
    fn continuation_is_not_searched_without_lookahead() {
        assert!(NO_LOOKAHEAD.tenpai_continuation.is_none());
    }

    #[test]
    fn continuation_does_not_change_the_normal_selection() {
        assert_eq!(CASE.selection, NO_LOOKAHEAD.selection);
    }
}
