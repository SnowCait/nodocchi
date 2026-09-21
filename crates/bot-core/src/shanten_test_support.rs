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

/// 暗槓判断用の東場東家局面。ツモ牌と副露・リーチ状況だけを差し替える。
///
/// 見え牌は自分の手牌とツモ牌だけで、カンの対象牌が4枚とも見えている状態になる。履歴依存
/// フリテンは非フリテンで確定させ、打点比較がロン可否 unknown で落ちないようにする。
pub(crate) fn ankan_context(
    hand: &[u8],
    drawn: u8,
    reached: [bool; 4],
    melds: [Vec<Meld>; 4],
    player_id: Option<u8>,
) -> GameContext {
    let hand_tiles: Vec<_> = hand.iter().map(|&value| tile(value)).collect();
    let mut visible = hand_tiles.clone();
    visible.push(tile(drawn));

    GameContext::from_parts_with_melds(
        Some(tile(drawn)),
        hand_tiles,
        vec![],
        TileType::from_mjai_type_str("E").ok(),
        TileType::from_mjai_type_str("E").ok(),
        visible,
        player_id,
        Some(0),
        Default::default(),
        reached,
        melds,
    )
    .with_history_furiten_facts(HistoryFuritenFacts {
        same_turn: Some(false),
        riichi_missed_win: Some(false),
    })
}

pub(crate) fn ankan_action(consumed: &[u8]) -> LegalAction {
    LegalAction::Ankan {
        consumed: consumed.iter().map(|&value| tile(value)).collect(),
    }
}

pub(crate) fn ankan_dahai_actions(hand: &[u8], drawn: u8) -> Vec<LegalAction> {
    hand.iter()
        .chain(std::iter::once(&drawn))
        .map(|&value| dahai(value))
        .collect()
}

/// 東 (108..111) の暗刻で 1p (36) 単騎テンパイし、4枚目の東 (111) をツモった局面。
///
/// 東を切っても暗槓しても待ちは 1p のままで、残枚数も 3 枚から変わらない。
pub(crate) const ANKAN_FREE_HAND: [u8; 13] = [108, 109, 110, 0, 4, 8, 12, 17, 20, 24, 28, 32, 36];
pub(crate) const ANKAN_FREE_DRAWN: u8 = 111;
pub(crate) const ANKAN_FREE_CONSUMED: [u8; 4] = [108, 109, 110, 111];

/// 1m (0..3) 4枚と 2m (4) で 1m1m1m + 1m2m の 3m 待ちテンパイになる局面。
///
/// 暗槓すると 2m が浮いてテンパイが崩れ、1向聴へ戻る。
pub(crate) const ANKAN_REGRESSING_HAND: [u8; 13] =
    [0, 1, 2, 3, 4, 48, 53, 56, 96, 100, 104, 36, 37];
pub(crate) const ANKAN_REGRESSING_DRAWN: u8 = 68;
pub(crate) const ANKAN_REGRESSING_CONSUMED: [u8; 4] = [0, 1, 2, 3];

/// 2m (4..7) の暗刻で 5p / 8p 待ちテンパイし、4枚目の 2m (7) をツモった局面。
///
/// 断么九だけの安い手なのでダマ打点が足りず、リーチ判断がリーチを選ぶ。暗槓しても待ちは
/// 5p / 8p のまま変わらず打点も下がらないので、リーチが合法でなければ暗槓の成立条件を満たす。
pub(crate) const ANKAN_REACH_HAND: [u8; 13] = [4, 5, 6, 44, 48, 53, 92, 96, 100, 84, 85, 56, 60];
pub(crate) const ANKAN_REACH_DRAWN: u8 = 7;
pub(crate) const ANKAN_REACH_CONSUMED: [u8; 4] = [4, 5, 6, 7];

/// 2m (4..7) 4枚と 3m (8) 4m (12) を持ち、4枚目の 2m (7) をツモった局面。
///
/// 暗槓しなければ 2m2m2m + 2m3m4m の2面子として使えて 6s / 9s の2種待ちになるが、暗槓すると
/// その使い分けが消えて 5m の1種待ちへ狭まる。テンパイのままなので、同じ向聴段階での受け入れ
/// 劣化として弾く局面になる。
pub(crate) const ANKAN_ACCEPTANCE_REGRESSING_HAND: [u8; 13] =
    [4, 5, 6, 8, 12, 48, 53, 56, 96, 100, 104, 36, 37];
pub(crate) const ANKAN_ACCEPTANCE_REGRESSING_DRAWN: u8 = 7;
pub(crate) const ANKAN_ACCEPTANCE_REGRESSING_CONSUMED: [u8; 4] = [4, 5, 6, 7];

/// 東 (108..111) の暗刻を持つ1向聴の局面。4枚目の東 (111) をツモった状態。
///
/// 東を切っても暗槓しても1向聴のままで受け入れも変わらないが、テンパイではないので既存の
/// 攻撃打点で暗槓前後を比較できない。
pub(crate) const ANKAN_IISHANTEN_HAND: [u8; 13] =
    [108, 109, 110, 0, 4, 8, 12, 17, 20, 24, 28, 36, 40];
pub(crate) const ANKAN_IISHANTEN_DRAWN: u8 = 111;
pub(crate) const ANKAN_IISHANTEN_CONSUMED: [u8; 4] = [108, 109, 110, 111];
