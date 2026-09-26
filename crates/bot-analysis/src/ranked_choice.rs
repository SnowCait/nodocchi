use bot_core::{
    AgentActionSource, CombinedDefenseCategory, DefenseFallbackKind, GameContext, LegalAction,
    OpenHandDefenseCategory, OpponentHonorValue, ShantenAgent, ShantenDecisionDiagnostic,
};
use bot_logic::{DiscardCandidateDiagnostic, DiscardComparisonReason};

/// 上位から順に並べた選択肢1件分の構造化結果。
///
/// choice 1 は呼び出し側が既に得ている production 診断そのもので、choice 2 以降は上位 choice が
/// 選んだ action を合法手から順に除外して production の [`ShantenAgent::diagnose`] を再実行した
/// 結果になる。内部では [`ShantenDecisionDiagnostic`] を使うが、consumer へはこの薄い結果だけを
/// 渡し、診断全体を公開しない。表示用の文字列も作らず、既存の enum と数値をそのまま保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedChoice {
    /// この choice が選んだ action。
    pub selected_action: LegalAction,
    /// リーチを選んだ場合に併せて選んだ打牌。他の action では `None`。
    pub selected_discard: Option<LegalAction>,
    /// action をどの経路で選んだか。
    pub selected_source: AgentActionSource,
    /// リーチ者向け防御 fallback 由来の場合のその種別。
    pub defense_fallback_kind: Option<DefenseFallbackKind>,
    /// 非リーチ相手向け防御 fallback 由来の場合のその大分類。
    pub open_hand_defense_category: Option<OpenHandDefenseCategory>,
    /// 複合 threat 向け防御 fallback 由来の場合のその大分類。
    pub combined_defense_category: Option<CombinedDefenseCategory>,
    /// HonorSafety の防御 fallback を選んだ場合の、相手にとっての役牌価値。
    pub opponent_honor_value: Option<AnalysisOpponentHonorValue>,
    /// 直前 choice との比較。choice 1 と、比較を取れない組み合わせでは `None`。
    pub comparison: Option<RankedChoiceComparison>,
}

/// 相手にとっての役牌価値。場風や親が不明で確定できない場合も、値なしとして区別して保持する。
///
/// choice と防御 section のどちらも同じ意味で使うので、1つの型を共有する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisOpponentHonorValue {
    Known(OpponentHonorValue),
    Unknown,
}

/// 隣り合う choice の比較結果。
///
/// 上位 choice の打牌診断が持つ値をそのまま写したもので、この構造体のために比較も評価も
/// やり直さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RankedChoiceComparison {
    /// 上位 choice に負けた比較軸。
    pub reason: DiscardComparisonReason,
    /// `reason` に対応する winner / loser の値。軸に数値が無い場合と、どちらかが確定しなかった
    /// 場合は `None`。
    pub values: Option<RankedChoiceComparisonValues>,
    /// 現在聴牌ツモ和了確率。比較にも選択にも使わない観測値で、両方とも確定しない場合は `None`。
    pub current_tenpai_self_tsumo_hit_probability: Option<RankedChoiceHitProbability>,
}

/// 比較軸ごとに意味の違う winner / loser の値。
///
/// どの値も `u64` だが尺度も単位も違うので、variant 名だけで意味が読めるように軸ごとに分ける。
/// どの軸だったかの source of truth は [`RankedChoiceComparison::reason`] のままで、この enum は
/// その値をどう読むかだけを表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankedChoiceComparisonValues {
    /// 現在聴牌の Σ(生きた和了牌 physical variant 残枚数 × 支払い合計)。枚数でも打点でもない。
    CurrentTenpaiOffenseWeightedTotal { winner: u64, loser: u64 },
    /// 将来テンパイの確定打点を枝の残枚数で重み付けした打点込みの前方集計値。
    WeightedProspectiveValue { winner: u64, loser: u64 },
    /// 枝の残枚数で重み付けした枚数・種類数。重み付けの分だけ実際の枚数より大きくなる。
    WeightedCount { winner: u64, loser: u64 },
    /// 重み付けのない受け入れ枚数・種類数。
    Count { winner: u64, loser: u64 },
    /// [`bot_logic::SELF_TSUMO_VALUE_SCALE`] でスケールした self-tsumo 期待支払い。
    SelfTsumoValue { winner: u64, loser: u64 },
}

