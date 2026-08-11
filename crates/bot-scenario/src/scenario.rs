use bot_core::{GameContext, LegalAction, Meld, MeldKind};
use bot_logic::{TileId, TileType};
use serde::Deserialize;

use crate::error::ScenarioError;
use crate::input::{LogicalTile, parse_tiles};
use crate::tiles::{TileAllocator, validate_unique_physical_tiles};

const CHI_TILE_COUNT: usize = 3;
const PON_TILE_COUNT: usize = 3;
const KAN_TILE_COUNT: usize = 4;

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
    pub melds: Option<Vec<Vec<MeldSpec>>>,
    #[serde(default)]
    pub extra_visible_tiles: Option<String>,
    #[serde(default)]
    pub legal_dahai: Option<String>,
    #[serde(default)]
    pub allow_reach: bool,
    #[serde(default)]
    pub allow_hora: bool,
    #[serde(default)]
    pub allow_ryukyoku: bool,
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
        let meld_inputs = resolve_meld_inputs(spec.melds.as_deref())?;
        let player_id = resolve_seat("player_id", spec.player_id)?;
        let oya = resolve_seat("oya", spec.oya)?;
        let round_wind = parse_wind("round_wind", spec.round_wind.as_deref())?;
        let seat_wind = resolve_seat_wind(spec.seat_wind.as_deref(), player_id, oya)?;

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

        let legal_actions = build_legal_actions(spec, &hand, draw)?;

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
        );

        Ok(Self {
            context,
            legal_actions,
        })
    }
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

fn resolve_seat(field: &str, value: Option<u8>) -> Result<Option<u8>, ScenarioError> {
    match value {
        Some(value) if value > 3 => Err(ScenarioError::SeatOutOfRange {
            field: field.to_string(),
            value,
        }),
        value => Ok(value),
    }
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

fn derive_seat_wind(player_id: Option<u8>, oya: Option<u8>) -> Option<TileType> {
    let (player_id, oya) = (player_id?, oya?);
    TileType::wind_from_seat_index((player_id + 4 - oya) % 4)
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
    let called = match spec.called_tile.as_deref() {
        Some(input) => {
            let parsed = parse_field(&called_field, input)?;
            if parsed.len() != 1 {
                return Err(ScenarioError::NotSingleTile {
                    field: called_field,
                    input: input.to_string(),
                    count: parsed.len(),
                });
            }
            parsed.first().copied()
        }
        None => None,
    };

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
    hand: &[TileId],
    draw: Option<TileId>,
) -> Result<Vec<LegalAction>, ScenarioError> {
    let mut actions = match spec.legal_dahai.as_deref() {
        Some(input) => explicit_dahai_actions(input, hand, draw)?,
        None => automatic_dahai_actions(hand, draw),
    };

    if spec.allow_reach {
        actions.push(LegalAction::Reach);
    }
    if spec.allow_hora {
        actions.push(LegalAction::Hora);
    }
    if spec.allow_ryukyoku {
        actions.push(LegalAction::Ryukyoku);
    }

    Ok(actions)
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(spec.extra_visible_tiles, None);
        assert_eq!(spec.legal_dahai, None);
        assert!(!spec.allow_reach);
        assert!(!spec.allow_hora);
        assert!(!spec.allow_ryukyoku);

        let scenario = resolve(&spec);
        let context = &scenario.context;
        assert_eq!(context.drawn_tile(), None);
        assert!(context.dora_indicators().is_empty());
        assert_eq!(context.round_wind(), None);
        assert_eq!(context.seat_wind(), None);
        assert_eq!(context.player_id(), None);
        assert_eq!(context.oya(), None);
        assert_eq!(context.reached(), &[false; 4]);
        assert!(
            context
                .discards()
                .iter()
                .all(|discards| discards.is_empty())
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
                "allow_reach": false,
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
            allow_reach: true,
            allow_hora: true,
            allow_ryukyoku: true,
            ..ScenarioSpec::default()
        };
        let scenario = resolve(&spec);
        assert!(scenario.legal_actions.contains(&LegalAction::Reach));
        assert!(scenario.legal_actions.contains(&LegalAction::Hora));
        assert!(scenario.legal_actions.contains(&LegalAction::Ryukyoku));
    }

    #[test]
    fn allow_flags_default_to_disabled() {
        let scenario = resolve(&hand_spec("123m", None));
        assert!(!scenario.legal_actions.contains(&LegalAction::Reach));
        assert!(!scenario.legal_actions.contains(&LegalAction::Hora));
        assert!(!scenario.legal_actions.contains(&LegalAction::Ryukyoku));
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
}
