use crate::action::{LegalAction, prefer_black_five_for_action};
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
//
// 最初の共通現物で牌種を決めた後、その同一牌種内では prefer_black_five_for_action で黒5を
// 優先する。牌種の選択や合法 action 間の牌種順は変えず、物理牌だけを黒牌へ正規化する。
pub fn select_genbutsu_fallback_action<'a>(
    context: &GameContext,
    legal_actions: &'a [LegalAction],
) -> Option<&'a LegalAction> {
    if !context.any_opponent_reached() {
        return None;
    }
    let chosen = genbutsu_dahai_actions_for_all_reached(legal_actions, context)
        .into_iter()
        .next()?;
    Some(prefer_black_five_for_action(legal_actions, chosen))
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

/// 対象リーチ者にとっての役牌価値。危険度は `GuestWind` < `SingleValueHonor` < `DoubleWind` で、
/// derive した `Ord` の順序と一致する。
///
/// これは防御上のヒューリスティックであって、「役牌ならルール上ロンされやすい」という意味では
/// ない。リーチ者はすでにリーチという役を持つため、客風でも待ち牌になり得る。相手が手牌に
/// 保持しやすいと考えられる役牌、特に連風牌を危険寄りに扱うための重み付けとして使う。
///
/// - `GuestWind`:        対象リーチ者にとって場風でも自風でもない風牌。
/// - `SingleValueHonor`: 三元牌、または場風だけ / 自風だけに該当する風牌。
/// - `DoubleWind`:       対象リーチ者にとって場風かつ自風の風牌(ダブ東・ダブ南等)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpponentHonorValue {
    GuestWind,
    SingleValueHonor,
    DoubleWind,
}

/// 指定 player の自風を親から導出する pure helper。4人麻雀の席順だけを前提にする。
///
/// `seat_index = (player + 4 - oya) % 4` で、`oya` 自身は東家。範囲外の `player` / `oya` は
/// 推測で補完せず `None`。`GameContext::seat_wind()` は自分の自風なので相手の判定には使わない。
pub fn seat_wind_for_player(player: usize, oya: u8) -> Option<TileType> {
    if player >= 4 || oya >= 4 {
        return None;
    }
    let seat_index = (player as u8 + 4 - oya) % 4;
    TileType::wind_from_seat_index(seat_index)
}

/// 指定 player にとっての役牌価値を求める pure helper。数牌は対象外で `None`。
///
/// 三元牌は常に `SingleValueHonor`。風牌は場風と player の自風の一致で分類し、特定の `E` / `S`
/// へはハードコードしない。
///
/// 場風または親が不明で風牌の分類を確定できない場合は推測せず `None` (unknown) を返す。
/// unknown を `GuestWind` として安全牌扱いしたり、根拠なく `DoubleWind` と決めつけたりしない。
pub fn opponent_honor_value_for(
    tile: TileType,
    player: usize,
    context: &GameContext,
) -> Option<OpponentHonorValue> {
    if tile.is_dragon() {
        return Some(OpponentHonorValue::SingleValueHonor);
    }
    if !tile.is_wind() {
        return None;
    }
    let round_wind = context.round_wind()?;
    let seat_wind = seat_wind_for_player(player, context.oya()?)?;

    let value = match (round_wind == tile, seat_wind == tile) {
        (true, true) => OpponentHonorValue::DoubleWind,
        (true, false) | (false, true) => OpponentHonorValue::SingleValueHonor,
        (false, false) => OpponentHonorValue::GuestWind,
    };
    Some(value)
}

/// 全リーチ者に対する役牌価値のうち最も危険な評価。数牌は対象外で `None`。
///
/// 対象牌が現物のリーチ者からはロンされないので、そのリーチ者は集約対象から除外する。
/// リーチ者がいない場合、全リーチ者に対して現物の場合、情報不足で誰の分も確定できない場合は
/// `None` (unknown)。unknown を安全側にも危険側にも倒さない。
///
/// これが字牌防御における役牌価値の source of truth。
pub fn opponent_honor_value_for_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<OpponentHonorValue> {
    context
        .reached_opponents()
        .iter()
        .filter(|&&player| !is_genbutsu_for(tile, player, context))
        .filter_map(|&player| opponent_honor_value_for(tile, player, context))
        .max()
}

// 字牌候補1件ぶんの並べ替え用データ。役牌価値は確定できなければ None。
type RankedHonorCandidate<'a> = (&'a LegalAction, HonorSafetyRank, Option<OpponentHonorValue>);

// HonorSafetyRank が同じ候補の並びを役牌価値で tie-break する。
//
// 危険度の昇順(GuestWind → SingleValueHonor → DoubleWind)へ安定に並べ替えるが、対象は役牌価値
// が確定している候補だけ。unknown の候補は元の位置に固定したまま残し、既知の候補どうしをその
// 空き位置へ順に詰め直す。unknown を既知の値と比較しないので、安全側にも危険側にも倒れない。
fn sort_group_by_opponent_honor_value(group: &mut [RankedHonorCandidate<'_>]) {
    let slots: Vec<usize> = (0..group.len())
        .filter(|&index| group[index].2.is_some())
        .collect();
    let mut ordered: Vec<RankedHonorCandidate<'_>> =
        slots.iter().map(|&index| group[index]).collect();
    ordered.sort_by_key(|candidate| candidate.2);

    for (&slot, candidate) in slots.iter().zip(ordered) {
        group[slot] = candidate;
    }
}

// 合法 Dahai のうち字牌のみを 見え枚数の安全度 → 役牌価値 → 元の順序 で並べる。
//
// 役牌価値は HonorSafetyRank が同じ候補どうしの tie-break にだけ使い、見え枚数を逆転しない。
// 役牌価値を確定できない候補はその値を理由に順位を付けず、元の順序を保つ。
pub fn honor_dahai_actions_by_safety<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Vec<(&'a LegalAction, HonorSafetyRank)> {
    let mut ranked: Vec<RankedHonorCandidate<'a>> = legal_actions
        .iter()
        .filter_map(|action| match action {
            LegalAction::Dahai { tile } => {
                let tile_type = tile.tile_type();
                honor_safety_rank(tile_type, context).map(|rank| {
                    (
                        action,
                        rank,
                        opponent_honor_value_for_reached(tile_type, context),
                    )
                })
            }
            _ => None,
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));

    let mut start = 0;
    while start < ranked.len() {
        let rank = ranked[start].1;
        let end = ranked[start..]
            .iter()
            .position(|candidate| candidate.1 != rank)
            .map_or(ranked.len(), |offset| start + offset);
        sort_group_by_opponent_honor_value(&mut ranked[start..end]);
        start = end;
    }

    ranked
        .into_iter()
        .map(|(action, rank, _)| (action, rank))
        .collect()
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

/// スジ安全度。両側スジ / 片スジ / 無スジ の3段階。
///
/// 安全度は `Suji` > `HalfSuji` > `NoSuji` で、derive した `Ord` の順序と一致する。
///
/// - `Suji`:     対象牌のスジ本数ぶんすべての根拠牌が河にある(4/5/6 なら両側)。
/// - `HalfSuji`: 2本のスジを持つ 4/5/6 で、片側の根拠牌だけが河にある片スジ。
/// - `NoSuji`:   根拠牌が河に無い。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SujiSafetyRank {
    NoSuji,
    HalfSuji,
    Suji,
}

// 対象牌のスジ根拠となる同一色の数字。4/5/6 だけが両側の2本を持つ。
fn suji_partner_numbers(number: u8) -> &'static [u8] {
    match number {
        1 => &[4],
        2 => &[5],
        3 => &[6],
        4 => &[1, 7],
        5 => &[2, 8],
        6 => &[3, 9],
        7 => &[4],
        8 => &[5],
        9 => &[6],
        _ => &[],
    }
}

/// 指定 player の河に対するスジ安全度を求める pure helper。字牌は対象外で `None`。
///
/// 同一色のスジ根拠牌(1/2/3 と 7/8/9 は1本、4/5/6 は両側2本)が河に何本あるかで分類する。
/// すべて揃えば `Suji`、一部だけなら `HalfSuji`、無ければ `NoSuji`。
///
/// player が範囲外で河を取得できない場合は推測せず `NoSuji` として扱い、安全側へ倒さない。
pub fn suji_safety_rank_for(
    tile: TileType,
    player: usize,
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    let (Some(number), Some(suit)) = (tile.number(), tile.suit()) else {
        return None;
    };
    let partners = suji_partner_numbers(number);
    if partners.is_empty() {
        return Some(SujiSafetyRank::NoSuji);
    }
    let Some(discards) = context.discards_of(player) else {
        return Some(SujiSafetyRank::NoSuji);
    };
    let found = partners
        .iter()
        .filter(|&&partner| {
            discards.iter().any(|discarded| {
                let discarded = discarded.tile_type();
                discarded.suit() == Some(suit) && discarded.number() == Some(partner)
            })
        })
        .count();
    let rank = if found == partners.len() {
        SujiSafetyRank::Suji
    } else if found > 0 {
        SujiSafetyRank::HalfSuji
    } else {
        SujiSafetyRank::NoSuji
    };
    Some(rank)
}

/// 指定 player に対して完全なスジか判定する。`SujiSafetyRank::Suji` のときだけ `true`。
///
/// 片スジ (`HalfSuji`) と無スジ (`NoSuji`) はいずれも `false`。字牌や範囲外 player も `false`。
pub fn is_suji_for(tile: TileType, player: usize, context: &GameContext) -> bool {
    suji_safety_rank_for(tile, player, context) == Some(SujiSafetyRank::Suji)
}

/// いずれかのリーチ者に対して完全なスジか判定する。リーチ者がいなければ `false`。
pub fn is_suji_for_any_reached(tile: TileType, context: &GameContext) -> bool {
    suji_safety_rank_for_any_reached(tile, context) == Some(SujiSafetyRank::Suji)
}