/// [`bot_logic::TSUMO_PROBABILITY_SCALE`] でスケールした現在聴牌ツモ和了確率。
/// 片方だけ確定する場合があるので、winner / loser それぞれで未確定を区別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RankedChoiceHitProbability {
    pub winner: Option<u64>,
    pub loser: Option<u64>,
}

/// primary 診断を choice 1 として、上位 choice の action を除外しながら `limit` 件まで順位付けする。
///
/// `diagnostic` は、ここへ渡す `context` と `legal_actions` そのものに対して得た primary 診断で
/// なければならない。choice 1 は渡された `diagnostic` そのもので、別の診断範囲で取り直さない。
/// choice 2 以降だけ、その同じ `legal_actions` から上位 choice が選んだ action を1件ずつ除外して
/// production の [`ShantenAgent::diagnose`] を再実行する。したがって別の合法手集合から得た診断を
/// 渡すと、choice 1 と choice 2 以降が違う前提の並びになる。
///
/// 追加診断の範囲は違っていてよい。[`DiagnosticOptions`](bot_core::DiagnosticOptions) は選択する
/// action を変えないので、同じ `context` / `legal_actions` から得た詳細診断も primary 診断として
/// 渡せる。この前提は runtime では検証しない。
///
/// action を除外できない場合と、合法手が無くなった場合、再診断が action を選べなかった場合は
/// そこで打ち切る。
pub fn rank_choices(
    context: &GameContext,
    legal_actions: &[LegalAction],
    diagnostic: &ShantenDecisionDiagnostic,
    limit: usize,
) -> Vec<RankedChoice> {
    let diagnostics = diagnose_choices(context, legal_actions, diagnostic, limit);
    diagnostics
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            let previous = index.checked_sub(1).map(|previous| &diagnostics[previous]);
            ranked_choice(choice, previous)
        })
        .collect()
}

fn ranked_choice(
    diagnostic: &ShantenDecisionDiagnostic,
    previous: Option<&ShantenDecisionDiagnostic>,
) -> RankedChoice {
    RankedChoice {
        selected_action: diagnostic.selected_action.clone(),
        selected_discard: selected_reach_discard(diagnostic).cloned(),
        selected_source: diagnostic.selected_source,
        defense_fallback_kind: diagnostic.defense_fallback_kind(),
        open_hand_defense_category: diagnostic.open_hand_defense_category(),
        combined_defense_category: diagnostic.combined_defense_category(),
        opponent_honor_value: honor_safety_opponent_honor_value(diagnostic),
        comparison: previous
            .and_then(|previous| choice_comparison(previous, diagnostic))
            .map(|comparison| RankedChoiceComparison {
                reason: comparison.loser.comparison_reason,
                values: comparison_values(&comparison),
                current_tenpai_self_tsumo_hit_probability: hit_probability(&comparison),
            }),
    }
}

fn selected_reach_discard(diagnostic: &ShantenDecisionDiagnostic) -> Option<&LegalAction> {
    if !matches!(diagnostic.selected_action, LegalAction::Reach) {
        return None;
    }
    diagnostic.reach.as_ref()?.selected_discard.as_ref()
}

fn honor_safety_opponent_honor_value(
    diagnostic: &ShantenDecisionDiagnostic,
) -> Option<AnalysisOpponentHonorValue> {
    if !matches!(
        diagnostic.defense_fallback_kind(),
        Some(DefenseFallbackKind::HonorSafety(_))
    ) {
        return None;
    }
    let selected = diagnostic.defense.as_ref()?.selected.as_ref()?;
    Some(match selected.selected_opponent_honor_value {
        Some(value) => AnalysisOpponentHonorValue::Known(value),
        None => AnalysisOpponentHonorValue::Unknown,
    })
}

