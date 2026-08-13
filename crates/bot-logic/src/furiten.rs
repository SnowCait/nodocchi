//! 自分の河による恒常フリテンの判定基盤。
//!
//! テンパイの待ちは既存の受け入れ ([`Acceptance`]) をそのまま source of truth として使い、
//! 待ち牌の一覧・残枚数をここで計算し直さない。判定に使う情報は
//!
//! - 既存 [`Acceptance`] が返すテンパイの待ち
//! - 自分が捨てた牌種 ([`OwnDiscards`])
//!
//! の2つだけで、`GameContext` のような上位層の局面型には依存しない。
//! 将来 [`crate::lookahead`] の枝で「既存の自分の河 + 1手目の打牌 + 2手目の打牌」を
//! 河として渡す場合も、[`OwnDiscards::with_discards`] で組み立てた値を渡すだけで同じ helper を
//! 使える。
//!
//! # 今回扱う範囲
//!
//! 扱うのは自分の河による恒常フリテンだけで、一時フリテン・同巡フリテン・見逃し・リーチ後の
//! 見逃しは扱わない。他家の河や見え牌はフリテン判定に使わない (見え牌は従来どおり
//! [`Acceptance`] の残枚数計算にだけ効く)。フリテンを打牌選択の点数へ変換するヒューリスティック
//! も持たず、事実だけを表現する。
//!
//! # 待ちがすべて見えている場合
//!
//! [`Acceptance`] は残枚数 0 の牌種を受け入れに含めないため、待ち牌が4枚とも見えている牌種は
//! ここでも待ちとして扱わない。この牌種を自分が捨てていた場合、恒常フリテン自体は `No` になるが
//! その待ちで和了できる牌は山にも他家の手にも残っていないので、ロン可否の結論は変わらない。

use crate::acceptance::Acceptance;
use crate::discard::DiscardEvaluation;
use crate::shanten::MinShanten;
use crate::tile::{TileId, TileType};

// フリテンを判定する向聴数。テンパイ形の待ちだけを対象にする。
const TENPAI_SHANTEN: i8 = 0;

/// 恒常フリテン判定に使う「自分が捨てた牌」。
///
/// 赤5と黒5を同じ牌種として扱うため、物理牌ではなく牌種で保持する。自分の河が特定できない場合と
/// 「河が空だと確定した」場合を区別するため、河を特定できたかどうかを別に持つ。
///
/// これから切ることが確定している牌 ([`with_discard`](Self::with_discard)) は、河そのものが
/// 不明でも確定情報として扱う。河が不明でもその牌が待ちに含まれていればフリテンだと断定できる
/// 一方、非フリテンだと断定することはできない。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OwnDiscards {
    tiles: Vec<TileType>,
    river_known: bool,
}

impl OwnDiscards {
    /// 自分の河を特定できない場合。player 0 を自分と推測しない。
    pub fn unknown() -> Self {
        Self::default()
    }

    /// 自分の河が分かっている場合。赤5は同じ牌種の黒5として扱う。
    pub fn from_river(river: &[TileId]) -> Self {
        Self::from_river_tile_types(river.iter().map(|tile| tile.tile_type()))
    }

    pub fn from_river_tile_types(river: impl IntoIterator<Item = TileType>) -> Self {
        Self {
            tiles: river.into_iter().collect(),
            river_known: true,
        }
    }

    /// 自分の河を特定できない場合を `None` として受け取る。
    ///
    /// `player_id` が無い局面のように自分の河が決まらない入力を、そのまま `Unknown` へ落とす。
    pub fn from_optional_river(river: Option<&[TileId]>) -> Self {
        river.map_or_else(Self::unknown, Self::from_river)
    }

    /// これから切ることが確定している牌を1枚足した河。
    pub fn with_discard(&self, tile: TileType) -> Self {
        self.with_discards([tile])
    }

