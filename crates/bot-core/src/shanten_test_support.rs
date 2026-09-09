use crate::action::LegalAction;
use crate::context::{GameContext, TableStateFacts};
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

pub(crate) const TENPAI_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 44, 48, 89, 90];
pub(crate) const TENPAI_DRAWN: u8 = 116;
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

// 待ち枚数が足りないテンパイで他家リーチを受ける局面。リーチ者の河に 1m を置いて手牌の
// 1m を現物にし、2m を4枚見えにする。
pub(crate) fn weak_tenpai_under_reach_context() -> GameContext {
    weak_tenpai_under_reach_context_with(None, [false, true, false, false])
}

pub(crate) fn weak_tenpai_under_reach_context_with(
    oya: Option<u8>,
    reached: [bool; 4],
) -> GameContext {
    GameContext::from_parts_with_table_state(
        Some(tile(WEAK_TENPAI_DRAWN)),
        WEAK_TENPAI_HAND.iter().map(|&value| tile(value)).collect(),
        vec![],
        None,
        None,
        [4u8, 5, 6, 7].iter().map(|&value| tile(value)).collect(),
        Some(0),
        oya,
        [vec![], vec![tile(1)], vec![], vec![]],
        reached,
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

pub(crate) fn unavailable_reach_meld() -> Meld {
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

/// 3向聴 Progress 軸の production 接続 regression 局面。
/// hand 45m46899p1124579s / dora indicator E / 東場北家 / player 0 / oya 1 / 残り66枚。
pub(crate) fn three_shanten_progress_regression_context() -> (GameContext, Vec<LegalAction>) {
    const HAND: [&str; 14] = [
        "4m", "5m", "4p", "6p", "8p", "9p", "9p", "1s", "1s", "2s", "4s", "5s", "7s", "9s",
    ];
    let mut used = Vec::new();
    let mut take = |mjai: &str| {
        let tile_type = TileType::from_mjai_type_str(mjai).expect("牌種として読める");
        let tile = TileId::copies(tile_type)
            .find(|tile| !tile.is_red() && !used.contains(tile))
            .expect("未使用の物理牌がある");
        used.push(tile);
        tile
    };
    let tiles: Vec<_> = HAND.iter().map(|tile| take(tile)).collect();
    let dora_indicator = take("E");
    let visible: Vec<_> = tiles.iter().copied().chain([dora_indicator]).collect();
    let context = GameContext::from_parts_with_table_state(
        None,
        tiles.clone(),
        vec![dora_indicator],
        Some(TileType::from_mjai_type_str("E").unwrap()),
        Some(TileType::from_mjai_type_str("N").unwrap()),
        visible,
        Some(0),
        Some(1),
        Default::default(),
        [false; 4],
    )
    .with_table_state_facts(TableStateFacts {
        remaining_tiles: Some(66),
        ..Default::default()
    })
    .with_history_furiten_facts(HistoryFuritenFacts {
        same_turn: Some(false),
        riichi_missed_win: Some(false),
    });
    let actions = tiles
        .iter()
        .map(|&tile| LegalAction::Dahai { tile })
        .collect();
    (context, actions)
}