/// 全リーチ者に対して完全なスジか判定する。リーチ者がいなければ `false`。
///
/// 一人でも片スジ / 無スジなら `false`。
pub fn is_suji_for_all_reached(tile: TileType, context: &GameContext) -> bool {
    suji_safety_rank_for_all_reached(tile, context) == Some(SujiSafetyRank::Suji)
}

/// いずれかのリーチ者の河に対するスジ安全度。数牌なら `Some`、字牌なら `None`。
///
/// 各リーチ者の [`suji_safety_rank_for`] の最大値(最も安全な評価)を採る。
/// リーチ者がいなければ `NoSuji`。
pub fn suji_safety_rank_for_any_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    let rank = context
        .reached_opponents()
        .iter()
        .filter_map(|&player| suji_safety_rank_for(tile, player, context))
        .max()
        .unwrap_or(SujiSafetyRank::NoSuji);
    Some(rank)
}

/// 全リーチ者の河に対するスジ安全度。数牌なら `Some`、字牌なら `None`。
///
/// 各リーチ者の [`suji_safety_rank_for`] の最小値(最も危険な評価)を採る。
/// 例えば player1 に対して `Suji`・player2 に対して `HalfSuji` なら全体は `HalfSuji`。
/// リーチ者がいなければ `NoSuji` で、安全牌としては扱わない。
///
/// これが数牌防御におけるスジ評価の source of truth。
pub fn suji_safety_rank_for_all_reached(
    tile: TileType,
    context: &GameContext,
) -> Option<SujiSafetyRank> {
    if tile.is_honor() {
        return None;
    }
    let rank = context
        .reached_opponents()
        .iter()
        .filter_map(|&player| suji_safety_rank_for(tile, player, context))
        .min()
        .unwrap_or(SujiSafetyRank::NoSuji);
    Some(rank)
}

// 合法 Dahai のうち数牌のみを安全度の高い順(Suji → HalfSuji → NoSuji)に並べる。
// 同安全度は元の順序を保つ。スジ判定は全リーチ者基準で、各リーチ者の rank の最小値を使う。
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
// 安全度は NoChance > OneChance > Suji > HalfSuji > NoSafety。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SuitedSafetyRank {
    NoSafety,
    HalfSuji,
    Suji,
    OneChance,
    NoChance,
}

// スジ安全度を数牌防御の安全度へ写す。壁が無い場合の分類にだけ使う。
fn suited_safety_rank_from_suji(rank: SujiSafetyRank) -> SuitedSafetyRank {
    match rank {
        SujiSafetyRank::Suji => SuitedSafetyRank::Suji,
        SujiSafetyRank::HalfSuji => SuitedSafetyRank::HalfSuji,
        SujiSafetyRank::NoSuji => SuitedSafetyRank::NoSafety,
    }
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
        WallRank::NoWall => suji_safety_rank_for_any_reached(tile, context)
            .map_or(SuitedSafetyRank::NoSafety, suited_safety_rank_from_suji),
    };
    Some(rank)
}

// 全リーチ者の河に対する数牌の安全度を壁 / スジから分類する。字牌は対象外で None。
// 壁評価はスジ評価より優先する。スジ評価は全リーチ者に対する rank の最小値を使う。
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
        WallRank::NoWall => suji_safety_rank_for_all_reached(tile, context)
            .map_or(SuitedSafetyRank::NoSafety, suited_safety_rank_from_suji),
    };
    Some(rank)
}

// 合法 Dahai のうち数牌のみを安全度の高い順
// (NoChance → OneChance → Suji → HalfSuji → NoSafety)に並べる。
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
//
// 安全度順と安定順序で牌種を決めた後、その同一牌種内では prefer_black_five_for_action で
// 黒5を優先する。安全度 rank や合法 action 間の牌種順は変えず、物理牌だけを黒牌へ正規化する。
pub fn select_suited_safety_fallback_action<'a>(
    legal_actions: &'a [LegalAction],
    context: &GameContext,
) -> Option<&'a LegalAction> {
    if !context.any_opponent_reached() {
        return None;
    }
    let chosen = suited_dahai_actions_by_safety(legal_actions, context)
        .into_iter()
        .find(|(_, rank)| *rank != SuitedSafetyRank::NoSafety)?
        .0;
    Some(prefer_black_five_for_action(legal_actions, chosen))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenseFallbackKind {
    Genbutsu,
    HonorSafety(HonorSafetyRank),
    SuitedSafety(SuitedSafetyRank),
}

// 他家リーチ中の防御 fallback を優先順位付きで選ぶ。
// 現物 → 字牌 safety → 数牌防御 の順に評価し、選ばれた種別を添えて返す。
//
// 現物は黒5対応済みの select_genbutsu_fallback_action をそのまま利用する。字牌 safety と
// 数牌 safety は DefenseFallbackKind に載せる rank を候補列から一度に得るため、候補列を直接
// 使ったうえで prefer_black_five_for_action で黒5へ正規化する(冪等)。字牌に赤5は無いので
// 字牌側の正規化は実質 no-op。いずれも牌種選択・種別・安全度 rank は変えない。
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
        let action = prefer_black_five_for_action(legal_actions, action);
        return Some((action, DefenseFallbackKind::HonorSafety(rank)));
    }

    if context.any_opponent_reached()
        && let Some((action, rank)) = suited_dahai_actions_by_safety(legal_actions, context)
            .into_iter()
            .find(|(_, rank)| *rank != SuitedSafetyRank::NoSafety)
    {
        let action = prefer_black_five_for_action(legal_actions, action);
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
    /// 現物のリーチ者を除いた全リーチ者に対する [`opponent_honor_value_for_reached`] の結果。
    ///
    /// 同じ `selected_honor_safety_rank` の字牌どうしの tie-break に使った値。数牌では `None`。
    pub selected_opponent_honor_value: Option<OpponentHonorValue>,
    pub selected_wall_rank: Option<WallRank>,
    /// 全リーチ者に対して完全なスジなら `true`。片スジ / 無スジはどちらも `false`。
    /// 片スジと無スジの区別は `selected_suji_safety_rank_for_all_reached` で分かる。
    pub selected_suji_for_all_reached: Option<bool>,
    /// 全リーチ者に対する [`suji_safety_rank_for_all_reached`] の結果そのもの。
    ///
    /// 壁と統合する前の純粋なスジ評価なので、`selected_suited_safety_rank` が壁由来の
    /// `OneChance` / `NoChance` になっている場合でも `HalfSuji` と `NoSuji` を区別できる。
    pub selected_suji_safety_rank_for_all_reached: Option<SujiSafetyRank>,
    pub selected_suited_safety_rank: Option<SuitedSafetyRank>,
}

impl DefenseFallbackDiagnostic {
    /// 選択された防御 fallback の action と種別から診断データを構築する pure helper。
    ///
    /// 数牌に対しては `wall_rank` / `is_suji_for_all_reached` / `suji_safety_rank_for_all_reached`
    /// / `suited_safety_rank_for_all_reached` を、字牌に対しては `honor_safety_rank` を計算する。
    /// Dahai 以外の action では牌由来の値は空。
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
            selected_opponent_honor_value: tile_type
                .and_then(|tile| opponent_honor_value_for_reached(tile, context)),
            selected_wall_rank: suited_tile.map(|tile| wall_rank(tile, context)),
            selected_suji_for_all_reached: suited_tile
                .map(|tile| is_suji_for_all_reached(tile, context)),
            selected_suji_safety_rank_for_all_reached: tile_type
                .and_then(|tile| suji_safety_rank_for_all_reached(tile, context)),
            selected_suited_safety_rank: tile_type
                .and_then(|tile| suited_safety_rank_for_all_reached(tile, context)),
        }
    }
}

/// 合法 Dahai 1件ごとの防御候補評価。
///
/// 防御 fallback の優先順位判断に使う値だけを pure に保持する解析用データで、これ自体が
/// action 選択を行うことはない。選択の source of truth は
/// [`select_defense_fallback_action_with_kind`] であり、`selected` はその結果を写したもの。
///
/// 数牌では `wall_rank` / `suji_for_all_reached` / `suited_safety_rank` が `Some`、字牌では
/// `honor_safety_rank` が `Some` になり、無関係なフィールドは `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefenseCandidateDiagnostic {
    /// 対象の合法 Dahai。物理牌(赤5 / 黒5)の区別を保持する。
    pub action: LegalAction,
    /// `action` の牌種。
    pub tile: TileType,
    /// この候補が防御 fallback として選ばれたか。
    pub selected: bool,
    pub genbutsu_for_all: bool,
    pub honor_safety_rank: Option<HonorSafetyRank>,
    /// 現物のリーチ者を除いた全リーチ者に対する [`opponent_honor_value_for_reached`] の結果。
    ///
    /// 同じ `honor_safety_rank` の字牌どうしの tie-break に使う値。数牌では `None`。
    pub opponent_honor_value: Option<OpponentHonorValue>,
    pub wall_rank: Option<WallRank>,
    /// 全リーチ者に対して完全なスジなら `true`。片スジ / 無スジはどちらも `false`。
    /// 片スジと無スジの区別は `suji_safety_rank_for_all_reached` で分かる。
    pub suji_for_all_reached: Option<bool>,
    /// 全リーチ者に対する [`suji_safety_rank_for_all_reached`] の結果そのもの。
    ///
    /// 壁と統合する前の純粋なスジ評価なので、`suited_safety_rank` が壁由来の
    /// `OneChance` / `NoChance` になっている場合でも `HalfSuji` と `NoSuji` を区別できる。
    pub suji_safety_rank_for_all_reached: Option<SujiSafetyRank>,
    pub suited_safety_rank: Option<SuitedSafetyRank>,
}

