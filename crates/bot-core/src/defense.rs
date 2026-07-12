use crate::action::LegalAction;
use crate::context::GameContext;
use bot_logic::TileType;

const LOG_TARGET: &str = "bot_core::defense";

// discards は防御・現物判定用、visible_tiles は枚数補正用なので用途を分ける。
pub fn is_genbutsu_for(tile: TileType, player: usize, context: &GameContext) -> bool {
    context
        .discards_of(player)
        .is_some_and(|discards| discards.iter().any(|t| t.tile_type() == tile))
}

// 全リーチ者に共通する現物か判定する。リーチ者がいなければ false。
pub fn is_genbutsu_for_all_reached(tile: TileType, context: &GameContext) -> bool {
    let reached = context.reached_opponents();
    if reached.is_empty() {
        return false;
    }
    reached
        .iter()
        .all(|&player| is_genbutsu_for(tile, player, context))
}

// 合法 Dahai の中から全リーチ者に共通する現物候補を、元の順序を保ったまま抽出する。
pub fn genbutsu_dahai_actions_for_all_reached<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<&'a LegalAction> {
    legal_actions
        .iter()
        .filter(|action| match action {
            LegalAction::Dahai { tile } => is_genbutsu_for_all_reached(tile.tile_type(), context),
            _ => false,
        })
        .collect()
}

// 他家リーチ中に、合法 Dahai の中から全リーチ者に共通する現物を fallback として選ぶ。
// 他家リーチがない、または共通現物がなければ None。合法 action からのみ選ぶ。
pub fn select_genbutsu_fallback_action<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<&'a LegalAction> {
    if !context.any_opponent_reached() {
        return None;
    }
    genbutsu_dahai_actions_for_all_reached(legal_actions, context)
        .into_iter()
        .next()
}

// visible_tiles 中で同じ TileType の枚数を数える。赤5も通常5と同じ TileType として数える。
pub fn visible_count_of(tile: TileType, context: &GameContext) -> u8 {
    context
        .visible_tiles()
        .iter()
        .filter(|visible| visible.tile_type() == tile)
        .count() as u8
}

// 字牌の見え枚数に基づく安全度。見えているほど当たりにくいので安全度が高い。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HonorSafetyRank {
    NoVisible,
    OneVisible,
    TwoVisible,
    ThreeOrMoreVisible,
}

// 字牌の安全度を見え枚数から求める。字牌でなければ None。
pub fn honor_safety_rank(tile: TileType, context: &GameContext) -> Option<HonorSafetyRank> {
    if !tile.is_honor() {
        return None;
    }
    let rank = match visible_count_of(tile, context) {
        0 => HonorSafetyRank::NoVisible,
        1 => HonorSafetyRank::OneVisible,
        2 => HonorSafetyRank::TwoVisible,
        _ => HonorSafetyRank::ThreeOrMoreVisible,
    };
    Some(rank)
}

// 合法 Dahai のうち字牌のみを安全度の高い順に並べる。同安全度は元の順序を保つ。
pub fn honor_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<(&'a LegalAction, HonorSafetyRank)> {
    let mut ranked: Vec<(&'a LegalAction, HonorSafetyRank)> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => {
                honor_safety_rank(tile.tile_type(), context).map(|rank| (action, rank))
            }
            _ => None,
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked
}

// 最も安全度の高い字牌 Dahai を fallback として選ぶ。候補がなければ None。
pub fn select_honor_safety_fallback_action<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Option<&'a LegalAction> {
    honor_dahai_actions_by_safety(legal_actions, context)
        .into_iter()
        .next()
        .map(|(action, _)| action)
}

// 簡易スジ安全度。現時点では無スジ / スジの2段階のみ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SujiSafetyRank {
    NoSuji,
    Suji,
}

// 指定 player の河から簡易スジ判定する。字牌は対象外。player が範囲外なら false。
// 同じ suit で number が ±3 の牌が河にあればスジ扱い。
pub fn is_suji_for(tile: TileType, player: usize, context: &GameContext) -> bool {
    let (Some(number), Some(suit)) = (tile.number(), tile.suit()) else {
        return false;
    };
    context.discards_of(player).is_some_and(|discards| {
        discards.iter().any(|discarded| {
            let discarded = discarded.tile_type();
            discarded.suit() == Some(suit)
                && discarded.number().is_some_and(|n| n.abs_diff(number) == 3)
        })
    })
}

// いずれかのリーチ者の河からスジ判定する。リーチ者がいなければ false。
pub fn is_suji_for_any_reached(tile: TileType, context: &GameContext) -> bool {
    let reached = context.reached_opponents();
    if reached.is_empty() {
        return false;
    }
    reached
        .iter()
        .any(|&player| is_suji_for(tile, player, context))
}

// 全リーチ者の河に対してスジか判定する。リーチ者がいなければ false。
// 全リーチ者について is_suji_for が true の場合だけ true。一人でも無スジなら false。
pub fn is_suji_for_all_reached(tile: TileType, context: &GameContext) -> bool {
    let reached = context.reached_opponents();
    if reached.is_empty() {
        return false;
    }
    reached
        .iter()
        .all(|&player| is_suji_for(tile, player, context))
}

// いずれかのリーチ者の河に対する簡易スジ安全度。数牌なら Some、字牌なら None。
pub fn suji_safety_rank_for_any_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    if is_suji_for_any_reached(tile, context) {
        Some(SujiSafetyRank::Suji)
    } else {
        Some(SujiSafetyRank::NoSuji)
    }
}

// 全リーチ者の河に対する簡易スジ安全度。数牌なら Some、字牌なら None。
pub fn suji_safety_rank_for_all_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    if is_suji_for_all_reached(tile, context) {
        Some(SujiSafetyRank::Suji)
    } else {
        Some(SujiSafetyRank::NoSuji)
    }
}

// 合法 Dahai のうち数牌のみを安全度の高い順(Suji → NoSuji)に並べる。同安全度は元の順序を保つ。
// スジ判定は全リーチ者基準。全リーチ者に対してスジの場合だけ Suji。
pub fn suji_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<(&'a LegalAction, SujiSafetyRank)> {
    let mut ranked: Vec<(&'a LegalAction, SujiSafetyRank)> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => {
                suji_safety_rank_for_all_reached(tile.tile_type(), context)
                    .map(|rank| (action, rank))
            }
            _ => None,
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked
}

// 数牌の順子待ち経路ごとの壁 / ワンチャンス分類。見えているほど当たり筋が減る。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WallRank {
    NoWall,
    OneChance,
    NoChance,
}

// 対象牌を和了牌とする順子待ち経路1本の壁分類。
//
// - Blocked:   経路を構成するどちらかの牌が4枚以上見えている(その順子待ちは残らない)。
// - OneChance: Blocked ではなく、どちらかの牌が3枚見えている。
// - Open:      それ以外。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceRouteRank {
    Open,
    OneChance,
    Blocked,
}

// 対象牌 n を和了牌とする、同一色内の順子待ち経路を列挙する。
//
// - n >= 3: [n-2, n-1]
// - n <= 7: [n+1, n+2]
//
// 字牌は対象外で空。1〜9 の範囲外へは進まないので、端牌は経路が1本だけになる。
// 対象牌自身の見え枚数は経路に含めない。
fn sequence_wait_routes(tile: TileType) -> Vec<[TileType; 2]> {
    let Some(number) = tile.number() else {
        return Vec::new();
    };
    // 同一色の 1 の TileType を基準にした相対計算。suit をまたがない。
    let suit_base = tile.raw() - (number - 1);
    let in_suit = |value: u8| TileType::new(suit_base + value - 1).unwrap();

    let mut routes = Vec::new();
    if number >= 3 {
        routes.push([in_suit(number - 2), in_suit(number - 1)]);
    }
    if number <= 7 {
        routes.push([in_suit(number + 1), in_suit(number + 2)]);
    }
    routes
}