fn diagnose_choices(
    context: &GameContext,
    legal_actions: &[LegalAction],
    diagnostic: &ShantenDecisionDiagnostic,
    limit: usize,
) -> Vec<ShantenDecisionDiagnostic> {
    if limit == 0 {
        return Vec::new();
    }

    let mut choices = vec![diagnostic.clone()];
    let mut remaining_actions = legal_actions.to_vec();
    while choices.len() < limit {
        let selected = &choices.last().unwrap().selected_action;
        if *selected == LegalAction::None {
            break;
        }

        let next_actions = legal_actions_without_selected(&remaining_actions, selected);
        if next_actions.is_empty() || next_actions.len() == remaining_actions.len() {
            break;
        }

        let next = ShantenAgent::diagnose(context, &next_actions);
        if next.selected_action == LegalAction::None {
            break;
        }
        remaining_actions = next_actions;
        choices.push(next);
    }

    choices
}

fn legal_actions_without_selected(
    legal_actions: &[LegalAction],
    selected: &LegalAction,
) -> Vec<LegalAction> {
    let mut excluded = false;
    legal_actions
        .iter()
        .filter(|action| {
            if !excluded && *action == selected {
                excluded = true;
                return false;
            }
            true
        })
        .cloned()
        .collect()
}

struct ChoiceComparison<'a> {
    winner: &'a DiscardCandidateDiagnostic,
    loser: &'a DiscardCandidateDiagnostic,
}

fn choice_comparison<'a>(
    diagnostic: &'a ShantenDecisionDiagnostic,
    choice: &ShantenDecisionDiagnostic,
) -> Option<ChoiceComparison<'a>> {
    if diagnostic.selected_source != AgentActionSource::NormalDiscard
        || choice.selected_source != AgentActionSource::NormalDiscard
    {
        return None;
    }

    let LegalAction::Dahai { tile } = &choice.selected_action else {
        return None;
    };

    let candidates = &diagnostic.normal_discard.as_ref()?.candidates;
    let loser = candidates.iter().find(|candidate| {
        candidate.evaluation.discard == tile.tile_type()
            && candidate.evaluation.discards_red_five == tile.is_red()
    })?;
    let winner = candidates.iter().find(|candidate| candidate.selected)?;
    Some(ChoiceComparison { winner, loser })
}