    /// これから切ることが確定している牌を足した河。
    ///
    /// 前方評価の枝のように「現在打牌 → テンパイに取るための打牌」を続けて足す用途を想定する。
    pub fn with_discards(&self, tiles: impl IntoIterator<Item = TileType>) -> Self {
        let mut extended = self.clone();
        extended.tiles.extend(tiles);
        extended
    }

    /// 自分の河そのものを特定できているか。
    pub fn is_river_known(&self) -> bool {
        self.river_known
    }

    pub fn tile_types(&self) -> &[TileType] {
        &self.tiles
    }

    /// 指定牌種を自分が既に捨てているか。河が不明で判断できない場合は `None`。
    pub fn contains(&self, tile: TileType) -> Option<bool> {
        if self.tiles.contains(&tile) {
            return Some(true);
        }
        self.river_known.then_some(false)
    }
}

/// 自分の河による恒常フリテンの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermanentFuriten {
    /// 自分の河を特定でき、現在の待ちと重複していない。
    No,
    /// 現在の待ちのうち1種類以上を自分が既に捨てている。
    Yes,
    /// 自分の河を特定できず、フリテンかどうか判断できない。非フリテンとは断定しない。
    Unknown,
}

/// 恒常フリテンの判定結果と、その根拠になった待ち牌。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentFuritenDiagnostic {
    pub status: PermanentFuriten,
    /// 現在の待ちのうち自分の河と重複した牌種。`status` が [`PermanentFuriten::Yes`] のときだけ
    /// 空でない。
    pub discarded_waits: Vec<TileType>,
}

impl PermanentFuritenDiagnostic {
    /// 恒常フリテンかどうか。判断できない場合は `None`。
    pub fn is_furiten(&self) -> Option<bool> {
        match self.status {
            PermanentFuriten::Yes => Some(true),
            PermanentFuriten::No => Some(false),
            PermanentFuriten::Unknown => None,
        }
    }
}

/// テンパイ時の待ちについて、ツモ和了とロン和了それぞれの可否を表す pure な診断。
///
/// `waits` / `tsumo_remaining` / `tsumo_type_count` は既存 [`Acceptance`] の値そのもので、
/// 恒常フリテンでも書き換えない。フリテンで変わるのはロン可否 ([`can_ron`](Self::can_ron)) だけで、
/// 「ロンできないから残枚数を 0 にする」のように受け入れの意味を変えない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenpaiWaitAvailability {
    /// テンパイの待ち牌種。既存 [`Acceptance`] の受け入れ牌そのもの。
    pub waits: Vec<TileType>,
    /// ツモ和了できる牌の残枚数。既存 [`Acceptance::total_remaining`] そのもの。
    pub tsumo_remaining: u8,
    /// 待ち牌の種類数。既存 [`Acceptance`] の受け入れ牌種数そのもの。
    pub tsumo_type_count: usize,
    pub furiten: PermanentFuritenDiagnostic,
}

impl TenpaiWaitAvailability {
    /// 恒常フリテンの観点からロンできるか。判断できない場合は `None`。
    ///
    /// 役の有無や一時フリテンなど、恒常フリテン以外の理由によるロン不可はここでは扱わない。
    pub fn can_ron(&self) -> Option<bool> {
        self.furiten.is_furiten().map(|furiten| !furiten)
    }

    pub fn permanent_furiten(&self) -> PermanentFuriten {
        self.furiten.status
    }

    pub fn discarded_waits(&self) -> &[TileType] {
        &self.furiten.discarded_waits
    }
}

/// 打牌候補1件について、その打牌でテンパイになる場合の待ちとロン可否。
///
/// 待ちは打牌後の既存受け入れ ([`DiscardEvaluation::acceptance_after_discard`]) をそのまま使い、
/// 自分の河にはその打牌自身を足す。テンパイにならない打牌候補の `tenpai` は `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardFuritenDiagnostic {
    pub discard: TileType,
    pub tenpai: Option<TenpaiWaitAvailability>,
}