// 順子待ち経路1本の壁分類。経路を構成する牌の見え枚数だけを使う。
fn sequence_route_rank(route: [TileType; 2], context: &GameContext) -> SequenceRouteRank {
    let max_visible = route
        .iter()
        .map(|&tile| visible_count_of(tile, context))
        .max()
        .unwrap_or(0);
    if max_visible >= 4 {
        SequenceRouteRank::Blocked
    } else if max_visible >= 3 {
        SequenceRouteRank::OneChance
    } else {
        SequenceRouteRank::Open
    }
}

/// 数牌の順子待ちに関する壁 / ワンチャンスを保守的に分類する。
///
/// 対象牌 `n` を和了牌とする順子待ち経路(`[n-2, n-1]` / `[n+1, n+2]`)を列挙し、各経路を
/// 構成する牌の見え枚数から集約する。**対象牌 `n` 自身の見え枚数は壁判定に使わない**。
/// 自分が `n` を暗刻で持っているだけで壁扱いされる誤分類を避けるためである。
///
/// 集約規則:
///
/// - すべての有効経路が Blocked: `NoChance`
/// - すべての有効経路が Blocked または OneChance で、少なくとも1経路が OneChance: `OneChance`
/// - Open の経路が1つでもある: `NoWall`
///
/// これは順子待ちに関する限定的な安全度でしかなく、単騎・双碰・嵌張などへの絶対的な安全は
/// 意味しない。字牌は対象外で常に `NoWall`。
pub fn wall_rank(tile: TileType, context: &GameContext) -> WallRank {
    let routes = sequence_wait_routes(tile);
    if routes.is_empty() {
        return WallRank::NoWall;
    }

    let mut has_open = false;
    let mut has_one_chance = false;
    for route in routes {
        match sequence_route_rank(route, context) {
            SequenceRouteRank::Open => has_open = true,
            SequenceRouteRank::OneChance => has_one_chance = true,
            SequenceRouteRank::Blocked => {}
        }
    }

    if has_open {
        WallRank::NoWall
    } else if has_one_chance {
        WallRank::OneChance
    } else {
        WallRank::NoChance
    }
}

// 順子待ち経路がワンチャンスの数牌か判定する。詳細は `wall_rank`。
pub fn is_one_chance(tile: TileType, context: &GameContext) -> bool {
    wall_rank(tile, context) == WallRank::OneChance
}

// 順子待ち経路がノーチャンスの数牌か判定する。詳細は `wall_rank`。
pub fn is_no_chance(tile: TileType, context: &GameContext) -> bool {
    wall_rank(tile, context) == WallRank::NoChance
}

// 数牌のみを TileType::all() の順序で壁分類と共に返す。NoWall も含める。
pub fn wall_tile_types_by_rank(context: &GameContext) -> Vec<(TileType, WallRank)> {
    TileType::all()
        .filter(|tile| !tile.is_honor())
        .map(|tile| (tile, wall_rank(tile, context)))
        .collect()
}

// 数牌の防御 fallback 用の安全度。壁 / スジを統合して分類する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SuitedSafetyRank {
    NoSafety,
    Suji,
    OneChance,
    NoChance,
}

// いずれかのリーチ者の河に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で None。
pub fn suited_safety_rank_for_any_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    let rank = match wall_rank(tile, context) {
        WallRank::NoChance => SuitedSafetyRank::NoChance,
        WallRank::OneChance => SuitedSafetyRank::OneChance,
        WallRank::NoWall => {
            if is_suji_for_any_reached(tile, context) {
                SuitedSafetyRank::Suji
            } else {
                SuitedSafetyRank::NoSafety
            }
        }
    };
    Some(rank)
}

// 全リーチ者の河に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で None。
// 壁評価はスジ評価より優先する。スジは全リーチ者に対してスジの場合だけ Suji。
pub fn suited_safety_rank_for_all_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SuitedSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    let rank = match wall_rank(tile, context) {
        WallRank::NoChance => SuitedSafetyRank::NoChance,
        WallRank::OneChance => SuitedSafetyRank::OneChance,
        WallRank::NoWall => {
            if is_suji_for_all_reached(tile, context) {
                SuitedSafetyRank::Suji
            } else {
                SuitedSafetyRank::NoSafety
            }
        }
    };
    Some(rank)
}

// 合法 Dahai のうち数牌のみを安全度の高い順(NoChance → OneChance → Suji → NoSafety)に並べる。
// 同安全度は元の順序を保つ。スジ判定は全リーチ者基準。
pub fn suited_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<(&'a LegalAction, SuitedSafetyRank)> {
    let mut ranked: Vec<(&'a LegalAction, SuitedSafetyRank)> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => {
                suited_safety_rank_for_all_reached(tile.tile_type(), context)
                    .map(|rank| (action, rank))
            }
            _ => None,
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked
}

// 他家リーチ中に、最も安全度の高い数牌 Dahai を fallback として選ぶ。
// 他家リーチがない、または NoSafety しか候補がなければ None。NoSafety は選ばない。
pub fn select_suited_safety_fallback_action<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Option<&'a LegalAction> {
    if !context.any_opponent_reached() {
        return None;
    }
    suited_dahai_actions_by_safety(legal_actions, context)
        .into_iter()
        .find(|(_, rank)| *rank != SuitedSafetyRank::NoSafety)
        .map(|(action, _)| action)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenseFallbackKind {
    Genbutsu,
    HonorSafety(HonorSafetyRank),
    SuitedSafety(SuitedSafetyRank),
}

// 他家リーチ中の防御 fallback を優先順位付きで選ぶ。
// 現物 → 字牌 safety → 数牌防御 の順に評価し、選ばれた種別を添えて返す。
pub fn select_defense_fallback_action_with_kind<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<(&'a LegalAction, DefenseFallbackKind)> {
    if let Some(action) = select_genbutsu_fallback_action(context, legal_actions) {
        return Some((action, DefenseFallbackKind::Genbutsu));
    }

    if context.any_opponent_reached()
        && let Some((action, rank)) = honor_dahai_actions_by_safety(legal_actions, context)
            .into_iter()
            .next()
    {
        return Some((action, DefenseFallbackKind::HonorSafety(rank)));
    }

    if context.any_opponent_reached()
        && let Some((action, rank)) = suited_dahai_actions_by_safety(legal_actions, context)
            .into_iter()
            .find(|(_, rank)| *rank != SuitedSafetyRank::NoSafety)
    {
        return Some((action, DefenseFallbackKind::SuitedSafety(rank)));
    }

    None
}

// 防御 fallback の action だけを返す薄い wrapper。
pub fn select_defense_fallback_action<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<&'a LegalAction> {
    select_defense_fallback_action_with_kind(context, legal_actions).map(|(action, _)| action)
}

/// 防御 fallback がどの理由で選ばれたかを表す診断データ。
///
/// tracing の出力文字列に依存せずテストできるよう、ログへ渡す値を pure に構築する。
/// 数牌なら壁 / スジ / 数牌 safety を、字牌なら字牌 safety を持ち、無関係なフィールドは `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefenseFallbackDiagnostic {
    pub selected_action: String,
    pub selected_kind: DefenseFallbackKind,
    pub opponent_reach_count: u8,
    pub selected_genbutsu_for_all: bool,
    pub selected_honor_safety_rank: Option<HonorSafetyRank>,
    pub selected_wall_rank: Option<WallRank>,
    pub selected_suji_for_all_reached: Option<bool>,
    pub selected_suited_safety_rank: Option<SuitedSafetyRank>,
}

impl DefenseFallbackDiagnostic {
    /// 選択された防御 fallback の action と種別から診断データを構築する pure helper。
    ///
    /// 数牌に対しては `wall_rank` / `is_suji_for_all_reached` / `suited_safety_rank_for_all_reached`
    /// を、字牌に対しては `honor_safety_rank` を計算する。Dahai 以外の action では牌由来の値は空。
    pub fn from_selection(
        context: &GameContext,
        action: &LegalAction,
        kind: DefenseFallbackKind,
    ) -> Self {
        let tile_type = match action {
            LegalAction::Dahai { tile } => Some(tile.tile_type()),
            _ => None,
        };
        let selected_action = match action {
            LegalAction::Dahai { tile } => tile.to_mjai_string(),
            other => format!("{other:?}"),
        };
        let suited_tile = tile_type.filter(|tile| !tile.is_honor());

        Self {
            selected_action,
            selected_kind: kind,
            opponent_reach_count: context.reached_opponents().len() as u8,
            selected_genbutsu_for_all: tile_type
                .is_some_and(|tile| is_genbutsu_for_all_reached(tile, context)),
            selected_honor_safety_rank: tile_type.and_then(|tile| honor_safety_rank(tile, context)),
            selected_wall_rank: suited_tile.map(|tile| wall_rank(tile, context)),
            selected_suji_for_all_reached: suited_tile
                .map(|tile| is_suji_for_all_reached(tile, context)),
            selected_suited_safety_rank: tile_type
                .and_then(|tile| suited_safety_rank_for_all_reached(tile, context)),
        }
    }
}

