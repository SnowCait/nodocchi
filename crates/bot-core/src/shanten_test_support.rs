use crate::action::LegalAction;
use crate::context::GameContext;
use crate::meld::{Meld, MeldKind};
use bot_logic::{HistoryFuritenFacts, TileId, TileType};

pub(crate) fn tile(value: u8) -> TileId {
    TileId::new(value).unwrap()
}

pub(crate) fn dahai(value: u8) -> LegalAction {
    LegalAction::Dahai { tile: tile(value) }
}

pub(crate) fn pon_meld() -> Meld {
    Meld::new(
        MeldKind::Pon,
        vec![tile(108), tile(109), tile(110)],
        Some(tile(108)),
    )
}

const TENPAI_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 44, 48, 89, 90];
const TENPAI_DRAWN: u8 = 116;
pub(crate) const TENPAI_SCARCE_VISIBLE: [u8; 6] = [40, 41, 42, 53, 54, 55];

pub(crate) fn tenpai_context(extra_visible: &[u8]) -> GameContext {
    let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
    let mut visible = hand.clone();
    visible.push(tile(TENPAI_DRAWN));
    visible.extend(extra_visible.iter().map(|&value| tile(value)));
    GameContext::from_parts_with_visible_tiles(
        Some(tile(TENPAI_DRAWN)),
        hand,
        vec![],
        None,
        None,
        visible,
    )
}

pub(crate) fn tenpai_dahai_actions() -> Vec<LegalAction> {
    TENPAI_HAND
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(TENPAI_DRAWN)])
        .collect()
}

pub(crate) fn tenpai_actions() -> Vec<LegalAction> {
    tenpai_dahai_actions()
        .into_iter()
        .chain([LegalAction::Reach])
        .collect()
}

pub(crate) fn tenpai_under_reach_context(oya: Option<u8>, reached: [bool; 4]) -> GameContext {
    let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
    GameContext::from_parts_with_table_state(
        Some(tile(TENPAI_DRAWN)),
        hand,
        vec![],
        None,
        None,
        Vec::new(),
        Some(0),
        oya,
        [vec![], vec![tile(16)], vec![], vec![]],
        reached,
    )
}

const WEAK_TENPAI_HAND: [u8; 13] = [0, 4, 8, 12, 13, 20, 24, 28, 32, 36, 40, 44, 89];
const WEAK_TENPAI_DRAWN: u8 = 88;

pub(crate) fn weak_tenpai_under_reach_context() -> GameContext {
    GameContext::from_parts_with_table_state(
        Some(tile(WEAK_TENPAI_DRAWN)),
        WEAK_TENPAI_HAND.iter().map(|&value| tile(value)).collect(),
        vec![],
        None,
        None,
        [4u8, 5, 6, 7].iter().map(|&value| tile(value)).collect(),
        Some(0),
        None,
        [vec![], vec![tile(1)], vec![], vec![]],
        [false, true, false, false],
    )
}

pub(crate) fn weak_tenpai_actions() -> Vec<LegalAction> {
    WEAK_TENPAI_HAND
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(WEAK_TENPAI_DRAWN)])
        .collect()
}

const FOLD_HAND: [u8; 13] = [0, 4, 17, 20, 36, 40, 56, 60, 89, 108, 112, 120, 124];
const FOLD_DRAWN: u8 = 16;

pub(crate) fn fold_under_reach_context() -> GameContext {
    let hand: Vec<_> = FOLD_HAND.iter().map(|&value| tile(value)).collect();
    GameContext::from_parts_with_table_state(
        Some(tile(FOLD_DRAWN)),
        hand,
        vec![],
        None,
        None,
        Vec::new(),
        Some(0),
        None,
        [vec![], vec![tile(89)], vec![], vec![]],
        [false, true, false, false],
    )
}

pub(crate) fn fold_actions() -> Vec<LegalAction> {
    FOLD_HAND
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(FOLD_DRAWN)])
        .collect()
}

pub(crate) fn opponent_reach_context(drawn_tile: Option<u8>, hand_values: &[u8]) -> GameContext {
    opponent_reach_context_with_visible(drawn_tile, hand_values, &[])
}

