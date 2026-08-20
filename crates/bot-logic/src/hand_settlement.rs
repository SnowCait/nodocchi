use thiserror::Error;

use crate::hand_value::HandValue;
use crate::payment::{Payment, PaymentBreakdown};

const HONBA_RON_POINTS: u32 = 300;
const HONBA_TSUMO_POINTS_PER_PAYER: u32 = 100;
const TSUMO_PAYER_COUNT: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MissingSettlementFact {
    Honba,
    KyotakuPoints,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum HandSettlementError {
    #[error("the hand value has no exact payment")]
    UnknownPayment,

    #[error("the table state is missing an exact settlement fact: {0:?}")]
    IncompleteTableState(MissingSettlementFact),

    #[error("the settlement overflows: honba {honba}, kyotaku points {kyotaku_points}")]
    Overflow { honba: u32, kyotaku_points: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HonbaPayments {
    Ron { pay_ron: u32 },
    Tsumo { pay_from_each_payer: u32 },
}

impl HonbaPayments {
    pub fn pay_ron(self) -> Option<u32> {
        match self {
            Self::Ron { pay_ron } => Some(pay_ron),
            Self::Tsumo { .. } => None,
        }
    }

    pub fn pay_from_each_payer(self) -> Option<u32> {
        match self {
            Self::Tsumo {
                pay_from_each_payer,
            } => Some(pay_from_each_payer),
            Self::Ron { .. } => None,
        }
    }

    fn checked_total(self) -> Option<u32> {
        match self {
            Self::Ron { pay_ron } => Some(pay_ron),
            Self::Tsumo {
                pay_from_each_payer,
            } => pay_from_each_payer.checked_mul(TSUMO_PAYER_COUNT),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandSettlement {
    base: Payment,
    honba: u32,
    honba_payments: HonbaPayments,
    honba_total: u32,
    kyotaku_points: u32,
    total_received: u32,
}

impl HandSettlement {
    pub fn base_payment(self) -> Payment {
        self.base
    }

    pub fn honba(self) -> u32 {
        self.honba
    }

    pub fn honba_payments(self) -> HonbaPayments {
        self.honba_payments
    }

    pub fn honba_total(self) -> u32 {
        self.honba_total
    }

    pub fn kyotaku_points(self) -> u32 {
        self.kyotaku_points
    }

    pub fn total_received(self) -> u32 {
        self.total_received
    }

    pub fn settled_pay_ron(self) -> Option<u32> {
        Some(self.base.breakdown().pay_ron()? + self.honba_payments.pay_ron()?)
    }

    pub fn settled_pay_from_dealer(self) -> Option<u32> {
        Some(self.base.breakdown().pay_from_dealer()? + self.honba_payments.pay_from_each_payer()?)
    }

    pub fn settled_pay_from_non_dealer(self) -> Option<u32> {
        Some(
            self.base.breakdown().pay_from_non_dealer()?
                + self.honba_payments.pay_from_each_payer()?,
        )
    }
}

pub fn evaluate_hand_settlement(
    hand_value: &HandValue<'_>,
    honba: Option<u32>,
    kyotaku_points: Option<u32>,
) -> Result<HandSettlement, HandSettlementError> {
    let base = hand_value
        .payment()
        .ok_or(HandSettlementError::UnknownPayment)?;
    let missing = HandSettlementError::IncompleteTableState;
    let honba = honba.ok_or(missing(MissingSettlementFact::Honba))?;
    let kyotaku_points = kyotaku_points.ok_or(missing(MissingSettlementFact::KyotakuPoints))?;

    settle(base, honba, kyotaku_points)
}

fn settle(
    base: Payment,
    honba: u32,
    kyotaku_points: u32,
) -> Result<HandSettlement, HandSettlementError> {
    let overflow = || HandSettlementError::Overflow {
        honba,
        kyotaku_points,
    };
    let honba_payments = honba_payments(base.breakdown(), honba).ok_or_else(overflow)?;
    let honba_total = honba_payments.checked_total().ok_or_else(overflow)?;
    let total_received = base
        .total()
        .checked_add(honba_total)
        .and_then(|received| received.checked_add(kyotaku_points))
        .ok_or_else(overflow)?;

    Ok(HandSettlement {
        base,
        honba,
        honba_payments,
        honba_total,
        kyotaku_points,
        total_received,
    })
}

fn honba_payments(breakdown: PaymentBreakdown, honba: u32) -> Option<HonbaPayments> {
    match breakdown {
        PaymentBreakdown::Ron { .. } => Some(HonbaPayments::Ron {
            pay_ron: honba.checked_mul(HONBA_RON_POINTS)?,
        }),
        PaymentBreakdown::DealerTsumo { .. } | PaymentBreakdown::NonDealerTsumo { .. } => {
            Some(HonbaPayments::Tsumo {
                pay_from_each_payer: honba.checked_mul(HONBA_TSUMO_POINTS_PER_PAYER)?,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completed_hand::{CompletedHandAnalysis, analyze_completed_hand};
    use crate::hand_value::evaluate_hand_value;
    use crate::normal_hand_scoring::evaluate_normal_hand_scoring;
    use crate::tile::{TileId, TileType};
    use crate::winning_context::{RiichiStatus, WinMethod, WinningContext};

    struct TileIdSource {
        used: [bool; TileId::COUNT],
    }

    impl TileIdSource {
        fn new() -> Self {
            Self {
                used: [false; TileId::COUNT],
            }
        }

        fn tiles(&mut self, strings: &[&str]) -> Vec<TileId> {
            strings.iter().map(|s| self.tile(s)).collect()
        }

        fn tile(&mut self, s: &str) -> TileId {
            let tile_type = tile_type(s);
            let id = (0..4)
                .filter_map(|copy| TileId::new(tile_type.raw() * 4 + copy))
                .find(|id| !id.is_red() && !self.used[id.index()])
                .unwrap();
            self.used[id.index()] = true;
            id
        }
    }

    fn tile_type(s: &str) -> TileType {
        TileType::from_mjai_type_str(s).unwrap()
    }

    struct Setup {
        analysis: CompletedHandAnalysis,
    }

    impl Setup {
        fn new(concealed: &[&str]) -> Self {
            let mut source = TileIdSource::new();
            let tiles = source.tiles(concealed);
            Self {
                analysis: analyze_completed_hand(&tiles, &[]).unwrap(),
            }
        }

        fn hand_value(&self, context: WinningContext, winning_tile: &str) -> HandValue<'_> {
            evaluate_hand_value(&self.analysis, context, tile_type(winning_tile), &[], None)
                .unwrap()
                .into_known()
                .unwrap()
        }
    }

    fn known_context(win_method: WinMethod, seat_wind: &str) -> WinningContext {
        WinningContext::new(win_method)
            .with_round_wind(Some(tile_type("E")))
            .with_seat_wind(Some(tile_type(seat_wind)))
            .with_riichi(RiichiStatus::NotDeclared)
            .with_chankan(Some(false))
            .with_rinshan(Some(false))
            .with_remaining_live_tiles(Some(1))
    }

    fn non_dealer(win_method: WinMethod) -> WinningContext {
        known_context(win_method, "S")
    }

    fn dealer(win_method: WinMethod) -> WinningContext {
        known_context(win_method, "E")
    }

    fn settlement(hand_value: &HandValue<'_>, honba: u32, kyotaku_points: u32) -> HandSettlement {
        evaluate_hand_settlement(hand_value, Some(honba), Some(kyotaku_points)).unwrap()
    }

    const SANANKOU_AND_IIPEIKOU: [&str; 14] = [
        "7p", "7p", "7p", "1m", "2m", "3m", "6p", "6p", "6p", "8p", "8p", "8p", "3s", "3s",
    ];
    const TANYAO_SUUANKOU: [&str; 14] = [
        "2m", "2m", "2m", "3m", "3m", "3m", "4p", "4p", "4p", "5s", "5s", "5s", "6p", "6p",
    ];
    const KOKUSHI_HAND: [&str; 14] = [
        "1m", "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
    ];
    const PINFU_TANYAO_HAND: [&str; 14] = [
        "2m", "3m", "4m", "3m", "4m", "5m", "4p", "5p", "6p", "6p", "7p", "8p", "5s", "5s",
    ];

    #[test]
    fn a_ron_without_honba_and_kyotaku_settles_to_the_base_payment() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Ron), "1m");
        let result = settlement(&hand_value, 0, 0);

        assert_eq!(result.base_payment().total(), 3200);
        assert_eq!(result.honba(), 0);
        assert_eq!(result.honba_payments(), HonbaPayments::Ron { pay_ron: 0 });
        assert_eq!(result.honba_total(), 0);
        assert_eq!(result.kyotaku_points(), 0);
        assert_eq!(result.settled_pay_ron(), Some(3200));
        assert_eq!(result.total_received(), 3200);
    }

    #[test]
    fn a_ron_adds_three_hundred_points_per_honba_to_the_discarder() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Ron), "1m");

        for (honba, pay_ron, total_received) in [(1, 3500, 3500), (2, 3800, 3800), (5, 4700, 4700)]
        {
            let result = settlement(&hand_value, honba, 0);

            assert_eq!(result.honba(), honba);
            assert_eq!(
                result.honba_payments(),
                HonbaPayments::Ron {
                    pay_ron: honba * 300,
                },
                "honba: {honba}"
            );
            assert_eq!(result.honba_total(), honba * 300, "honba: {honba}");
            assert_eq!(result.settled_pay_ron(), Some(pay_ron), "honba: {honba}");
            assert_eq!(result.total_received(), total_received, "honba: {honba}");
        }
    }

    #[test]
    fn a_dealer_ron_adds_the_same_honba_points_as_a_non_dealer_ron() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let non_dealer_value = setup.hand_value(non_dealer(WinMethod::Ron), "1m");
        let dealer_value = setup.hand_value(dealer(WinMethod::Ron), "1m");

        let non_dealer_result = settlement(&non_dealer_value, 2, 0);
        let dealer_result = settlement(&dealer_value, 2, 0);

        assert_eq!(non_dealer_result.base_payment().total(), 3200);
        assert_eq!(dealer_result.base_payment().total(), 4800);
        assert_eq!(non_dealer_result.honba_total(), 600);
        assert_eq!(dealer_result.honba_total(), 600);
        assert_eq!(non_dealer_result.total_received(), 3800);
        assert_eq!(dealer_result.total_received(), 5400);
    }

    #[test]
    fn a_non_dealer_tsumo_adds_a_hundred_points_per_honba_to_every_payer() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Tsumo), "1m");
        let result = settlement(&hand_value, 2, 0);

        assert_eq!(
            result.base_payment().breakdown().pay_from_dealer(),
            Some(2600)
        );
        assert_eq!(
            result.base_payment().breakdown().pay_from_non_dealer(),
            Some(1300)
        );
        assert_eq!(
            result.honba_payments(),
            HonbaPayments::Tsumo {
                pay_from_each_payer: 200,
            }
        );
        assert_eq!(result.settled_pay_from_dealer(), Some(2800));
        assert_eq!(result.settled_pay_from_non_dealer(), Some(1500));
        assert_eq!(result.honba_total(), 600);
        assert_eq!(result.total_received(), 5800);
    }

    #[test]
    fn a_dealer_tsumo_adds_a_hundred_points_per_honba_to_every_non_dealer() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(dealer(WinMethod::Tsumo), "1m");
        let result = settlement(&hand_value, 3, 0);

        assert_eq!(
            result.base_payment().breakdown().pay_from_non_dealer(),
            Some(2600)
        );
        assert_eq!(
            result.honba_payments(),
            HonbaPayments::Tsumo {
                pay_from_each_payer: 300,
            }
        );
        assert_eq!(result.settled_pay_from_dealer(), None);
        assert_eq!(result.settled_pay_from_non_dealer(), Some(2900));
        assert_eq!(result.honba_total(), 900);
        assert_eq!(result.total_received(), 8700);
    }

    #[test]
    fn a_tsumo_honba_is_three_hundred_points_in_total_per_honba() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);

        for context in [non_dealer(WinMethod::Tsumo), dealer(WinMethod::Tsumo)] {
            let hand_value = setup.hand_value(context, "1m");

            for honba in [0, 1, 2, 5] {
                let result = settlement(&hand_value, honba, 0);

                assert_eq!(result.honba_total(), honba * 300, "honba: {honba}");
                assert_eq!(
                    result.total_received(),
                    result.base_payment().total() + honba * 300,
                    "honba: {honba}"
                );
            }
        }
    }

    #[test]
    fn a_ron_and_a_tsumo_receive_the_same_honba_total() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let ron = setup.hand_value(non_dealer(WinMethod::Ron), "1m");
        let tsumo = setup.hand_value(non_dealer(WinMethod::Tsumo), "1m");

        for honba in [0, 1, 4] {
            assert_eq!(
                settlement(&ron, honba, 0).honba_total(),
                settlement(&tsumo, honba, 0).honba_total(),
                "honba: {honba}"
            );
        }
    }

    #[test]
    fn the_kyotaku_points_are_added_to_the_received_total_only() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Ron), "1m");

        for kyotaku_points in [0, 1000, 2000, 3000] {
            let result = settlement(&hand_value, 0, kyotaku_points);

            assert_eq!(
                result.kyotaku_points(),
                kyotaku_points,
                "kyotaku: {kyotaku_points}"
            );
            assert_eq!(
                result.settled_pay_ron(),
                Some(3200),
                "kyotaku: {kyotaku_points}"
            );
            assert_eq!(
                result.total_received(),
                3200 + kyotaku_points,
                "kyotaku: {kyotaku_points}"
            );
        }
    }

    #[test]
    fn the_kyotaku_points_are_not_collected_from_a_tsumo_payer() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Tsumo), "1m");
        let result = settlement(&hand_value, 0, 2000);

        assert_eq!(result.settled_pay_from_dealer(), Some(2600));
        assert_eq!(result.settled_pay_from_non_dealer(), Some(1300));
        assert_eq!(result.honba_total(), 0);
        assert_eq!(result.total_received(), 5200 + 2000);
    }

    #[test]
    fn the_honba_and_the_kyotaku_points_apply_together() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let ron = setup.hand_value(non_dealer(WinMethod::Ron), "1m");
        let tsumo = setup.hand_value(non_dealer(WinMethod::Tsumo), "1m");

        let ron_result = settlement(&ron, 2, 1000);
        let tsumo_result = settlement(&tsumo, 2, 1000);

        assert_eq!(ron_result.settled_pay_ron(), Some(3800));
        assert_eq!(ron_result.total_received(), 3200 + 600 + 1000);
        assert_eq!(tsumo_result.settled_pay_from_dealer(), Some(2800));
        assert_eq!(tsumo_result.settled_pay_from_non_dealer(), Some(1500));
        assert_eq!(tsumo_result.total_received(), 5200 + 600 + 1000);
    }

    #[test]
    fn a_yakuman_ron_settles_with_the_same_honba_and_kyotaku_semantics() {
        let setup = Setup::new(&KOKUSHI_HAND);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Ron), "9m");
        let result = settlement(&hand_value, 3, 1000);

        assert!(hand_value.is_yakuman());
        assert_eq!(result.base_payment().total(), 32000);
        assert_eq!(result.honba_payments(), HonbaPayments::Ron { pay_ron: 900 });
        assert_eq!(result.settled_pay_ron(), Some(32900));
        assert_eq!(result.total_received(), 32000 + 900 + 1000);
    }

    #[test]
    fn a_yakuman_tsumo_settles_with_the_same_honba_and_kyotaku_semantics() {
        let setup = Setup::new(&TANYAO_SUUANKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Tsumo), "5s");
        let result = settlement(&hand_value, 1, 2000);

        assert!(hand_value.is_yakuman());
        assert_eq!(result.base_payment().total(), 32000);
        assert_eq!(
            result.honba_payments(),
            HonbaPayments::Tsumo {
                pay_from_each_payer: 100,
            }
        );
        assert_eq!(result.settled_pay_from_dealer(), Some(16100));
        assert_eq!(result.settled_pay_from_non_dealer(), Some(8100));
        assert_eq!(result.total_received(), 32000 + 300 + 2000);
    }

    #[test]
    fn an_unknown_honba_is_not_treated_as_zero() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Ron), "1m");

        assert_eq!(
            evaluate_hand_settlement(&hand_value, None, Some(0)),
            Err(HandSettlementError::IncompleteTableState(
                MissingSettlementFact::Honba
            ))
        );
        assert_eq!(
            evaluate_hand_settlement(&hand_value, None, None),
            Err(HandSettlementError::IncompleteTableState(
                MissingSettlementFact::Honba
            ))
        );
        assert!(evaluate_hand_settlement(&hand_value, Some(0), Some(0)).is_ok());
    }

    #[test]
    fn an_unknown_kyotaku_is_not_treated_as_zero() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Ron), "1m");

        assert_eq!(
            evaluate_hand_settlement(&hand_value, Some(1), None),
            Err(HandSettlementError::IncompleteTableState(
                MissingSettlementFact::KyotakuPoints
            ))
        );
        assert_eq!(
            evaluate_hand_settlement(&hand_value, Some(1), Some(0))
                .unwrap()
                .total_received(),
            3500
        );
    }

    #[test]
    fn an_observed_zero_differs_from_an_unknown_table_state() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Ron), "1m");

        assert_ne!(
            evaluate_hand_settlement(&hand_value, Some(0), Some(0)).map(|_| ()),
            evaluate_hand_settlement(&hand_value, None, Some(0)).map(|_| ())
        );
        assert_ne!(
            evaluate_hand_settlement(&hand_value, Some(0), Some(0)).map(|_| ()),
            evaluate_hand_settlement(&hand_value, Some(0), None).map(|_| ())
        );
    }

    #[test]
    fn a_hand_value_without_an_exact_payment_has_no_settlement() {
        let setup = Setup::new(&PINFU_TANYAO_HAND);
        let context = non_dealer(WinMethod::Ron)
            .with_riichi(RiichiStatus::Riichi)
            .with_ippatsu(Some(false));
        let candidates =
            evaluate_normal_hand_scoring(&setup.analysis, context, tile_type("2m"), &[], None)
                .unwrap();
        let hand_value = HandValue::Normal(candidates[0].clone());

        assert_eq!(hand_value.payment(), None);
        assert_eq!(
            evaluate_hand_settlement(&hand_value, Some(1), Some(1000)),
            Err(HandSettlementError::UnknownPayment)
        );
    }

    #[test]
    fn the_base_payment_keeps_its_own_semantics() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);

        for context in [
            non_dealer(WinMethod::Ron),
            non_dealer(WinMethod::Tsumo),
            dealer(WinMethod::Ron),
            dealer(WinMethod::Tsumo),
        ] {
            let hand_value = setup.hand_value(context, "1m");
            let base = hand_value.payment().unwrap();

            for honba in [0, 1, 3] {
                for kyotaku_points in [0, 1000, 3000] {
                    let result = settlement(&hand_value, honba, kyotaku_points);

                    assert_eq!(
                        result.base_payment(),
                        base,
                        "honba: {honba}, kyotaku: {kyotaku_points}"
                    );
                    assert_eq!(
                        result.base_payment().total(),
                        base.total(),
                        "honba: {honba}, kyotaku: {kyotaku_points}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_ron_settlement_has_no_tsumo_payer() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Ron), "1m");
        let result = settlement(&hand_value, 1, 1000);

        assert_eq!(result.honba_payments().pay_from_each_payer(), None);
        assert_eq!(result.settled_pay_from_dealer(), None);
        assert_eq!(result.settled_pay_from_non_dealer(), None);
    }

    #[test]
    fn a_tsumo_settlement_has_no_ron_payer() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Tsumo), "1m");
        let result = settlement(&hand_value, 1, 1000);

        assert_eq!(result.honba_payments().pay_ron(), None);
        assert_eq!(result.settled_pay_ron(), None);
    }

    #[test]
    fn rejects_a_honba_that_overflows_the_settlement() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);

        for context in [non_dealer(WinMethod::Ron), non_dealer(WinMethod::Tsumo)] {
            let hand_value = setup.hand_value(context, "1m");

            assert_eq!(
                evaluate_hand_settlement(&hand_value, Some(u32::MAX), Some(0)),
                Err(HandSettlementError::Overflow {
                    honba: u32::MAX,
                    kyotaku_points: 0,
                })
            );
        }
    }

    #[test]
    fn rejects_kyotaku_points_that_overflow_the_settlement() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);
        let hand_value = setup.hand_value(non_dealer(WinMethod::Ron), "1m");

        assert_eq!(
            evaluate_hand_settlement(&hand_value, Some(1), Some(u32::MAX)),
            Err(HandSettlementError::Overflow {
                honba: 1,
                kyotaku_points: u32::MAX,
            })
        );
    }

    #[test]
    fn settlements_are_deterministic() {
        let setup = Setup::new(&SANANKOU_AND_IIPEIKOU);

        for context in [
            non_dealer(WinMethod::Ron),
            non_dealer(WinMethod::Tsumo),
            dealer(WinMethod::Ron),
            dealer(WinMethod::Tsumo),
        ] {
            let hand_value = setup.hand_value(context, "1m");

            for honba in [0, 1, 7] {
                for kyotaku_points in [0, 1000, 4000] {
                    assert_eq!(
                        evaluate_hand_settlement(&hand_value, Some(honba), Some(kyotaku_points)),
                        evaluate_hand_settlement(&hand_value, Some(honba), Some(kyotaku_points)),
                        "honba: {honba}, kyotaku: {kyotaku_points}"
                    );
                }
            }
        }
    }
}