fn comparison_values(comparison: &ChoiceComparison) -> Option<RankedChoiceComparisonValues> {
    let ChoiceComparison { winner, loser } = comparison;
    let values = match loser.comparison_reason {
        DiscardComparisonReason::CurrentTenpaiOffenseWeightedTotal => {
            RankedChoiceComparisonValues::CurrentTenpaiOffenseWeightedTotal {
                winner: winner.current_tenpai_offense_weighted_total?,
                loser: loser.current_tenpai_offense_weighted_total?,
            }
        }
        DiscardComparisonReason::CurrentTenpaiExpectedSelfTsumoValue => {
            RankedChoiceComparisonValues::SelfTsumoValue {
                winner: winner.current_tenpai_expected_self_tsumo_value?,
                loser: loser.current_tenpai_expected_self_tsumo_value?,
            }
        }
        DiscardComparisonReason::CurrentTenpaiContinuationSelfTsumoValue => {
            RankedChoiceComparisonValues::SelfTsumoValue {
                winner: winner.current_tenpai_continuation_self_tsumo_value?,
                loser: loser.current_tenpai_continuation_self_tsumo_value?,
            }
        }
        DiscardComparisonReason::ExpectedSelfTsumoValue => {
            RankedChoiceComparisonValues::SelfTsumoValue {
                winner: winner.expected_self_tsumo_value?,
                loser: loser.expected_self_tsumo_value?,
            }
        }
        DiscardComparisonReason::UntilRyukyokuExpectedSelfTsumoValue => {
            RankedChoiceComparisonValues::SelfTsumoValue {
                winner: winner.until_ryukyoku_expected_self_tsumo_value?,
                loser: loser.until_ryukyoku_expected_self_tsumo_value?,
            }
        }
        DiscardComparisonReason::TwoShantenExpectedSelfTsumoValue => {
            RankedChoiceComparisonValues::SelfTsumoValue {
                winner: winner.two_shanten_expected_self_tsumo_value?,
                loser: loser.two_shanten_expected_self_tsumo_value?,
            }
        }
        DiscardComparisonReason::TwoShantenProgressSelfTsumoValue => {
            RankedChoiceComparisonValues::SelfTsumoValue {
                winner: winner.two_shanten_progress_self_tsumo_value?,
                loser: loser.two_shanten_progress_self_tsumo_value?,
            }
        }
        DiscardComparisonReason::ThreeShantenProgressSelfTsumoValue => {
            RankedChoiceComparisonValues::SelfTsumoValue {
                winner: winner.three_shanten_progress_self_tsumo_value?,
                loser: loser.three_shanten_progress_self_tsumo_value?,
            }
        }
        DiscardComparisonReason::WeightedProspectiveValue => {
            RankedChoiceComparisonValues::WeightedProspectiveValue {
                winner: winner.prospective_value?,
                loser: loser.prospective_value?,
            }
        }
        DiscardComparisonReason::WeightedTenpaiWaitRemaining => {
            RankedChoiceComparisonValues::WeightedCount {
                winner: winner.tenpai_wait?.weighted_remaining.into(),
                loser: loser.tenpai_wait?.weighted_remaining.into(),
            }
        }
        DiscardComparisonReason::WeightedTenpaiWaitTypeCount => {
            RankedChoiceComparisonValues::WeightedCount {
                winner: winner.tenpai_wait?.weighted_type_count.into(),
                loser: loser.tenpai_wait?.weighted_type_count.into(),
            }
        }
        DiscardComparisonReason::WeightedNextAcceptanceRemaining => {
            RankedChoiceComparisonValues::WeightedCount {
                winner: winner.next_acceptance?.weighted_remaining.into(),
                loser: loser.next_acceptance?.weighted_remaining.into(),
            }
        }
        DiscardComparisonReason::WeightedNextAcceptanceTypeCount => {
            RankedChoiceComparisonValues::WeightedCount {
                winner: winner.next_acceptance?.weighted_type_count.into(),
                loser: loser.next_acceptance?.weighted_type_count.into(),
            }
        }
        DiscardComparisonReason::AcceptanceRemaining => RankedChoiceComparisonValues::Count {
            winner: winner.evaluation.acceptance_total_remaining().into(),
            loser: loser.evaluation.acceptance_total_remaining().into(),
        },
        DiscardComparisonReason::AcceptanceTypeCount => RankedChoiceComparisonValues::Count {
            winner: winner.evaluation.acceptance_type_count() as u64,
            loser: loser.evaluation.acceptance_type_count() as u64,
        },
        _ => return None,
    };
    Some(values)
}

