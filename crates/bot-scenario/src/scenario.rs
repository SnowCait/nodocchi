use bot_core::{
    GameContext, LegalAction, Meld, MeldKind, ReachLegalityFacts, TableStateFacts, is_reach_legal,
    seat_wind_for_player,
};
use bot_logic::{
    FixedMeldCount, HistoryFuritenFacts, TileCounts, TileId, TileType,
    calculate_shanten_with_fixed_melds, is_menzen,
};
use serde::Deserialize;

use crate::error::ScenarioError;
use crate::input::{LogicalTile, parse_tiles};
use crate::tiles::{TileAllocator, validate_unique_physical_tiles};

const CHI_TILE_COUNT: usize = 3;
const PON_TILE_COUNT: usize = 3;
const KAN_TILE_COUNT: usize = 4;
const PON_CONSUMED_TILE_COUNT: usize = 2;

// リーチを生成する打牌後の向聴数。
const REACH_TENPAI_SHANTEN: i8 = 0;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSpec {
    pub hand: String,
    #[serde(default)]
    pub draw: Option<String>,
    #[serde(default)]
    pub dora_indicators: Option<String>,
    #[serde(default)]
    pub round_wind: Option<String>,
    #[serde(default)]
    pub seat_wind: Option<String>,
    #[serde(default)]
    pub player_id: Option<u8>,
    #[serde(default)]
    pub oya: Option<u8>,
    #[serde(default)]
    pub reached: Option<Vec<bool>>,
    #[serde(default)]
    pub discards: Option<Vec<String>>,
    #[serde(default)]
    pub post_reach_passed: Option<Vec<String>>,
    #[serde(default)]
    pub temporary_passed: Option<Vec<String>>,
    #[serde(default)]
    pub melds: Option<Vec<Vec<MeldSpec>>>,
    #[serde(default)]
    pub extra_visible_tiles: Option<String>,
    #[serde(default)]
    pub remaining_tiles: Option<u32>,
    #[serde(default)]
    pub honba: Option<u32>,
    #[serde(default)]
    pub kyotaku_points: Option<u32>,
    #[serde(default)]
    pub scores: Option<Vec<i32>>,
    #[serde(default)]
    pub kyoku: Option<u8>,
    #[serde(default)]
    pub history_furiten: Option<HistoryFuritenSpec>,
    #[serde(default)]
    pub legal_dahai: Option<String>,
    #[serde(default)]
    pub legal_pon: Option<Vec<PonActionSpec>>,
    #[serde(default)]
    pub allow_hora: bool,
    #[serde(default)]
    pub allow_ryukyoku: bool,
    #[serde(default)]
    pub allow_none: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryFuritenSpec {
    #[serde(default)]
    pub same_turn: Option<bool>,
    #[serde(default)]
    pub riichi_missed_win: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PonActionSpec {
    pub from_player: u8,
    pub tile: String,
    pub consumed: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeldKindSpec {
    Chi,
    Pon,
    Daiminkan,
    Ankan,
    Kakan,
}

impl MeldKindSpec {
    fn kind(self) -> MeldKind {
        match self {
            Self::Chi => MeldKind::Chi,
            Self::Pon => MeldKind::Pon,
            Self::Daiminkan => MeldKind::Daiminkan,
            Self::Ankan => MeldKind::Ankan,
            Self::Kakan => MeldKind::Kakan,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Chi => "chi",
            Self::Pon => "pon",
            Self::Daiminkan => "daiminkan",
            Self::Ankan => "ankan",
            Self::Kakan => "kakan",
        }
    }

    fn tile_count(self) -> usize {
        match self {
            Self::Chi => CHI_TILE_COUNT,
            Self::Pon => PON_TILE_COUNT,
            Self::Daiminkan | Self::Ankan | Self::Kakan => KAN_TILE_COUNT,
        }
    }

    fn needs_called_tile(self) -> bool {
        !matches!(self, Self::Ankan)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeldSpec {
    pub kind: MeldKindSpec,
    pub tiles: String,
    #[serde(default)]
    pub called_tile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    pub context: GameContext,
    pub legal_actions: Vec<LegalAction>,
}

impl Scenario {
    pub fn resolve(spec: &ScenarioSpec) -> Result<Self, ScenarioError> {
        let reached = resolve_reached(spec.reached.as_deref())?;
        let discard_inputs = resolve_discard_inputs(spec.discards.as_deref())?;
        let post_reach_passed_tiles =
            resolve_post_reach_passed_tiles(spec.post_reach_passed.as_deref())?;
        let temporary_passed_tiles =
            resolve_temporary_passed_tiles(spec.temporary_passed.as_deref())?;
        let meld_inputs = resolve_meld_inputs(spec.melds.as_deref())?;
        let player_id = resolve_seat("player_id", spec.player_id)?;
        let oya = resolve_seat("oya", spec.oya)?;
        let round_wind = parse_wind("round_wind", spec.round_wind.as_deref())?;
        let seat_wind = resolve_seat_wind(spec.seat_wind.as_deref(), player_id, oya)?;
        let table_state = resolve_table_state_facts(spec)?;
        let reaction_source_player = spec.legal_pon.as_deref().and_then(|calls| {
            let source = calls.first()?.from_player;
            calls
                .iter()
                .all(|call| call.from_player == source)
                .then_some(source)
        });

        let mut allocator = TileAllocator::new();
        let hand = allocate_field(&mut allocator, "hand", &spec.hand)?;
        let draw = allocate_draw(&mut allocator, spec.draw.as_deref())?;
        let dora_indicators = allocate_field(
            &mut allocator,
            "dora_indicators",
            spec.dora_indicators.as_deref().unwrap_or_default(),
        )?;
        let discards = allocate_discards(&mut allocator, &discard_inputs)?;
        let melds = allocate_melds(&mut allocator, &meld_inputs, &discards)?;
        let extra_visible_tiles = allocate_field(
            &mut allocator,
            "extra_visible_tiles",
            spec.extra_visible_tiles.as_deref().unwrap_or_default(),
        )?;

        let visible_tiles = collect_visible_tiles(
            &hand,
            draw,
            &dora_indicators,
            &discards,
            &melds,
            &extra_visible_tiles,
        );
        validate_unique_physical_tiles(&visible_tiles)
            .map_err(|source| ScenarioError::PhysicalTiles { source })?;

        let context = GameContext::from_parts_with_melds(
            draw,
            hand,
            dora_indicators,
            round_wind,
            seat_wind,
            visible_tiles,
            player_id,
            oya,
            discards,
            reached,
            melds,
        )
        .with_reaction_source_player(reaction_source_player)
        .with_post_reach_passed_tiles(post_reach_passed_tiles)
        .with_temporary_passed_tiles(temporary_passed_tiles)
        .with_table_state_facts(table_state)
        .with_history_furiten_facts(resolve_history_furiten_facts(spec));

        let legal_actions = build_legal_actions(spec, &context)?;

        Ok(Self {
            context,
            legal_actions,
        })
    }
}

fn resolve_history_furiten_facts(spec: &ScenarioSpec) -> HistoryFuritenFacts {
    spec.history_furiten
        .map_or_else(HistoryFuritenFacts::default, |facts| HistoryFuritenFacts {
            same_turn: facts.same_turn,
            riichi_missed_win: facts.riichi_missed_win,
        })
}

// 省略した field は unknown のままにし、0 や 25000 点などの初期値で補完しない。
fn resolve_table_state_facts(spec: &ScenarioSpec) -> Result<TableStateFacts, ScenarioError> {
    Ok(TableStateFacts {
        remaining_tiles: spec.remaining_tiles,
        honba: spec.honba,
        kyotaku_points: spec.kyotaku_points,
        scores: resolve_scores(spec.scores.as_deref())?,
        kyoku: resolve_kyoku(spec.kyoku)?,
    })
}

fn resolve_scores(scores: Option<&[i32]>) -> Result<Option<[i32; 4]>, ScenarioError> {
    let Some(values) = scores else {
        return Ok(None);
    };
    values
        .try_into()
        .map(Some)
        .map_err(|_| ScenarioError::ScoresLength {
            count: values.len(),
        })
}

fn resolve_kyoku(kyoku: Option<u8>) -> Result<Option<u8>, ScenarioError> {
    let Some(value) = kyoku else {
        return Ok(None);
    };
    if !(1..=4).contains(&value) {
        return Err(ScenarioError::KyokuOutOfRange { value });
    }
    Ok(Some(value))
}

fn resolve_reached(reached: Option<&[bool]>) -> Result<[bool; 4], ScenarioError> {
    let Some(values) = reached else {
        return Ok([false; 4]);
    };
    values.try_into().map_err(|_| ScenarioError::ReachedLength {
        count: values.len(),
    })
}

fn resolve_discard_inputs(discards: Option<&[String]>) -> Result<[String; 4], ScenarioError> {
    let Some(values) = discards else {
        return Ok(std::array::from_fn(|_| String::new()));
    };
    if values.len() != 4 {
        return Err(ScenarioError::DiscardsLength {
            count: values.len(),
        });
    }
    Ok(std::array::from_fn(|index| {
        values.get(index).cloned().unwrap_or_default()
    }))
}

fn resolve_post_reach_passed_tiles(
    post_reach_passed: Option<&[String]>,
) -> Result<[Vec<TileType>; 4], ScenarioError> {
    let Some(values) = post_reach_passed else {
        return Ok(std::array::from_fn(|_| Vec::new()));
    };
    if values.len() != 4 {
        return Err(ScenarioError::PostReachPassedLength {
            count: values.len(),
        });
    }

    let mut tiles: [Vec<TileType>; 4] = std::array::from_fn(|_| Vec::new());
    for (player, slot) in tiles.iter_mut().enumerate() {
        let input = values.get(player).map(String::as_str).unwrap_or_default();
        *slot = parse_field(&format!("post_reach_passed[{player}]"), input)?
            .into_iter()
            .map(|tile| tile.tile_type)
            .collect();
    }
    Ok(tiles)
}

fn resolve_temporary_passed_tiles(
    temporary_passed: Option<&[String]>,
) -> Result<Option<[Vec<TileType>; 4]>, ScenarioError> {
    let Some(values) = temporary_passed else {
        return Ok(None);
    };
    if values.len() != 4 {
        return Err(ScenarioError::TemporaryPassedLength {
            count: values.len(),
        });
    }

    let mut tiles: [Vec<TileType>; 4] = std::array::from_fn(|_| Vec::new());
    for (player, slot) in tiles.iter_mut().enumerate() {
        let input = values.get(player).map(String::as_str).unwrap_or_default();
        *slot = parse_field(&format!("temporary_passed[{player}]"), input)?
            .into_iter()
            .map(|tile| tile.tile_type)
            .collect();
    }
    Ok(Some(tiles))
}

fn resolve_seat(field: &str, value: Option<u8>) -> Result<Option<u8>, ScenarioError> {
    value.map(|value| validate_seat(field, value)).transpose()
}

fn validate_seat(field: &str, value: u8) -> Result<u8, ScenarioError> {
    if value > 3 {
        return Err(ScenarioError::SeatOutOfRange {
            field: field.to_string(),
            value,
        });
    }
    Ok(value)
}

fn resolve_seat_wind(
    seat_wind: Option<&str>,
    player_id: Option<u8>,
    oya: Option<u8>,
) -> Result<Option<TileType>, ScenarioError> {
    let explicit = parse_wind("seat_wind", seat_wind)?;
    let derived = derive_seat_wind(player_id, oya);

    if let (Some(explicit), Some(derived), Some(player_id), Some(oya)) =
        (explicit, derived, player_id, oya)
        && explicit != derived
    {
        return Err(ScenarioError::SeatWindConflict {
            explicit: explicit.to_mjai_string(),
            derived: derived.to_mjai_string(),
            player_id,
            oya,
        });
    }

    Ok(explicit.or(derived))
}

// 席順からの自風導出は bot-core の pure helper を再利用し、同じ計算を持たない。
fn derive_seat_wind(player_id: Option<u8>, oya: Option<u8>) -> Option<TileType> {
    seat_wind_for_player(usize::from(player_id?), oya?)
}

fn parse_wind(field: &str, input: Option<&str>) -> Result<Option<TileType>, ScenarioError> {
    let Some(input) = input else {
        return Ok(None);
    };
    let tiles = parse_field(field, input)?;

    let Some(tile) = tiles.first().copied() else {
        return Ok(None);
    };
    if tiles.len() != 1 {
        return Err(ScenarioError::NotSingleTile {
            field: field.to_string(),
            input: input.to_string(),
            count: tiles.len(),
        });
    }
    if !tile.tile_type.is_wind() {
        return Err(ScenarioError::NotWind {
            field: field.to_string(),
            input: input.to_string(),
        });
    }

    Ok(Some(tile.tile_type))
}

fn parse_field(field: &str, input: &str) -> Result<Vec<LogicalTile>, ScenarioError> {
    parse_tiles(input).map_err(|source| ScenarioError::TileInput {
        field: field.to_string(),
        input: input.to_string(),
        source,
    })
}

fn parse_single_tile(field: &str, input: &str) -> Result<LogicalTile, ScenarioError> {
    let tiles = parse_field(field, input)?;
    match tiles.as_slice() {
        [tile] => Ok(*tile),
        tiles => Err(ScenarioError::NotSingleTile {
            field: field.to_string(),
            input: input.to_string(),
            count: tiles.len(),
        }),
    }
}

fn allocate_field(
    allocator: &mut TileAllocator,
    field: &str,
    input: &str,
) -> Result<Vec<TileId>, ScenarioError> {
    let tiles = parse_field(field, input)?;
    allocate_logical(allocator, field, input, &tiles)
}

fn allocate_logical(
    allocator: &mut TileAllocator,
    field: &str,
    input: &str,
    tiles: &[LogicalTile],
) -> Result<Vec<TileId>, ScenarioError> {
    tiles
        .iter()
        .map(|tile| {
            allocator
                .allocate(*tile)
                .map_err(|source| ScenarioError::TileAllocation {
                    field: field.to_string(),
                    input: input.to_string(),
                    source,
                })
        })
        .collect()
}

fn allocate_draw(
    allocator: &mut TileAllocator,
    draw: Option<&str>,
) -> Result<Option<TileId>, ScenarioError> {
    let input = draw.unwrap_or_default();
    let tiles = parse_field("draw", input)?;
    if tiles.len() > 1 {
        return Err(ScenarioError::MultipleDrawTiles {
            input: input.to_string(),
            count: tiles.len(),
        });
    }
    let allocated = allocate_logical(allocator, "draw", input, &tiles)?;
    Ok(allocated.into_iter().next())
}

fn allocate_discards(
    allocator: &mut TileAllocator,
    inputs: &[String; 4],
) -> Result<[Vec<TileId>; 4], ScenarioError> {
    let mut discards: [Vec<TileId>; 4] = std::array::from_fn(|_| Vec::new());
    for (player, (slot, input)) in discards.iter_mut().zip(inputs).enumerate() {
        *slot = allocate_field(allocator, &format!("discards[{player}]"), input)?;
    }
    Ok(discards)
}

fn resolve_meld_inputs(
    melds: Option<&[Vec<MeldSpec>]>,
) -> Result<[Vec<MeldSpec>; 4], ScenarioError> {
    let Some(values) = melds else {
        return Ok(std::array::from_fn(|_| Vec::new()));
    };
    if values.len() != 4 {
        return Err(ScenarioError::MeldsLength {
            count: values.len(),
        });
    }
    Ok(std::array::from_fn(|index| {
        values.get(index).cloned().unwrap_or_default()
    }))
}

fn allocate_melds(
    allocator: &mut TileAllocator,
    inputs: &[Vec<MeldSpec>; 4],
    discards: &[Vec<TileId>; 4],
) -> Result<[Vec<Meld>; 4], ScenarioError> {
    let mut claimed_discards: Vec<TileId> = Vec::new();
    let mut melds: [Vec<Meld>; 4] = std::array::from_fn(|_| Vec::new());
    for (player, (slot, player_inputs)) in melds.iter_mut().zip(inputs).enumerate() {
        for (index, spec) in player_inputs.iter().enumerate() {
            slot.push(allocate_meld(
                allocator,
                &format!("melds[{player}][{index}]"),
                spec,
                discards,
                &mut claimed_discards,
            )?);
        }
    }
    Ok(melds)
}

fn allocate_meld(
    allocator: &mut TileAllocator,
    field: &str,
    spec: &MeldSpec,
    discards: &[Vec<TileId>; 4],
    claimed_discards: &mut Vec<TileId>,
) -> Result<Meld, ScenarioError> {
    let tiles = parse_field(field, &spec.tiles)?;
    validate_meld_shape(field, spec.kind, &tiles)?;

    let called_logical = resolve_meld_called_tile(field, spec, &tiles)?;
    let called_position =
        called_logical.and_then(|called| tiles.iter().position(|tile| *tile == called));
    let called_tile = called_logical
        .map(|called| claim_discarded_tile(field, called, discards, claimed_discards))
        .transpose()?;

    let mut remaining = tiles;
    if let Some(position) = called_position {
        remaining.remove(position);
    }

    let mut meld_tiles = allocate_logical(allocator, field, &spec.tiles, &remaining)?;
    if let (Some(position), Some(called_tile)) = (called_position, called_tile) {
        meld_tiles.insert(position, called_tile);
    }

    Ok(Meld::new(spec.kind.kind(), meld_tiles, called_tile))
}

fn validate_meld_shape(
    field: &str,
    kind: MeldKindSpec,
    tiles: &[LogicalTile],
) -> Result<(), ScenarioError> {
    if tiles.len() != kind.tile_count() {
        return Err(ScenarioError::MeldTileCount {
            field: field.to_string(),
            kind: kind.label().to_string(),
            expected: kind.tile_count(),
            count: tiles.len(),
        });
    }

    let matches_shape = match kind {
        MeldKindSpec::Chi => is_sequence(tiles),
        _ => is_same_tile_type(tiles),
    };
    if !matches_shape {
        return Err(ScenarioError::MeldShape {
            field: field.to_string(),
            kind: kind.label().to_string(),
            input: tiles
                .iter()
                .map(|tile| tile.to_mjai_string())
                .collect::<Vec<_>>()
                .join(" "),
        });
    }

    Ok(())
}

fn is_same_tile_type(tiles: &[LogicalTile]) -> bool {
    tiles
        .windows(2)
        .all(|pair| pair[0].tile_type == pair[1].tile_type)
}

fn is_sequence(tiles: &[LogicalTile]) -> bool {
    let Some(first) = tiles.first() else {
        return false;
    };
    if first.tile_type.is_honor() {
        return false;
    }
    let mut raws: Vec<u8> = tiles.iter().map(|tile| tile.tile_type.raw()).collect();
    raws.sort_unstable();
    tiles
        .iter()
        .all(|tile| tile.tile_type.suit() == first.tile_type.suit())
        && raws.windows(2).all(|pair| pair[1] == pair[0] + 1)
}

fn resolve_meld_called_tile(
    field: &str,
    spec: &MeldSpec,
    tiles: &[LogicalTile],
) -> Result<Option<LogicalTile>, ScenarioError> {
    let called_field = format!("{field}.called_tile");
    let called = spec
        .called_tile
        .as_deref()
        .map(|input| parse_single_tile(&called_field, input))
        .transpose()?;

    match (spec.kind.needs_called_tile(), called) {
        (true, None) => Err(ScenarioError::MeldCalledTileMissing {
            field: field.to_string(),
            kind: spec.kind.label().to_string(),
        }),
        (false, Some(called)) => Err(ScenarioError::MeldCalledTileNotAllowed {
            field: field.to_string(),
            kind: spec.kind.label().to_string(),
            tile: called.to_mjai_string(),
        }),
        (_, Some(called)) if !tiles.contains(&called) => {
            Err(ScenarioError::MeldCalledTileNotInMeld {
                field: field.to_string(),
                tile: called.to_mjai_string(),
            })
        }
        (_, called) => Ok(called),
    }
}

fn claim_discarded_tile(
    field: &str,
    called: LogicalTile,
    discards: &[Vec<TileId>; 4],
    claimed_discards: &mut Vec<TileId>,
) -> Result<TileId, ScenarioError> {
    let tile = discards
        .iter()
        .flatten()
        .copied()
        .find(|tile| {
            tile.tile_type() == called.tile_type
                && tile.is_red() == called.red
                && !claimed_discards.contains(tile)
        })
        .ok_or_else(|| ScenarioError::MeldCalledTileNotDiscarded {
            field: field.to_string(),
            tile: called.to_mjai_string(),
        })?;
    claimed_discards.push(tile);
    Ok(tile)
}

fn collect_visible_tiles(
    hand: &[TileId],
    draw: Option<TileId>,
    dora_indicators: &[TileId],
    discards: &[Vec<TileId>; 4],
    melds: &[Vec<Meld>; 4],
    extra_visible_tiles: &[TileId],
) -> Vec<TileId> {
    hand.iter()
        .copied()
        .chain(draw)
        .chain(dora_indicators.iter().copied())
        .chain(discards.iter().flatten().copied())
        .chain(melds.iter().flatten().flat_map(meld_visible_tiles))
        .chain(extra_visible_tiles.iter().copied())
        .collect()
}

fn meld_visible_tiles(meld: &Meld) -> Vec<TileId> {
    let mut called_tile = meld.called_tile();
    let mut tiles = Vec::new();
    for &tile in meld.tiles() {
        if called_tile == Some(tile) {
            called_tile = None;
            continue;
        }
        tiles.push(tile);
    }
    tiles
}

fn build_legal_actions(
    spec: &ScenarioSpec,
    context: &GameContext,
) -> Result<Vec<LegalAction>, ScenarioError> {
    let hand = context.hand_tiles();
    let draw = context.drawn_tile();

    let mut actions = match spec.legal_dahai.as_deref() {
        Some(input) => explicit_dahai_actions(input, hand, draw)?,
        None => automatic_dahai_actions(hand, draw),
    };

    if let Some(specs) = spec.legal_pon.as_deref() {
        actions.extend(pon_actions(
            specs,
            hand,
            context.discards(),
            context.player_id(),
        )?);
    }

    if is_reach_legal(reach_legality_facts(context, &actions)) {
        actions.push(LegalAction::Reach);
    }
    if spec.allow_hora {
        actions.push(LegalAction::Hora);
    }
    if spec.allow_ryukyoku {
        actions.push(LegalAction::Ryukyoku);
    }
    if spec.allow_none {
        actions.push(LegalAction::None);
    }

    Ok(actions)
}

/// 局面から `LegalAction::Reach` を生成できるかを判定するための材料を集める。
///
/// 条件そのものは production と共有する [`is_reach_legal`] が持ち、ここでは局面から材料を
/// 取り出すだけにする。分からない材料は `None` のまま渡し、リーチ不可と推測しない。
fn reach_legality_facts(context: &GameContext, actions: &[LegalAction]) -> ReachLegalityFacts {
    ReachLegalityFacts {
        menzen: context.own_melds().map(is_menzen),
        already_reached: context
            .player_id()
            .map(|player| context.is_reached(usize::from(player))),
        score: context.own_score(),
        remaining_tiles: context.remaining_tiles(),
        tenpai_after_discard: reaches_tenpai_after_any_legal_dahai(context, actions),
    }
}

// 副露済み面子数を含む既存の向聴計算をそのまま使い、リーチ専用の向聴・受け入れは計算しない。
fn reaches_tenpai_after_any_legal_dahai(context: &GameContext, actions: &[LegalAction]) -> bool {
    let fixed_meld_count = context
        .own_fixed_meld_count()
        .unwrap_or(FixedMeldCount::NONE);
    let counts = TileCounts::from_tiles(
        context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile()),
    );

    actions.iter().any(|action| {
        let LegalAction::Dahai { tile } = action else {
            return false;
        };
        let mut after_discard = counts;
        after_discard.remove(tile.tile_type()).is_ok()
            && calculate_shanten_with_fixed_melds(&after_discard, fixed_meld_count).min()
                == REACH_TENPAI_SHANTEN
    })
}

fn automatic_dahai_actions(hand: &[TileId], draw: Option<TileId>) -> Vec<LegalAction> {
    let mut meanings: Vec<(TileType, bool)> = Vec::new();
    let mut actions = Vec::new();

    for tile in hand.iter().copied().chain(draw) {
        let meaning = (tile.tile_type(), tile.is_red());
        if meanings.contains(&meaning) {
            continue;
        }
        meanings.push(meaning);
        actions.push(LegalAction::Dahai { tile });
    }

    actions
}

fn explicit_dahai_actions(
    input: &str,
    hand: &[TileId],
    draw: Option<TileId>,
) -> Result<Vec<LegalAction>, ScenarioError> {
    let requested = parse_field("legal_dahai", input)?;
    let held: Vec<TileId> = hand.iter().copied().chain(draw).collect();

    let mut meanings: Vec<(TileType, bool)> = Vec::new();
    let mut actions = Vec::new();

    for tile in requested {
        let meaning = (tile.tile_type, tile.red);
        if meanings.contains(&meaning) {
            return Err(ScenarioError::LegalDahaiDuplicate {
                tile: tile.to_mjai_string(),
            });
        }
        meanings.push(meaning);

        let held_tile = held
            .iter()
            .copied()
            .find(|held| held.tile_type() == tile.tile_type && held.is_red() == tile.red);

        let Some(held_tile) = held_tile else {
            let same_type = held
                .iter()
                .copied()
                .find(|held| held.tile_type() == tile.tile_type);
            return Err(match same_type {
                Some(same_type) => ScenarioError::LegalDahaiRedMismatch {
                    tile: tile.to_mjai_string(),
                    held: same_type.to_mjai_string(),
                },
                None => ScenarioError::LegalDahaiNotHeld {
                    tile: tile.to_mjai_string(),
                },
            });
        };

        actions.push(LegalAction::Dahai { tile: held_tile });
    }

    Ok(actions)
}

fn pon_actions(
    specs: &[PonActionSpec],
    hand: &[TileId],
    discards: &[Vec<TileId>; 4],
    player_id: Option<u8>,
) -> Result<Vec<LegalAction>, ScenarioError> {
    specs
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            pon_action(
                &format!("legal_pon[{index}]"),
                spec,
                hand,
                discards,
                player_id,
            )
        })
        .collect()
}

fn pon_action(
    field: &str,
    spec: &PonActionSpec,
    hand: &[TileId],
    discards: &[Vec<TileId>; 4],
    player_id: Option<u8>,
) -> Result<LegalAction, ScenarioError> {
    let Some(player_id) = player_id else {
        return Err(ScenarioError::LegalPonWithoutPlayerId {
            field: field.to_string(),
        });
    };

    let from_player = validate_seat(&format!("{field}.from_player"), spec.from_player)?;
    if from_player == player_id {
        return Err(ScenarioError::LegalPonFromOwnDiscard {
            field: field.to_string(),
            player_id,
        });
    }

    let tile = parse_single_tile(&format!("{field}.tile"), &spec.tile)?;
    let consumed = parse_field(&format!("{field}.consumed"), &spec.consumed)?;
    if consumed.len() != PON_CONSUMED_TILE_COUNT {
        return Err(ScenarioError::LegalPonConsumedCount {
            field: field.to_string(),
            expected: PON_CONSUMED_TILE_COUNT,
            count: consumed.len(),
        });
    }
    if consumed
        .iter()
        .any(|consumed| consumed.tile_type != tile.tile_type)
    {
        return Err(ScenarioError::LegalPonTileType {
            field: field.to_string(),
            tile: tile.to_mjai_string(),
            consumed: spec.consumed.clone(),
        });
    }

    let target = pon_target_tile(field, tile, from_player, discards)?;
    let consumed = pon_consumed_tiles(field, &consumed, hand)?;

    Ok(LegalAction::Pon {
        tile: target,
        consumed,
    })
}

fn pon_target_tile(
    field: &str,
    tile: LogicalTile,
    from_player: u8,
    discards: &[Vec<TileId>; 4],
) -> Result<TileId, ScenarioError> {
    let target = discards[usize::from(from_player)]
        .last()
        .copied()
        .ok_or_else(|| ScenarioError::LegalPonNoDiscard {
            field: field.to_string(),
            from_player,
        })?;

    if target.tile_type() != tile.tile_type || target.is_red() != tile.red {
        return Err(ScenarioError::LegalPonTargetMismatch {
            field: field.to_string(),
            tile: tile.to_mjai_string(),
            discarded: target.to_mjai_string(),
            from_player,
        });
    }

    Ok(target)
}

fn pon_consumed_tiles(
    field: &str,
    consumed: &[LogicalTile],
    hand: &[TileId],
) -> Result<Vec<TileId>, ScenarioError> {
    let mut tiles: Vec<TileId> = Vec::new();

    for tile in consumed {
        let held = hand.iter().copied().find(|held| {
            held.tile_type() == tile.tile_type && held.is_red() == tile.red && !tiles.contains(held)
        });
        let Some(held) = held else {
            return Err(ScenarioError::LegalPonConsumedNotHeld {
                field: field.to_string(),
                tile: tile.to_mjai_string(),
            });
        };
        tiles.push(held);
    }

    Ok(tiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_core::{
        Agent, DefenseCandidateDiagnostic, DefenseFallbackKind, DiagnosticOptions, HonorSafetyRank,
        OpponentHonorValue, ShantenAgent, SuitedSafetyRank, SujiSafetyRank, is_genbutsu_for,
        is_genbutsu_for_all_reached, select_defense_fallback_action_with_kind,
        suji_safety_rank_for,
    };

    fn spec_from_json(json: &str) -> ScenarioSpec {
        serde_json::from_str(json).unwrap()
    }

    fn resolve(spec: &ScenarioSpec) -> Scenario {
        Scenario::resolve(spec).unwrap()
    }

    fn hand_spec(hand: &str, draw: Option<&str>) -> ScenarioSpec {
        ScenarioSpec {
            hand: hand.to_string(),
            draw: draw.map(str::to_string),
            ..ScenarioSpec::default()
        }
    }

    fn labels(tiles: &[TileId]) -> Vec<String> {
        tiles.iter().map(|tile| tile.to_mjai_string()).collect()
    }

    fn dahai_labels(actions: &[LegalAction]) -> Vec<String> {
        actions
            .iter()
            .filter_map(|action| match action {
                LegalAction::Dahai { tile } => Some(tile.to_mjai_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn resolves_hand_and_draw() {
        let scenario = resolve(&hand_spec("234m455p789s1123z", Some("N")));
        assert_eq!(
            labels(scenario.context.hand_tiles()),
            [
                "2m", "3m", "4m", "4p", "5p", "5p", "7s", "8s", "9s", "E", "E", "S", "W"
            ]
        );
        assert_eq!(
            scenario
                .context
                .drawn_tile()
                .map(|tile| tile.to_mjai_string()),
            Some("N".to_string())
        );
    }

    #[test]
    fn json_defaults_are_applied() {
        let spec = spec_from_json(r#"{"hand": "123m456p789s11z"}"#);
        assert_eq!(spec.draw, None);
        assert_eq!(spec.dora_indicators, None);
        assert_eq!(spec.round_wind, None);
        assert_eq!(spec.seat_wind, None);
        assert_eq!(spec.player_id, None);
        assert_eq!(spec.oya, None);
        assert_eq!(spec.reached, None);
        assert_eq!(spec.discards, None);
        assert_eq!(spec.post_reach_passed, None);
        assert_eq!(spec.extra_visible_tiles, None);
        assert_eq!(spec.history_furiten, None);
        assert_eq!(spec.legal_dahai, None);
        assert_eq!(spec.legal_pon, None);
        assert!(!spec.allow_hora);
        assert!(!spec.allow_ryukyoku);
        assert!(!spec.allow_none);

        let scenario = resolve(&spec);
        let context = &scenario.context;
        assert_eq!(context.drawn_tile(), None);
        assert!(context.dora_indicators().is_empty());
        assert_eq!(context.round_wind(), None);
        assert_eq!(context.seat_wind(), None);
        assert_eq!(context.player_id(), None);
        assert_eq!(context.oya(), None);
        assert_eq!(context.history_furiten().same_turn, None);
        assert_eq!(context.history_furiten().riichi_missed_win, None);
        assert_eq!(context.reached(), &[false; 4]);
        assert!(
            context
                .discards()
                .iter()
                .all(|discards| discards.is_empty())
        );
        assert!(
            context
                .post_reach_passed_tiles()
                .iter()
                .all(|passed| passed.is_empty())
        );
        assert!(
            scenario
                .legal_actions
                .iter()
                .all(|action| matches!(action, LegalAction::Dahai { .. }))
        );
    }

    #[test]
    fn json_full_scenario_is_parsed() {
        let spec = spec_from_json(
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "dora_indicators": "3p",
                "round_wind": "E",
                "seat_wind": "N",
                "player_id": 0,
                "oya": 1,
                "reached": [false, true, false, false],
                "discards": ["", "1m 4m 7p E", "", ""],
                "extra_visible_tiles": "444p",
                "legal_dahai": null,
                "allow_hora": false,
                "allow_ryukyoku": false
            }"#,
        );
        let scenario = resolve(&spec);
        let context = &scenario.context;

        assert_eq!(labels(context.dora_indicators()), ["3p"]);
        assert_eq!(
            context.round_wind().map(|wind| wind.to_mjai_string()),
            Some("E".to_string())
        );
        assert_eq!(
            context.seat_wind().map(|wind| wind.to_mjai_string()),
            Some("N".to_string())
        );
        assert_eq!(context.player_id(), Some(0));
        assert_eq!(context.oya(), Some(1));
        assert_eq!(context.reached(), &[false, true, false, false]);
        assert_eq!(context.reached_opponents(), vec![1]);
        assert_eq!(
            labels(context.discards_of(1).unwrap()),
            ["1m", "4m", "7p", "E"]
        );
        assert!(context.discards_of(0).unwrap().is_empty());
    }

    #[test]
    fn json_rejects_unknown_field() {
        let error =
            serde_json::from_str::<ScenarioSpec>(r#"{"hand": "1m", "visible_tiles": "1m"}"#);
        assert!(error.is_err());
    }

    #[test]
    fn json_requires_hand() {
        assert!(serde_json::from_str::<ScenarioSpec>(r#"{"draw": "N"}"#).is_err());
    }

    #[test]
    fn visible_tiles_reuse_allocated_physical_tiles() {
        let spec = spec_from_json(
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "dora_indicators": "3p",
                "discards": ["1m", "4m 7p E", "", "9s"],
                "extra_visible_tiles": "444p"
            }"#,
        );
        let scenario = resolve(&spec);
        let context = &scenario.context;

        let expected: Vec<TileId> = context
            .hand_tiles()
            .iter()
            .copied()
            .chain(context.drawn_tile())
            .chain(context.dora_indicators().iter().copied())
            .chain(context.discards().iter().flatten().copied())
            .collect();

        for tile in &expected {
            assert_eq!(
                context
                    .visible_tiles()
                    .iter()
                    .filter(|visible| *visible == tile)
                    .count(),
                1,
                "{tile:?} must appear exactly once in visible_tiles"
            );
        }

        assert_eq!(context.visible_tiles().len(), expected.len() + 3);
        assert!(validate_unique_physical_tiles(context.visible_tiles()).is_ok());
    }

    #[test]
    fn rejects_fifth_copy_including_draw_and_dora() {
        let spec = spec_from_json(
            r#"{
                "hand": "111m22p33s44m5p",
                "draw": "1m",
                "dora_indicators": "1m"
            }"#,
        );
        let error = Scenario::resolve(&spec).unwrap_err();
        assert!(
            matches!(error, ScenarioError::TileAllocation { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn extra_visible_tiles_are_visible_but_not_in_hand() {
        let spec = spec_from_json(r#"{"hand": "123m", "extra_visible_tiles": "444p"}"#);
        let scenario = resolve(&spec);
        assert_eq!(labels(scenario.context.hand_tiles()), ["1m", "2m", "3m"]);
        assert_eq!(
            labels(scenario.context.visible_tiles()),
            ["1m", "2m", "3m", "4p", "4p", "4p"]
        );
    }

    #[test]
    fn seat_wind_is_derived_from_player_id_and_oya() {
        for (player_id, oya, expected) in [
            (0, 0, "E"),
            (1, 0, "S"),
            (2, 0, "W"),
            (3, 0, "N"),
            (0, 1, "N"),
            (0, 2, "W"),
            (0, 3, "S"),
        ] {
            let spec = ScenarioSpec {
                hand: "123m".to_string(),
                player_id: Some(player_id),
                oya: Some(oya),
                ..ScenarioSpec::default()
            };
            let scenario = resolve(&spec);
            assert_eq!(
                scenario
                    .context
                    .seat_wind()
                    .map(|wind| wind.to_mjai_string()),
                Some(expected.to_string()),
                "player_id {player_id}, oya {oya}"
            );
        }
    }

    #[test]
    fn explicit_seat_wind_matching_derived_is_accepted() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            seat_wind: Some("N".to_string()),
            player_id: Some(0),
            oya: Some(1),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            resolve(&spec)
                .context
                .seat_wind()
                .map(|wind| wind.to_mjai_string()),
            Some("N".to_string())
        );
    }

    #[test]
    fn explicit_seat_wind_conflicting_with_derived_is_rejected() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            seat_wind: Some("S".to_string()),
            player_id: Some(0),
            oya: Some(1),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::SeatWindConflict {
                explicit: "S".to_string(),
                derived: "N".to_string(),
                player_id: 0,
                oya: 1,
            })
        );
    }

    #[test]
    fn explicit_seat_wind_without_seats_is_used() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            seat_wind: Some("W".to_string()),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            resolve(&spec)
                .context
                .seat_wind()
                .map(|wind| wind.to_mjai_string()),
            Some("W".to_string())
        );
    }

    #[test]
    fn rejects_out_of_range_seats() {
        for (field, spec) in [
            (
                "player_id",
                ScenarioSpec {
                    hand: "123m".to_string(),
                    player_id: Some(4),
                    ..ScenarioSpec::default()
                },
            ),
            (
                "oya",
                ScenarioSpec {
                    hand: "123m".to_string(),
                    oya: Some(9),
                    ..ScenarioSpec::default()
                },
            ),
        ] {
            let error = Scenario::resolve(&spec).unwrap_err();
            assert!(
                matches!(&error, ScenarioError::SeatOutOfRange { field: name, .. } if name == field),
                "{error:?}"
            );
        }
    }

    #[test]
    fn rejects_non_wind_round_wind() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            round_wind: Some("P".to_string()),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::NotWind {
                field: "round_wind".to_string(),
                input: "P".to_string(),
            })
        );
    }

    #[test]
    fn rejects_non_wind_seat_wind() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            seat_wind: Some("1p".to_string()),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::NotWind {
                field: "seat_wind".to_string(),
                input: "1p".to_string(),
            })
        );
    }

    #[test]
    fn rejects_multiple_round_wind_tiles() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            round_wind: Some("E S".to_string()),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::NotSingleTile {
                field: "round_wind".to_string(),
                input: "E S".to_string(),
                count: 2,
            })
        );
    }

    #[test]
    fn rejects_multiple_draw_tiles() {
        let spec = hand_spec("123m", Some("1p2p"));
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::MultipleDrawTiles {
                input: "1p2p".to_string(),
                count: 2,
            })
        );
    }

    #[test]
    fn rejects_wrong_reached_length() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            reached: Some(vec![false, true]),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::ReachedLength { count: 2 })
        );
    }

    #[test]
    fn rejects_wrong_post_reach_passed_length() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            post_reach_passed: Some(vec!["4s".to_string()]),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::PostReachPassedLength { count: 1 })
        );
    }

    #[test]
    fn rejects_invalid_post_reach_passed_tile() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            post_reach_passed: Some(vec![
                String::new(),
                "4x".to_string(),
                String::new(),
                String::new(),
            ]),
            ..ScenarioSpec::default()
        };
        let error = Scenario::resolve(&spec).unwrap_err();
        assert!(
            matches!(&error, ScenarioError::TileInput { field, .. } if field == "post_reach_passed[1]"),
            "{error:?}"
        );
    }

    #[test]
    fn table_state_is_unknown_when_omitted() {
        let scenario = resolve(&hand_spec("234m455p789s1123z", Some("N")));
        assert_eq!(scenario.context.table_state(), TableStateFacts::default());
        assert_eq!(scenario.context.remaining_tiles(), None);
        assert_eq!(scenario.context.honba(), None);
        assert_eq!(scenario.context.kyotaku_points(), None);
        assert_eq!(scenario.context.scores(), None);
        assert_eq!(scenario.context.kyoku(), None);
    }

    #[test]
    fn resolves_every_table_state_field() {
        let spec = spec_from_json(
            r#"{
                "hand": "234m455p789s1123z",
                "remaining_tiles": 42,
                "honba": 1,
                "kyotaku_points": 2000,
                "scores": [25000, 24000, 26000, 25000],
                "kyoku": 2
            }"#,
        );
        let scenario = resolve(&spec);

        assert_eq!(scenario.context.remaining_tiles(), Some(42));
        assert_eq!(scenario.context.honba(), Some(1));
        assert_eq!(scenario.context.kyotaku_points(), Some(2000));
        assert_eq!(
            scenario.context.scores(),
            Some([25000, 24000, 26000, 25000])
        );
        assert_eq!(scenario.context.kyoku(), Some(2));
    }

    #[test]
    fn keeps_a_table_state_zero_as_a_known_zero() {
        let spec = spec_from_json(
            r#"{
                "hand": "234m455p789s1123z",
                "remaining_tiles": 0,
                "honba": 0,
                "kyotaku_points": 0,
                "scores": [0, 0, 0, 0]
            }"#,
        );
        let scenario = resolve(&spec);

        assert_eq!(scenario.context.remaining_tiles(), Some(0));
        assert_eq!(scenario.context.honba(), Some(0));
        assert_eq!(scenario.context.kyotaku_points(), Some(0));
        assert_eq!(scenario.context.scores(), Some([0; 4]));
        assert_ne!(scenario.context.table_state(), TableStateFacts::default());
    }

    #[test]
    fn resolves_each_table_state_field_independently() {
        let spec = spec_from_json(r#"{"hand": "123m", "honba": 3}"#);
        let scenario = resolve(&spec);

        assert_eq!(scenario.context.honba(), Some(3));
        assert_eq!(scenario.context.remaining_tiles(), None);
        assert_eq!(scenario.context.kyotaku_points(), None);
        assert_eq!(scenario.context.scores(), None);
        assert_eq!(scenario.context.kyoku(), None);
    }

    #[test]
    fn scores_follow_the_player_index() {
        let spec = spec_from_json(
            r#"{"hand": "123m", "player_id": 2, "scores": [12300, 28700, 40100, 18900]}"#,
        );
        let scenario = resolve(&spec);

        assert_eq!(scenario.context.score_of(0), Some(12300));
        assert_eq!(scenario.context.score_of(3), Some(18900));
        assert_eq!(scenario.context.own_score(), Some(40100));
    }

    #[test]
    fn resolves_negative_scores() {
        let spec = spec_from_json(r#"{"hand": "123m", "scores": [-1500, 51500, 25000, 25000]}"#);
        assert_eq!(
            resolve(&spec).context.scores(),
            Some([-1500, 51500, 25000, 25000])
        );
    }

    #[test]
    fn rejects_wrong_scores_length() {
        for (json, count) in [
            (r#"{"hand": "123m", "scores": [25000, 25000, 25000]}"#, 3),
            (
                r#"{"hand": "123m", "scores": [25000, 25000, 25000, 25000, 25000]}"#,
                5,
            ),
            (r#"{"hand": "123m", "scores": []}"#, 0),
        ] {
            assert_eq!(
                Scenario::resolve(&spec_from_json(json)),
                Err(ScenarioError::ScoresLength { count }),
                "{json}"
            );
        }
    }

    #[test]
    fn rejects_out_of_range_kyoku() {
        for value in [0, 5, u8::MAX] {
            let spec = ScenarioSpec {
                hand: "123m".to_string(),
                kyoku: Some(value),
                ..ScenarioSpec::default()
            };
            assert_eq!(
                Scenario::resolve(&spec),
                Err(ScenarioError::KyokuOutOfRange { value }),
                "kyoku: {value}"
            );
        }
    }

    #[test]
    fn accepts_every_kyoku_in_range() {
        for value in 1..=4 {
            let spec = ScenarioSpec {
                hand: "123m".to_string(),
                kyoku: Some(value),
                ..ScenarioSpec::default()
            };
            assert_eq!(resolve(&spec).context.kyoku(), Some(value));
        }
    }

    #[test]
    fn rejects_unknown_table_state_fields() {
        let error =
            serde_json::from_str::<ScenarioSpec>(r#"{"hand": "123m", "kyotaku": 2}"#).unwrap_err();
        assert!(error.to_string().contains("kyotaku"), "{error}");
    }

    #[test]
    fn table_state_does_not_change_the_resolved_hand_or_legal_actions() {
        let base = resolve(&hand_spec("234m455p789s1123z", Some("N")));
        let spec = ScenarioSpec {
            remaining_tiles: Some(42),
            honba: Some(1),
            kyotaku_points: Some(2000),
            scores: Some(vec![12300, 28700, 40100, 18900]),
            kyoku: Some(2),
            ..hand_spec("234m455p789s1123z", Some("N"))
        };
        let scenario = resolve(&spec);

        assert_eq!(scenario.context.hand_tiles(), base.context.hand_tiles());
        assert_eq!(scenario.context.drawn_tile(), base.context.drawn_tile());
        assert_eq!(
            scenario.context.visible_tiles(),
            base.context.visible_tiles()
        );
        assert_eq!(scenario.legal_actions, base.legal_actions);
        assert_ne!(scenario.context.table_state(), base.context.table_state());
    }

    #[test]
    fn rejects_wrong_discards_length() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            discards: Some(vec!["1m".to_string()]),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::DiscardsLength { count: 1 })
        );
    }

    #[test]
    fn rejects_fifth_tile_of_a_type_across_zones() {
        let spec = spec_from_json(r#"{"hand": "1111m", "discards": ["1m", "", "", ""]}"#);
        let error = Scenario::resolve(&spec).unwrap_err();
        assert!(
            matches!(
                &error,
                ScenarioError::TileAllocation { field, .. } if field == "discards[0]"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_duplicated_red_five_across_zones() {
        let spec = spec_from_json(r#"{"hand": "0m", "dora_indicators": "5mr"}"#);
        let error = Scenario::resolve(&spec).unwrap_err();
        assert!(
            matches!(
                &error,
                ScenarioError::TileAllocation { field, .. } if field == "dora_indicators"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn automatic_legal_dahai_deduplicates_black_tiles() {
        let scenario = resolve(&hand_spec("111m222p", None));
        assert_eq!(dahai_labels(&scenario.legal_actions), ["1m", "2p"]);
    }

    #[test]
    fn automatic_legal_dahai_keeps_black_and_red_five() {
        let scenario = resolve(&hand_spec("55m0m", None));
        assert_eq!(dahai_labels(&scenario.legal_actions), ["5m", "5mr"]);
    }

    #[test]
    fn automatic_legal_dahai_follows_input_order() {
        let scenario = resolve(&hand_spec("9s1m5p", Some("E")));
        assert_eq!(
            dahai_labels(&scenario.legal_actions),
            ["9s", "1m", "5p", "E"]
        );
    }

    #[test]
    fn automatic_legal_dahai_order_is_deterministic() {
        let first = resolve(&hand_spec("234m455p789s1123z", Some("N")));
        let second = resolve(&hand_spec("234m455p789s1123z", Some("N")));
        assert_eq!(first.legal_actions, second.legal_actions);
        assert_eq!(
            dahai_labels(&first.legal_actions),
            [
                "2m", "3m", "4m", "4p", "5p", "7s", "8s", "9s", "E", "S", "W", "N"
            ]
        );
    }

    #[test]
    fn automatic_legal_dahai_uses_drawn_tile_last() {
        let scenario = resolve(&hand_spec("123m", Some("9p")));
        assert_eq!(
            dahai_labels(&scenario.legal_actions),
            ["1m", "2m", "3m", "9p"]
        );
    }

    #[test]
    fn explicit_legal_dahai_keeps_given_order() {
        let spec = ScenarioSpec {
            hand: "234m455p789s1123z".to_string(),
            draw: Some("N".to_string()),
            legal_dahai: Some("4p 7s N".to_string()),
            ..ScenarioSpec::default()
        };
        let scenario = resolve(&spec);
        assert_eq!(dahai_labels(&scenario.legal_actions), ["4p", "7s", "N"]);
    }

    #[test]
    fn explicit_legal_dahai_uses_held_physical_tiles() {
        let spec = ScenarioSpec {
            hand: "55m0m7s".to_string(),
            legal_dahai: Some("5mr 5m 7s".to_string()),
            ..ScenarioSpec::default()
        };
        let scenario = resolve(&spec);
        assert_eq!(dahai_labels(&scenario.legal_actions), ["5mr", "5m", "7s"]);

        let held: Vec<TileId> = scenario.context.hand_tiles().to_vec();
        for action in &scenario.legal_actions {
            if let LegalAction::Dahai { tile } = action {
                assert!(held.contains(tile), "{tile:?} must come from the hand");
            }
        }
    }

    #[test]
    fn explicit_legal_dahai_rejects_tile_not_held() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            legal_dahai: Some("9p".to_string()),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::LegalDahaiNotHeld {
                tile: "9p".to_string(),
            })
        );
    }

    #[test]
    fn explicit_legal_dahai_rejects_red_when_only_black_is_held() {
        let spec = ScenarioSpec {
            hand: "555m".to_string(),
            legal_dahai: Some("5mr".to_string()),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::LegalDahaiRedMismatch {
                tile: "5mr".to_string(),
                held: "5m".to_string(),
            })
        );
    }

    #[test]
    fn explicit_legal_dahai_rejects_black_when_only_red_is_held() {
        let spec = ScenarioSpec {
            hand: "0m".to_string(),
            legal_dahai: Some("5m".to_string()),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::LegalDahaiRedMismatch {
                tile: "5m".to_string(),
                held: "5mr".to_string(),
            })
        );
    }

    #[test]
    fn explicit_legal_dahai_rejects_duplicated_meaning() {
        let spec = ScenarioSpec {
            hand: "55m".to_string(),
            legal_dahai: Some("5m 5m".to_string()),
            ..ScenarioSpec::default()
        };
        assert_eq!(
            Scenario::resolve(&spec),
            Err(ScenarioError::LegalDahaiDuplicate {
                tile: "5m".to_string(),
            })
        );
    }

    #[test]
    fn explicit_legal_dahai_does_not_allocate_new_tiles() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            legal_dahai: Some("1m 2m".to_string()),
            ..ScenarioSpec::default()
        };
        let scenario = resolve(&spec);
        assert_eq!(scenario.context.visible_tiles().len(), 3);
        assert_eq!(dahai_labels(&scenario.legal_actions), ["1m", "2m"]);
    }

    #[test]
    fn allow_flags_append_actions() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            allow_hora: true,
            allow_ryukyoku: true,
            ..ScenarioSpec::default()
        };
        let scenario = resolve(&spec);
        assert!(scenario.legal_actions.contains(&LegalAction::Hora));
        assert!(scenario.legal_actions.contains(&LegalAction::Ryukyoku));
    }

    #[test]
    fn allow_flags_default_to_disabled() {
        let scenario = resolve(&hand_spec("123m", None));
        assert!(!scenario.legal_actions.contains(&LegalAction::Hora));
        assert!(!scenario.legal_actions.contains(&LegalAction::Ryukyoku));
        assert!(!scenario.legal_actions.contains(&LegalAction::None));
    }

    // 打 W で 4p / 7p のテンパイになる門前の何切る局面。リーチ自動生成の基準形として使う。
    const REACH_TENPAI_HAND: &str = "12388m56p234789s3z";

    fn reach_actions(scenario: &Scenario) -> Vec<&LegalAction> {
        scenario
            .legal_actions
            .iter()
            .filter(|action| matches!(action, LegalAction::Reach))
            .collect()
    }

    fn generates_reach(json: &str) -> bool {
        !reach_actions(&resolve(&spec_from_json(json))).is_empty()
    }

    #[test]
    fn menzen_tenpai_generates_reach_without_any_option() {
        let scenario = resolve(&spec_from_json(&format!(
            r#"{{"hand": "{REACH_TENPAI_HAND}", "player_id": 0, "oya": 0}}"#
        )));
        assert_eq!(reach_actions(&scenario).len(), 1);
        assert_eq!(scenario.legal_actions.last(), Some(&LegalAction::Reach));
    }

    #[test]
    fn a_hand_that_is_not_tenpai_after_any_legal_dahai_does_not_generate_reach() {
        assert!(!generates_reach(
            r#"{"hand": "234m455p789s1123z", "draw": "N", "player_id": 0, "oya": 0}"#
        ));
    }

    // 合法 Dahai を W 以外に絞ると、テンパイになる打牌が無くなりリーチも生成されない。
    #[test]
    fn a_legal_dahai_that_never_reaches_tenpai_does_not_generate_reach() {
        assert!(!generates_reach(&format!(
            r#"{{"hand": "{REACH_TENPAI_HAND}", "player_id": 0, "oya": 0, "legal_dahai": "1m"}}"#
        )));
    }

    // 同じテンパイ形で副露済み面子の種類だけが違う対照。チーは非門前なのでリーチできず、
    // 暗槓は門前のままなのでリーチを生成する。
    #[test]
    fn fixed_melds_decide_whether_the_hand_is_menzen_for_reach() {
        assert!(!generates_reach(OPEN_TENPAI_SCENARIO));
        assert!(generates_reach(ANKAN_TENPAI_SCENARIO));
    }

    const OPEN_TENPAI_SCENARIO: &str = r#"{
        "hand": "12388m56p234s3z",
        "player_id": 0,
        "oya": 0,
        "discards": ["", "", "", "7s"],
        "melds": [
            [{"kind": "chi", "tiles": "7s 8s 9s", "called_tile": "7s"}],
            [],
            [],
            []
        ]
    }"#;

    const ANKAN_TENPAI_SCENARIO: &str = r#"{
        "hand": "12388m56p234s3z",
        "player_id": 0,
        "oya": 0,
        "melds": [
            [{"kind": "ankan", "tiles": "1s 1s 1s 1s"}],
            [],
            [],
            []
        ]
    }"#;

    #[test]
    fn a_score_below_the_reach_declaration_does_not_generate_reach() {
        assert!(!generates_reach(&format!(
            r#"{{"hand": "{REACH_TENPAI_HAND}", "player_id": 0, "oya": 0,
                 "scores": [900, 25000, 25000, 25000]}}"#
        )));
        assert!(generates_reach(&format!(
            r#"{{"hand": "{REACH_TENPAI_HAND}", "player_id": 0, "oya": 0,
                 "scores": [1000, 25000, 25000, 25000]}}"#
        )));
    }

    // 持ち点が unknown ならリーチ不可と推測せず、他の条件だけで判定する。
    #[test]
    fn an_unknown_score_generates_reach() {
        let scenario = resolve(&spec_from_json(&format!(
            r#"{{"hand": "{REACH_TENPAI_HAND}", "player_id": 0, "oya": 0}}"#
        )));
        assert_eq!(scenario.context.own_score(), None);
        assert_eq!(reach_actions(&scenario).len(), 1);
    }

    #[test]
    fn too_few_remaining_tiles_do_not_generate_reach() {
        assert!(!generates_reach(&format!(
            r#"{{"hand": "{REACH_TENPAI_HAND}", "player_id": 0, "oya": 0, "remaining_tiles": 3}}"#
        )));
        assert!(generates_reach(&format!(
            r#"{{"hand": "{REACH_TENPAI_HAND}", "player_id": 0, "oya": 0, "remaining_tiles": 4}}"#
        )));
    }

    // 残りツモ牌数が unknown ならリーチ不可と推測しない。
    #[test]
    fn unknown_remaining_tiles_generate_reach() {
        let scenario = resolve(&spec_from_json(&format!(
            r#"{{"hand": "{REACH_TENPAI_HAND}", "player_id": 0, "oya": 0}}"#
        )));
        assert_eq!(scenario.context.remaining_tiles(), None);
        assert_eq!(reach_actions(&scenario).len(), 1);
    }

    #[test]
    fn an_already_reached_player_does_not_generate_reach() {
        assert!(!generates_reach(&format!(
            r#"{{"hand": "{REACH_TENPAI_HAND}", "player_id": 0, "oya": 0,
                 "reached": [true, false, false, false]}}"#
        )));
        assert!(generates_reach(&format!(
            r#"{{"hand": "{REACH_TENPAI_HAND}", "player_id": 0, "oya": 0,
                 "reached": [false, true, false, false]}}"#
        )));
    }

    const PON_REACTION_SCENARIO: &str = include_str!("../scenarios/pon_reaction.json");

    fn pon_actions_of(scenario: &Scenario) -> Vec<(TileId, Vec<TileId>)> {
        scenario
            .legal_actions
            .iter()
            .filter_map(|action| match action {
                LegalAction::Pon { tile, consumed } => Some((*tile, consumed.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn allow_none_appends_the_none_action() {
        let spec = ScenarioSpec {
            hand: "123m".to_string(),
            allow_none: true,
            ..ScenarioSpec::default()
        };
        let scenario = resolve(&spec);
        assert_eq!(scenario.legal_actions.last(), Some(&LegalAction::None));
    }

    #[test]
    fn legal_pon_and_allow_none_keep_the_existing_action_order() {
        let scenario = resolve(&spec_from_json(
            r#"{
                "hand": "123456789m 55p P P",
                "draw": "1p",
                "player_id": 0,
                "oya": 0,
                "discards": ["", "P", "", ""],
                "legal_dahai": "1p 5p",
                "legal_pon": [{"from_player": 1, "tile": "P", "consumed": "P P"}],
                "allow_hora": true,
                "allow_ryukyoku": true,
                "allow_none": true
            }"#,
        ));
        let labels: Vec<String> = scenario
            .legal_actions
            .iter()
            .map(|action| match action {
                LegalAction::Dahai { tile } => tile.to_mjai_string(),
                LegalAction::Pon { .. } => "Pon".to_string(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            labels,
            ["1p", "5p", "Pon", "Reach", "Hora", "Ryukyoku", "None"]
        );
    }

    #[test]
    fn legal_pon_builds_a_pon_action() {
        let scenario = resolve(&spec_from_json(PON_REACTION_SCENARIO));
        assert_eq!(scenario.context.reaction_source_player(), Some(1));
        let actions = pon_actions_of(&scenario);
        assert_eq!(actions.len(), 1);

        let (tile, consumed) = &actions[0];
        assert_eq!(tile.to_mjai_string(), "P");
        assert_eq!(labels(consumed), ["P", "P"]);
        assert_eq!(dahai_labels(&scenario.legal_actions), Vec::<String>::new());
        assert_eq!(scenario.legal_actions.last(), Some(&LegalAction::None));
        assert_eq!(scenario.legal_actions.len(), 2);
    }

    #[test]
    fn legal_pon_reuses_the_discarded_and_held_physical_tiles() {
        let scenario = resolve(&spec_from_json(PON_REACTION_SCENARIO));
        let context = &scenario.context;
        let (tile, consumed) = pon_actions_of(&scenario).remove(0);

        assert_eq!(context.discards_of(1).unwrap().last().copied(), Some(tile));
        assert_eq!(consumed.len(), 2);
        assert_ne!(consumed[0], consumed[1]);
        for consumed in &consumed {
            assert!(
                context.hand_tiles().contains(consumed),
                "{consumed:?} must come from the hand"
            );
        }
    }

    #[test]
    fn legal_pon_does_not_add_visible_tiles_or_melds() {
        let mut without = spec_from_json(PON_REACTION_SCENARIO);
        without.legal_pon = None;
        let without = resolve(&without);
        let with = resolve(&spec_from_json(PON_REACTION_SCENARIO));

        assert_eq!(
            with.context.visible_tiles().len(),
            without.context.visible_tiles().len()
        );
        assert_eq!(
            with.context.visible_tiles(),
            without.context.visible_tiles()
        );
        assert!(with.context.melds().iter().all(|melds| melds.is_empty()));
        assert_eq!(
            with.context.own_fixed_meld_count(),
            Some(bot_logic::FixedMeldCount::NONE)
        );
        assert!(validate_unique_physical_tiles(with.context.visible_tiles()).is_ok());
    }

    #[test]
    fn pon_reaction_scenario_is_one_shanten() {
        let scenario = resolve(&spec_from_json(PON_REACTION_SCENARIO));
        let counts =
            bot_logic::TileCounts::from_tiles(scenario.context.hand_tiles().iter().copied());
        let shanten = bot_logic::calculate_shanten_with_fixed_melds(
            &counts,
            scenario
                .context
                .own_fixed_meld_count()
                .unwrap_or(bot_logic::FixedMeldCount::NONE),
        );
        assert_eq!(shanten.min(), 1);
    }

    #[test]
    fn scenario_without_legal_pon_and_allow_none_is_unchanged() {
        let without = resolve(&spec_from_json(
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "player_id": 0,
                "oya": 1,
                "discards": ["", "P", "", ""]
            }"#,
        ));
        let with_defaults = resolve(&spec_from_json(
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "player_id": 0,
                "oya": 1,
                "discards": ["", "P", "", ""],
                "legal_pon": null,
                "allow_none": false
            }"#,
        ));
        assert_eq!(without.context, with_defaults.context);
        assert_eq!(without.legal_actions, with_defaults.legal_actions);
        assert!(
            without
                .legal_actions
                .iter()
                .all(|action| matches!(action, LegalAction::Dahai { .. }))
        );
    }

    fn pon_error(discards: &str, legal_pon: &str, extra: &str) -> ScenarioError {
        let json = format!(
            r#"{{
                "hand": "123456m55p78s455z",
                "discards": {discards},
                "legal_dahai": "",
                "legal_pon": {legal_pon}
                {extra}
            }}"#
        );
        Scenario::resolve(&spec_from_json(&json)).unwrap_err()
    }

    #[test]
    fn rejects_legal_pon_without_player_id() {
        let error = pon_error(
            r#"["", "P", "", ""]"#,
            r#"[{"from_player": 1, "tile": "P", "consumed": "P P"}]"#,
            "",
        );
        assert_eq!(
            error,
            ScenarioError::LegalPonWithoutPlayerId {
                field: "legal_pon[0]".to_string(),
            }
        );
    }

    #[test]
    fn rejects_legal_pon_from_player_out_of_range() {
        let error = pon_error(
            r#"["", "P", "", ""]"#,
            r#"[{"from_player": 4, "tile": "P", "consumed": "P P"}]"#,
            r#", "player_id": 0, "oya": 0"#,
        );
        assert_eq!(
            error,
            ScenarioError::SeatOutOfRange {
                field: "legal_pon[0].from_player".to_string(),
                value: 4,
            }
        );
    }

    #[test]
    fn rejects_legal_pon_from_own_discard() {
        let error = pon_error(
            r#"["P", "", "", ""]"#,
            r#"[{"from_player": 0, "tile": "P", "consumed": "P P"}]"#,
            r#", "player_id": 0, "oya": 0"#,
        );
        assert_eq!(
            error,
            ScenarioError::LegalPonFromOwnDiscard {
                field: "legal_pon[0]".to_string(),
                player_id: 0,
            }
        );
    }

    #[test]
    fn rejects_legal_pon_target_that_is_not_the_last_discard() {
        let error = pon_error(
            r#"["", "P F", "", ""]"#,
            r#"[{"from_player": 1, "tile": "P", "consumed": "P P"}]"#,
            r#", "player_id": 0, "oya": 0"#,
        );
        assert_eq!(
            error,
            ScenarioError::LegalPonTargetMismatch {
                field: "legal_pon[0]".to_string(),
                tile: "P".to_string(),
                discarded: "F".to_string(),
                from_player: 1,
            }
        );
    }

    #[test]
    fn rejects_legal_pon_without_any_discard() {
        let error = pon_error(
            r#"["", "", "", ""]"#,
            r#"[{"from_player": 1, "tile": "P", "consumed": "P P"}]"#,
            r#", "player_id": 0, "oya": 0"#,
        );
        assert_eq!(
            error,
            ScenarioError::LegalPonNoDiscard {
                field: "legal_pon[0]".to_string(),
                from_player: 1,
            }
        );
    }

    #[test]
    fn rejects_legal_pon_with_wrong_consumed_count() {
        for (consumed, count) in [("P", 1), ("P P P", 3)] {
            let error = pon_error(
                r#"["", "P", "", ""]"#,
                &format!(r#"[{{"from_player": 1, "tile": "P", "consumed": "{consumed}"}}]"#),
                r#", "player_id": 0, "oya": 0"#,
            );
            assert_eq!(
                error,
                ScenarioError::LegalPonConsumedCount {
                    field: "legal_pon[0]".to_string(),
                    expected: 2,
                    count,
                },
                "{consumed}"
            );
        }
    }

    #[test]
    fn rejects_legal_pon_with_mixed_consumed_tile_types() {
        let error = pon_error(
            r#"["", "P", "", ""]"#,
            r#"[{"from_player": 1, "tile": "P", "consumed": "P F"}]"#,
            r#", "player_id": 0, "oya": 0"#,
        );
        assert_eq!(
            error,
            ScenarioError::LegalPonTileType {
                field: "legal_pon[0]".to_string(),
                tile: "P".to_string(),
                consumed: "P F".to_string(),
            }
        );
    }

    #[test]
    fn rejects_legal_pon_consumed_that_is_not_held() {
        let json = r#"{
            "hand": "123456m55p789s45z",
            "player_id": 0,
            "oya": 0,
            "discards": ["", "P", "", ""],
            "legal_dahai": "",
            "legal_pon": [{"from_player": 1, "tile": "P", "consumed": "P P"}]
        }"#;
        assert_eq!(
            Scenario::resolve(&spec_from_json(json)).unwrap_err(),
            ScenarioError::LegalPonConsumedNotHeld {
                field: "legal_pon[0]".to_string(),
                tile: "P".to_string(),
            }
        );
    }

    #[test]
    fn json_rejects_unknown_legal_pon_field() {
        assert!(
            serde_json::from_str::<ScenarioSpec>(
                r#"{"hand": "1m", "legal_pon": [{"from_player": 1, "tile": "P", "consumed": "P P", "kind": "pon"}]}"#
            )
            .is_err()
        );
    }

    const OWN_PON_SCENARIO: &str = r#"{
        "hand": "234m455p789s",
        "draw": "N",
        "player_id": 0,
        "oya": 1,
        "discards": ["", "E", "", ""],
        "melds": [
            [{"kind": "pon", "tiles": "E E E", "called_tile": "E"}],
            [],
            [],
            []
        ]
    }"#;

    fn visible_count(scenario: &Scenario, label: &str) -> usize {
        scenario
            .context
            .visible_tiles()
            .iter()
            .filter(|tile| tile.to_mjai_string() == label)
            .count()
    }

    #[test]
    fn json_melds_default_to_empty() {
        let spec = spec_from_json(r#"{"hand": "123m456p789s11z"}"#);
        assert_eq!(spec.melds, None);
        let scenario = resolve(&spec);
        assert!(
            scenario
                .context
                .melds()
                .iter()
                .all(|melds| melds.is_empty())
        );
    }

    #[test]
    fn scenario_without_melds_keeps_the_previous_context() {
        let without = resolve(&spec_from_json(
            r#"{"hand": "234m455p789s1123z", "draw": "N", "player_id": 0, "oya": 1}"#,
        ));
        let with_empty = resolve(&spec_from_json(
            r#"{
                "hand": "234m455p789s1123z",
                "draw": "N",
                "player_id": 0,
                "oya": 1,
                "melds": [[], [], [], []]
            }"#,
        ));
        assert_eq!(without.context, with_empty.context);
        assert_eq!(without.legal_actions, with_empty.legal_actions);
        assert_eq!(
            without.context.own_fixed_meld_count(),
            Some(bot_logic::FixedMeldCount::NONE)
        );
    }

    #[test]
    fn json_pon_meld_is_resolved() {
        let scenario = resolve(&spec_from_json(OWN_PON_SCENARIO));
        let melds = scenario.context.melds_of(0).unwrap();
        assert_eq!(melds.len(), 1);
        assert_eq!(melds[0].kind(), MeldKind::Pon);
        assert_eq!(labels(melds[0].tiles()), ["E", "E", "E"]);
        assert_eq!(
            melds[0].called_tile().map(|tile| tile.to_mjai_string()),
            Some("E".to_string())
        );
        assert!(melds[0].is_open());
        assert!(scenario.context.melds_of(1).unwrap().is_empty());
    }

    #[test]
    fn json_pon_meld_makes_own_fixed_meld_count_one() {
        let scenario = resolve(&spec_from_json(OWN_PON_SCENARIO));
        assert_eq!(
            scenario
                .context
                .own_fixed_meld_count()
                .map(bot_logic::FixedMeldCount::get),
            Some(1)
        );
    }

    #[test]
    fn called_tile_reuses_the_discarded_physical_tile() {
        let scenario = resolve(&spec_from_json(OWN_PON_SCENARIO));
        let called_tile = scenario.context.melds_of(0).unwrap()[0]
            .called_tile()
            .unwrap();
        assert!(
            scenario
                .context
                .discards_of(1)
                .unwrap()
                .contains(&called_tile)
        );
        assert_eq!(visible_count(&scenario, "E"), 3);
        assert!(validate_unique_physical_tiles(scenario.context.visible_tiles()).is_ok());
    }

    #[test]
    fn meld_tiles_keep_the_input_order_with_the_called_tile_in_place() {
        let scenario = resolve(&spec_from_json(
            r#"{
                "hand": "455p789s",
                "player_id": 0,
                "discards": ["", "2m", "", ""],
                "melds": [[{"kind": "chi", "tiles": "123m", "called_tile": "2m"}], [], [], []]
            }"#,
        ));
        let meld = &scenario.context.melds_of(0).unwrap()[0];
        assert_eq!(labels(meld.tiles()), ["1m", "2m", "3m"]);
        assert_eq!(meld.tiles()[1], meld.called_tile().unwrap());
        assert_eq!(
            meld.called_tile(),
            scenario.context.discards_of(1).unwrap().first().copied()
        );
    }

    #[test]
    fn red_five_in_a_meld_keeps_its_physical_tile() {
        let scenario = resolve(&spec_from_json(
            r#"{
                "hand": "455p789s",
                "player_id": 0,
                "discards": ["", "0m", "", ""],
                "melds": [[{"kind": "chi", "tiles": "406m", "called_tile": "0m"}], [], [], []]
            }"#,
        ));
        let meld = &scenario.context.melds_of(0).unwrap()[0];
        assert_eq!(labels(meld.tiles()), ["4m", "5mr", "6m"]);
        assert!(meld.called_tile().unwrap().is_red());
        assert_eq!(visible_count(&scenario, "5mr"), 1);
    }

    #[test]
    fn ankan_meld_is_fully_visible_and_not_open() {
        let scenario = resolve(&spec_from_json(
            r#"{
                "hand": "234m455p789s",
                "player_id": 0,
                "melds": [[{"kind": "ankan", "tiles": "1111z"}], [], [], []]
            }"#,
        ));
        let melds = scenario.context.melds_of(0).unwrap();
        assert_eq!(melds[0].kind(), MeldKind::Ankan);
        assert_eq!(melds[0].called_tile(), None);
        assert!(!melds[0].is_open());
        assert_eq!(visible_count(&scenario, "E"), 4);
        assert_eq!(
            scenario
                .context
                .own_fixed_meld_count()
                .map(bot_logic::FixedMeldCount::get),
            Some(1)
        );
    }

    #[test]
    fn chi_and_kan_melds_are_resolved() {
        let scenario = resolve(&spec_from_json(
            r#"{
                "hand": "455p789s",
                "player_id": 0,
                "discards": ["", "3m", "1z", "2z"],
                "melds": [
                    [{"kind": "chi", "tiles": "123m", "called_tile": "3m"}],
                    [],
                    [{"kind": "daiminkan", "tiles": "1111z", "called_tile": "1z"}],
                    [{"kind": "kakan", "tiles": "2222z", "called_tile": "2z"}]
                ]
            }"#,
        ));
        assert_eq!(
            scenario.context.melds_of(0).unwrap()[0].kind(),
            MeldKind::Chi
        );
        assert_eq!(
            scenario.context.melds_of(2).unwrap()[0].kind(),
            MeldKind::Daiminkan
        );
        assert_eq!(
            scenario.context.melds_of(3).unwrap()[0].kind(),
            MeldKind::Kakan
        );
        assert_eq!(visible_count(&scenario, "1m"), 1);
        assert_eq!(visible_count(&scenario, "E"), 4);
        assert_eq!(visible_count(&scenario, "S"), 4);
        assert!(validate_unique_physical_tiles(scenario.context.visible_tiles()).is_ok());
    }

    #[test]
    fn kakan_stays_a_single_fixed_meld() {
        let scenario = resolve(&spec_from_json(
            r#"{
                "hand": "455p789s",
                "player_id": 0,
                "discards": ["", "1z", "", ""],
                "melds": [[{"kind": "kakan", "tiles": "1111z", "called_tile": "1z"}], [], [], []]
            }"#,
        ));
        assert_eq!(scenario.context.melds_of(0).unwrap().len(), 1);
        assert_eq!(
            scenario
                .context
                .own_fixed_meld_count()
                .map(bot_logic::FixedMeldCount::get),
            Some(1)
        );
    }

    #[test]
    fn melds_of_all_players_are_kept_separately() {
        let scenario = resolve(&spec_from_json(
            r#"{
                "hand": "455p789s",
                "player_id": 1,
                "discards": ["", "", "3m", ""],
                "melds": [
                    [],
                    [{"kind": "chi", "tiles": "123m", "called_tile": "3m"}],
                    [],
                    [{"kind": "ankan", "tiles": "1111z"}]
                ]
            }"#,
        ));
        assert!(scenario.context.melds_of(0).unwrap().is_empty());
        assert_eq!(scenario.context.melds_of(1).unwrap().len(), 1);
        assert_eq!(scenario.context.melds_of(3).unwrap().len(), 1);
        assert_eq!(
            scenario
                .context
                .own_fixed_meld_count()
                .map(bot_logic::FixedMeldCount::get),
            Some(1)
        );
    }

    #[test]
    fn melds_without_player_id_have_no_own_fixed_meld_count() {
        let scenario = resolve(&spec_from_json(
            r#"{
                "hand": "455p789s",
                "melds": [[{"kind": "ankan", "tiles": "1111z"}], [], [], []]
            }"#,
        ));
        assert_eq!(scenario.context.melds_of(0).unwrap().len(), 1);
        assert_eq!(scenario.context.own_melds(), None);
        assert_eq!(scenario.context.own_fixed_meld_count(), None);
    }

    #[test]
    fn rejects_wrong_melds_length() {
        let error = Scenario::resolve(&spec_from_json(
            r#"{"hand": "123m", "melds": [[], [], []]}"#,
        ))
        .unwrap_err();
        assert_eq!(error, ScenarioError::MeldsLength { count: 3 });
    }

    #[test]
    fn rejects_meld_with_wrong_tile_count() {
        let error = Scenario::resolve(&spec_from_json(
            r#"{
                "hand": "123m",
                "discards": ["", "1z", "", ""],
                "melds": [[{"kind": "pon", "tiles": "11z", "called_tile": "1z"}], [], [], []]
            }"#,
        ))
        .unwrap_err();
        assert!(
            matches!(
                &error,
                ScenarioError::MeldTileCount { field, kind, expected, count }
                    if field == "melds[0][0]" && kind == "pon" && *expected == 3 && *count == 2
            ),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_chi_that_is_not_a_sequence() {
        let error = Scenario::resolve(&spec_from_json(
            r#"{
                "hand": "456p",
                "discards": ["", "1m", "", ""],
                "melds": [[{"kind": "chi", "tiles": "135m", "called_tile": "1m"}], [], [], []]
            }"#,
        ))
        .unwrap_err();
        assert!(
            matches!(&error, ScenarioError::MeldShape { kind, .. } if kind == "chi"),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_chi_of_honor_tiles() {
        let error = Scenario::resolve(&spec_from_json(
            r#"{
                "hand": "456p",
                "discards": ["", "1z", "", ""],
                "melds": [[{"kind": "chi", "tiles": "123z", "called_tile": "1z"}], [], [], []]
            }"#,
        ))
        .unwrap_err();
        assert!(
            matches!(&error, ScenarioError::MeldShape { kind, .. } if kind == "chi"),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_pon_of_mixed_tiles() {
        let error = Scenario::resolve(&spec_from_json(
            r#"{
                "hand": "456p",
                "discards": ["", "1z", "", ""],
                "melds": [[{"kind": "pon", "tiles": "112z", "called_tile": "1z"}], [], [], []]
            }"#,
        ))
        .unwrap_err();
        assert!(
            matches!(&error, ScenarioError::MeldShape { kind, .. } if kind == "pon"),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_called_meld_without_called_tile() {
        for (kind, tiles) in [
            ("chi", "123m"),
            ("pon", "111z"),
            ("daiminkan", "1111z"),
            ("kakan", "1111z"),
        ] {
            let json = format!(
                r#"{{
                    "hand": "456p",
                    "discards": ["", "1m 1z", "", ""],
                    "melds": [[{{"kind": "{kind}", "tiles": "{tiles}"}}], [], [], []]
                }}"#
            );
            let error = Scenario::resolve(&spec_from_json(&json)).unwrap_err();
            assert!(
                matches!(
                    &error,
                    ScenarioError::MeldCalledTileMissing { kind: label, .. } if label == kind
                ),
                "{kind}: {error:?}"
            );
        }
    }

    #[test]
    fn rejects_ankan_with_called_tile() {
        let error = Scenario::resolve(&spec_from_json(
            r#"{
                "hand": "456p",
                "discards": ["", "1z", "", ""],
                "melds": [[{"kind": "ankan", "tiles": "1111z", "called_tile": "1z"}], [], [], []]
            }"#,
        ))
        .unwrap_err();
        assert!(
            matches!(
                &error,
                ScenarioError::MeldCalledTileNotAllowed { kind, tile, .. }
                    if kind == "ankan" && tile == "E"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_called_tile_outside_the_meld_tiles() {
        let error = Scenario::resolve(&spec_from_json(
            r#"{
                "hand": "456p",
                "discards": ["", "2z", "", ""],
                "melds": [[{"kind": "pon", "tiles": "111z", "called_tile": "2z"}], [], [], []]
            }"#,
        ))
        .unwrap_err();
        assert!(
            matches!(
                &error,
                ScenarioError::MeldCalledTileNotInMeld { tile, .. } if tile == "S"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_called_tile_that_is_not_discarded() {
        let error = Scenario::resolve(&spec_from_json(
            r#"{
                "hand": "456p",
                "melds": [[{"kind": "pon", "tiles": "111z", "called_tile": "1z"}], [], [], []]
            }"#,
        ))
        .unwrap_err();
        assert!(
            matches!(
                &error,
                ScenarioError::MeldCalledTileNotDiscarded { field, tile }
                    if field == "melds[0][0]" && tile == "E"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_two_melds_claiming_the_same_discarded_tile() {
        let error = Scenario::resolve(&spec_from_json(
            r#"{
                "hand": "456p",
                "discards": ["", "1z", "", ""],
                "melds": [
                    [{"kind": "pon", "tiles": "111z", "called_tile": "1z"}],
                    [],
                    [{"kind": "pon", "tiles": "111z", "called_tile": "1z"}],
                    []
                ]
            }"#,
        ))
        .unwrap_err();
        assert!(
            matches!(
                &error,
                ScenarioError::MeldCalledTileNotDiscarded { field, .. } if field == "melds[2][0]"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_fifth_copy_created_by_a_meld() {
        let error = Scenario::resolve(&spec_from_json(
            r#"{
                "hand": "11z",
                "discards": ["", "1z", "", ""],
                "melds": [[{"kind": "daiminkan", "tiles": "1111z", "called_tile": "1z"}], [], [], []]
            }"#,
        ))
        .unwrap_err();
        assert!(
            matches!(
                &error,
                ScenarioError::TileAllocation { field, .. } if field == "melds[0][0]"
            ),
            "{error:?}"
        );
    }

    #[test]
    fn json_rejects_unknown_meld_field() {
        assert!(
            serde_json::from_str::<ScenarioSpec>(
                r#"{"hand": "1m", "melds": [[{"kind": "pon", "tiles": "111z", "from": 1}], [], [], []]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn json_rejects_unknown_meld_kind() {
        assert!(
            serde_json::from_str::<ScenarioSpec>(
                r#"{"hand": "1m", "melds": [[{"kind": "nuki", "tiles": "111z"}], [], [], []]}"#
            )
            .is_err()
        );
    }

    const POST_REACH_GENBUTSU_SCENARIO: &str =
        include_str!("../scenarios/post_reach_genbutsu.json");
    const MULTI_RIICHI_DOUBLE_WIND_SCENARIO: &str =
        include_str!("../scenarios/defense_multi_riichi_double_wind.json");

    fn tile_type(mjai: &str) -> TileType {
        TileType::from_mjai_type_str(mjai).unwrap()
    }

    #[test]
    fn post_reach_passed_is_resolved_per_player() {
        let spec = spec_from_json(
            r#"{
                "hand": "123m456p789s11z",
                "player_id": 0,
                "reached": [false, true, true, false],
                "discards": ["", "3p", "4s", ""],
                "post_reach_passed": ["", "4s", "", ""]
            }"#,
        );
        let context = resolve(&spec).context;
        assert_eq!(
            context.post_reach_passed_tiles_of(1),
            Some([tile_type("4s")].as_slice())
        );
        assert_eq!(context.post_reach_passed_tiles_of(2), Some([].as_slice()));
    }

    #[test]
    fn temporary_passed_is_resolved_per_player_and_normalizes_red_five() {
        let spec = spec_from_json(
            r#"{
                "hand": "123m456p789s11z",
                "temporary_passed": ["", "5sr", "9m", ""]
            }"#,
        );
        let context = resolve(&spec).context;
        assert_eq!(
            context.temporary_passed_tiles_of(1),
            Some([tile_type("5s")].as_slice())
        );
        assert_eq!(
            context.temporary_passed_tiles_of(2),
            Some([tile_type("9m")].as_slice())
        );
    }

    #[test]
    fn omitted_temporary_passed_is_unknown() {
        let spec = spec_from_json(r#"{"hand": "123m456p789s11z"}"#);
        assert_eq!(resolve(&spec).context.temporary_passed_tiles(), None);
    }

    #[test]
    fn post_reach_passed_does_not_allocate_physical_tiles() {
        let spec = spec_from_json(
            r#"{
                "hand": "1111m",
                "post_reach_passed": ["", "1m 1m 1m", "", ""]
            }"#,
        );
        let context = resolve(&spec).context;
        assert_eq!(context.visible_tiles().len(), 4);
        assert_eq!(
            context.post_reach_passed_tiles_of(1).map(<[_]>::len),
            Some(3)
        );
    }

    #[test]
    fn post_reach_passed_red_five_is_stored_as_the_black_tile_type() {
        let spec = spec_from_json(
            r#"{
                "hand": "1m",
                "post_reach_passed": ["", "5sr", "", ""]
            }"#,
        );
        let context = resolve(&spec).context;
        assert_eq!(
            context.post_reach_passed_tiles_of(1),
            Some([tile_type("5s")].as_slice())
        );
    }

    #[test]
    fn post_reach_genbutsu_scenario_makes_the_passed_tile_genbutsu_for_both_reachers() {
        let spec = spec_from_json(POST_REACH_GENBUTSU_SCENARIO);
        let context = resolve(&spec).context;
        let four_sou = tile_type("4s");

        assert_eq!(context.reached_opponents(), vec![1, 2]);
        assert!(is_genbutsu_for(four_sou, 1, &context));
        assert!(is_genbutsu_for(four_sou, 2, &context));
        assert!(is_genbutsu_for_all_reached(four_sou, &context));
    }

    #[test]
    fn post_reach_genbutsu_scenario_selects_the_passed_tile_as_genbutsu_fallback() {
        let spec = spec_from_json(POST_REACH_GENBUTSU_SCENARIO);
        let scenario = resolve(&spec);
        let selected =
            select_defense_fallback_action_with_kind(&scenario.context, &scenario.legal_actions);

        assert_eq!(
            selected.map(|(action, kind)| (action.clone(), kind)),
            Some((
                LegalAction::Dahai {
                    tile: TileId::new(tile_type("4s").raw() * 4).unwrap(),
                },
                DefenseFallbackKind::Genbutsu
            ))
        );
    }

    #[test]
    fn multi_riichi_double_wind_scenario_prefers_suji_and_keeps_diagnostics_consistent() {
        // Player 2's open guest-wind Pon intentionally makes exact reach evaluation unavailable,
        // keeping this scenario focused on the legacy multi-riichi Suji fallback.
        let scenario = resolve(&spec_from_json(MULTI_RIICHI_DOUBLE_WIND_SCENARIO));
        let nine_man = tile_type("9m");
        let south = tile_type("S");

        assert_eq!(scenario.context.reached_opponents(), vec![1, 2]);
        assert!(!is_genbutsu_for(nine_man, 1, &scenario.context));
        assert!(is_genbutsu_for(nine_man, 2, &scenario.context));
        assert_eq!(
            suji_safety_rank_for(nine_man, 1, &scenario.context),
            Some(SujiSafetyRank::Suji)
        );

        let selected =
            select_defense_fallback_action_with_kind(&scenario.context, &scenario.legal_actions)
                .expect("defense fallback");
        assert_eq!(
            selected.1,
            DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji)
        );
        assert!(matches!(selected.0, LegalAction::Dahai { tile } if tile.tile_type() == nine_man));

        let candidates = DefenseCandidateDiagnostic::for_legal_actions(
            &scenario.context,
            &scenario.legal_actions,
            Some(selected.0),
        );
        let south = candidates
            .iter()
            .find(|candidate| candidate.tile == south)
            .unwrap();
        assert_eq!(south.honor_safety_rank, Some(HonorSafetyRank::OneVisible));
        assert_eq!(
            south.opponent_honor_value,
            Some(OpponentHonorValue::DoubleWind)
        );

        let mut agent = ShantenAgent;
        let action = agent.act(&scenario.context, &scenario.legal_actions);
        let diagnostic = ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions);
        let with_lookahead = ShantenAgent::diagnose_with_options(
            &scenario.context,
            &scenario.legal_actions,
            DiagnosticOptions::WITH_LOOKAHEAD,
        );
        assert_eq!(action, diagnostic.selected_action);
        assert_eq!(action, with_lookahead.selected_action);
        assert!(matches!(action, LegalAction::Dahai { tile } if tile.tile_type() == nine_man));
        assert_eq!(
            diagnostic.defense_fallback_kind(),
            Some(DefenseFallbackKind::SuitedSafety(SuitedSafetyRank::Suji))
        );
        assert_eq!(
            with_lookahead.defense_fallback_kind(),
            diagnostic.defense_fallback_kind()
        );
    }

    #[test]
    fn history_furiten_json_distinguishes_true_false_and_unknown() {
        let omitted = resolve(&spec_from_json(r#"{"hand":"123m"}"#));
        assert_eq!(
            omitted.context.history_furiten(),
            HistoryFuritenFacts::default()
        );

        let explicit = resolve(&spec_from_json(
            r#"{"hand":"123m","history_furiten":{"same_turn":true,"riichi_missed_win":false}}"#,
        ));
        assert_eq!(
            explicit.context.history_furiten(),
            HistoryFuritenFacts {
                same_turn: Some(true),
                riichi_missed_win: Some(false),
            }
        );
        assert!(
            serde_json::from_str::<ScenarioSpec>(
                r#"{"hand":"123m","history_furiten":{"same_turn":"yes"}}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ScenarioSpec>(
                r#"{"hand":"123m","history_furiten":{"temporary":true}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn history_furiten_does_not_change_selection_or_diagnostic_consistency() {
        let base = resolve(&spec_from_json(MULTI_RIICHI_DOUBLE_WIND_SCENARIO));
        let context = base
            .context
            .clone()
            .with_history_furiten_facts(HistoryFuritenFacts {
                same_turn: Some(true),
                riichi_missed_win: Some(true),
            });
        let mut agent = ShantenAgent;
        let action = agent.act(&context, &base.legal_actions);
        let diagnostic = ShantenAgent::diagnose(&context, &base.legal_actions);
        let lookahead = ShantenAgent::diagnose_with_options(
            &context,
            &base.legal_actions,
            DiagnosticOptions::WITH_LOOKAHEAD,
        );
        assert_eq!(action, diagnostic.selected_action);
        assert_eq!(action, lookahead.selected_action);
        assert_eq!(diagnostic.history_furiten, context.history_furiten());
        assert_eq!(lookahead.history_furiten, context.history_furiten());
    }
}