impl DefenseCandidateDiagnostic {
    /// 合法 Dahai 1件から防御候補評価を構築する pure helper。Dahai 以外の action では `None`。
    pub fn for_dahai_action(
        context: &GameContext,
        action: &LegalAction,
        selected: bool,
    ) -> Option<Self> {
        let LegalAction::Dahai { tile } = action else {
            return None;
        };
        let tile_type = tile.tile_type();
        let suited_tile = (!tile_type.is_honor()).then_some(tile_type);

        Some(Self {
            action: action.clone(),
            tile: tile_type,
            selected,
            genbutsu_for_all: is_genbutsu_for_all_reached(tile_type, context),
            honor_safety_rank: honor_safety_rank(tile_type, context),
            opponent_honor_value: opponent_honor_value_for_reached(tile_type, context),
            wall_rank: suited_tile.map(|tile| wall_rank(tile, context)),
            suji_for_all_reached: suited_tile.map(|tile| is_suji_for_all_reached(tile, context)),
            suji_safety_rank_for_all_reached: suji_safety_rank_for_all_reached(tile_type, context),
            suited_safety_rank: suited_safety_rank_for_all_reached(tile_type, context),
        })
    }

    /// 合法 action のうち Dahai だけを、元の順序を保って防御候補評価へ変換する。
    ///
    /// `selected_action` は防御 fallback として実際に選ばれた action。一致する候補の `selected`
    /// だけが `true` になる。
    pub fn for_legal_actions(
        context: &GameContext,
        legal_actions: &[LegalAction],
        selected_action: Option<&LegalAction>,
    ) -> Vec<Self> {
        legal_actions
            .iter()
            .filter_map(|action| {
                Self::for_dahai_action(context, action, selected_action == Some(action))
            })
            .collect()
    }
}

/// 防御 fallback を検討した局面の構造化診断。
///
/// `selected` は防御 fallback を採用した場合の既存診断で、検討したが候補が無かった場合は `None`。
/// `candidates` は採否にかかわらず全合法 Dahai の防御評価を保持する解析用データで、
/// 「なぜその牌を切ったか」を後から追跡するために使う。
///
/// 防御選択ロジックは再実装しない。採用結果は [`select_defense_fallback_action_with_kind`] の
/// 結果をそのまま写す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefenseDecisionDiagnostic {
    pub selected: Option<DefenseFallbackDiagnostic>,
    pub candidates: Vec<DefenseCandidateDiagnostic>,
}

impl DefenseDecisionDiagnostic {
    /// 実際の防御 fallback 選択結果と合法 action から診断データを構築する pure helper。
    ///
    /// `selected` には [`select_defense_fallback_action_with_kind`] の戻り値をそのまま渡す。
    pub fn from_selection(
        context: &GameContext,
        legal_actions: &[LegalAction],
        selected: Option<(&LegalAction, DefenseFallbackKind)>,
    ) -> Self {
        Self {
            selected: selected.map(|(action, kind)| {
                DefenseFallbackDiagnostic::from_selection(context, action, kind)
            }),
            candidates: DefenseCandidateDiagnostic::for_legal_actions(
                context,
                legal_actions,
                selected.map(|(action, _)| action),
            ),
        }
    }

    /// 採用された防御 fallback の種別。検討したが候補が無かった場合は `None`。
    pub fn selected_kind(&self) -> Option<DefenseFallbackKind> {
        self.selected
            .as_ref()
            .map(|diagnostic| diagnostic.selected_kind)
    }
}

/// 防御 fallback を実際に採用したとき DEBUG イベントを1件出す opt-in ログ。
///
/// `RUST_LOG=bot_core::defense=debug` で有効化する。debug が無効な通常時は診断値や文字列を
/// 一切構築しない。TRACE が有効なら、合法 Dahai ごとの防御評価も追加で記録する。
///
/// 出力値は pure な診断データ (`DefenseFallbackDiagnostic` / `DefenseCandidateDiagnostic`) から
/// 作る。ログを解析して診断データを作る向きにはしない。
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
        selected_opponent_honor_value = ?diagnostic.selected_opponent_honor_value,
        selected_wall_rank = ?diagnostic.selected_wall_rank,
        selected_suji_for_all_reached = ?diagnostic.selected_suji_for_all_reached,
        selected_suji_safety_rank = ?diagnostic.selected_suji_safety_rank_for_all_reached,
        selected_suited_safety_rank = ?diagnostic.selected_suited_safety_rank,
        "defense fallback decision",
    );

    if tracing::enabled!(target: LOG_TARGET, tracing::Level::TRACE) {
        for candidate in
            DefenseCandidateDiagnostic::for_legal_actions(context, legal_actions, Some(action))
        {
            log_defense_fallback_candidate(&candidate);
        }
    }
}