impl DiscardFuritenDiagnostic {
    /// テンパイになる場合の恒常フリテン状態。テンパイにならない打牌候補は `None`。
    pub fn permanent_furiten(&self) -> Option<PermanentFuriten> {
        self.tenpai
            .as_ref()
            .map(TenpaiWaitAvailability::permanent_furiten)
    }

    /// 自分の河と重複した待ち牌。テンパイにならない打牌候補では空。
    pub fn discarded_waits(&self) -> &[TileType] {
        self.tenpai
            .as_ref()
            .map_or(&[], TenpaiWaitAvailability::discarded_waits)
    }
}

/// 待ち牌種の一覧と自分の河から恒常フリテンを判定する。
///
/// 待ちのうち1種類でも自分が捨てていればフリテンで、ロンは待ち全体に対して不可になる。
pub fn permanent_furiten_for_waits(
    waits: &[TileType],
    own_discards: &OwnDiscards,
) -> PermanentFuritenDiagnostic {
    let discarded_waits: Vec<TileType> = waits
        .iter()
        .copied()
        .filter(|wait| own_discards.contains(*wait) == Some(true))
        .collect();

    let status = if !discarded_waits.is_empty() {
        PermanentFuriten::Yes
    } else if own_discards.is_river_known() {
        PermanentFuriten::No
    } else {
        PermanentFuriten::Unknown
    };

    PermanentFuritenDiagnostic {
        status,
        discarded_waits,
    }
}

/// 既存の受け入れと自分の河からテンパイの待ちとロン可否を求める。
///
/// テンパイ形 (最小向聴数 0) 以外では待ちが定まらないため `None`。門前形・副露形のどちらの
/// 受け入れでも同じ helper を使う。
pub fn tenpai_wait_availability<S: MinShanten>(
    acceptance: &Acceptance<S>,
    own_discards: &OwnDiscards,
) -> Option<TenpaiWaitAvailability> {
    if acceptance.current_min_shanten() != TENPAI_SHANTEN {
        return None;
    }

    let waits: Vec<TileType> = acceptance.tiles.iter().map(|tile| tile.tile).collect();
    let furiten = permanent_furiten_for_waits(&waits, own_discards);

    Some(TenpaiWaitAvailability {
        tsumo_remaining: acceptance.total_remaining(),
        tsumo_type_count: waits.len(),
        waits,
        furiten,
    })
}

/// 打牌候補1件について、その打牌でテンパイになる場合の待ちとロン可否を求める。
///
/// その打牌自身も自分の河に入るため、判定に使う河は `own_discards` へ打牌牌種を足したものになる。
pub fn discard_tenpai_wait_availability(
    evaluation: &DiscardEvaluation,
    own_discards: &OwnDiscards,
) -> Option<TenpaiWaitAvailability> {
    tenpai_wait_availability(
        &evaluation.acceptance_after_discard,
        &own_discards.with_discard(evaluation.discard),
    )
}

