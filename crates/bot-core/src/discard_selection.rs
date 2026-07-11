use crate::action::LegalAction;
use crate::context::GameContext;
use bot_logic::{
    AcceptanceTile, DiscardCandidateDiagnostic, DiscardDecisionDiagnostic, DiscardEvaluation,
    TileCounts, TileId, diagnose_discard_evaluations, evaluate_discards_from_tiles_with_context,
    evaluate_discards_from_tiles_with_visible_tiles, select_best_discard_from_tiles_with_context,
    select_best_discard_from_tiles_with_visible_tiles,
};

const LOG_TARGET: &str = "bot_core::discard_selection";

pub fn select_discard_action(
    context: &GameContext,
    legal_actions: &[LegalAction],
) -> Option<LegalAction> {
    let tiles: Vec<_> = context
        .hand_tiles()
        .iter()
        .copied()
        .chain(context.drawn_tile())
        .collect();

    let selected = if tracing::enabled!(target: LOG_TARGET, tracing::Level::DEBUG) {
        select_best_discard_with_diagnostics(context, &tiles)
    } else {
        select_best_discard_evaluation(context, &tiles)
    };
    let selected_type = selected?.discard;

    let mut red_fallback = None;
    for action in legal_actions {
        let LegalAction::Dahai { tile } = action else {
            continue;
        };
        if tile.tile_type() != selected_type {
            continue;
        }
        if tile.is_red() {
            red_fallback.get_or_insert_with(|| action.clone());
        } else {
            return Some(action.clone());
        }
    }

    red_fallback
}

pub(crate) fn select_best_discard_evaluation(
    context: &GameContext,
    tiles: &[TileId],
) -> Option<DiscardEvaluation> {
    if context.visible_tiles().is_empty() {
        select_best_discard_from_tiles_with_context(
            tiles,
            context.dora_indicators(),
            context.round_wind(),
            context.seat_wind(),
        )
    } else {
        select_best_discard_from_tiles_with_visible_tiles(
            tiles,
            context.dora_indicators(),
            context.round_wind(),
            context.seat_wind(),
            context.visible_tiles(),
        )
    }
}

fn select_best_discard_with_diagnostics(
    context: &GameContext,
    tiles: &[TileId],
) -> Option<DiscardEvaluation> {
    let evaluations = if context.visible_tiles().is_empty() {
        evaluate_discards_from_tiles_with_context(
            tiles,
            context.dora_indicators(),
            context.round_wind(),
            context.seat_wind(),
        )
    } else {
        evaluate_discards_from_tiles_with_visible_tiles(
            tiles,
            context.dora_indicators(),
            context.round_wind(),
            context.seat_wind(),
            context.visible_tiles(),
        )
    };

    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let diagnostic = diagnose_discard_evaluations(&counts, &evaluations);
    log_discard_diagnostic(context, tiles, &diagnostic);
    diagnostic.selected
}