/// 防御 fallback を実際に採用したとき DEBUG イベントを1件出す opt-in ログ。
///
/// `RUST_LOG=bot_core::defense=debug` で有効化する。debug が無効な通常時は診断値や文字列を
/// 一切構築しない。TRACE が有効なら、合法 Dahai ごとの防御評価も追加で記録する。
pub fn log_defense_fallback_decision(
    context: &GameContext,
    action: &LegalAction,
    kind: DefenseFallbackKind,
    legal_actions: &[LegalAction],
) {
    if !tracing::enabled!(target: LOG_TARGET, tracing::Level::DEBUG) {
        return;
    }

    let diagnostic = DefenseFallbackDiagnostic::from_selection(context, action, kind);
    tracing::debug!(
        target: LOG_TARGET,
        selected_action = %diagnostic.selected_action,
        selected_kind = ?diagnostic.selected_kind,
        opponent_reach_count = diagnostic.opponent_reach_count,
        selected_genbutsu_for_all = diagnostic.selected_genbutsu_for_all,
        selected_honor_safety_rank = ?diagnostic.selected_honor_safety_rank,
        selected_wall_rank = ?diagnostic.selected_wall_rank,
        selected_suji_for_all_reached = ?diagnostic.selected_suji_for_all_reached,
        selected_suited_safety_rank = ?diagnostic.selected_suited_safety_rank,
        "defense fallback decision",
    );

    if tracing::enabled!(target: LOG_TARGET, tracing::Level::TRACE) {
        for candidate in legal_actions {
            let LegalAction::Dahai { tile } = candidate else {
                continue;
            };
            let tile_type = tile.tile_type();
            let suited_tile = (!tile_type.is_honor()).then_some(tile_type);
            tracing::trace!(
                target: LOG_TARGET,
                tile = %tile.to_mjai_string(),
                genbutsu_for_all = is_genbutsu_for_all_reached(tile_type, context),
                honor_safety_rank = ?honor_safety_rank(tile_type, context),
                wall_rank = ?suited_tile.map(|tile| wall_rank(tile, context)),
                suji_for_all_reached = ?suited_tile.map(|tile| is_suji_for_all_reached(tile, context)),
                suited_safety_rank = ?suited_safety_rank_for_all_reached(tile_type, context),
                "defense fallback candidate",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_logic::TileId;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn table_state_context(
        player_id: Option<u8>,
        oya: Option<u8>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
    ) -> GameContext {
        GameContext::from_parts_with_table_state(
            None,
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            player_id,
            oya,
            discards,
            reached,
        )
    }

    #[test]
    fn is_genbutsu_for_detects_discarded_tile_type() {
        let discards = [vec![tile(0)], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(3), None, discards, [false; 4]);
        let one_man = tile(0).tile_type();
        assert!(is_genbutsu_for(one_man, 0, &context));
        assert!(!is_genbutsu_for(one_man, 1, &context));
    }

    #[test]
    fn is_genbutsu_for_out_of_range_player_is_false() {
        let context = GameContext::default();
        assert!(!is_genbutsu_for(tile(0).tile_type(), 4, &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_false_without_reachers() {
        let discards = [
            vec![tile(16)],
            vec![tile(16)],
            vec![tile(16)],
            vec![tile(16)],
        ];
        let context = table_state_context(Some(0), None, discards, [false; 4]);
        assert!(!is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_single_reacher_hit() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_single_reacher_miss() {
        let discards = [vec![], vec![tile(0)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(!is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_multiple_reachers_all_hit() {
        let discards = [vec![], vec![tile(16)], vec![tile(17)], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
        assert!(is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_multiple_reachers_partial_miss() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
        assert!(!is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_ignores_own_reach() {
        // 自分(0)の河にはあるが自分のリーチは対象外、他家リーチ者の河には無い。
        let discards = [vec![tile(16)], vec![], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [true, true, false, false]);
        assert!(!is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_without_player_id_targets_all_reached() {
        let discards = [vec![tile(16)], vec![], vec![], vec![]];
        let context = table_state_context(None, None, discards, [true, false, false, false]);
        assert!(is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_genbutsu_for_all_reached_treats_red_five_as_same_type() {
        // 河に通常5m(tile 17)、判定対象が赤5m相当(tile 16)。同じ TileType として現物扱い。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(is_genbutsu_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_empty_without_reachers() {
        let discards = [vec![tile(16)], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false; 4]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert!(genbutsu_dahai_actions_for_all_reached(&actions, &context).is_empty());
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_filters_to_common_genbutsu() {
        let discards = [vec![], vec![tile(16)], vec![tile(0)], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(56) },
        ];
        let filtered = genbutsu_dahai_actions_for_all_reached(&actions, &context);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_returns_matching_dahai() {
        let discards = [vec![], vec![tile(16), tile(0)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(56) },
        ];
        let filtered = genbutsu_dahai_actions_for_all_reached(&actions, &context);
        assert_eq!(
            filtered,
            vec![
                &LegalAction::Dahai { tile: tile(16) },
                &LegalAction::Dahai { tile: tile(0) },
            ]
        );
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_excludes_non_dahai() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Hora,
            LegalAction::Ryukyoku,
            LegalAction::None,
            LegalAction::Pon {
                tile: tile(16),
                consumed: vec![tile(17), tile(18)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(16), tile(17), tile(18), tile(19)],
            },
            LegalAction::Dahai { tile: tile(16) },
        ];
        let filtered = genbutsu_dahai_actions_for_all_reached(&actions, &context);
        assert_eq!(filtered, vec![&LegalAction::Dahai { tile: tile(16) }]);
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_preserves_order() {
        let discards = [vec![], vec![tile(0), tile(16), tile(56)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(56) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        let filtered = genbutsu_dahai_actions_for_all_reached(&actions, &context);
        assert_eq!(
            filtered,
            vec![
                &LegalAction::Dahai { tile: tile(56) },
                &LegalAction::Dahai { tile: tile(0) },
                &LegalAction::Dahai { tile: tile(16) },
            ]
        );
    }

    #[test]
    fn genbutsu_dahai_actions_for_all_reached_matches_red_five() {
        // 河に通常5m(tile 17)、Dahai が赤5m相当(tile 16)でも同じ TileType の現物として抽出。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        let filtered = genbutsu_dahai_actions_for_all_reached(&actions, &context);
        assert_eq!(filtered, vec![&LegalAction::Dahai { tile: tile(16) }]);
    }

    #[test]
    fn select_genbutsu_fallback_action_none_without_opponent_reach() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false; 4]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert_eq!(select_genbutsu_fallback_action(&context, &actions), None);
    }

    #[test]
    fn select_genbutsu_fallback_action_ignores_only_own_reach() {
        // 自分(0)だけがリーチしている場合は他家リーチ扱いにしない。
        let discards = [vec![tile(16)], vec![], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [true, false, false, false]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert_eq!(select_genbutsu_fallback_action(&context, &actions), None);
    }

    #[test]
    fn select_genbutsu_fallback_action_returns_common_genbutsu() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(16) })
        );
    }

    #[test]
    fn select_genbutsu_fallback_action_none_when_no_common_genbutsu() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(56) },
        ];
        assert_eq!(select_genbutsu_fallback_action(&context, &actions), None);
    }

    #[test]
    fn select_genbutsu_fallback_action_returns_first_in_legal_order() {
        let discards = [vec![], vec![tile(0), tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(56) },
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
        ];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(16) })
        );
    }

    #[test]
    fn select_genbutsu_fallback_action_never_returns_non_dahai() {
        // 現物になり得るのは Dahai のみ。Reach/Hora/Ryukyoku/None/副露・カンは返さない。
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Hora,
            LegalAction::Ryukyoku,
            LegalAction::None,
            LegalAction::Pon {
                tile: tile(16),
                consumed: vec![tile(17), tile(18)],
            },
            LegalAction::Ankan {
                consumed: vec![tile(16), tile(17), tile(18), tile(19)],
            },
        ];
        assert_eq!(select_genbutsu_fallback_action(&context, &actions), None);
    }

    #[test]
    fn select_genbutsu_fallback_action_matches_red_five_dahai() {
        // 河に通常5m(tile 17)、Dahai が赤5m相当(tile 16)でも同じ TileType の現物として選ぶ。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(16) })
        );
    }

    fn visible_context(visible_tiles: Vec<TileId>) -> GameContext {
        GameContext::from_parts_with_visible_tiles(None, vec![], vec![], None, None, visible_tiles)
    }

    #[test]
    fn visible_count_of_counts_duplicate_tile_types() {
        // 東(tile 108-110)を3枚、白(tile 124)を1枚見えている状態。
        let context = visible_context(vec![tile(108), tile(109), tile(110), tile(124)]);
        assert_eq!(visible_count_of(tile(108).tile_type(), &context), 3);
        assert_eq!(visible_count_of(tile(124).tile_type(), &context), 1);
        assert_eq!(visible_count_of(tile(0).tile_type(), &context), 0);
    }

    #[test]
    fn visible_count_of_treats_red_five_as_same_type() {
        // 赤5m(tile 16)と通常5m(tile 17)は同じ TileType として数える。
        let context = visible_context(vec![tile(16), tile(17)]);
        assert_eq!(visible_count_of(tile(16).tile_type(), &context), 2);
    }

    #[test]
    fn honor_safety_rank_none_for_number_tiles() {
        let context = visible_context(vec![]);
        assert_eq!(honor_safety_rank(tile(0).tile_type(), &context), None);
        assert_eq!(honor_safety_rank(tile(16).tile_type(), &context), None);
        assert_eq!(honor_safety_rank(tile(104).tile_type(), &context), None);
    }

    #[test]
    fn honor_safety_rank_classifies_visible_count() {
        // 東を0/1/2/3枚見えているそれぞれのケース。
        let east = tile(108).tile_type();
        assert_eq!(
            honor_safety_rank(east, &visible_context(vec![])),
            Some(HonorSafetyRank::NoVisible)
        );
        assert_eq!(
            honor_safety_rank(east, &visible_context(vec![tile(108)])),
            Some(HonorSafetyRank::OneVisible)
        );
        assert_eq!(
            honor_safety_rank(east, &visible_context(vec![tile(108), tile(109)])),
            Some(HonorSafetyRank::TwoVisible)
        );
        assert_eq!(
            honor_safety_rank(
                east,
                &visible_context(vec![tile(108), tile(109), tile(110)])
            ),
            Some(HonorSafetyRank::ThreeOrMoreVisible)
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_excludes_non_dahai() {
        let context = visible_context(vec![tile(108)]);
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Pon {
                tile: tile(108),
                consumed: vec![tile(109), tile(110)],
            },
            LegalAction::Dahai { tile: tile(108) },
        ];
        let ranked = honor_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(
                &LegalAction::Dahai { tile: tile(108) },
                HonorSafetyRank::OneVisible
            )]
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_excludes_number_dahai() {
        let context = visible_context(vec![]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(108) },
        ];
        let ranked = honor_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(
                &LegalAction::Dahai { tile: tile(108) },
                HonorSafetyRank::NoVisible
            )]
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_orders_high_safety_first() {
        // 東は3枚見え、南は1枚見え、白は0枚見え。安全度の高い順に並ぶ。
        let context = visible_context(vec![tile(108), tile(109), tile(110), tile(112)]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(124) },
            LegalAction::Dahai { tile: tile(112) },
            LegalAction::Dahai { tile: tile(108) },
        ];
        let ranked = honor_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(108) },
                    HonorSafetyRank::ThreeOrMoreVisible
                ),
                (
                    &LegalAction::Dahai { tile: tile(112) },
                    HonorSafetyRank::OneVisible
                ),
                (
                    &LegalAction::Dahai { tile: tile(124) },
                    HonorSafetyRank::NoVisible
                ),
            ]
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_preserves_order_within_same_rank() {
        // すべて0枚見えの字牌 Dahai は元の順序を保つ。
        let context = visible_context(vec![]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(124) },
            LegalAction::Dahai { tile: tile(108) },
            LegalAction::Dahai { tile: tile(120) },
        ];
        let ranked = honor_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(124) },
                    HonorSafetyRank::NoVisible
                ),
                (
                    &LegalAction::Dahai { tile: tile(108) },
                    HonorSafetyRank::NoVisible
                ),
                (
                    &LegalAction::Dahai { tile: tile(120) },
                    HonorSafetyRank::NoVisible
                ),
            ]
        );
    }

    #[test]
    fn select_honor_safety_fallback_action_returns_safest_honor_dahai() {
        // 東は2枚見え、南は0枚見え。より安全な東を選ぶ。
        let context = visible_context(vec![tile(108), tile(109)]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(112) },
            LegalAction::Dahai { tile: tile(108) },
        ];
        assert_eq!(
            select_honor_safety_fallback_action(&actions, &context),
            Some(&LegalAction::Dahai { tile: tile(108) })
        );
    }

    #[test]
    fn select_honor_safety_fallback_action_none_without_honor_dahai() {
        let context = visible_context(vec![]);
        let actions = vec![LegalAction::Dahai { tile: tile(0) }, LegalAction::Reach];
        assert_eq!(
            select_honor_safety_fallback_action(&actions, &context),
            None
        );
    }

    #[test]
    fn suji_safety_rank_for_any_reached_none_for_honor() {
        // 字牌はスジ判定対象外なので None。
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [false, true, false, false],
        );
        assert_eq!(
            suji_safety_rank_for_any_reached(tile(108).tile_type(), &context),
            None
        );
    }

    #[test]
    fn suji_safety_rank_for_any_reached_classifies_number_tiles() {
        // リーチ者(1)の河に 4m。1m はスジ、5m は無スジ。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert_eq!(
            suji_safety_rank_for_any_reached(tile(0).tile_type(), &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suji_safety_rank_for_any_reached(tile(16).tile_type(), &context),
            Some(SujiSafetyRank::NoSuji)
        );
    }

    #[test]
    fn is_suji_for_out_of_range_player_is_false() {
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(!is_suji_for(tile(0).tile_type(), 4, &context));
    }

    #[test]
    fn is_suji_for_any_reached_false_without_reachers() {
        // 河に 4m があっても、リーチ者がいなければ false。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false; 4]);
        assert!(!is_suji_for_any_reached(tile(0).tile_type(), &context));
    }

    #[test]
    fn is_suji_for_detects_plus_minus_three_same_suit() {
        // 4m が河にあれば、同じ suit で ±3 の 1m と 7m はスジ。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(is_suji_for(tile(0).tile_type(), 1, &context));
        assert!(is_suji_for(tile(24).tile_type(), 1, &context));
    }

    #[test]
    fn is_suji_for_false_for_different_suit() {
        // 4p が河にあっても 1m はスジにならない。
        let discards = [vec![], vec![tile(48)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(!is_suji_for(tile(0).tile_type(), 1, &context));
    }

    #[test]
    fn is_suji_for_detects_one_four_seven() {
        // 1m 河 → 4m スジ、7m 河 → 4m スジ、4m 河 → 1m/7m スジ。
        let context = table_state_context(
            Some(0),
            None,
            [vec![], vec![tile(0)], vec![], vec![]],
            [false, true, false, false],
        );
        assert!(is_suji_for(tile(12).tile_type(), 1, &context));

        let context = table_state_context(
            Some(0),
            None,
            [vec![], vec![tile(24)], vec![], vec![]],
            [false, true, false, false],
        );
        assert!(is_suji_for(tile(12).tile_type(), 1, &context));

        let context = table_state_context(
            Some(0),
            None,
            [vec![], vec![tile(12)], vec![], vec![]],
            [false, true, false, false],
        );
        assert!(is_suji_for(tile(0).tile_type(), 1, &context));
        assert!(is_suji_for(tile(24).tile_type(), 1, &context));
    }

    #[test]
    fn is_suji_for_detects_two_five_eight() {
        // 5m が河にあれば 2m と 8m はスジ。
        let context = table_state_context(
            Some(0),
            None,
            [vec![], vec![tile(16)], vec![], vec![]],
            [false, true, false, false],
        );
        assert!(is_suji_for(tile(4).tile_type(), 1, &context));
        assert!(is_suji_for(tile(28).tile_type(), 1, &context));
    }

    #[test]
    fn is_suji_for_detects_three_six_nine() {
        // 6m が河にあれば 3m と 9m はスジ。
        let context = table_state_context(
            Some(0),
            None,
            [vec![], vec![tile(20)], vec![], vec![]],
            [false, true, false, false],
        );
        assert!(is_suji_for(tile(8).tile_type(), 1, &context));
        assert!(is_suji_for(tile(32).tile_type(), 1, &context));
    }

    #[test]
    fn suji_dahai_actions_by_safety_excludes_non_dahai() {
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Pon {
                tile: tile(0),
                consumed: vec![tile(1), tile(2)],
            },
            LegalAction::Dahai { tile: tile(0) },
        ];
        let ranked = suji_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(&LegalAction::Dahai { tile: tile(0) }, SujiSafetyRank::Suji)]
        );
    }

    #[test]
    fn suji_dahai_actions_by_safety_excludes_honor_dahai() {
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(108) },
            LegalAction::Dahai { tile: tile(0) },
        ];
        let ranked = suji_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(&LegalAction::Dahai { tile: tile(0) }, SujiSafetyRank::Suji)]
        );
    }

    #[test]
    fn suji_dahai_actions_by_safety_orders_suji_first() {
        // 4m 河 → 1m はスジ、5m は無スジ。Suji → NoSuji の順。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
        ];
        let ranked = suji_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (&LegalAction::Dahai { tile: tile(0) }, SujiSafetyRank::Suji),
                (
                    &LegalAction::Dahai { tile: tile(16) },
                    SujiSafetyRank::NoSuji
                ),
            ]
        );
    }

    #[test]
    fn suji_dahai_actions_by_safety_preserves_order_within_same_rank() {
        // リーチ者はいるが河は空なので全て NoSuji。元の順序を保つ。
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(32) },
        ];
        let ranked = suji_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(16) },
                    SujiSafetyRank::NoSuji
                ),
                (
                    &LegalAction::Dahai { tile: tile(0) },
                    SujiSafetyRank::NoSuji
                ),
                (
                    &LegalAction::Dahai { tile: tile(32) },
                    SujiSafetyRank::NoSuji
                ),
            ]
        );
    }

    // 6p(tile 56-59)を対象に、経路構成牌 4p/5p/7p/8p の見え枚数で壁を作る helper。
    // 4p: 48-51, 5p: 52-55, 6p: 56-59, 7p: 60-63, 8p: 64-67。

    #[test]
    fn wall_rank_no_wall_when_own_count_high_but_routes_open() {
        // 対象牌自身(6p)を3枚見えていても、経路 4p/5p/7p/8p に壁がなければ NoWall。
        // 対象牌自身の見え枚数を壁判定に使わないことの回帰テスト。
        let six_pin = tile(56).tile_type();
        assert_eq!(
            wall_rank(
                six_pin,
                &visible_context(vec![tile(56), tile(57), tile(58)])
            ),
            WallRank::NoWall
        );
    }

    #[test]
    fn wall_rank_no_wall_when_own_count_four_but_routes_open() {
        // 対象牌自身(6p)を4枚見えていても、経路に壁がなければ NoWall。人工的な pure test。
        let six_pin = tile(56).tile_type();
        assert_eq!(
            wall_rank(
                six_pin,
                &visible_context(vec![tile(56), tile(57), tile(58), tile(59)])
            ),
            WallRank::NoWall
        );
    }

    #[test]
    fn wall_rank_no_chance_when_both_routes_blocked() {
        // 5p を4枚・7p を4枚見え。経路 [4p,5p] と [7p,8p] が両方 Blocked なので NoChance。
        let six_pin = tile(56).tile_type();
        let visible = vec![
            tile(52),
            tile(53),
            tile(54),
            tile(55),
            tile(60),
            tile(61),
            tile(62),
            tile(63),
        ];
        assert_eq!(
            wall_rank(six_pin, &visible_context(visible)),
            WallRank::NoChance
        );
    }

    #[test]
    fn wall_rank_no_wall_when_one_route_blocked_and_other_open() {
        // 5p を4枚見え(経路 [4p,5p] は Blocked)だが、7p/8p は見えず経路 [7p,8p] は Open。NoWall。
        let six_pin = tile(56).tile_type();
        let visible = vec![tile(52), tile(53), tile(54), tile(55)];
        assert_eq!(
            wall_rank(six_pin, &visible_context(visible)),
            WallRank::NoWall
        );
    }

    #[test]
    fn wall_rank_one_chance_when_blocked_and_one_chance() {
        // 5p を4枚見え(Blocked)、7p を3枚見え(OneChance)。Open が無く OneChance が残るので OneChance。
        let six_pin = tile(56).tile_type();
        let visible = vec![
            tile(52),
            tile(53),
            tile(54),
            tile(55),
            tile(60),
            tile(61),
            tile(62),
        ];
        assert_eq!(
            wall_rank(six_pin, &visible_context(visible)),
            WallRank::OneChance
        );
    }

    #[test]
    fn wall_rank_no_wall_when_one_chance_and_open() {
        // 5p を3枚見え(経路 [4p,5p] は OneChance)、7p/8p は見えず経路 [7p,8p] は Open。NoWall。
        let six_pin = tile(56).tile_type();
        let visible = vec![tile(52), tile(53), tile(54)];
        assert_eq!(
            wall_rank(six_pin, &visible_context(visible)),
            WallRank::NoWall
        );
    }

    #[test]
    fn wall_rank_no_wall_for_honor() {
        // 字牌は経路を持たないので、何枚見えていても NoWall。
        let east = tile(108).tile_type();
        assert_eq!(
            wall_rank(
                east,
                &visible_context(vec![tile(108), tile(109), tile(110), tile(111)])
            ),
            WallRank::NoWall
        );
    }

    #[test]
    fn wall_rank_terminal_uses_only_in_range_route() {
        // 1p は経路 [2p,3p] のみ、9p は経路 [7p,8p] のみを評価し、範囲外へは進まない。
        // 1p: 2p(40-43)を4枚見えで NoChance。9p: 8p(64-67)を4枚見えで NoChance。
        let one_pin = tile(36).tile_type();
        let nine_pin = tile(68).tile_type();
        assert_eq!(
            wall_rank(
                one_pin,
                &visible_context(vec![tile(40), tile(41), tile(42), tile(43)])
            ),
            WallRank::NoChance
        );
        assert_eq!(
            wall_rank(
                nine_pin,
                &visible_context(vec![tile(64), tile(65), tile(66), tile(67)])
            ),
            WallRank::NoChance
        );
    }

    #[test]
    fn sequence_wait_routes_stay_in_suit_and_in_range() {
        // 端牌は経路1本、中張牌は2本。suit をまたがず 1〜9 の範囲内に収まる。
        let one_pin = tile(36).tile_type();
        assert_eq!(sequence_wait_routes(one_pin).len(), 1);
        let nine_pin = tile(68).tile_type();
        assert_eq!(sequence_wait_routes(nine_pin).len(), 1);
        let five_pin = tile(52).tile_type();
        assert_eq!(sequence_wait_routes(five_pin).len(), 2);
        // 字牌は経路なし。
        assert!(sequence_wait_routes(tile(108).tile_type()).is_empty());
    }

    #[test]
    fn wall_rank_counts_red_five_in_route_as_same_type() {
        // 経路構成牌 5p の壁を赤5p(tile 52)込みの4枚で作る。赤5も通常5と同じ TileType。
        // 6p の経路 [4p,5p] が Blocked、[7p,8p] は Open なので NoWall。
        let six_pin = tile(56).tile_type();
        let visible = vec![tile(52), tile(53), tile(54), tile(55)];
        assert_eq!(
            visible_count_of(tile(53).tile_type(), &visible_context(visible.clone())),
            4
        );
        assert_eq!(
            wall_rank(six_pin, &visible_context(visible)),
            WallRank::NoWall
        );
    }

    #[test]
    fn is_one_chance_reflects_route_one_chance() {
        // 6p の経路 [4p,5p] を Blocked、[7p,8p] を OneChance にすると is_one_chance == true。
        let six_pin = tile(56).tile_type();
        let one_chance = vec![
            tile(52),
            tile(53),
            tile(54),
            tile(55),
            tile(60),
            tile(61),
            tile(62),
        ];
        assert!(is_one_chance(six_pin, &visible_context(one_chance)));
        // 経路が Open のままなら false。
        assert!(!is_one_chance(six_pin, &visible_context(vec![])));
        // 字牌は経路を持たないので false。
        let east = tile(108).tile_type();
        assert!(!is_one_chance(
            east,
            &visible_context(vec![tile(108), tile(109), tile(110)])
        ));
    }

    #[test]
    fn is_no_chance_reflects_route_blocked() {
        // 6p の両経路を Blocked にすると is_no_chance == true。片方 Open なら false。
        let six_pin = tile(56).tile_type();
        let no_chance = vec![
            tile(52),
            tile(53),
            tile(54),
            tile(55),
            tile(60),
            tile(61),
            tile(62),
            tile(63),
        ];
        assert!(is_no_chance(six_pin, &visible_context(no_chance)));
        assert!(!is_no_chance(
            six_pin,
            &visible_context(vec![tile(52), tile(53), tile(54), tile(55)])
        ));
        // 字牌は4枚見えでも false。
        let east = tile(108).tile_type();
        assert!(!is_no_chance(
            east,
            &visible_context(vec![tile(108), tile(109), tile(110), tile(111)])
        ));
    }

    #[test]
    fn wall_tile_types_by_rank_excludes_honors() {
        let context = visible_context(vec![]);
        let ranked = wall_tile_types_by_rank(&context);
        assert!(ranked.iter().all(|(tile, _)| !tile.is_honor()));
    }

    #[test]
    fn wall_tile_types_by_rank_returns_number_tiles_in_all_order() {
        let context = visible_context(vec![]);
        let ranked = wall_tile_types_by_rank(&context);
        let expected: Vec<(TileType, WallRank)> = TileType::all()
            .filter(|tile| !tile.is_honor())
            .map(|tile| (tile, WallRank::NoWall))
            .collect();
        assert_eq!(ranked, expected);
        // 数牌は27種。
        assert_eq!(ranked.len(), 27);
    }

    #[test]
    fn wall_tile_types_by_rank_includes_no_wall_entries() {
        // 2m を4枚見え。経路 [2m,3m] が Blocked になる 1m だけ NoChance、他は NoWall。
        let context = visible_context(vec![tile(4), tile(5), tile(6), tile(7)]);
        let ranked = wall_tile_types_by_rank(&context);
        let one_man = tile(0).tile_type();
        assert_eq!(
            ranked
                .iter()
                .find(|(tile, _)| *tile == one_man)
                .map(|(_, rank)| *rank),
            Some(WallRank::NoChance)
        );
        assert!(
            ranked
                .iter()
                .any(|(tile, rank)| *tile != one_man && *rank == WallRank::NoWall)
        );
        assert_eq!(ranked.len(), 27);
    }

    fn suited_context(
        visible_tiles: Vec<TileId>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
    ) -> GameContext {
        GameContext::from_parts_with_table_state(
            None,
            vec![],
            vec![],
            None,
            None,
            visible_tiles,
            Some(0),
            None,
            discards,
            reached,
        )
    }

    #[test]
    fn suited_safety_rank_for_any_reached_none_for_honor() {
        // 字牌は数牌防御対象外なので None。
        let context = suited_context(
            vec![tile(108), tile(109), tile(110), tile(111)],
            Default::default(),
            [false, true, false, false],
        );
        assert_eq!(
            suited_safety_rank_for_any_reached(tile(108).tile_type(), &context),
            None
        );
    }

    #[test]
    fn suited_safety_rank_for_any_reached_no_chance_over_one_chance_and_suji() {
        // 1m は 4m 河でスジ。経路 [2m,3m] を 2m 4枚で Blocked にすると NoChance。壁が最優先。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(
            vec![tile(4), tile(5), tile(6), tile(7)],
            discards,
            [false, true, false, false],
        );
        assert_eq!(
            suited_safety_rank_for_any_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::NoChance)
        );
    }

    #[test]
    fn suited_safety_rank_for_any_reached_one_chance_over_suji() {
        // 1m は 4m 河でスジ。経路 [2m,3m] を 2m 3枚で OneChance にすると OneChance が Suji より優先。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(
            vec![tile(4), tile(5), tile(6)],
            discards,
            [false, true, false, false],
        );
        assert_eq!(
            suited_safety_rank_for_any_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::OneChance)
        );
    }

    #[test]
    fn suited_safety_rank_for_any_reached_suji_over_no_safety() {
        // 4m 河で 1m はスジ(Suji)、5m は無スジ(NoSafety)。壁は無し。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(vec![], discards, [false, true, false, false]);
        assert_eq!(
            suited_safety_rank_for_any_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_any_reached(tile(16).tile_type(), &context),
            Some(SuitedSafetyRank::NoSafety)
        );
    }

    #[test]
    fn suited_dahai_actions_by_safety_excludes_non_dahai() {
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(vec![], discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Reach,
            LegalAction::Pon {
                tile: tile(0),
                consumed: vec![tile(1), tile(2)],
            },
            LegalAction::Dahai { tile: tile(0) },
        ];
        let ranked = suited_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(
                &LegalAction::Dahai { tile: tile(0) },
                SuitedSafetyRank::Suji
            )]
        );
    }

    #[test]
    fn suited_dahai_actions_by_safety_excludes_honor_dahai() {
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(vec![], discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(108) },
            LegalAction::Dahai { tile: tile(0) },
        ];
        let ranked = suited_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![(
                &LegalAction::Dahai { tile: tile(0) },
                SuitedSafetyRank::Suji
            )]
        );
    }

    #[test]
    fn suited_dahai_actions_by_safety_orders_by_safety() {
        // 経路壁で安全度を作る。1p は 2p 4枚で NoChance、9p は 8p 3枚で OneChance、
        // 1s は 4s 河でスジ(Suji)、5s は無スジ・壁なし(NoSafety)。順序が入れ替わっても安全度順に並ぶ。
        let discards = [vec![], vec![tile(84)], vec![], vec![]];
        let context = suited_context(
            vec![
                tile(40),
                tile(41),
                tile(42),
                tile(43),
                tile(64),
                tile(65),
                tile(66),
            ],
            discards,
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(88) },
            LegalAction::Dahai { tile: tile(72) },
            LegalAction::Dahai { tile: tile(68) },
            LegalAction::Dahai { tile: tile(36) },
        ];
        let ranked = suited_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(36) },
                    SuitedSafetyRank::NoChance
                ),
                (
                    &LegalAction::Dahai { tile: tile(68) },
                    SuitedSafetyRank::OneChance
                ),
                (
                    &LegalAction::Dahai { tile: tile(72) },
                    SuitedSafetyRank::Suji
                ),
                (
                    &LegalAction::Dahai { tile: tile(88) },
                    SuitedSafetyRank::NoSafety
                ),
            ]
        );
    }

    #[test]
    fn suited_dahai_actions_by_safety_includes_no_safety_and_preserves_order() {
        // リーチ者はいるが河は空・壁も無しなので全て NoSafety。NoSafety も含み元の順序を保つ。
        let context = suited_context(vec![], Default::default(), [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(32) },
        ];
        let ranked = suited_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(16) },
                    SuitedSafetyRank::NoSafety
                ),
                (
                    &LegalAction::Dahai { tile: tile(0) },
                    SuitedSafetyRank::NoSafety
                ),
                (
                    &LegalAction::Dahai { tile: tile(32) },
                    SuitedSafetyRank::NoSafety
                ),
            ]
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_none_without_opponent_reach() {
        // 他家リーチがいなければ、1m が 2m 4枚で NoChance でも選ばない。
        let context = suited_context(
            vec![tile(4), tile(5), tile(6), tile(7)],
            Default::default(),
            [false; 4],
        );
        let actions = vec![LegalAction::Dahai { tile: tile(0) }];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            None
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_none_when_only_no_safety() {
        // 全て NoSafety の数牌しかない場合は None。
        let context = suited_context(vec![], Default::default(), [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            None
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_returns_safest_dahai() {
        // 4m 河でスジ判定。4m を4枚見えにして 2m を NoChance、1m はスジ(Suji)。最も安全な 2m を選ぶ。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = suited_context(
            vec![tile(12), tile(13), tile(14), tile(15)],
            discards,
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(4) },
        ];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            Some(&LegalAction::Dahai { tile: tile(4) })
        );
    }

    #[test]
    fn select_defense_fallback_action_with_kind_none_without_opponent_reach() {
        let context = suited_context(
            vec![tile(0), tile(1), tile(2), tile(3)],
            [vec![], vec![tile(16)], vec![], vec![]],
            [false; 4],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(108) },
            LegalAction::Dahai { tile: tile(0) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            None
        );
    }

    #[test]
    fn select_defense_fallback_action_with_kind_returns_genbutsu() {
        let context = suited_context(
            vec![],
            [vec![], vec![tile(16)], vec![], vec![]],
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(16) },
                DefenseFallbackKind::Genbutsu
            ))
        );
    }

    #[test]
    fn select_defense_fallback_action_with_kind_prefers_genbutsu_over_honor() {
        // 共通現物 16(5m) と字牌 108(東) が両方候補でも Genbutsu を優先する。
        let context = suited_context(
            vec![],
            [vec![], vec![tile(16)], vec![], vec![]],
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(108) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(16) },
                DefenseFallbackKind::Genbutsu
            ))
        );
    }

    #[test]
    fn select_defense_fallback_action_with_kind_returns_honor_safety_with_rank() {
        // 共通現物なし。東は2枚見えなので HonorSafety(TwoVisible)。
        let context = suited_context(
            vec![tile(108), tile(109)],
            Default::default(),
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(112) },
            LegalAction::Dahai { tile: tile(108) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(108) },
                DefenseFallbackKind::HonorSafety(HonorSafetyRank::TwoVisible)
            ))
        );
    }

    #[test]
    fn select_defense_fallback_action_with_kind_returns_suited_safety_no_chance() {
        // 共通現物も字牌もなし。4m を4枚見えにして経路 [3m,4m] を Blocked にし 2m を NoChance。
        let context = suited_context(
            vec![tile(12), tile(13), tile(14), tile(15)],
            Default::default(),
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(4) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(4) },
                DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoChance)
            ))
        );
    }

    #[test]
    fn select_defense_fallback_action_with_kind_returns_suited_safety_one_chance() {
        // 4m を3枚見えにして経路 [3m,4m] を OneChance にし 2m を OneChance。
        let context = suited_context(
            vec![tile(12), tile(13), tile(14)],
            Default::default(),
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(4) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(4) },
                DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::OneChance)
            ))
        );
    }

    #[test]
    fn select_defense_fallback_action_with_kind_returns_suited_safety_suji() {
        // リーチ者の河に 12(4m)。1m はスジで Suji。
        let context = suited_context(
            vec![],
            [vec![], vec![tile(12)], vec![], vec![]],
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(0) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(0) },
                DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
            ))
        );
    }

    #[test]
    fn select_defense_fallback_action_with_kind_none_when_only_no_safety() {
        // 共通現物も字牌もなく、数牌が全て NoSafety なら None。
        let context = suited_context(vec![], Default::default(), [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            None
        );
    }

    #[test]
    fn select_defense_fallback_action_returns_action_only() {
        let context = suited_context(
            vec![],
            [vec![], vec![tile(16)], vec![], vec![]],
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_defense_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(16) })
        );
    }

    #[test]
    fn is_suji_for_all_reached_false_without_reachers() {
        // 河に 4m があっても、リーチ者がいなければ false。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false; 4]);
        assert!(!is_suji_for_all_reached(tile(0).tile_type(), &context));
    }

    #[test]
    fn is_suji_for_all_reached_single_reacher_classifies_number_tiles() {
        // 単独リーチ者(1)の河に 4m。1m と 7m はスジ、5m は無スジ。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(is_suji_for_all_reached(tile(0).tile_type(), &context));
        assert!(is_suji_for_all_reached(tile(24).tile_type(), &context));
        assert!(!is_suji_for_all_reached(tile(16).tile_type(), &context));
    }

    #[test]
    fn is_suji_for_all_reached_matches_any_for_single_reacher() {
        // 単独リーチでは any 判定と all 判定が一致する。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        for value in [0u8, 16, 24] {
            let tile_type = tile(value).tile_type();
            assert_eq!(
                is_suji_for_any_reached(tile_type, &context),
                is_suji_for_all_reached(tile_type, &context)
            );
        }
    }

    #[test]
    fn is_suji_for_all_reached_true_when_all_reachers_have_suji() {
        // 二人のリーチ者の河にそれぞれ 4m。1m は全員にスジ。
        let discards = [vec![], vec![tile(12)], vec![tile(13)], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
        assert!(is_suji_for_all_reached(tile(0).tile_type(), &context));
    }

    #[test]
    fn is_suji_for_all_reached_false_when_only_one_reacher_has_suji() {
        // 一人目の河にだけ 4m。any は true でも all は false(主要な回帰テスト)。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
        assert!(is_suji_for_any_reached(tile(0).tile_type(), &context));
        assert!(!is_suji_for_all_reached(tile(0).tile_type(), &context));
    }

    #[test]
    fn is_suji_for_all_reached_ignores_own_reach() {
        // 自分(0)の河にだけ 4m。自分のリーチは対象外で、他家リーチ者(1)の河には根拠なし。
        let discards = [vec![tile(12)], vec![], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [true, true, false, false]);
        assert!(!is_suji_for_all_reached(tile(0).tile_type(), &context));
    }

    #[test]
    fn is_suji_for_all_reached_without_player_id_targets_all_reached() {
        // player_id なしはリーチフラグが立っている全席を対象にする。
        let discards = [vec![tile(12)], vec![], vec![], vec![]];
        let context = table_state_context(None, None, discards, [true, false, false, false]);
        assert!(is_suji_for_all_reached(tile(0).tile_type(), &context));
    }

    #[test]
    fn is_suji_for_all_reached_false_for_honor() {
        // 字牌はスジ判定対象外なので false。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        assert!(!is_suji_for_all_reached(tile(108).tile_type(), &context));
    }

    #[test]
    fn suji_safety_rank_for_all_reached_none_for_honor() {
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [false, true, false, false],
        );
        assert_eq!(
            suji_safety_rank_for_all_reached(tile(108).tile_type(), &context),
            None
        );
    }

    #[test]
    fn suji_safety_rank_for_all_reached_no_suji_when_only_one_reacher_has_suji() {
        // 二人リーチで一人だけにスジ。all 基準では NoSuji / NoSafety。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
        assert_eq!(
            suji_safety_rank_for_all_reached(tile(0).tile_type(), &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::NoSafety)
        );
    }

    #[test]
    fn suji_safety_rank_for_all_reached_suji_when_all_reachers_have_suji() {
        // 二人リーチで全員にスジ。all 基準でも Suji。
        let discards = [vec![], vec![tile(12)], vec![tile(13)], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, true, false]);
        assert_eq!(
            suji_safety_rank_for_all_reached(tile(0).tile_type(), &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::Suji)
        );
    }

    #[test]
    fn suited_safety_rank_for_all_reached_none_for_honor() {
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [false, true, false, false],
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(108).tile_type(), &context),
            None
        );
    }

    #[test]
    fn suited_safety_rank_for_all_reached_keeps_wall_priority_over_suji() {
        // 二人リーチで一人だけにスジの 1m でも、壁評価はスジより優先される。
        let discards = [vec![], vec![tile(12)], vec![], vec![]];
        // 2m を4枚見えにして経路 [2m,3m] を Blocked -> 1m は NoChance。
        let context = suited_context(
            vec![tile(4), tile(5), tile(6), tile(7)],
            discards.clone(),
            [false, true, true, false],
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::NoChance)
        );
        // 2m を3枚見え -> 1m は OneChance。
        let context = suited_context(
            vec![tile(4), tile(5), tile(6)],
            discards,
            [false, true, true, false],
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::OneChance)
        );
    }

    // 二人のリーチ者について、2m は全員にスジ・1m は一人にだけスジになる状況を作る。
    // player1 の河: 4m(1m スジ根拠) と 5m(2m スジ根拠)。player2 の河: 5m のみ。
    fn all_reached_partial_suji_context(visible_tiles: Vec<TileId>) -> GameContext {
        suited_context(
            visible_tiles,
            [vec![], vec![tile(12), tile(16)], vec![tile(17)], vec![]],
            [false, true, true, false],
        )
    }

    #[test]
    fn suji_dahai_actions_by_safety_uses_all_reached_basis() {
        // 合法 Dahai を 1m, 2m の順で渡す。2m は全員スジ、1m は一人だけスジ。
        let context = all_reached_partial_suji_context(vec![]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(4) },
        ];
        let ranked = suji_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (&LegalAction::Dahai { tile: tile(4) }, SujiSafetyRank::Suji),
                (
                    &LegalAction::Dahai { tile: tile(0) },
                    SujiSafetyRank::NoSuji
                ),
            ]
        );
    }

    #[test]
    fn suited_dahai_actions_by_safety_uses_all_reached_basis() {
        let context = all_reached_partial_suji_context(vec![]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(4) },
        ];
        let ranked = suited_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(4) },
                    SuitedSafetyRank::Suji
                ),
                (
                    &LegalAction::Dahai { tile: tile(0) },
                    SuitedSafetyRank::NoSafety
                ),
            ]
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_prefers_all_reached_suji() {
        // 一人だけスジの 1m と全員スジの 2m がある場合、全員スジの 2m を選ぶ。
        let context = all_reached_partial_suji_context(vec![]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(4) },
        ];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            Some(&LegalAction::Dahai { tile: tile(4) })
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_none_when_only_partial_suji() {
        // 一人だけスジの牌しかなく壁もない場合は None。
        let context = all_reached_partial_suji_context(vec![]);
        let actions = vec![LegalAction::Dahai { tile: tile(0) }];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            None
        );
    }

    #[test]
    fn select_defense_fallback_action_with_kind_prefers_all_reached_suji() {
        // 共通現物なし・字牌 Dahai なし。一人だけスジと全員スジがあれば全員スジを選ぶ。
        let context = all_reached_partial_suji_context(vec![]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(4) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(4) },
                DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
            ))
        );
    }

    // 実戦問題の最小回帰。合法 Dahai は 6p(56) と 1s(72)。6p は自身が3枚見えているが周辺牌に壁なし、
    // リーチ者(1人)の河は 4s(84) のみ。6p は現物でなく無スジ、1s は 4s に対してスジ。
    #[test]
    fn real_world_regression_prefers_suji_1s_over_self_visible_6p() {
        let six_pin = tile(56).tile_type();
        let one_sou = tile(72).tile_type();
        let context = suited_context(
            vec![tile(56), tile(57), tile(58)],
            [vec![], vec![tile(84)], vec![], vec![]],
            [false, true, false, false],
        );

        // 6p 自身が3枚見えていても、経路 4p/5p/7p/8p に壁がないので NoWall。
        assert_eq!(wall_rank(six_pin, &context), WallRank::NoWall);
        assert_eq!(
            suited_safety_rank_for_all_reached(six_pin, &context),
            Some(SuitedSafetyRank::NoSafety)
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(one_sou, &context),
            Some(SuitedSafetyRank::Suji)
        );

        let actions = vec![
            LegalAction::Dahai { tile: tile(56) },
            LegalAction::Dahai { tile: tile(72) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(72) },
                DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
            ))
        );
    }

    // 同じ候補でも、リーチ者の河に 6p があれば 6p を現物として選ぶ。現物優先が壊れていないこと。
    #[test]
    fn real_world_regression_keeps_genbutsu_6p_when_in_river() {
        let context = suited_context(
            vec![tile(56), tile(57), tile(58)],
            [vec![], vec![tile(59), tile(84)], vec![], vec![]],
            [false, true, false, false],
        );
        let actions = vec![
            LegalAction::Dahai { tile: tile(56) },
            LegalAction::Dahai { tile: tile(72) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(56) },
                DefenseFallbackKind::Genbutsu
            ))
        );
    }

    #[test]
    fn defense_fallback_diagnostic_from_selection_for_suited_suji() {
        // 1s をスジとして選んだ場合の診断データ。壁は NoWall、suji は true、suited safety は Suji。
        let context = suited_context(
            vec![tile(56), tile(57), tile(58)],
            [vec![], vec![tile(84)], vec![], vec![]],
            [false, true, false, false],
        );
        let action = LegalAction::Dahai { tile: tile(72) };
        let diagnostic = DefenseFallbackDiagnostic::from_selection(
            &context,
            &action,
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji),
        );
        assert_eq!(diagnostic.selected_action, "1s");
        assert_eq!(
            diagnostic.selected_kind,
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
        );
        assert_eq!(diagnostic.opponent_reach_count, 1);
        assert!(!diagnostic.selected_genbutsu_for_all);
        assert_eq!(diagnostic.selected_honor_safety_rank, None);
        assert_eq!(diagnostic.selected_wall_rank, Some(WallRank::NoWall));
        assert_eq!(diagnostic.selected_suji_for_all_reached, Some(true));
        assert_eq!(
            diagnostic.selected_suited_safety_rank,
            Some(SuitedSafetyRank::Suji)
        );
    }

    #[test]
    fn defense_fallback_diagnostic_from_selection_for_genbutsu() {
        // 6p を現物として選んだ場合の診断データ。genbutsu は true、数牌 safety も算出される。
        let context = suited_context(
            vec![tile(56), tile(57), tile(58)],
            [vec![], vec![tile(59)], vec![], vec![]],
            [false, true, false, false],
        );
        let action = LegalAction::Dahai { tile: tile(56) };
        let diagnostic = DefenseFallbackDiagnostic::from_selection(
            &context,
            &action,
            DefenseFallbackKind::Genbutsu,
        );
        assert_eq!(diagnostic.selected_action, "6p");
        assert_eq!(diagnostic.selected_kind, DefenseFallbackKind::Genbutsu);
        assert!(diagnostic.selected_genbutsu_for_all);
        assert_eq!(diagnostic.selected_wall_rank, Some(WallRank::NoWall));
    }

    #[test]
    fn defense_fallback_diagnostic_from_selection_for_honor() {
        // 字牌を選んだ場合、壁・スジ・数牌 safety は None で字牌 safety だけ算出される。
        let context = suited_context(
            vec![tile(108), tile(109)],
            Default::default(),
            [false, true, false, false],
        );
        let action = LegalAction::Dahai { tile: tile(108) };
        let diagnostic = DefenseFallbackDiagnostic::from_selection(
            &context,
            &action,
            DefenseFallbackKind::HonorSafety(HonorSafetyRank::TwoVisible),
        );
        assert_eq!(diagnostic.selected_action, "E");
        assert_eq!(
            diagnostic.selected_honor_safety_rank,
            Some(HonorSafetyRank::TwoVisible)
        );
        assert_eq!(diagnostic.selected_wall_rank, None);
        assert_eq!(diagnostic.selected_suji_for_all_reached, None);
        assert_eq!(diagnostic.selected_suited_safety_rank, None);
    }
}