fn hit_probability(comparison: &ChoiceComparison) -> Option<RankedChoiceHitProbability> {
    let ChoiceComparison { winner, loser } = comparison;
    let winner = winner.current_tenpai_self_tsumo_hit_probability;
    let loser = loser.current_tenpai_self_tsumo_hit_probability;
    if winner.is_none() && loser.is_none() {
        return None;
    }
    Some(RankedChoiceHitProbability { winner, loser })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Scenario, ScenarioSpec};
    use bot_core::DiagnosticOptions;

    const NORMAL_SCENARIO: &str = r#"{
        "hand": "234m455p789s1123z",
        "draw": "N",
        "dora_indicators": "3p",
        "round_wind": "E",
        "seat_wind": "S",
        "player_id": 0,
        "oya": 3
    }"#;

    // 打 北 で 2p / 5p の両面テンパイになる局面。
    const REACH_SCENARIO: &str = r#"{
        "hand": "123456789m34p55s",
        "draw": "N"
    }"#;

    const SINGLE_ACTION_SCENARIO: &str = r#"{
        "hand": "234m455p789s1123z",
        "draw": "N",
        "legal_dahai": "N"
    }"#;

    // 同じ 5m を通常牌と赤牌の2通りで切れる局面。
    const RED_FIVE_SCENARIO: &str = r#"{
        "hand": "0m5m234m789p123s11z",
        "legal_dahai": "5m 5mr"
    }"#;

    // 2向聴で、打牌候補が次打牌後の受け入れ残枚数で決着する局面。
    const NEXT_ACCEPTANCE_SCENARIO: &str = r#"{
        "hand": "78m4467p446s3666z",
        "draw": "3z"
    }"#;

    // 1向聴で、打牌候補が重み付けのない受け入れ残枚数で決着する局面。
    const ACCEPTANCE_SCENARIO: &str = r#"{
        "hand": "234567m234p13s5s",
        "draw": "9p",
        "remaining_tiles": 66,
        "player_id": 0,
        "oya": 1,
        "history_furiten": {
            "same_turn": false,
            "riichi_missed_win": false
        }
    }"#;

    // 2向聴で、打牌候補が Progress 枝の self-tsumo 期待支払いで決着する局面。
    const TWO_SHANTEN_SELF_TSUMO_SCENARIO: &str = r#"{
        "hand": "11258m234789p13s",
        "draw": "9s",
        "remaining_tiles": 66,
        "round_wind": "E",
        "seat_wind": "N",
        "player_id": 0,
        "oya": 1,
        "history_furiten": {
            "same_turn": false,
            "riichi_missed_win": false
        }
    }"#;

    fn scenario_from_json(json: &str) -> Scenario {
        let spec: ScenarioSpec = serde_json::from_str(json).unwrap();
        Scenario::resolve(&spec).unwrap()
    }

    fn diagnose(scenario: &Scenario) -> ShantenDecisionDiagnostic {
        ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions)
    }

    fn ranked(
        json: &str,
        limit: usize,
    ) -> (Scenario, ShantenDecisionDiagnostic, Vec<RankedChoice>) {
        let scenario = scenario_from_json(json);
        let diagnostic = diagnose(&scenario);
        let choices = rank_choices(
            &scenario.context,
            &scenario.legal_actions,
            &diagnostic,
            limit,
        );
        (scenario, diagnostic, choices)
    }

    fn selected_candidate(diagnostic: &ShantenDecisionDiagnostic) -> &DiscardCandidateDiagnostic {
        candidates(diagnostic)
            .iter()
            .find(|candidate| candidate.selected)
            .expect("selected candidate")
    }

    fn candidate_for<'a>(
        diagnostic: &'a ShantenDecisionDiagnostic,
        action: &LegalAction,
    ) -> &'a DiscardCandidateDiagnostic {
        let discard = dahai_tile_type(action);
        candidates(diagnostic)
            .iter()
            .find(|candidate| candidate.evaluation.discard == discard)
            .expect("candidate for the action")
    }

    fn candidates(diagnostic: &ShantenDecisionDiagnostic) -> &[DiscardCandidateDiagnostic] {
        &diagnostic
            .normal_discard
            .as_ref()
            .expect("normal discard evaluated")
            .candidates
    }

    fn dahai_tile_type(action: &LegalAction) -> bot_logic::TileType {
        match action {
            LegalAction::Dahai { tile } => tile.tile_type(),
            other => panic!("expected a discard, got {other:?}"),
        }
    }

    #[test]
    fn ranks_three_normal_discard_choices() {
        let (_, diagnostic, choices) = ranked(NORMAL_SCENARIO, 3);
        assert_eq!(choices.len(), 3);
        assert_eq!(choices[0].selected_action, diagnostic.selected_action);
        assert!(
            choices
                .iter()
                .all(|choice| choice.selected_source == AgentActionSource::NormalDiscard)
        );

        // 上位 choice の action を除外して得るので、同じ action は二度現れない。
        for (index, choice) in choices.iter().enumerate() {
            assert!(
                choices[..index]
                    .iter()
                    .all(|earlier| earlier.selected_action != choice.selected_action)
            );
        }
    }

    #[test]
    fn keeps_the_reach_discard_of_the_first_choice() {
        let (_, _, choices) = ranked(REACH_SCENARIO, 3);
        assert_eq!(choices[0].selected_action, LegalAction::Reach);
        assert_eq!(choices[0].selected_source, AgentActionSource::Reach);
        assert_eq!(
            choices[0]
                .selected_discard
                .as_ref()
                .map(|discard| dahai_tile_type(discard).to_mjai_string()),
            Some("N".to_string())
        );
        assert!(choices[0].comparison.is_none());

        // リーチとは経路が違う打牌 choice なので、隣り合っていても比較は取らない。
        assert_eq!(choices[1].selected_source, AgentActionSource::NormalDiscard);
        assert!(choices[1].selected_discard.is_none());
        assert!(choices[1].comparison.is_none());
    }

    #[test]
    fn stops_when_no_lower_choice_remains() {
        let (scenario, _, choices) = ranked(SINGLE_ACTION_SCENARIO, 3);
        assert_eq!(scenario.legal_actions.len(), 1);
        assert_eq!(choices.len(), 1);
        assert!(choices[0].comparison.is_none());
    }

    #[test]
    fn ranks_nothing_for_a_zero_limit() {
        let scenario = scenario_from_json(NORMAL_SCENARIO);
        let diagnostic = diagnose(&scenario);
        assert!(
            rank_choices(&scenario.context, &scenario.legal_actions, &diagnostic, 0).is_empty()
        );
    }

    #[test]
    fn keeps_the_comparison_reason_of_the_previous_choice() {
        let (_, _, choices) = ranked(NORMAL_SCENARIO, 3);
        let comparison = choices[1].comparison.expect("choice 2 comparison");
        assert_eq!(comparison.reason, DiscardComparisonReason::StableOrder);
        // 数値を持たない軸では比較値を作らない。
        assert_eq!(comparison.values, None);
        assert_eq!(
            choices[2].comparison.expect("choice 3 comparison").reason,
            DiscardComparisonReason::ValueHonor
        );
    }

    #[test]
    fn keeps_a_weighted_count_as_a_weighted_count() {
        let (_, diagnostic, choices) = ranked(NEXT_ACCEPTANCE_SCENARIO, 3);
        assert_eq!(choices.len(), 3);

        let winner = selected_candidate(&diagnostic);
        let loser = candidate_for(&diagnostic, &choices[1].selected_action);
        let comparison = choices[1].comparison.expect("choice 2 comparison");
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::WeightedNextAcceptanceRemaining
        );
        assert_eq!(
            comparison.values,
            Some(RankedChoiceComparisonValues::WeightedCount {
                winner: winner
                    .next_acceptance
                    .expect("winner next acceptance")
                    .weighted_remaining
                    .into(),
                loser: loser
                    .next_acceptance
                    .expect("loser next acceptance")
                    .weighted_remaining
                    .into(),
            })
        );
        // 数値を持たない観測値は確定しないので行そのものを作らない。
        assert_eq!(comparison.current_tenpai_self_tsumo_hit_probability, None);

        // choice 3 は choice 1 ではなく choice 2 と比べる。choice 2 が負けた値が choice 3 の
        // winner 値になる。
        let lower = choices[2].comparison.expect("choice 3 comparison");
        let Some(RankedChoiceComparisonValues::WeightedCount {
            winner: lower_winner,
            ..
        }) = lower.values
        else {
            panic!("weighted next acceptance is a weighted count");
        };
        assert_eq!(
            Some(lower_winner),
            loser
                .next_acceptance
                .map(|acceptance| acceptance.weighted_remaining.into())
        );
    }

    #[test]
    fn keeps_a_plain_acceptance_count_as_a_count() {
        let (_, diagnostic, choices) = ranked(ACCEPTANCE_SCENARIO, 3);

        let winner = selected_candidate(&diagnostic);
        let loser = candidate_for(&diagnostic, &choices[1].selected_action);
        let comparison = choices[1].comparison.expect("choice 2 comparison");
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::AcceptanceRemaining
        );
        assert_eq!(
            comparison.values,
            Some(RankedChoiceComparisonValues::Count {
                winner: winner.evaluation.acceptance_total_remaining().into(),
                loser: loser.evaluation.acceptance_total_remaining().into(),
            })
        );
    }

    #[test]
    fn keeps_a_self_tsumo_comparison_as_a_self_tsumo_value() {
        let (_, diagnostic, choices) = ranked(TWO_SHANTEN_SELF_TSUMO_SCENARIO, 3);

        let winner = selected_candidate(&diagnostic);
        let loser = candidate_for(&diagnostic, &choices[1].selected_action);
        let comparison = choices[1].comparison.expect("choice 2 comparison");
        assert_eq!(
            comparison.reason,
            DiscardComparisonReason::TwoShantenProgressSelfTsumoValue
        );
        assert_eq!(
            comparison.values,
            Some(RankedChoiceComparisonValues::SelfTsumoValue {
                winner: winner
                    .two_shanten_progress_self_tsumo_value
                    .expect("winner progress self-tsumo value"),
                loser: loser
                    .two_shanten_progress_self_tsumo_value
                    .expect("loser progress self-tsumo value"),
            })
        );
    }

    #[test]
    fn excluding_a_red_five_keeps_the_other_five() {
        let scenario = scenario_from_json(RED_FIVE_SCENARIO);
        let diagnostic = diagnose(&scenario);
        assert_eq!(scenario.legal_actions.len(), 2);

        let remaining =
            legal_actions_without_selected(&scenario.legal_actions, &diagnostic.selected_action);
        assert_eq!(remaining.len(), 1);
        assert_ne!(remaining[0], diagnostic.selected_action);

        let choices = rank_choices(&scenario.context, &scenario.legal_actions, &diagnostic, 3);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].selected_action, diagnostic.selected_action);
        assert_eq!(choices[1].selected_action, remaining[0]);
    }

    #[test]
    fn excluding_the_selected_action_keeps_the_remaining_order() {
        let scenario = scenario_from_json(
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "legal_dahai": "4p 7s 9s"
            }"#,
        );
        let selected = scenario.legal_actions[1].clone();
        let remaining = legal_actions_without_selected(&scenario.legal_actions, &selected);
        assert_eq!(
            remaining,
            [
                scenario.legal_actions[0].clone(),
                scenario.legal_actions[2].clone()
            ]
        );
    }

    // choice 1 は呼び出し側が構築した診断そのもので、別の診断範囲で取り直さない。
    #[test]
    fn keeps_the_given_primary_diagnostic_as_the_first_choice() {
        let scenario = scenario_from_json(NORMAL_SCENARIO);
        let diagnostic = ShantenAgent::diagnose_with_options(
            &scenario.context,
            &scenario.legal_actions,
            DiagnosticOptions::WITH_LOOKAHEAD,
        );
        assert!(diagnostic.normal_discard_lookahead.is_some());

        let choices = diagnose_choices(&scenario.context, &scenario.legal_actions, &diagnostic, 3);
        assert_eq!(choices.len(), 3);
        assert_eq!(choices[0], diagnostic);
        // 下位 choice は production の既定範囲で再診断するだけで、追加探索を有効にしない。
        assert!(choices[1..].iter().all(|choice| {
            choice.normal_discard_lookahead.is_none()
                && choice.normal_discard_lookahead_value.is_none()
                && choice.normal_discard_tenpai_continuation.is_none()
        }));
    }
}
