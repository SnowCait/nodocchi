//! 防御候補 ordering が既存 production selector と一致することの検証。
//!
//! ranking は rank 1 だけを別扱いせず、`ordered_defense_fallback_candidates` の先頭が既存
//! selector の選択と一致する。

use super::common::*;
use super::hidden_hand_states::{ankan, reached_fixture};
use crate::action::LegalAction;
use crate::defense::*;
use crate::meld::{Meld, MeldKind};
use bot_logic::TileId;

// production evaluation が実際に構築する exact evidence と同じものを ordering へ渡す。
fn ordered<'a>(
    context: &GameContext,
    actions: &'a [LegalAction],
) -> Vec<(&'a LegalAction, DefenseFallbackKind)> {
    let vectors =
        evaluate_defense_fallback_action_with_kind(context, actions, true).ron_risk_vectors;
    ordered_defense_fallback_candidates(context, actions, vectors.as_deref())
}

// ordering の先頭が既存 selector の選択と一致することを確認する。NoSafety の数牌は既存 selector
// の対象外なので、順位だけ持たせて末尾へ置く。
fn assert_ordering_leads_with_the_selection(context: &GameContext, actions: &[LegalAction]) {
    let ordered = ordered(context, actions);
    let selectable = ordered
        .iter()
        .find(|&&(_, kind)| kind != DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoSafety))
        .copied();

    assert_eq!(
        selectable,
        select_defense_fallback_action_with_kind(context, actions)
    );

    // 合法 Dahai を牌種ごとに1件ずつ、順位を落とさず並べる。
    let mut tile_types: Vec<_> = actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => Some(tile.tile_type()),
            _ => None,
        })
        .collect();
    tile_types.sort_by_key(|tile| tile.index());
    tile_types.dedup();
    assert_eq!(ordered.len(), tile_types.len());
}

#[test]
fn the_genbutsu_tier_leads_the_ordering() {
    let context = suited_context(
        vec![],
        [vec![], vec![tile(16)], vec![], vec![]],
        [false, true, false, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(108) },
        LegalAction::Dahai { tile: tile(16) },
    ];

    let ordered = ordered(&context, &actions);
    assert_eq!(
        ordered.first().copied(),
        Some((&actions[2], DefenseFallbackKind::Genbutsu))
    );
    // 現物の後ろは exact `R/T` の順で並ぶ。現物で決着した局面でも exact evidence を共有する。
    assert!(
        ordered[1..]
            .iter()
            .all(|&(_, kind)| kind == DefenseFallbackKind::ExactRonRisk),
        "{ordered:?}"
    );
    assert_ordering_leads_with_the_selection(&context, &actions);
}

#[test]
fn the_exact_tier_orders_candidates_by_the_production_comparator() {
    let context = reached_fixture(
        &[("1m", 1), ("2m", 2)],
        ["5p", "6p", "7p", "8p"].map(ankan).to_vec(),
        &[],
        &[],
    );
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
        LegalAction::Dahai {
            tile: discarded("2m"),
        },
    ];

    let ordered = ordered(&context, &actions);
    let vectors = reached_opponents_dahai_actions_by_ron_risk(&context, &actions).unwrap();
    let expected: Vec<_> = {
        let mut sorted: Vec<_> = vectors.iter().collect();
        sorted.sort_by(|left, right| {
            compare_lexicographic_minimax_ron_risk(&left.player_evidence, &right.player_evidence)
                .expect("exact 比較できる")
        });
        sorted.into_iter().map(|vector| vector.action).collect()
    };

    assert_eq!(
        ordered
            .iter()
            .map(|&(action, _)| action)
            .collect::<Vec<_>>(),
        expected
    );
    assert_ordering_leads_with_the_selection(&context, &actions);
}

#[test]
fn the_legacy_tier_orders_honor_before_suited() {
    // exact model が unavailable な複数リーチ。既存 selector と同じく字牌を先に置く。
    let context = legacy_suited_context(
        vec![tile(108), tile(109)],
        [vec![], vec![tile(12)], vec![tile(13)], vec![]],
        [false, true, true, false],
    );
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(108) },
    ];

    assert_eq!(
        ordered(&context, &actions),
        vec![
            (
                &actions[1],
                DefenseFallbackKind::HonorSafety(HonorSafetyRank::TwoVisible)
            ),
            (
                &actions[0],
                DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
            ),
        ]
    );
    assert_ordering_leads_with_the_selection(&context, &actions);
}