fn tiles_to_mjai(tiles: &[TileId]) -> String {
    tiles
        .iter()
        .map(|tile| tile.to_mjai_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn log_discard_diagnostic(
    context: &GameContext,
    tiles: &[TileId],
    diagnostic: &DiscardDecisionDiagnostic,
) {
    let Some(selected) = diagnostic.selected.as_ref() else {
        return;
    };

    let hand_tiles = tiles_to_mjai(context.hand_tiles());
    let all_tiles = tiles_to_mjai(tiles);
    let drawn_tile = context.drawn_tile().map(|tile| tile.to_mjai_string());
    let dora_indicators = tiles_to_mjai(context.dora_indicators());
    let round_wind = context.round_wind().map(|wind| wind.to_mjai_string());
    let seat_wind = context.seat_wind().map(|wind| wind.to_mjai_string());

    tracing::debug!(
        target: LOG_TARGET,
        hand_tiles = %hand_tiles,
        drawn_tile = ?drawn_tile,
        all_tiles = %all_tiles,
        dora_indicators = %dora_indicators,
        round_wind = ?round_wind,
        seat_wind = ?seat_wind,
        visible_tile_count = context.visible_tiles().len(),
        candidate_count = diagnostic.candidates.len(),
        selected_discard = %selected.discard.to_mjai_string(),
        selected_standard_shanten = selected.shanten_after_discard.standard,
        selected_chiitoitsu_shanten = selected.shanten_after_discard.chiitoitsu,
        selected_kokushi_shanten = selected.shanten_after_discard.kokushi,
        selected_min_shanten = selected.min_shanten_after_discard(),
        selected_acceptance_total_remaining = selected.acceptance_total_remaining(),
        selected_acceptance_type_count = selected.acceptance_type_count(),
        selected_shape_penalty = selected.shape_penalty,
        selected_floating_tile_value = selected.floating_tile_value,
        selected_discarded_dora_count = selected.discarded_dora_count,
        selected_discarded_value_honor_count = selected.discarded_value_honor_count,
        selected_discards_red_five = selected.discards_red_five,
        "discard decision",
    );

    if tracing::enabled!(target: LOG_TARGET, tracing::Level::TRACE) {
        for candidate in &diagnostic.candidates {
            log_discard_candidate(candidate);
        }
    }
}

fn acceptance_tile_diagnostic(tile: &AcceptanceTile) -> (String, u8, i8, i8, i8, i8) {
    (
        tile.tile.to_mjai_string(),
        tile.remaining,
        tile.shanten_after_draw.standard,
        tile.shanten_after_draw.chiitoitsu,
        tile.shanten_after_draw.kokushi,
        tile.shanten_after_draw.min(),
    )
}

fn log_discard_candidate(candidate: &DiscardCandidateDiagnostic) {
    let evaluation = &candidate.evaluation;
    let acceptance_tiles = evaluation
        .acceptance_after_discard
        .tiles
        .iter()
        .map(acceptance_tile_diagnostic)
        .collect::<Vec<_>>();

    tracing::trace!(
        target: LOG_TARGET,
        discard = %evaluation.discard.to_mjai_string(),
        selected = candidate.selected,
        selected_is_strictly_better_than_candidate =
            candidate.selected_is_strictly_better_than_candidate,
        comparison_reason = ?candidate.comparison_reason,
        count_before_discard = evaluation.count_before_discard,
        standard_shanten_after_discard = evaluation.shanten_after_discard.standard,
        chiitoitsu_shanten_after_discard = evaluation.shanten_after_discard.chiitoitsu,
        kokushi_shanten_after_discard = evaluation.shanten_after_discard.kokushi,
        min_shanten_after_discard = evaluation.min_shanten_after_discard(),
        acceptance_total_remaining = evaluation.acceptance_total_remaining(),
        acceptance_type_count = evaluation.acceptance_type_count(),
        acceptance_tiles = ?acceptance_tiles,
        shape_penalty = evaluation.shape_penalty,
        floating_tile_value = evaluation.floating_tile_value,
        discarded_dora_count = evaluation.discarded_dora_count,
        discarded_value_honor_count = evaluation.discarded_value_honor_count,
        discards_red_five = evaluation.discards_red_five,
        shape_breakdown = ?candidate.shape_breakdown,
        pair_context = ?candidate.pair_context,
        block_context = ?candidate.block_context,
        floating_tile_value_breakdown = ?candidate.floating_tile_value_breakdown,
        "discard candidate",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_logic::TileId;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn dahai(value: u8) -> LegalAction {
        LegalAction::Dahai { tile: tile(value) }
    }

    #[test]
    fn returns_none_for_empty_legal_actions() {
        let context = GameContext::from_parts(Some(tile(0)), vec![tile(4)]);
        assert_eq!(select_discard_action(&context, &[]), None);
    }

    #[test]
    fn returns_none_without_dahai_action() {
        let context = GameContext::from_parts(Some(tile(0)), vec![tile(1)]);
        let actions = vec![LegalAction::Reach, LegalAction::None];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn returns_none_without_context_tiles() {
        let context = GameContext::default();
        let actions = vec![dahai(0)];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn returns_dahai_matching_best_discard() {
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116)])
            .collect();

        let selected_action = select_discard_action(&context, &actions).unwrap();

        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();
        let selected_type = bot_logic::select_best_discard_from_tiles(&tiles)
            .unwrap()
            .discard;

        assert!(matches!(
            selected_action,
            LegalAction::Dahai { tile } if tile.tile_type() == selected_type
        ));
    }

    #[test]
    fn evaluates_drawn_tile() {
        let context = GameContext::with_drawn_tile(tile(0));
        let actions = vec![dahai(0)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(0)));
    }

    #[test]
    fn evaluates_hand_tiles() {
        let context = GameContext::with_hand_tiles(vec![tile(0), tile(4), tile(8)]);
        let actions = vec![dahai(0), dahai(4), dahai(8)];
        assert!(matches!(
            select_discard_action(&context, &actions),
            Some(LegalAction::Dahai { .. })
        ));
    }

    #[test]
    fn returns_first_dahai_of_same_tile_type() {
        let context = GameContext::from_parts(Some(tile(16)), vec![tile(17)]);
        let actions = vec![dahai(17), dahai(16)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(17)));
    }

    #[test]
    fn prefers_black_five_over_red_of_selected_type() {
        let context = GameContext::from_parts(None, vec![tile(16), tile(17)]);
        let actions = vec![dahai(16), dahai(17)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(17)));
    }

    #[test]
    fn falls_back_to_red_five_when_only_red_available() {
        let context = GameContext::from_parts(None, vec![tile(16)]);
        let actions = vec![dahai(16)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(16)));
    }

    #[test]
    fn returns_none_without_context_tiles_even_with_dahai() {
        let context = GameContext::default();
        let actions = vec![dahai(16)];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn returns_none_when_selected_type_has_no_dahai() {
        let context = GameContext::with_hand_tiles(vec![tile(0)]);
        let actions = vec![dahai(4)];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn perfect_tie_avoids_discarding_dora() {
        // 123m 456m 789m 123p + 東(浮き) 西(浮き), ドラ表示 南 -> 西 がドラ
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_dora(Some(tile(116)), hand, vec![tile(112)]);
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108, 116]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "E");
    }

    #[test]
    fn discards_dora_when_it_lowers_shanten() {
        // 5m を切るとテンパイになる形。5m がドラでも向聴を優先して切る
        let hand: Vec<_> = [40u8, 44, 48, 56, 60, 64, 76, 80, 84, 108, 109, 96, 100]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_dora(Some(tile(16)), hand, vec![tile(12)]);
        let actions: Vec<LegalAction> =
            [40u8, 44, 48, 56, 60, 64, 76, 80, 84, 108, 109, 96, 100, 16]
                .iter()
                .map(|&value| dahai(value))
                .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "5m");
    }

    #[test]
    fn prefers_black_five_over_red_with_dora_indicator() {
        // 赤5と通常5が併存する場合は通常5を切る
        let context =
            GameContext::from_parts_with_dora(None, vec![tile(16), tile(17)], vec![tile(12)]);
        let actions = vec![dahai(16), dahai(17)];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(17)));
    }

    #[test]
    fn empty_tiles_yield_no_action_with_dora() {
        let context = GameContext::from_parts_with_dora(None, vec![], vec![tile(12)]);
        let actions = vec![dahai(0)];
        assert_eq!(select_discard_action(&context, &actions), None);
    }

    #[test]
    fn perfect_tie_keeps_value_honor() {
        // 123m 456m 789m 123p + 中(浮き) 北(浮き)。役牌でない北を切る
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context =
            GameContext::from_parts_with_context(Some(tile(120)), hand, vec![], None, None);
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 120]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "N");
    }

    #[test]
    fn round_wind_makes_wind_harder_to_discard() {
        // 東場。孤立した東(場風)と北(客風)では、役牌でない北を切る
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_context(
            Some(tile(120)),
            hand,
            vec![],
            Some(bot_logic::TileType::new(27).unwrap()),
            Some(bot_logic::TileType::new(28).unwrap()),
        );
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108, 120]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "N");
    }

    #[test]
    fn double_wind_kept_over_single_value_honor() {
        // 東場東家。ダブル東(場風かつ自風)と中(単役牌)では中を切る
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_context(
            Some(tile(132)),
            hand,
            vec![],
            Some(bot_logic::TileType::new(27).unwrap()),
            Some(bot_logic::TileType::new(27).unwrap()),
        );
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 108, 132]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "C");
    }

    #[test]
    fn shanten_outranks_value_honor() {
        // 中を切るとテンパイ。中が役牌でも向聴を優先して切る
        let hand: Vec<_> = [40u8, 44, 48, 56, 60, 64, 76, 80, 84, 108, 109, 96, 100]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_context(
            Some(tile(132)),
            hand,
            vec![],
            Some(bot_logic::TileType::new(27).unwrap()),
            Some(bot_logic::TileType::new(28).unwrap()),
        );
        let actions: Vec<LegalAction> =
            [40u8, 44, 48, 56, 60, 64, 76, 80, 84, 108, 109, 96, 100, 132]
                .iter()
                .map(|&value| dahai(value))
                .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "C");
    }

    #[test]
    fn dora_outranks_value_honor() {
        // 中(役牌・非ドラ)と北(客風・ドラ)。ドラを温存し中を切る
        // ドラ表示 西 -> 北 がドラ
        let hand: Vec<_> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132]
            .iter()
            .map(|&value| tile(value))
            .collect();
        let context = GameContext::from_parts_with_context(
            Some(tile(120)),
            hand,
            vec![tile(116)],
            Some(bot_logic::TileType::new(27).unwrap()),
            Some(bot_logic::TileType::new(28).unwrap()),
        );
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 132, 120]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "C");
    }

    fn tiles(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&value| tile(value)).collect()
    }

    #[test]
    fn uses_visible_tiles_when_present() {
        let hand = tiles(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36]);
        let mut visible = hand.clone();
        visible.extend(tiles(&[68, 69, 70, 71]));
        let context = GameContext::from_parts_with_visible_tiles(
            Some(tile(68)),
            hand,
            vec![],
            None,
            None,
            visible,
        );
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36, 68]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "9p");
    }

    #[test]
    fn empty_visible_tiles_falls_back_to_context_path() {
        let hand = tiles(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36]);
        let context =
            GameContext::from_parts_with_context(Some(tile(68)), hand, vec![], None, None);
        let actions: Vec<LegalAction> = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36, 68]
            .iter()
            .map(|&value| dahai(value))
            .collect();

        let selected = select_discard_action(&context, &actions).unwrap();
        let LegalAction::Dahai { tile } = selected else {
            panic!("expected dahai");
        };
        assert_eq!(tile.tile_type().to_mjai_string(), "1p");
    }

    #[test]
    fn diagnostics_path_selects_same_discard_as_normal_path() {
        let hand_values = [0u8, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
        let context = GameContext::from_parts_with_context(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
            vec![tile(12)],
            Some(bot_logic::TileType::new(27).unwrap()),
            Some(bot_logic::TileType::new(28).unwrap()),
        );
        let tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();

        let normal = select_best_discard_evaluation(&context, &tiles);
        let diagnostic = select_best_discard_with_diagnostics(&context, &tiles);
        assert_eq!(normal, diagnostic);
        assert!(normal.is_some());
    }

    #[test]
    fn diagnostics_path_matches_normal_path_with_visible_tiles() {
        let hand = tiles(&[0, 4, 8, 12, 17, 20, 24, 28, 32, 48, 53, 56, 36]);
        let mut visible = hand.clone();
        visible.extend(tiles(&[68, 69, 70, 71]));
        let context = GameContext::from_parts_with_visible_tiles(
            Some(tile(68)),
            hand,
            vec![],
            None,
            None,
            visible,
        );
        let all_tiles: Vec<_> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .collect();

        let normal = select_best_discard_evaluation(&context, &all_tiles);
        let diagnostic = select_best_discard_with_diagnostics(&context, &all_tiles);
        assert_eq!(normal, diagnostic);
        assert!(normal.is_some());
    }

    #[test]
    fn acceptance_tile_diagnostic_preserves_all_shanten_kinds() {
        use bot_logic::{AcceptanceTile, Shanten, TileType};

        let source = AcceptanceTile {
            tile: TileType::from_mjai_type_str("5mr").unwrap(),
            remaining: 3,
            shanten_after_draw: Shanten {
                standard: 1,
                chiitoitsu: 2,
                kokushi: 5,
            },
        };
        let before = source;

        let (tile, remaining, standard, chiitoitsu, kokushi, min) =
            acceptance_tile_diagnostic(&source);

        assert_eq!(tile, "5m");
        assert_eq!(remaining, 3);
        assert_eq!(standard, 1);
        assert_eq!(chiitoitsu, 2);
        assert_eq!(kokushi, 5);
        assert_eq!(min, 1);
        assert_eq!(source, before);
    }

    #[test]
    fn does_not_select_non_dahai_actions() {
        let context = GameContext::with_drawn_tile(tile(0));
        let actions = vec![
            LegalAction::Hora,
            LegalAction::Reach,
            LegalAction::Ryukyoku,
            LegalAction::None,
            dahai(0),
        ];
        assert_eq!(select_discard_action(&context, &actions), Some(dahai(0)));
    }
}