pub(crate) fn opponent_reach_context_with_visible(
    drawn_tile: Option<u8>,
    hand_values: &[u8],
    visible_values: &[u8],
) -> GameContext {
    GameContext::from_parts_with_table_state(
        drawn_tile.map(tile),
        hand_values.iter().map(|&value| tile(value)).collect(),
        vec![],
        None,
        None,
        visible_values.iter().map(|&value| tile(value)).collect(),
        Some(0),
        None,
        [vec![], vec![tile(16)], vec![], vec![]],
        [false, true, false, false],
    )
}

fn unavailable_reach_meld() -> Meld {
    let tiles = vec![tile(68), tile(69), tile(70)];
    Meld::new(MeldKind::Pon, tiles.clone(), Some(tiles[0]))
}

pub(crate) fn suited_reach_context(
    drawn_tile: Option<u8>,
    hand_values: &[u8],
    visible_values: &[u8],
    reacher_discards: &[u8],
) -> GameContext {
    suited_reach_context_with_reached(
        drawn_tile,
        hand_values,
        visible_values,
        reacher_discards,
        [false, true, false, false],
    )
}

pub(crate) fn suited_reach_context_with_reached(
    drawn_tile: Option<u8>,
    hand_values: &[u8],
    visible_values: &[u8],
    reacher_discards: &[u8],
    reached: [bool; 4],
) -> GameContext {
    let discards = [
        vec![],
        reacher_discards.iter().map(|&value| tile(value)).collect(),
        vec![],
        vec![],
    ];
    let mut melds: [Vec<Meld>; 4] = Default::default();
    if reached.iter().filter(|&&is_reached| is_reached).count() >= 2 {
        let unavailable_player = reached
            .iter()
            .enumerate()
            .filter(|(player, is_reached)| *player != 0 && **is_reached)
            .nth(1)
            .map(|(player, _)| player)
            .expect("two reached opponents");
        melds[unavailable_player] = vec![unavailable_reach_meld()];
    }
    GameContext::from_parts_with_melds(
        drawn_tile.map(tile),
        hand_values.iter().map(|&value| tile(value)).collect(),
        vec![],
        None,
        None,
        visible_values.iter().map(|&value| tile(value)).collect(),
        Some(0),
        None,
        discards,
        reached,
        melds,
    )
}

pub(crate) const OPPONENT_MELD_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
pub(crate) const OPPONENT_MELD_DRAW: u8 = 120;

pub(crate) fn opponent_meld_actions() -> Vec<LegalAction> {
    OPPONENT_MELD_HAND
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(OPPONENT_MELD_DRAW)])
        .collect()
}

pub(crate) fn pinfu_tanyao_context_and_actions() -> (GameContext, Vec<LegalAction>) {
    const HAND: [&str; 13] = [
        "2m", "3m", "4m", "6m", "7m", "8m", "2p", "2p", "3s", "4s", "5s", "4s", "5s",
    ];

    let mut used = [false; 136];
    let mut allocate = |value: &str| {
        let tile_type = TileType::from_mjai_type_str(value).unwrap();
        let tile = TileId::copies(tile_type)
            .find(|tile| !tile.is_red() && !used[tile.index()])
            .expect("fixture does not reuse a physical tile");
        used[tile.index()] = true;
        tile
    };
    let hand: Vec<_> = HAND.iter().map(|value| allocate(value)).collect();
    let drawn = allocate("N");
    let visible = hand.iter().chain([&drawn]).copied().collect();
    let actions = hand
        .iter()
        .chain([&drawn])
        .map(|&tile| LegalAction::Dahai { tile })
        .chain([LegalAction::Reach])
        .collect();
    let context = GameContext::from_parts_with_table_state(
        Some(drawn),
        hand,
        vec![],
        TileType::from_mjai_type_str("E").ok(),
        TileType::from_mjai_type_str("S").ok(),
        visible,
        Some(0),
        Some(3),
        Default::default(),
        [false; 4],
    )
    .with_history_furiten_facts(HistoryFuritenFacts {
        same_turn: Some(false),
        riichi_missed_win: Some(false),
    });

    (context, actions)
}