// 場風 E・親 1 なので player 1 にとって E は連風牌。E を1枚だけ見えている状態にして
// OneVisible にし、player 1 の河の 4m で 1m を完全スジにする。副露で exact model を
// unavailable にして legacy 段そのものを検証する。
fn double_wind_legacy_context() -> GameContext {
    let mut melds: [Vec<Meld>; 4] = Default::default();
    let pon_tiles: Vec<_> = TileId::copies(tile_type("9p")).take(3).collect();
    melds[1] = vec![Meld::new(
        MeldKind::Pon,
        pon_tiles.clone(),
        Some(pon_tiles[0]),
    )];
    GameContext::from_parts_with_melds(
        None,
        vec![],
        vec![],
        Some(honor(EAST)),
        None,
        vec![held("E")],
        Some(0),
        Some(1),
        [vec![], vec![discarded("4m")], vec![], vec![]],
        [false, true, false, false],
        melds,
    )
}

#[test]
fn the_legacy_tier_uses_the_existing_cross_comparison_for_a_double_wind() {
    let context = double_wind_legacy_context();
    let actions = vec![
        LegalAction::Dahai {
            tile: discarded("E"),
        },
        LegalAction::Dahai {
            tile: discarded("1m"),
        },
    ];

    assert_eq!(
        honor_safety_rank(tile_type("E"), &context),
        Some(HonorSafetyRank::OneVisible)
    );
    assert_eq!(
        opponent_honor_value_for_reached(tile_type("E"), &context),
        Some(OpponentHonorValue::DoubleWind)
    );
    assert!(suited_safety_outweighs_honor(
        HonorSafetyRank::OneVisible,
        Some(OpponentHonorValue::DoubleWind),
        SuitedSafetyRank::Suji,
    ));

    // 1枚見えの連風牌より完全スジの数牌を先に置く既存の限定的な横断比較をそのまま使う。
    assert_eq!(
        ordered(&context, &actions),
        vec![
            (
                &actions[1],
                DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
            ),
            (
                &actions[0],
                DefenseFallbackKind::HonorSafety(HonorSafetyRank::OneVisible)
            ),
        ]
    );
    assert_ordering_leads_with_the_selection(&context, &actions);
}

#[test]
fn a_no_safety_suited_candidate_keeps_a_rank_without_being_selected() {
    let context = legacy_suited_context(vec![], Default::default(), [false, true, true, false]);
    let actions = vec![
        LegalAction::Dahai { tile: tile(0) },
        LegalAction::Dahai { tile: tile(16) },
    ];

    // 既存 selector は NoSafety を選ばないので None のまま。ranking では順位を持つ。
    assert_eq!(
        select_defense_fallback_action_with_kind(&context, &actions),
        None
    );
    let ordered = ordered(&context, &actions);
    assert_eq!(ordered.len(), 2);
    assert!(
        ordered.iter().all(|&(_, kind)| kind
            == DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoSafety)),
        "{ordered:?}"
    );
    assert_ordering_leads_with_the_selection(&context, &actions);
}

#[test]
fn the_ordering_normalizes_a_red_five_to_the_black_five_of_the_same_type() {
    let context = suited_context(
        vec![],
        [vec![], vec![tile(16)], vec![], vec![]],
        [false, true, false, false],
    );
    // 16 は 5m の赤5、17 は同牌種の黒5。
    let actions = vec![
        LegalAction::Dahai { tile: tile(16) },
        LegalAction::Dahai { tile: tile(17) },
        LegalAction::Dahai { tile: tile(0) },
    ];

    let ordered = ordered(&context, &actions);
    assert_eq!(
        ordered.first().copied(),
        Some((&actions[1], DefenseFallbackKind::Genbutsu))
    );
    // 同じ牌種は1つの候補として扱う。
    assert_eq!(ordered.len(), 2);
    assert_ordering_leads_with_the_selection(&context, &actions);
}