/// 全打牌候補分の恒常フリテン診断。
///
/// 戻り値は `evaluations` と同じ順序・同じ件数。既存の打牌評価が持つ受け入れをそのまま使うので、
/// 向聴・受け入れ・待ちを再計算しない。
pub fn diagnose_discard_furiten(
    evaluations: &[DiscardEvaluation],
    own_discards: &OwnDiscards,
) -> Vec<DiscardFuritenDiagnostic> {
    evaluations
        .iter()
        .map(|evaluation| DiscardFuritenDiagnostic {
            discard: evaluation.discard,
            tenpai: discard_tenpai_wait_availability(evaluation, own_discards),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acceptance::{
        EffectiveAcceptance, calculate_acceptance, calculate_acceptance_with_fixed_melds,
        calculate_acceptance_with_fixed_melds_and_visible_tiles,
        calculate_acceptance_with_visible_tiles,
    };
    use crate::discard::evaluate_discards_from_tiles;
    use crate::shanten::{FixedMeldCount, Shanten};
    use crate::tile_counts::TileCounts;

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    fn tiles(strings: &[&str]) -> Vec<TileType> {
        strings.iter().map(|s| tile(s)).collect()
    }

    fn counts(strings: &[&str]) -> TileCounts {
        TileCounts::from_tile_types(strings.iter().map(|s| tile(s)))
    }

    fn ids(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&v| TileId::new(v).unwrap()).collect()
    }

    fn river(strings: &[&str]) -> OwnDiscards {
        OwnDiscards::from_river_tile_types(strings.iter().map(|s| tile(s)))
    }

    // 123m456m789m 123p 5s の単騎待ち。待ちは 5s だけ。
    fn tanki_tenpai() -> Acceptance<Shanten> {
        calculate_acceptance(&counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
        ]))
    }

    // 123m456m 3456789s の3面待ち。待ちは 3s / 6s / 9s。
    fn three_sided_tenpai() -> Acceptance<Shanten> {
        calculate_acceptance(&counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "3s", "4s", "5s", "6s", "7s", "8s", "9s",
        ]))
    }

    fn availability(
        acceptance: &Acceptance<Shanten>,
        own_discards: &OwnDiscards,
    ) -> TenpaiWaitAvailability {
        tenpai_wait_availability(acceptance, own_discards).expect("テンパイ形である")
    }

    #[test]
    fn tanki_wait_in_the_own_river_is_permanent_furiten() {
        let acceptance = tanki_tenpai();
        let availability = availability(&acceptance, &river(&["1m", "5s", "E"]));

        assert_eq!(availability.waits, tiles(&["5s"]));
        assert_eq!(availability.permanent_furiten(), PermanentFuriten::Yes);
        assert_eq!(availability.discarded_waits(), tiles(&["5s"]));
        assert_eq!(availability.can_ron(), Some(false));
    }

    #[test]
    fn permanent_furiten_keeps_the_tsumo_side_of_the_existing_acceptance() {
        // フリテンでもツモ和了の残枚数・種類数は既存受け入れのまま。
        let acceptance = tanki_tenpai();
        let availability = availability(&acceptance, &river(&["5s"]));

        assert_eq!(availability.tsumo_remaining, acceptance.total_remaining());
        assert_eq!(availability.tsumo_type_count, acceptance.tiles.len());
        assert_eq!(availability.tsumo_remaining, 3);
    }

    #[test]
    fn one_discarded_wait_blocks_the_whole_multi_sided_wait() {
        let acceptance = three_sided_tenpai();
        let availability = availability(&acceptance, &river(&["6s"]));

        assert_eq!(availability.waits, tiles(&["3s", "6s", "9s"]));
        assert_eq!(availability.tsumo_type_count, 3);
        assert_eq!(availability.discarded_waits(), tiles(&["6s"]));
        assert_eq!(availability.can_ron(), Some(false));
    }

    #[test]
    fn an_empty_own_river_is_not_furiten() {
        let acceptance = three_sided_tenpai();
        let availability = availability(&acceptance, &river(&[]));

        assert_eq!(availability.permanent_furiten(), PermanentFuriten::No);
        assert!(availability.discarded_waits().is_empty());
        assert_eq!(availability.can_ron(), Some(true));
    }

    #[test]
    fn tiles_in_the_own_river_that_are_not_waits_are_not_furiten() {
        let acceptance = three_sided_tenpai();
        let availability = availability(&acceptance, &river(&["9m", "5p", "E", "2s"]));

        assert_eq!(availability.permanent_furiten(), PermanentFuriten::No);
        assert_eq!(availability.can_ron(), Some(true));
    }

    #[test]
    fn only_the_own_river_decides_furiten() {
        // 他家の河や見え牌は判定に渡さない。同じ待ちで自分の河だけを変えると結果が変わる。
        let acceptance = three_sided_tenpai();

        assert_eq!(
            availability(&acceptance, &river(&["1m", "9p"])).permanent_furiten(),
            PermanentFuriten::No
        );
        assert_eq!(
            availability(&acceptance, &river(&["1m", "9p", "9s"])).permanent_furiten(),
            PermanentFuriten::Yes
        );
    }

    #[test]
    fn a_discarded_red_five_is_furiten_for_the_black_five_wait() {
        // 123m456m789m 123p 5p 待ちで、河には赤5p だけがある。
        let acceptance = calculate_acceptance(&counts(&[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5p",
        ]));
        let red_five_pin = TileId::new(52).unwrap();
        assert!(red_five_pin.is_red());
        assert_eq!(red_five_pin.tile_type(), tile("5p"));

        let availability = availability(&acceptance, &OwnDiscards::from_river(&[red_five_pin]));

        assert_eq!(availability.waits, tiles(&["5p"]));
        assert_eq!(availability.permanent_furiten(), PermanentFuriten::Yes);
        assert_eq!(availability.discarded_waits(), tiles(&["5p"]));
    }

    #[test]
    fn an_unknown_own_river_is_not_reported_as_non_furiten() {
        let acceptance = three_sided_tenpai();
        let availability = availability(&acceptance, &OwnDiscards::unknown());

        assert_eq!(availability.permanent_furiten(), PermanentFuriten::Unknown);
        assert_eq!(availability.can_ron(), None);
        assert!(availability.discarded_waits().is_empty());
        // 待ち自体は既存受け入れのまま分かる。
        assert_eq!(availability.waits, tiles(&["3s", "6s", "9s"]));
    }

    #[test]
    fn an_unknown_river_still_reports_furiten_for_a_certain_discard() {
        // 河が不明でも、これから切る牌が待ちに含まれていればフリテンだと断定できる。
        let acceptance = three_sided_tenpai();
        let availability = availability(
            &acceptance,
            &OwnDiscards::unknown().with_discard(tile("9s")),
        );

        assert_eq!(availability.permanent_furiten(), PermanentFuriten::Yes);
        assert_eq!(availability.discarded_waits(), tiles(&["9s"]));
    }

    #[test]
    fn planned_discards_can_be_appended_for_a_lookahead_branch() {
        // 「既存の河 + 1手目の打牌 + 2手目の打牌」を河として渡せる。
        let acceptance = three_sided_tenpai();
        let own_discards = river(&["E"]).with_discards(tiles(&["9m", "3s"]));

        assert_eq!(
            own_discards.tile_types(),
            tiles(&["E", "9m", "3s"]).as_slice()
        );
        let availability = availability(&acceptance, &own_discards);
        assert_eq!(availability.discarded_waits(), tiles(&["3s"]));
        assert_eq!(availability.can_ron(), Some(false));
    }

    #[test]
    fn visible_tiles_reduce_the_remaining_but_not_the_furiten_decision() {
        // 見え牌は残枚数だけに効き、フリテン判定は自分の河だけで決まる。
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut visible = hand.clone();
        visible.extend(ids(&[90, 91]));

        let plain = calculate_acceptance(&counts);
        let with_visible = calculate_acceptance_with_visible_tiles(&counts, &visible);
        assert_eq!(plain.total_remaining(), 3);
        assert_eq!(with_visible.total_remaining(), 1);

        for own_discards in [river(&["5s"]), OwnDiscards::unknown(), river(&["1m"])] {
            let expected = availability(&plain, &own_discards);
            let actual = availability(&with_visible, &own_discards);
            assert_eq!(actual.furiten, expected.furiten);
            assert_eq!(actual.waits, expected.waits);
            assert_ne!(actual.tsumo_remaining, expected.tsumo_remaining);
        }
    }

    #[test]
    fn chiitoitsu_tenpai_uses_the_acceptance_wait() {
        // 七対子テンパイ。待ちは単騎の E だけ。
        let acceptance = calculate_acceptance(&counts(&[
            "1m", "1m", "2m", "2m", "3m", "3m", "4p", "4p", "5p", "5p", "6s", "6s", "E",
        ]));
        assert_eq!(acceptance.current.chiitoitsu, 0);

        assert_eq!(
            availability(&acceptance, &river(&["E"])).permanent_furiten(),
            PermanentFuriten::Yes
        );
        assert_eq!(
            availability(&acceptance, &river(&["S"])).permanent_furiten(),
            PermanentFuriten::No
        );
    }

    #[test]
    fn kokushi_thirteen_wait_is_furiten_when_any_yaochu_was_discarded() {
        // 国士13面待ち。1種類でも自分が捨てていればロン不可。
        let acceptance = calculate_acceptance(&counts(&[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
        ]));
        assert_eq!(acceptance.tiles.len(), 13);

        let furiten = availability(&acceptance, &river(&["C"]));
        assert_eq!(furiten.tsumo_type_count, 13);
        assert_eq!(furiten.tsumo_remaining, 39);
        assert_eq!(furiten.discarded_waits(), tiles(&["C"]));
        assert_eq!(furiten.can_ron(), Some(false));

        assert_eq!(
            availability(&acceptance, &river(&["2m", "5p"])).permanent_furiten(),
            PermanentFuriten::No
        );
    }

    #[test]
    fn fixed_meld_tenpai_keeps_the_effective_acceptance_semantics() {
        // 副露1つの通常形テンパイ。EffectiveAcceptance をそのまま待ちとして使う。
        let counts = counts(&["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "5p"]);
        let acceptance: EffectiveAcceptance =
            calculate_acceptance_with_fixed_melds(&counts, FixedMeldCount::new(1).unwrap());
        assert_eq!(acceptance.current.concealed(), None);

        let furiten =
            tenpai_wait_availability(&acceptance, &river(&["5p"])).expect("副露テンパイ形である");
        assert_eq!(furiten.waits, tiles(&["5p"]));
        assert_eq!(furiten.tsumo_remaining, acceptance.total_remaining());
        assert_eq!(furiten.can_ron(), Some(false));

        let non_furiten =
            tenpai_wait_availability(&acceptance, &river(&["1p"])).expect("副露テンパイ形である");
        assert_eq!(non_furiten.can_ron(), Some(true));
    }

    #[test]
    fn fixed_meld_tenpai_reflects_visible_tiles_in_the_tsumo_remaining() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 53]);
        let counts = TileCounts::from_tiles(hand.iter().copied());
        let mut visible = hand.clone();
        visible.extend(ids(&[54, 55]));
        let acceptance = calculate_acceptance_with_fixed_melds_and_visible_tiles(
            &counts,
            FixedMeldCount::new(1).unwrap(),
            &visible,
        );

        let availability =
            tenpai_wait_availability(&acceptance, &river(&["5p"])).expect("副露テンパイ形である");
        assert_eq!(availability.tsumo_remaining, 1);
        assert_eq!(availability.can_ron(), Some(false));
    }

    #[test]
    fn a_hand_that_is_not_tenpai_has_no_wait() {
        let acceptance = calculate_acceptance(&counts(&[
            "1m", "3m", "5m", "7m", "9m", "1p", "3p", "5p", "7p", "9p", "1s", "3s", "5s",
        ]));
        assert!(acceptance.current.min() > 0);

        assert_eq!(tenpai_wait_availability(&acceptance, &river(&[])), None);
    }

    #[test]
    fn discard_furiten_counts_the_candidate_discard_itself() {
        // 123m456m789m 123p 5s5s から 5s を切ると 5s 単騎テンパイになり、その 5s が河に入る。
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89, 90]);
        let evaluations = evaluate_discards_from_tiles(&hand);
        let diagnostics = diagnose_discard_furiten(&evaluations, &river(&[]));

        assert_eq!(diagnostics.len(), evaluations.len());
        let five_sou = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.discard == tile("5s"))
            .expect("打 5s の候補がある");
        let tenpai = five_sou.tenpai.as_ref().expect("テンパイになる");
        assert_eq!(tenpai.waits, tiles(&["5s"]));
        assert_eq!(five_sou.discarded_waits(), tiles(&["5s"]));
        assert_eq!(five_sou.permanent_furiten(), Some(PermanentFuriten::Yes));
        assert_eq!(tenpai.can_ron(), Some(false));
    }

    #[test]
    fn discard_furiten_is_absent_for_candidates_that_do_not_reach_tenpai() {
        let hand = ids(&[0, 8, 20, 28, 48, 56, 68, 76, 88, 100, 108, 116, 124, 132]);
        let evaluations = evaluate_discards_from_tiles(&hand);
        let diagnostics = diagnose_discard_furiten(&evaluations, &river(&[]));

        assert!(!diagnostics.is_empty());
        assert!(diagnostics.iter().all(|diagnostic| {
            diagnostic.tenpai.is_none()
                && diagnostic.permanent_furiten().is_none()
                && diagnostic.discarded_waits().is_empty()
        }));
    }

    #[test]
    fn discard_furiten_keeps_the_existing_acceptance_of_the_candidate() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89, 90]);
        let evaluations = evaluate_discards_from_tiles(&hand);
        let diagnostics = diagnose_discard_furiten(&evaluations, &river(&["1m"]));

        for (diagnostic, evaluation) in diagnostics.iter().zip(evaluations.iter()) {
            assert_eq!(diagnostic.discard, evaluation.discard);
            let Some(tenpai) = diagnostic.tenpai.as_ref() else {
                assert_ne!(evaluation.min_shanten_after_discard(), 0);
                continue;
            };
            assert_eq!(
                tenpai.tsumo_remaining,
                evaluation.acceptance_total_remaining()
            );
            assert_eq!(tenpai.tsumo_type_count, evaluation.acceptance_type_count());
            assert_eq!(
                tenpai.waits,
                evaluation
                    .acceptance_after_discard
                    .tiles
                    .iter()
                    .map(|accepted| accepted.tile)
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn an_unknown_river_leaves_the_discard_diagnostic_unknown() {
        let hand = ids(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89, 108]);
        let evaluations = evaluate_discards_from_tiles(&hand);
        let diagnostics = diagnose_discard_furiten(&evaluations, &OwnDiscards::unknown());

        let east = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.discard == tile("E"))
            .expect("打 E の候補がある");
        assert_eq!(east.permanent_furiten(), Some(PermanentFuriten::Unknown));
        assert_eq!(
            east.tenpai.as_ref().expect("テンパイになる").can_ron(),
            None
        );
    }

    #[test]
    fn own_discards_report_containment_per_tile_type() {
        let known = river(&["1m", "E"]);
        assert_eq!(known.contains(tile("1m")), Some(true));
        assert_eq!(known.contains(tile("2m")), Some(false));
        assert!(known.is_river_known());

        let unknown = OwnDiscards::unknown();
        assert_eq!(unknown.contains(tile("1m")), None);
        assert!(!unknown.is_river_known());
        assert!(unknown.tile_types().is_empty());

        let unknown_with_discard = unknown.with_discard(tile("1m"));
        assert_eq!(unknown_with_discard.contains(tile("1m")), Some(true));
        assert_eq!(unknown_with_discard.contains(tile("2m")), None);
        assert!(!unknown_with_discard.is_river_known());
    }

    #[test]
    fn optional_river_maps_none_to_unknown() {
        assert_eq!(
            OwnDiscards::from_optional_river(None),
            OwnDiscards::unknown()
        );

        let river_tiles = ids(&[0, 16]);
        assert_eq!(
            OwnDiscards::from_optional_river(Some(&river_tiles)),
            OwnDiscards::from_river(&river_tiles)
        );
        // 赤5m は黒5m と同じ牌種として保持する。
        assert_eq!(
            OwnDiscards::from_river(&river_tiles).tile_types(),
            tiles(&["1m", "5m"]).as_slice()
        );
    }
}