// 合法 Dahai ごとの防御候補評価を TRACE で1件記録する。値は pure な診断データから取り出す。
fn log_defense_fallback_candidate(candidate: &DefenseCandidateDiagnostic) {
    let tile = match &candidate.action {
        LegalAction::Dahai { tile } => tile.to_mjai_string(),
        other => format!("{other:?}"),
    };

    tracing::trace!(
        target: LOG_TARGET,
        tile = %tile,
        genbutsu_for_all = candidate.genbutsu_for_all,
        honor_safety_rank = ?candidate.honor_safety_rank,
        opponent_honor_value = ?candidate.opponent_honor_value,
        wall_rank = ?candidate.wall_rank,
        suji_for_all_reached = ?candidate.suji_for_all_reached,
        suji_safety_rank = ?candidate.suji_safety_rank_for_all_reached,
        suited_safety_rank = ?candidate.suited_safety_rank,
        "defense fallback candidate",
    );
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

    #[test]
    fn select_genbutsu_fallback_action_prefers_black_five() {
        // 河に5m系があり現物。合法 Dahai [赤5m, 黒5m] なら黒5m を選ぶ。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(17) })
        );
    }

    #[test]
    fn select_genbutsu_fallback_action_prefers_black_five_when_reversed() {
        // 合法 Dahai の順序が [黒5m, 赤5m] でも黒5m を選ぶ。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(17) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(17) })
        );
    }

    #[test]
    fn select_genbutsu_fallback_action_keeps_red_five_when_only_red() {
        // 赤5m しか合法でなければ赤5m を維持する。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(16) })
        );
    }

    #[test]
    fn select_genbutsu_fallback_action_keeps_leading_tile_type_over_black_five() {
        // 合法順 [1p, 赤5m, 黒5m] で 1p と 5m系がともに現物。先頭牌種 1p を維持する。
        let discards = [vec![], vec![tile(17), tile(36)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(36) },
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(36) })
        );
    }

    #[test]
    fn select_genbutsu_fallback_action_normalizes_black_five_when_type_leads() {
        // 合法順 [赤5m, 1p, 黒5m] で 1p と 5m系がともに現物。先頭牌種 5m のまま黒5m へ正規化する。
        let discards = [vec![], vec![tile(17), tile(36)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(36) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        assert_eq!(
            select_genbutsu_fallback_action(&context, &actions),
            Some(&LegalAction::Dahai { tile: tile(17) })
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

    fn honor(value: u8) -> TileType {
        TileType::new(value).unwrap()
    }

    const EAST: u8 = 27;
    const SOUTH: u8 = 28;
    const WEST: u8 = 29;
    const NORTH: u8 = 30;
    const HAKU: u8 = 31;
    const HATSU: u8 = 32;
    const CHUN: u8 = 33;

    // 自分は player0 固定。自分の自風は相手の役牌判定に使わないので None のままにする。
    fn honor_value_context(
        round_wind: Option<TileType>,
        oya: Option<u8>,
        reached: [bool; 4],
        discards: [Vec<TileId>; 4],
        visible_tiles: Vec<TileId>,
    ) -> GameContext {
        GameContext::from_parts_with_table_state(
            None,
            vec![],
            vec![],
            round_wind,
            None,
            visible_tiles,
            Some(0),
            oya,
            discards,
            reached,
        )
    }

    // 東場・自分 player0・player1 だけがリーチ。字牌の見え枚数は0枚に揃える。
    fn single_reacher_honor_context(oya: u8) -> GameContext {
        honor_value_context(
            Some(honor(EAST)),
            Some(oya),
            [false, true, false, false],
            Default::default(),
            vec![],
        )
    }

    #[test]
    fn seat_wind_for_player_derives_from_oya() {
        assert_eq!(seat_wind_for_player(0, 0), Some(honor(EAST)));
        assert_eq!(seat_wind_for_player(1, 0), Some(honor(SOUTH)));
        assert_eq!(seat_wind_for_player(2, 0), Some(honor(WEST)));
        assert_eq!(seat_wind_for_player(3, 0), Some(honor(NORTH)));
        assert_eq!(seat_wind_for_player(1, 1), Some(honor(EAST)));
        assert_eq!(seat_wind_for_player(0, 3), Some(honor(SOUTH)));
        assert_eq!(seat_wind_for_player(1, 3), Some(honor(WEST)));
    }

    #[test]
    fn seat_wind_for_player_rejects_out_of_range() {
        assert_eq!(seat_wind_for_player(4, 0), None);
        assert_eq!(seat_wind_for_player(0, 4), None);
        assert_eq!(seat_wind_for_player(usize::MAX, 0), None);
        assert_eq!(seat_wind_for_player(0, 255), None);
    }

    #[test]
    fn opponent_honor_value_for_dragons_is_single_value_honor() {
        // 三元牌は誰にとっても役牌なので、場風 / 親が分からなくても SingleValueHonor。
        let context = single_reacher_honor_context(3);
        for dragon in [HAKU, HATSU, CHUN] {
            assert_eq!(
                opponent_honor_value_for(honor(dragon), 1, &context),
                Some(OpponentHonorValue::SingleValueHonor)
            );
        }

        let unknown = honor_value_context(
            None,
            None,
            [false, true, false, false],
            Default::default(),
            vec![],
        );
        assert_eq!(
            opponent_honor_value_for(honor(CHUN), 1, &unknown),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn opponent_honor_value_for_round_wind_only_is_single_value_honor() {
        // 東場で player1 の自風は南。東は場風だけに該当する。
        let context = single_reacher_honor_context(0);
        assert_eq!(seat_wind_for_player(1, 0), Some(honor(SOUTH)));
        assert_eq!(
            opponent_honor_value_for(honor(EAST), 1, &context),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn opponent_honor_value_for_seat_wind_only_is_single_value_honor() {
        // 東場で player1 の自風は南。南は自風だけに該当する。
        let context = single_reacher_honor_context(0);
        assert_eq!(
            opponent_honor_value_for(honor(SOUTH), 1, &context),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn opponent_honor_value_for_guest_wind() {
        // 東場で player1 の自風は南。西と北はどちらにも該当しない客風。
        let context = single_reacher_honor_context(0);
        assert_eq!(
            opponent_honor_value_for(honor(WEST), 1, &context),
            Some(OpponentHonorValue::GuestWind)
        );
        assert_eq!(
            opponent_honor_value_for(honor(NORTH), 1, &context),
            Some(OpponentHonorValue::GuestWind)
        );
    }

    #[test]
    fn opponent_honor_value_for_double_east() {
        // 東場・oya = player1 なので player1 の自風は東。ダブ東。
        let context = single_reacher_honor_context(1);
        assert_eq!(seat_wind_for_player(1, 1), Some(honor(EAST)));
        assert_eq!(
            opponent_honor_value_for(honor(EAST), 1, &context),
            Some(OpponentHonorValue::DoubleWind)
        );
    }

    #[test]
    fn opponent_honor_value_for_double_south() {
        // 南場・oya = player0 なので player1 の自風は南。ダブ南。
        let context = honor_value_context(
            Some(honor(SOUTH)),
            Some(0),
            [false, true, false, false],
            Default::default(),
            vec![],
        );
        assert_eq!(seat_wind_for_player(1, 0), Some(honor(SOUTH)));
        assert_eq!(
            opponent_honor_value_for(honor(SOUTH), 1, &context),
            Some(OpponentHonorValue::DoubleWind)
        );
    }

    #[test]
    fn opponent_honor_value_for_number_tile_is_none() {
        let context = single_reacher_honor_context(0);
        assert_eq!(
            opponent_honor_value_for(tile(0).tile_type(), 1, &context),
            None
        );
        assert_eq!(
            opponent_honor_value_for(tile(16).tile_type(), 1, &context),
            None
        );
    }

    #[test]
    fn opponent_honor_value_for_wind_without_round_wind_is_unknown() {
        // 場風が不明なら風牌を確定できない。推測で GuestWind へ倒さない。
        let context = honor_value_context(
            None,
            Some(0),
            [false, true, false, false],
            Default::default(),
            vec![],
        );
        for value in [EAST, SOUTH, WEST, NORTH] {
            assert_eq!(opponent_honor_value_for(honor(value), 1, &context), None);
        }
    }

    #[test]
    fn opponent_honor_value_for_wind_without_oya_is_unknown() {
        // 親が不明なら相手の自風を導出できない。推測で DoubleWind とも決めつけない。
        let context = honor_value_context(
            Some(honor(EAST)),
            None,
            [false, true, false, false],
            Default::default(),
            vec![],
        );
        for value in [EAST, SOUTH, WEST, NORTH] {
            assert_eq!(opponent_honor_value_for(honor(value), 1, &context), None);
        }
    }

    #[test]
    fn opponent_honor_value_for_out_of_range_player_is_unknown() {
        let context = single_reacher_honor_context(0);
        assert_eq!(opponent_honor_value_for(honor(EAST), 4, &context), None);
    }

    #[test]
    fn opponent_honor_value_for_ignores_own_seat_wind() {
        // context.seat_wind() は自分の自風。相手の判定へ流用していないことを確かめる。
        let context = GameContext::from_parts_with_table_state(
            None,
            vec![],
            vec![],
            Some(honor(EAST)),
            Some(honor(NORTH)),
            Vec::new(),
            Some(0),
            Some(0),
            Default::default(),
            [false, true, false, false],
        );
        assert_eq!(
            opponent_honor_value_for(honor(NORTH), 1, &context),
            Some(OpponentHonorValue::GuestWind)
        );
    }

    #[test]
    fn opponent_honor_value_for_reached_takes_most_dangerous_guest_and_guest() {
        // 東場・oya = player0。player1 は南家、player3 は北家。西はどちらにも客風。
        let context = honor_value_context(
            Some(honor(EAST)),
            Some(0),
            [false, true, false, true],
            Default::default(),
            vec![],
        );
        assert_eq!(
            opponent_honor_value_for_reached(honor(WEST), &context),
            Some(OpponentHonorValue::GuestWind)
        );
    }

    #[test]
    fn opponent_honor_value_for_reached_takes_most_dangerous_guest_and_single() {
        // 東場・oya = player0。player1 は南家(客風)、player2 は西家(自風)。
        let context = honor_value_context(
            Some(honor(EAST)),
            Some(0),
            [false, true, true, false],
            Default::default(),
            vec![],
        );
        assert_eq!(
            opponent_honor_value_for_reached(honor(WEST), &context),
            Some(OpponentHonorValue::SingleValueHonor)
        );
    }

    #[test]
    fn opponent_honor_value_for_reached_takes_most_dangerous_single_and_double() {
        // 東場・oya = player0・自分は player3。player0 は東家でダブ東、player1 は場風のみ。
        let context = GameContext::from_parts_with_table_state(
            None,
            vec![],
            vec![],
            Some(honor(EAST)),
            None,
            Vec::new(),
            Some(3),
            Some(0),
            Default::default(),
            [true, true, false, false],
        );
        assert_eq!(
            opponent_honor_value_for(honor(EAST), 0, &context),
            Some(OpponentHonorValue::DoubleWind)
        );
        assert_eq!(
            opponent_honor_value_for(honor(EAST), 1, &context),
            Some(OpponentHonorValue::SingleValueHonor)
        );
        assert_eq!(
            opponent_honor_value_for_reached(honor(EAST), &context),
            Some(OpponentHonorValue::DoubleWind)
        );
    }

    #[test]
    fn opponent_honor_value_for_reached_excludes_genbutsu_player() {
        // 東場・oya = player0。player1 は南家で西は客風、player2 は西家だが西が現物。
        // 現物の player2 からはロンされないので、その SingleValueHonor は集約へ入れない。
        let discards = [vec![], vec![], vec![tile(116)], vec![]];
        let context = honor_value_context(
            Some(honor(EAST)),
            Some(0),
            [false, true, true, false],
            discards,
            vec![],
        );
        assert_eq!(
            opponent_honor_value_for(honor(WEST), 2, &context),
            Some(OpponentHonorValue::SingleValueHonor)
        );
        assert_eq!(
            opponent_honor_value_for_reached(honor(WEST), &context),
            Some(OpponentHonorValue::GuestWind)
        );
    }

    #[test]
    fn opponent_honor_value_for_reached_is_unknown_when_all_reachers_have_genbutsu() {
        let discards = [vec![], vec![tile(116)], vec![], vec![]];
        let context = honor_value_context(
            Some(honor(EAST)),
            Some(0),
            [false, true, false, false],
            discards,
            vec![],
        );
        assert_eq!(
            opponent_honor_value_for_reached(honor(WEST), &context),
            None
        );
    }

    #[test]
    fn opponent_honor_value_for_reached_is_unknown_without_reachers() {
        let context = honor_value_context(
            Some(honor(EAST)),
            Some(0),
            [false; 4],
            Default::default(),
            vec![],
        );
        assert_eq!(
            opponent_honor_value_for_reached(honor(WEST), &context),
            None
        );
        assert_eq!(
            opponent_honor_value_for_reached(honor(CHUN), &context),
            None
        );
    }

    #[test]
    fn opponent_honor_value_for_reached_is_none_for_number_tiles() {
        let context = single_reacher_honor_context(0);
        assert_eq!(
            opponent_honor_value_for_reached(tile(0).tile_type(), &context),
            None
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_breaks_ties_by_opponent_honor_value() {
        // 東場・oya = player3 なので player1 の自風は西。北は客風、中は役牌。
        let context = single_reacher_honor_context(3);
        let actions = vec![
            LegalAction::Dahai { tile: tile(132) },
            LegalAction::Dahai { tile: tile(120) },
        ];
        let ranked = honor_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(120) },
                    HonorSafetyRank::NoVisible
                ),
                (
                    &LegalAction::Dahai { tile: tile(132) },
                    HonorSafetyRank::NoVisible
                ),
            ]
        );
    }

    #[test]
    fn select_honor_safety_fallback_action_prefers_guest_wind_over_value_honor() {
        // 最重要回帰ケース1。東場・oya = player3・player1 リーチ(自風は西)。
        // 中は SingleValueHonor、北は GuestWind。同じ見え枚数なら北を先に切る。
        let context = single_reacher_honor_context(3);
        let chun = LegalAction::Dahai { tile: tile(132) };
        let north = LegalAction::Dahai { tile: tile(120) };

        assert_eq!(
            select_honor_safety_fallback_action(&[chun.clone(), north.clone()], &context),
            Some(&north)
        );
        assert_eq!(
            select_honor_safety_fallback_action(&[north.clone(), chun.clone()], &context),
            Some(&north)
        );
    }

    #[test]
    fn select_honor_safety_fallback_action_prefers_value_honor_over_double_wind() {
        // 最重要回帰ケース2。東場・oya = player1・player1 リーチ(自風は東)。
        // 東は DoubleWind、中は SingleValueHonor。同じ見え枚数なら中を先に切る。
        let context = single_reacher_honor_context(1);
        let east = LegalAction::Dahai { tile: tile(108) };
        let chun = LegalAction::Dahai { tile: tile(132) };

        assert_eq!(
            select_honor_safety_fallback_action(&[east.clone(), chun.clone()], &context),
            Some(&chun)
        );
        assert_eq!(
            select_honor_safety_fallback_action(&[chun.clone(), east.clone()], &context),
            Some(&chun)
        );
    }

    #[test]
    fn select_honor_safety_fallback_action_prefers_guest_wind_over_double_wind() {
        // 最重要回帰ケース3。同条件で北は GuestWind、東は DoubleWind。北を先に切る。
        let context = single_reacher_honor_context(1);
        let north = LegalAction::Dahai { tile: tile(120) };
        let east = LegalAction::Dahai { tile: tile(108) };

        assert_eq!(
            select_honor_safety_fallback_action(&[east.clone(), north.clone()], &context),
            Some(&north)
        );
        assert_eq!(
            select_honor_safety_fallback_action(&[north.clone(), east.clone()], &context),
            Some(&north)
        );
    }

    #[test]
    fn select_honor_safety_fallback_action_uses_partial_genbutsu_aggregation() {
        // 最重要回帰ケース4。東場・oya = player0・player1 と player2 がリーチ。
        // 西は player1 に客風・非現物、player2 に自風だが現物。全体評価は GuestWind。
        let discards = [vec![], vec![], vec![tile(116)], vec![]];
        let context = honor_value_context(
            Some(honor(EAST)),
            Some(0),
            [false, true, true, false],
            discards,
            vec![],
        );
        let west = LegalAction::Dahai { tile: tile(116) };
        let chun = LegalAction::Dahai { tile: tile(132) };

        assert_eq!(
            opponent_honor_value_for_reached(honor(WEST), &context),
            Some(OpponentHonorValue::GuestWind)
        );
        assert_eq!(
            select_honor_safety_fallback_action(&[chun.clone(), west.clone()], &context),
            Some(&west)
        );
    }

    #[test]
    fn select_honor_safety_fallback_action_keeps_visible_count_priority() {
        // 中は3枚見えの SingleValueHonor、西は1枚見えの GuestWind。見え枚数を逆転しない。
        let context = honor_value_context(
            Some(honor(EAST)),
            Some(1),
            [false, true, false, false],
            Default::default(),
            vec![tile(132), tile(133), tile(134), tile(116)],
        );
        let west = LegalAction::Dahai { tile: tile(117) };
        let chun = LegalAction::Dahai { tile: tile(135) };

        assert_eq!(
            honor_safety_rank(honor(CHUN), &context),
            Some(HonorSafetyRank::ThreeOrMoreVisible)
        );
        assert_eq!(
            honor_safety_rank(honor(WEST), &context),
            Some(HonorSafetyRank::OneVisible)
        );
        assert_eq!(
            opponent_honor_value_for_reached(honor(WEST), &context),
            Some(OpponentHonorValue::GuestWind)
        );
        assert_eq!(
            select_honor_safety_fallback_action(&[west.clone(), chun.clone()], &context),
            Some(&chun)
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_preserves_order_for_equal_value_honors() {
        // 白と中はどちらも SingleValueHonor。見え枚数も同じなので元の順序を保つ。
        let context = single_reacher_honor_context(1);
        let actions = vec![
            LegalAction::Dahai { tile: tile(132) },
            LegalAction::Dahai { tile: tile(124) },
        ];
        let ranked: Vec<&LegalAction> = honor_dahai_actions_by_safety(&actions, &context)
            .into_iter()
            .map(|(action, _)| action)
            .collect();
        assert_eq!(ranked, vec![&actions[0], &actions[1]]);
    }

    #[test]
    fn honor_dahai_actions_by_safety_preserves_order_for_equal_guest_winds() {
        // 東場・oya = player1(自風は東)。西と北はどちらも客風なので元の順序を保つ。
        let context = single_reacher_honor_context(1);
        let actions = vec![
            LegalAction::Dahai { tile: tile(120) },
            LegalAction::Dahai { tile: tile(116) },
        ];
        let ranked: Vec<&LegalAction> = honor_dahai_actions_by_safety(&actions, &context)
            .into_iter()
            .map(|(action, _)| action)
            .collect();
        assert_eq!(ranked, vec![&actions[0], &actions[1]]);
    }

    #[test]
    fn honor_dahai_actions_by_safety_leaves_unknown_value_to_stable_order() {
        // 場風が不明なので東は unknown、中は SingleValueHonor。unknown を理由に順序を変えない。
        let context = honor_value_context(
            None,
            Some(1),
            [false, true, false, false],
            Default::default(),
            vec![],
        );
        let east = LegalAction::Dahai { tile: tile(108) };
        let chun = LegalAction::Dahai { tile: tile(132) };

        assert_eq!(
            opponent_honor_value_for_reached(honor(EAST), &context),
            None
        );
        assert_eq!(
            select_honor_safety_fallback_action(&[east.clone(), chun.clone()], &context),
            Some(&east)
        );
        assert_eq!(
            select_honor_safety_fallback_action(&[chun.clone(), east.clone()], &context),
            Some(&chun)
        );
    }

    #[test]
    fn honor_dahai_actions_by_safety_pins_unknown_candidate_in_place() {
        // 東場・oya = player1・player1 リーチ(自風は東)。西は player1 の現物なので unknown。
        // 中と北は既知なので互いに並べ替わるが、unknown の西は元の位置に残る。
        let discards = [vec![], vec![tile(116)], vec![], vec![]];
        let context = honor_value_context(
            Some(honor(EAST)),
            Some(1),
            [false, true, false, false],
            discards,
            vec![tile(117), tile(132), tile(120)],
        );
        let chun = LegalAction::Dahai { tile: tile(133) };
        let west = LegalAction::Dahai { tile: tile(118) };
        let north = LegalAction::Dahai { tile: tile(121) };
        let actions = vec![chun.clone(), west.clone(), north.clone()];

        for tile_type in [honor(CHUN), honor(WEST), honor(NORTH)] {
            assert_eq!(
                honor_safety_rank(tile_type, &context),
                Some(HonorSafetyRank::OneVisible)
            );
        }
        assert_eq!(
            opponent_honor_value_for_reached(honor(WEST), &context),
            None
        );

        let ranked: Vec<&LegalAction> = honor_dahai_actions_by_safety(&actions, &context)
            .into_iter()
            .map(|(action, _)| action)
            .collect();
        assert_eq!(ranked, vec![&north, &west, &chun]);
    }

    #[test]
    fn honor_dahai_actions_by_safety_without_reachers_keeps_stable_order() {
        // リーチ者がいなければ役牌価値は unknown。従来どおり見え枚数と元の順序だけで決まる。
        let context = honor_value_context(
            Some(honor(EAST)),
            Some(1),
            [false; 4],
            Default::default(),
            vec![],
        );
        let east = LegalAction::Dahai { tile: tile(108) };
        let chun = LegalAction::Dahai { tile: tile(132) };

        assert_eq!(
            select_honor_safety_fallback_action(&[east.clone(), chun.clone()], &context),
            Some(&east)
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
        // 1m 河 → 4m は片スジなので is_suji_for は false、4m 河 → 1m/7m は完全スジ。
        let context = single_reacher_discards_context(vec![tile(0)]);
        assert!(!is_suji_for(tile(12).tile_type(), 1, &context));

        let context = single_reacher_discards_context(vec![tile(24)]);
        assert!(!is_suji_for(tile(12).tile_type(), 1, &context));

        let context = single_reacher_discards_context(vec![tile(0), tile(24)]);
        assert!(is_suji_for(tile(12).tile_type(), 1, &context));

        let context = single_reacher_discards_context(vec![tile(12)]);
        assert!(is_suji_for(tile(0).tile_type(), 1, &context));
        assert!(is_suji_for(tile(24).tile_type(), 1, &context));
    }

    #[test]
    fn is_suji_for_detects_two_five_eight() {
        // 5m が河にあれば 2m と 8m はスジ。5m は片スジなので false。
        let context = single_reacher_discards_context(vec![tile(16)]);
        assert!(is_suji_for(tile(4).tile_type(), 1, &context));
        assert!(is_suji_for(tile(28).tile_type(), 1, &context));
        assert!(!is_suji_for(tile(16).tile_type(), 1, &context));
    }

    #[test]
    fn is_suji_for_detects_three_six_nine() {
        // 6m が河にあれば 3m と 9m はスジ。6m は片スジなので false。
        let context = single_reacher_discards_context(vec![tile(20)]);
        assert!(is_suji_for(tile(8).tile_type(), 1, &context));
        assert!(is_suji_for(tile(32).tile_type(), 1, &context));
        assert!(!is_suji_for(tile(20).tile_type(), 1, &context));
    }

    // player1 だけがリーチしている状況で、その河だけを差し替える helper。
    fn single_reacher_discards_context(discards: Vec<TileId>) -> GameContext {
        table_state_context(
            Some(0),
            None,
            [vec![], discards, vec![], vec![]],
            [false, true, false, false],
        )
    }

    // 単独リーチ者の河から、対象牌のスジ安全度を求める。
    fn single_reacher_suji_rank(target: u8, discards: Vec<TileId>) -> Option<SujiSafetyRank> {
        let context = single_reacher_discards_context(discards);
        suji_safety_rank_for_all_reached(tile(target).tile_type(), &context)
    }

    #[test]
    fn suji_safety_rank_for_four_distinguishes_half_and_full_suji() {
        // 4p は 1p-4p と 4p-7p の2本。1p(36) / 7p(60) の有無で NoSuji / HalfSuji / Suji。
        assert_eq!(
            single_reacher_suji_rank(48, vec![]),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            single_reacher_suji_rank(48, vec![tile(36)]),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            single_reacher_suji_rank(48, vec![tile(60)]),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            single_reacher_suji_rank(48, vec![tile(36), tile(60)]),
            Some(SujiSafetyRank::Suji)
        );
    }

    #[test]
    fn suji_safety_rank_for_five_distinguishes_half_and_full_suji() {
        // 5p は 2p(40) と 8p(64) の2本。
        assert_eq!(
            single_reacher_suji_rank(52, vec![]),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            single_reacher_suji_rank(52, vec![tile(40)]),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            single_reacher_suji_rank(52, vec![tile(64)]),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            single_reacher_suji_rank(52, vec![tile(40), tile(64)]),
            Some(SujiSafetyRank::Suji)
        );
    }

    #[test]
    fn suji_safety_rank_for_six_distinguishes_half_and_full_suji() {
        // 6p は 3p(44) と 9p(68) の2本。
        assert_eq!(
            single_reacher_suji_rank(56, vec![]),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            single_reacher_suji_rank(56, vec![tile(44)]),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            single_reacher_suji_rank(56, vec![tile(68)]),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            single_reacher_suji_rank(56, vec![tile(44), tile(68)]),
            Some(SujiSafetyRank::Suji)
        );
    }

    #[test]
    fn suji_safety_rank_for_terminal_side_is_never_half_suji() {
        // 1/2/3 と 7/8/9 はスジが1本だけ。対応牌があれば Suji、無ければ NoSuji。
        for (target, partner) in [
            (36u8, 48u8),
            (40, 52),
            (44, 56),
            (60, 48),
            (64, 52),
            (68, 56),
        ] {
            assert_eq!(
                single_reacher_suji_rank(target, vec![tile(partner)]),
                Some(SujiSafetyRank::Suji)
            );
            assert_eq!(
                single_reacher_suji_rank(target, vec![]),
                Some(SujiSafetyRank::NoSuji)
            );
        }
    }

    #[test]
    fn suji_safety_rank_for_honor_is_none() {
        // 字牌はスジ評価対象外。player 単位でも全リーチ者基準でも None。
        let context = single_reacher_discards_context(vec![tile(12)]);
        assert_eq!(
            suji_safety_rank_for(tile(108).tile_type(), 1, &context),
            None
        );
        assert_eq!(
            suji_safety_rank_for_all_reached(tile(108).tile_type(), &context),
            None
        );
    }

    #[test]
    fn suji_safety_rank_for_out_of_range_player_is_no_suji() {
        // 河を取得できない player は推測せず NoSuji。安全側へは倒さない。
        let context = single_reacher_discards_context(vec![tile(12)]);
        assert_eq!(
            suji_safety_rank_for(tile(0).tile_type(), 4, &context),
            Some(SujiSafetyRank::NoSuji)
        );
    }

    // 二人リーチで、player1 の河と player2 の河を個別に与える helper。
    fn two_reachers_context(first: Vec<TileId>, second: Vec<TileId>) -> GameContext {
        table_state_context(
            Some(0),
            None,
            [vec![], first, second, vec![]],
            [false, true, true, false],
        )
    }

    #[test]
    fn suji_safety_rank_for_all_reached_takes_most_dangerous_rank() {
        // 4p を対象に、二人のリーチ者の rank の最小値を採る。
        let four_pin = tile(48).tile_type();

        // 両者とも 1p/7p 持ち → Suji。
        let context = two_reachers_context(vec![tile(36), tile(60)], vec![tile(37), tile(61)]);
        assert_eq!(
            suji_safety_rank_for_all_reached(four_pin, &context),
            Some(SujiSafetyRank::Suji)
        );

        // player1 は Suji、player2 は 1p だけで HalfSuji → 全体は HalfSuji。
        let context = two_reachers_context(vec![tile(36), tile(60)], vec![tile(37)]);
        assert_eq!(
            suji_safety_rank_for(four_pin, 1, &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suji_safety_rank_for(four_pin, 2, &context),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            suji_safety_rank_for_all_reached(four_pin, &context),
            Some(SujiSafetyRank::HalfSuji)
        );

        // player1 は Suji、player2 は根拠なしで NoSuji → 全体は NoSuji。
        let context = two_reachers_context(vec![tile(36), tile(60)], vec![]);
        assert_eq!(
            suji_safety_rank_for_all_reached(four_pin, &context),
            Some(SujiSafetyRank::NoSuji)
        );
    }

    #[test]
    fn suji_safety_rank_for_any_reached_takes_safest_rank() {
        // any 基準は最大値。片方が Suji なら全体も Suji。
        let four_pin = tile(48).tile_type();
        let context = two_reachers_context(vec![tile(36), tile(60)], vec![]);
        assert_eq!(
            suji_safety_rank_for_any_reached(four_pin, &context),
            Some(SujiSafetyRank::Suji)
        );

        // 片スジと無スジなら HalfSuji。
        let context = two_reachers_context(vec![tile(36)], vec![]);
        assert_eq!(
            suji_safety_rank_for_any_reached(four_pin, &context),
            Some(SujiSafetyRank::HalfSuji)
        );
    }

    #[test]
    fn suji_safety_rank_no_suji_without_reachers() {
        // リーチ者がいなければ、河に根拠があっても安全牌として扱わない。
        let discards = [vec![], vec![tile(36), tile(60)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false; 4]);
        assert_eq!(
            suji_safety_rank_for_all_reached(tile(48).tile_type(), &context),
            Some(SujiSafetyRank::NoSuji)
        );
        assert_eq!(
            suji_safety_rank_for_any_reached(tile(48).tile_type(), &context),
            Some(SujiSafetyRank::NoSuji)
        );
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

    // 5m を NoChance にする visible。経路 [3m,4m] は 4m(12-15)4枚で Blocked、
    // 経路 [6m,7m] は 6m(20-23)4枚で Blocked。5m 自身は含めない。
    fn five_man_no_chance_visible() -> Vec<TileId> {
        vec![
            tile(12),
            tile(13),
            tile(14),
            tile(15),
            tile(20),
            tile(21),
            tile(22),
            tile(23),
        ]
    }

    // 上記 visible で他家(player 1)リーチ中の context。
    fn five_man_no_chance_visible_context() -> GameContext {
        suited_context(
            five_man_no_chance_visible(),
            Default::default(),
            [false, true, false, false],
        )
    }

    #[test]
    fn select_suited_safety_fallback_action_prefers_black_five() {
        // 5m が NoChance。合法 Dahai [赤5m, 黒5m] なら黒5m を選ぶ。安全度 rank は不変。
        let context = five_man_no_chance_visible_context();
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            Some(&LegalAction::Dahai { tile: tile(17) })
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(17).tile_type(), &context),
            Some(SuitedSafetyRank::NoChance)
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_prefers_black_five_when_reversed() {
        // 合法 Dahai の順序が [黒5m, 赤5m] でも黒5m を選ぶ。
        let context = five_man_no_chance_visible_context();
        let actions = vec![
            LegalAction::Dahai { tile: tile(17) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            Some(&LegalAction::Dahai { tile: tile(17) })
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_keeps_red_five_when_only_red() {
        // 赤5m しか合法でなければ赤5m を維持する。
        let context = five_man_no_chance_visible_context();
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            Some(&LegalAction::Dahai { tile: tile(16) })
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_keeps_leading_tile_type_over_black_five() {
        // 1p も NoChance にして、合法順 [1p, 赤5m, 黒5m] で 1p と 5m が同じ rank。
        // 先頭牌種 1p を維持し、黒5優先で 5m を前へ出さない。
        // 1p の唯一の経路 [2p,3p] を 2p(40-43)4枚で Blocked にする。
        let mut visible = five_man_no_chance_visible();
        visible.extend([tile(40), tile(41), tile(42), tile(43)]);
        let context = suited_context(visible, Default::default(), [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(36) },
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(36).tile_type(), &context),
            Some(SuitedSafetyRank::NoChance)
        );
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            Some(&LegalAction::Dahai { tile: tile(36) })
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
    fn select_defense_fallback_action_matches_with_kind_on_black_five() {
        // 薄い wrapper が with_kind と同じ黒5の action を返す。現物 5m系 [赤5m, 黒5m]。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        let with_kind =
            select_defense_fallback_action_with_kind(&context, &actions).map(|(action, _)| action);
        assert_eq!(with_kind, Some(&LegalAction::Dahai { tile: tile(17) }));
        assert_eq!(
            select_defense_fallback_action(&context, &actions),
            with_kind
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

    // 片スジ回帰局面。リーチ者(1)の河は 1p(36) と 4s(84)。
    // 手牌 444p147m258p123s7s + ツモ 9m を visible にしても壁は両方 NoWall なので、
    // 4p(1p だけスジ)と 7s(4s でスジ)の差はスジ分類だけで決まる。
    fn half_suji_regression_context() -> GameContext {
        let hand = vec![
            tile(48),
            tile(49),
            tile(50),
            tile(0),
            tile(12),
            tile(24),
            tile(40),
            tile(52),
            tile(64),
            tile(72),
            tile(76),
            tile(80),
            tile(96),
            tile(32),
        ];
        suited_context(
            hand,
            [vec![], vec![tile(36), tile(84)], vec![], vec![]],
            [false, true, false, false],
        )
    }

    #[test]
    fn suited_safety_rank_reflects_half_suji() {
        // 4p は片スジで HalfSuji、7s は完全スジで Suji。壁はどちらも NoWall。
        let context = half_suji_regression_context();
        let four_pin = tile(48).tile_type();
        let seven_sou = tile(96).tile_type();

        assert_eq!(wall_rank(four_pin, &context), WallRank::NoWall);
        assert_eq!(wall_rank(seven_sou, &context), WallRank::NoWall);
        assert_eq!(
            suji_safety_rank_for_all_reached(four_pin, &context),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            suji_safety_rank_for_all_reached(seven_sou, &context),
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(four_pin, &context),
            Some(SuitedSafetyRank::HalfSuji)
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(seven_sou, &context),
            Some(SuitedSafetyRank::Suji)
        );
    }

    #[test]
    fn suited_safety_rank_orders_half_suji_between_suji_and_no_safety() {
        assert!(SuitedSafetyRank::Suji > SuitedSafetyRank::HalfSuji);
        assert!(SuitedSafetyRank::HalfSuji > SuitedSafetyRank::NoSafety);
        assert!(SuitedSafetyRank::OneChance > SuitedSafetyRank::Suji);
        assert!(SujiSafetyRank::Suji > SujiSafetyRank::HalfSuji);
        assert!(SujiSafetyRank::HalfSuji > SujiSafetyRank::NoSuji);
    }

    #[test]
    fn suited_safety_rank_keeps_wall_priority_over_half_suji() {
        // 片スジの 4p でも、経路 [2p,3p] と [5p,6p] を塞げば壁評価が優先される。
        let visible = vec![
            tile(40),
            tile(41),
            tile(42),
            tile(43),
            tile(52),
            tile(53),
            tile(54),
            tile(55),
        ];
        let context = suited_context(
            visible,
            [vec![], vec![tile(36)], vec![], vec![]],
            [false, true, false, false],
        );
        assert_eq!(
            suji_safety_rank_for_all_reached(tile(48).tile_type(), &context),
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(48).tile_type(), &context),
            Some(SuitedSafetyRank::NoChance)
        );
    }

    #[test]
    fn suited_dahai_actions_by_safety_orders_full_suji_over_half_suji() {
        let context = half_suji_regression_context();
        let actions = vec![
            LegalAction::Dahai { tile: tile(48) },
            LegalAction::Dahai { tile: tile(96) },
        ];
        let ranked = suited_dahai_actions_by_safety(&actions, &context);
        assert_eq!(
            ranked,
            vec![
                (
                    &LegalAction::Dahai { tile: tile(96) },
                    SuitedSafetyRank::Suji
                ),
                (
                    &LegalAction::Dahai { tile: tile(48) },
                    SuitedSafetyRank::HalfSuji
                ),
            ]
        );
    }

    #[test]
    fn half_suji_regression_prefers_full_suji_regardless_of_action_order() {
        // 合法 action の順序に関係なく、完全スジの 7s を片スジの 4p より優先する。
        let context = half_suji_regression_context();
        let expected = Some((
            &LegalAction::Dahai { tile: tile(96) },
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji),
        ));

        let actions = vec![
            LegalAction::Dahai { tile: tile(48) },
            LegalAction::Dahai { tile: tile(96) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            expected
        );

        let actions = vec![
            LegalAction::Dahai { tile: tile(96) },
            LegalAction::Dahai { tile: tile(48) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            expected
        );
    }

    #[test]
    fn select_suited_safety_fallback_action_takes_half_suji_over_no_safety() {
        // 片スジは無スジより安全なので、他に候補が無ければ片スジを選ぶ。
        let context = half_suji_regression_context();
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(48) },
        ];
        assert_eq!(
            suited_safety_rank_for_all_reached(tile(0).tile_type(), &context),
            Some(SuitedSafetyRank::NoSafety)
        );
        assert_eq!(
            select_suited_safety_fallback_action(&actions, &context),
            Some(&LegalAction::Dahai { tile: tile(48) })
        );
    }

    #[test]
    fn defense_candidate_diagnostic_reports_half_suji() {
        // 壁なしの片スジ。bool は false でも、純粋なスジ rank から HalfSuji と分かる。
        let context = half_suji_regression_context();
        let action = LegalAction::Dahai { tile: tile(48) };
        let candidate =
            DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();

        assert_eq!(candidate.wall_rank, Some(WallRank::NoWall));
        assert_eq!(candidate.suji_for_all_reached, Some(false));
        assert_eq!(
            candidate.suji_safety_rank_for_all_reached,
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            candidate.suited_safety_rank,
            Some(SuitedSafetyRank::HalfSuji)
        );
    }

    #[test]
    fn defense_candidate_diagnostic_reports_half_suji_behind_one_chance_wall() {
        // 4p は 1p だけ河にある片スジ。経路 [2p,3p] は 2p 4枚で Blocked、[5p,6p] は 5p 3枚で
        // OneChance。suited_safety_rank は壁由来の OneChance になるが、純粋なスジ rank は HalfSuji。
        let visible = vec![
            tile(40),
            tile(41),
            tile(42),
            tile(43),
            tile(52),
            tile(53),
            tile(54),
        ];
        let context = suited_context(
            visible,
            [vec![], vec![tile(36)], vec![], vec![]],
            [false, true, false, false],
        );
        let action = LegalAction::Dahai { tile: tile(48) };
        let candidate =
            DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();

        assert_eq!(candidate.wall_rank, Some(WallRank::OneChance));
        assert_eq!(candidate.suji_for_all_reached, Some(false));
        assert_eq!(
            candidate.suji_safety_rank_for_all_reached,
            Some(SujiSafetyRank::HalfSuji)
        );
        assert_eq!(
            candidate.suited_safety_rank,
            Some(SuitedSafetyRank::OneChance)
        );
    }

    #[test]
    fn defense_candidate_diagnostic_reports_no_suji_and_full_suji() {
        // 無スジの 1m は NoSuji、完全スジの 7s は Suji。bool と rank の対応も確認する。
        let context = half_suji_regression_context();

        let action = LegalAction::Dahai { tile: tile(0) };
        let candidate =
            DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();
        assert_eq!(candidate.suji_for_all_reached, Some(false));
        assert_eq!(
            candidate.suji_safety_rank_for_all_reached,
            Some(SujiSafetyRank::NoSuji)
        );

        let action = LegalAction::Dahai { tile: tile(96) };
        let candidate =
            DefenseCandidateDiagnostic::for_dahai_action(&context, &action, true).unwrap();
        assert_eq!(candidate.suji_for_all_reached, Some(true));
        assert_eq!(
            candidate.suji_safety_rank_for_all_reached,
            Some(SujiSafetyRank::Suji)
        );
        assert_eq!(candidate.suited_safety_rank, Some(SuitedSafetyRank::Suji));
    }

    #[test]
    fn defense_fallback_diagnostic_reports_pure_suji_safety_rank() {
        // 選択牌側でも同じ rank を保持する。7s は完全スジ、4p は片スジ。
        let context = half_suji_regression_context();
        let actions = vec![
            LegalAction::Dahai { tile: tile(48) },
            LegalAction::Dahai { tile: tile(96) },
        ];
        let (action, kind) = select_defense_fallback_action_with_kind(&context, &actions).unwrap();
        let diagnostic = DefenseFallbackDiagnostic::from_selection(&context, action, kind);

        assert_eq!(diagnostic.selected_action, "7s");
        assert_eq!(diagnostic.selected_suji_for_all_reached, Some(true));
        assert_eq!(
            diagnostic.selected_suji_safety_rank_for_all_reached,
            Some(SujiSafetyRank::Suji)
        );

        let half_suji = LegalAction::Dahai { tile: tile(48) };
        let diagnostic = DefenseFallbackDiagnostic::from_selection(
            &context,
            &half_suji,
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::HalfSuji),
        );
        assert_eq!(diagnostic.selected_suji_for_all_reached, Some(false));
        assert_eq!(
            diagnostic.selected_suji_safety_rank_for_all_reached,
            Some(SujiSafetyRank::HalfSuji)
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
            diagnostic.selected_suji_safety_rank_for_all_reached,
            Some(SujiSafetyRank::Suji)
        );
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
        assert_eq!(diagnostic.selected_suji_safety_rank_for_all_reached, None);
        assert_eq!(diagnostic.selected_suited_safety_rank, None);
    }

    // ---- 防御 fallback の黒5優先(物理牌正規化)テスト ----

    #[test]
    fn defense_fallback_genbutsu_prefers_black_five() {
        // リーチ者の河に 5m があり 5m 系は現物。合法 Dahai が [赤5m, 黒5m] でも黒5m を選ぶ。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(17) },
                DefenseFallbackKind::Genbutsu
            ))
        );
    }

    #[test]
    fn defense_fallback_genbutsu_prefers_black_five_when_red_first_reversed() {
        // 合法 action の順序を逆(黒5m→赤5m)にしても黒5m を選ぶ。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(17) },
            LegalAction::Dahai { tile: tile(16) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions).map(|(a, _)| a),
            Some(&LegalAction::Dahai { tile: tile(17) })
        );
    }

    #[test]
    fn defense_fallback_genbutsu_keeps_red_five_when_only_red_legal() {
        // 赤5m しか合法でなければ赤5m を選ぶ。
        let discards = [vec![], vec![tile(17)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(16) },
                DefenseFallbackKind::Genbutsu
            ))
        );
    }

    #[test]
    fn defense_fallback_suited_safety_prefers_black_five() {
        // 5m を NoChance にする。4m(12-15)を4枚見せて経路 [4m,5m]... ではなく 5m を対象に
        // 経路 [3m,4m] と [6m,7m] を塞ぐ必要がある。ここでは 4m と 6m を各4枚見せる。
        // すると 5m の経路 [3m,4m]=Blocked, [6m,7m]=Blocked で NoChance。
        let visible = vec![
            tile(12),
            tile(13),
            tile(14),
            tile(15),
            tile(20),
            tile(21),
            tile(22),
            tile(23),
        ];
        let context = suited_context(visible, Default::default(), [false, true, false, false]);
        // 5m の合法 Dahai が [赤5m, 黒5m]。NoChance の 5m を選び、物理牌は黒5m。
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(17) },
                DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoChance)
            ))
        );
    }

    #[test]
    fn defense_fallback_suited_safety_keeps_red_when_only_red_legal() {
        // 同じ NoChance 5m だが合法 Dahai が赤5m だけなら赤5m を選ぶ。安全度は変わらない。
        let visible = vec![
            tile(12),
            tile(13),
            tile(14),
            tile(15),
            tile(20),
            tile(21),
            tile(22),
            tile(23),
        ];
        let context = suited_context(visible, Default::default(), [false, true, false, false]);
        let actions = vec![LegalAction::Dahai { tile: tile(16) }];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(16) },
                DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoChance)
            ))
        );
    }

    #[test]
    fn defense_fallback_does_not_change_tile_type_for_black_five() {
        // 同一安全度で [赤5m, 1p, 5m] の順。先頭牌種は 5m。黒5優先で 1p へは変えず黒5m を選ぶ。
        // リーチ者はいるが河・visible が空なので 5m も 1p も NoSafety で同一安全度。
        // NoSafety は防御 fallback の対象外なので、この局面では防御 fallback は None になる。
        // 牌種順維持の確認は現物経路で行う(下記テスト)。
        let discards = [vec![], vec![tile(17), tile(36)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        // 5m 系と 1p が現物。合法順 [赤5m, 1p, 黒5m] で先頭現物牌種は 5m。黒5m を選ぶ。
        let actions = vec![
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(36) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(17) },
                DefenseFallbackKind::Genbutsu
            ))
        );
    }

    #[test]
    fn defense_fallback_keeps_leading_tile_type_over_black_five() {
        // 合法順 [1p, 赤5m, 黒5m] で 1p と 5m 系がともに現物。先頭現物牌種 1p を維持する。
        // 黒5優先のために 5m を 1p より前へ移動しない。
        let discards = [vec![], vec![tile(17), tile(36)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(36) },
            LegalAction::Dahai { tile: tile(16) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        assert_eq!(
            select_defense_fallback_action_with_kind(&context, &actions),
            Some((
                &LegalAction::Dahai { tile: tile(36) },
                DefenseFallbackKind::Genbutsu
            ))
        );
    }

    // ---- 合法 Dahai ごとの防御候補診断 ----

    #[test]
    fn defense_candidate_diagnostic_for_suited_tile() {
        // 2m は 4m 4枚見えで NoChance。スジではないので suji は false。
        let context = suited_context(
            vec![tile(12), tile(13), tile(14), tile(15)],
            Default::default(),
            [false, true, false, false],
        );
        let action = LegalAction::Dahai { tile: tile(4) };
        let candidate =
            DefenseCandidateDiagnostic::for_dahai_action(&context, &action, true).unwrap();

        assert_eq!(candidate.action, action);
        assert_eq!(candidate.tile, tile(4).tile_type());
        assert!(candidate.selected);
        assert!(!candidate.genbutsu_for_all);
        assert_eq!(candidate.honor_safety_rank, None);
        assert_eq!(candidate.wall_rank, Some(WallRank::NoChance));
        assert_eq!(candidate.suji_for_all_reached, Some(false));
        assert_eq!(
            candidate.suited_safety_rank,
            Some(SuitedSafetyRank::NoChance)
        );
    }

    #[test]
    fn defense_candidate_diagnostic_for_honor_tile() {
        // 東が2枚見え。字牌なので壁・スジ・数牌 safety は None。
        let context = suited_context(
            vec![tile(108), tile(109)],
            Default::default(),
            [false, true, false, false],
        );
        let action = LegalAction::Dahai { tile: tile(108) };
        let candidate =
            DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();

        assert_eq!(candidate.tile, tile(108).tile_type());
        assert!(!candidate.selected);
        assert_eq!(
            candidate.honor_safety_rank,
            Some(HonorSafetyRank::TwoVisible)
        );
        assert_eq!(candidate.wall_rank, None);
        assert_eq!(candidate.suji_for_all_reached, None);
        assert_eq!(candidate.suji_safety_rank_for_all_reached, None);
        assert_eq!(candidate.suited_safety_rank, None);
    }

    #[test]
    fn defense_candidate_diagnostic_reports_opponent_honor_value() {
        // 東場・oya = player1・player1 リーチ(自風は東)。東はダブ東、北は客風、数牌は対象外。
        let context = single_reacher_honor_context(1);
        let candidates: Vec<Option<OpponentHonorValue>> = [tile(108), tile(120), tile(0)]
            .into_iter()
            .map(|tile| {
                let action = LegalAction::Dahai { tile };
                DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false)
                    .unwrap()
                    .opponent_honor_value
            })
            .collect();

        assert_eq!(
            candidates,
            vec![
                Some(OpponentHonorValue::DoubleWind),
                Some(OpponentHonorValue::GuestWind),
                None,
            ]
        );
    }

    #[test]
    fn defense_candidate_diagnostic_opponent_honor_value_excludes_genbutsu_player() {
        // 診断側でも現物のリーチ者を集約から除外した値を持つ。判定は同じ helper を使う。
        let discards = [vec![], vec![], vec![tile(116)], vec![]];
        let context = honor_value_context(
            Some(honor(EAST)),
            Some(0),
            [false, true, true, false],
            discards,
            vec![],
        );
        let action = LegalAction::Dahai { tile: tile(117) };
        let candidate =
            DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();

        assert_eq!(
            candidate.opponent_honor_value,
            Some(OpponentHonorValue::GuestWind)
        );
    }

    #[test]
    fn defense_fallback_diagnostic_reports_selected_opponent_honor_value() {
        // 東場・oya = player1・player1 リーチ(自風は東)。選んだ北は客風。
        let context = single_reacher_honor_context(1);
        let action = LegalAction::Dahai { tile: tile(120) };
        let diagnostic = DefenseFallbackDiagnostic::from_selection(
            &context,
            &action,
            DefenseFallbackKind::HonorSafety(HonorSafetyRank::NoVisible),
        );

        assert_eq!(diagnostic.selected_action, "N");
        assert_eq!(
            diagnostic.selected_honor_safety_rank,
            Some(HonorSafetyRank::NoVisible)
        );
        assert_eq!(
            diagnostic.selected_opponent_honor_value,
            Some(OpponentHonorValue::GuestWind)
        );
    }

    #[test]
    fn defense_fallback_diagnostic_has_no_opponent_honor_value_for_suited_tile() {
        let context = single_reacher_honor_context(1);
        let action = LegalAction::Dahai { tile: tile(0) };
        let diagnostic = DefenseFallbackDiagnostic::from_selection(
            &context,
            &action,
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::NoSafety),
        );

        assert_eq!(diagnostic.selected_opponent_honor_value, None);
    }

    #[test]
    fn defense_candidate_diagnostic_marks_genbutsu() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let action = LegalAction::Dahai { tile: tile(17) };
        let candidate =
            DefenseCandidateDiagnostic::for_dahai_action(&context, &action, false).unwrap();

        assert!(candidate.genbutsu_for_all);
    }

    #[test]
    fn defense_candidate_diagnostic_skips_non_dahai_actions() {
        let context = suited_context(vec![], Default::default(), [false, true, false, false]);
        assert_eq!(
            DefenseCandidateDiagnostic::for_dahai_action(&context, &LegalAction::None, false),
            None
        );
        assert_eq!(
            DefenseCandidateDiagnostic::for_dahai_action(
                &context,
                &LegalAction::Pon {
                    tile: tile(108),
                    consumed: vec![tile(109), tile(110)],
                },
                false
            ),
            None
        );
    }

    #[test]
    fn defense_candidates_keep_legal_action_order_and_mark_selected() {
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(108) },
            LegalAction::None,
            LegalAction::Dahai { tile: tile(17) },
        ];
        let selected = LegalAction::Dahai { tile: tile(17) };

        let candidates =
            DefenseCandidateDiagnostic::for_legal_actions(&context, &actions, Some(&selected));

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].tile, tile(108).tile_type());
        assert!(!candidates[0].selected);
        assert_eq!(candidates[1].tile, tile(17).tile_type());
        assert!(candidates[1].selected);
    }

    #[test]
    fn defense_decision_diagnostic_holds_actual_selection() {
        // 現物 5m が選ばれる局面。実際の選択結果をそのまま保持し、候補評価も全合法 Dahai 分持つ。
        let discards = [vec![], vec![tile(16)], vec![], vec![]];
        let context = table_state_context(Some(0), None, discards, [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(108) },
            LegalAction::Dahai { tile: tile(17) },
        ];
        let selected = select_defense_fallback_action_with_kind(&context, &actions);

        let diagnostic = DefenseDecisionDiagnostic::from_selection(&context, &actions, selected);

        assert_eq!(
            diagnostic.selected_kind(),
            Some(DefenseFallbackKind::Genbutsu)
        );
        assert_eq!(
            diagnostic.selected.as_ref().unwrap().selected_action,
            "5m".to_string()
        );
        assert_eq!(diagnostic.candidates.len(), 2);
        assert_eq!(
            diagnostic
                .candidates
                .iter()
                .filter(|candidate| candidate.selected)
                .count(),
            1
        );
    }

    #[test]
    fn defense_decision_diagnostic_keeps_candidates_without_selection() {
        // 防御 fallback 候補が無い(全て NoSafety)局面でも、候補評価は保持する。
        let context = suited_context(vec![], Default::default(), [false, true, false, false]);
        let actions = vec![
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Dahai { tile: tile(56) },
        ];
        let selected = select_defense_fallback_action_with_kind(&context, &actions);
        assert_eq!(selected, None);

        let diagnostic = DefenseDecisionDiagnostic::from_selection(&context, &actions, selected);

        assert_eq!(diagnostic.selected, None);
        assert_eq!(diagnostic.selected_kind(), None);
        assert_eq!(diagnostic.candidates.len(), 2);
        assert!(
            diagnostic
                .candidates
                .iter()
                .all(|candidate| !candidate.selected)
        );
        assert!(
            diagnostic
                .candidates
                .iter()
                .all(|candidate| candidate.suited_safety_rank == Some(SuitedSafetyRank::NoSafety))
        );
    }
}
