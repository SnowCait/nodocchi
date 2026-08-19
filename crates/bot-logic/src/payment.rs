use thiserror::Error;

use crate::winning_context::WinMethod;

const PAYMENT_UNIT: u32 = 100;

const DEALER_RON_MULTIPLIER: u32 = 6;
const NON_DEALER_RON_MULTIPLIER: u32 = 4;
const DEALER_PAYER_TSUMO_MULTIPLIER: u32 = 2;
const NON_DEALER_PAYER_TSUMO_MULTIPLIER: u32 = 1;

const NON_DEALER_COUNT: u32 = 3;
const OTHER_NON_DEALER_COUNT: u32 = NON_DEALER_COUNT - 1;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PaymentError {
    #[error("basic points overflow the payment: {0}")]
    BasicPointsOverflow(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaymentBreakdown {
    Ron {
        pay_ron: u32,
    },
    DealerTsumo {
        pay_from_non_dealer: u32,
    },
    NonDealerTsumo {
        pay_from_dealer: u32,
        pay_from_non_dealer: u32,
    },
}

impl PaymentBreakdown {
    pub fn pay_ron(self) -> Option<u32> {
        match self {
            Self::Ron { pay_ron } => Some(pay_ron),
            Self::DealerTsumo { .. } | Self::NonDealerTsumo { .. } => None,
        }
    }

    pub fn pay_from_dealer(self) -> Option<u32> {
        match self {
            Self::NonDealerTsumo {
                pay_from_dealer, ..
            } => Some(pay_from_dealer),
            Self::Ron { .. } | Self::DealerTsumo { .. } => None,
        }
    }

    pub fn pay_from_non_dealer(self) -> Option<u32> {
        match self {
            Self::DealerTsumo {
                pay_from_non_dealer,
            }
            | Self::NonDealerTsumo {
                pay_from_non_dealer,
                ..
            } => Some(pay_from_non_dealer),
            Self::Ron { .. } => None,
        }
    }

    fn checked_total(self) -> Option<u32> {
        match self {
            Self::Ron { pay_ron } => Some(pay_ron),
            Self::DealerTsumo {
                pay_from_non_dealer,
            } => pay_from_non_dealer.checked_mul(NON_DEALER_COUNT),
            Self::NonDealerTsumo {
                pay_from_dealer,
                pay_from_non_dealer,
            } => pay_from_non_dealer
                .checked_mul(OTHER_NON_DEALER_COUNT)
                .and_then(|non_dealers| pay_from_dealer.checked_add(non_dealers)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Payment {
    basic_points: u32,
    is_dealer: bool,
    breakdown: PaymentBreakdown,
    total: u32,
}

impl Payment {
    pub fn basic_points(self) -> u32 {
        self.basic_points
    }

    pub fn is_dealer(self) -> bool {
        self.is_dealer
    }

    pub fn breakdown(self) -> PaymentBreakdown {
        self.breakdown
    }

    pub fn total(self) -> u32 {
        self.total
    }
}

pub fn evaluate_payment(
    basic_points: u32,
    is_dealer: bool,
    win_method: WinMethod,
) -> Result<Payment, PaymentError> {
    let overflow = || PaymentError::BasicPointsOverflow(basic_points);
    let payment = |multiplier| checked_payment(basic_points, multiplier).ok_or_else(overflow);

    let breakdown = match (win_method, is_dealer) {
        (WinMethod::Ron, true) => PaymentBreakdown::Ron {
            pay_ron: payment(DEALER_RON_MULTIPLIER)?,
        },
        (WinMethod::Ron, false) => PaymentBreakdown::Ron {
            pay_ron: payment(NON_DEALER_RON_MULTIPLIER)?,
        },
        (WinMethod::Tsumo, true) => PaymentBreakdown::DealerTsumo {
            pay_from_non_dealer: payment(DEALER_PAYER_TSUMO_MULTIPLIER)?,
        },
        (WinMethod::Tsumo, false) => PaymentBreakdown::NonDealerTsumo {
            pay_from_dealer: payment(DEALER_PAYER_TSUMO_MULTIPLIER)?,
            pay_from_non_dealer: payment(NON_DEALER_PAYER_TSUMO_MULTIPLIER)?,
        },
    };

    Ok(Payment {
        basic_points,
        is_dealer,
        breakdown,
        total: breakdown.checked_total().ok_or_else(overflow)?,
    })
}

fn checked_payment(basic_points: u32, multiplier: u32) -> Option<u32> {
    basic_points
        .checked_mul(multiplier)
        .and_then(ceil_to_payment_unit)
}

fn ceil_to_payment_unit(points: u32) -> Option<u32> {
    points.div_ceil(PAYMENT_UNIT).checked_mul(PAYMENT_UNIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normal_score::evaluate_normal_score_base;

    fn payment(basic_points: u32, is_dealer: bool, win_method: WinMethod) -> Payment {
        evaluate_payment(basic_points, is_dealer, win_method).unwrap()
    }

    fn ron(basic_points: u32, is_dealer: bool) -> Payment {
        payment(basic_points, is_dealer, WinMethod::Ron)
    }

    fn tsumo(basic_points: u32, is_dealer: bool) -> Payment {
        payment(basic_points, is_dealer, WinMethod::Tsumo)
    }

    #[test]
    fn non_dealer_ron_is_four_times_the_basic_points() {
        for (basic_points, pay_ron) in [
            (240, 1000),
            (1280, 5200),
            (2000, 8000),
            (3000, 12000),
            (8000, 32000),
        ] {
            let result = ron(basic_points, false);

            assert_eq!(
                result.breakdown(),
                PaymentBreakdown::Ron { pay_ron },
                "basic points: {basic_points}"
            );
            assert_eq!(result.total(), pay_ron, "basic points: {basic_points}");
        }
    }

    #[test]
    fn dealer_ron_is_six_times_the_basic_points() {
        for (basic_points, pay_ron) in [
            (240, 1500),
            (1280, 7700),
            (2000, 12000),
            (3000, 18000),
            (8000, 48000),
        ] {
            let result = ron(basic_points, true);

            assert_eq!(
                result.breakdown(),
                PaymentBreakdown::Ron { pay_ron },
                "basic points: {basic_points}"
            );
            assert_eq!(result.total(), pay_ron, "basic points: {basic_points}");
        }
    }

    #[test]
    fn non_dealer_tsumo_doubles_only_the_dealer_payment() {
        for (basic_points, pay_from_dealer, pay_from_non_dealer, total) in [
            (240, 500, 300, 1100),
            (1280, 2600, 1300, 5200),
            (2000, 4000, 2000, 8000),
            (3000, 6000, 3000, 12000),
            (8000, 16000, 8000, 32000),
        ] {
            let result = tsumo(basic_points, false);

            assert_eq!(
                result.breakdown(),
                PaymentBreakdown::NonDealerTsumo {
                    pay_from_dealer,
                    pay_from_non_dealer,
                },
                "basic points: {basic_points}"
            );
            assert_eq!(result.total(), total, "basic points: {basic_points}");
        }
    }

    #[test]
    fn dealer_tsumo_doubles_every_non_dealer_payment() {
        for (basic_points, pay_from_non_dealer, total) in [
            (240, 500, 1500),
            (1280, 2600, 7800),
            (2000, 4000, 12000),
            (3000, 6000, 18000),
            (8000, 16000, 48000),
        ] {
            let result = tsumo(basic_points, true);

            assert_eq!(
                result.breakdown(),
                PaymentBreakdown::DealerTsumo {
                    pay_from_non_dealer,
                },
                "basic points: {basic_points}"
            );
            assert_eq!(result.total(), total, "basic points: {basic_points}");
        }
    }

    #[test]
    fn rounds_each_payment_up_to_a_hundred() {
        assert_eq!(ron(240, false).total(), 1000);
        assert_eq!(ron(240, true).total(), 1500);
        assert_eq!(ron(1280, false).total(), 5200);
        assert_eq!(ron(1280, true).total(), 7700);

        let result = tsumo(1280, false);

        assert_eq!(result.breakdown().pay_from_dealer(), Some(2600));
        assert_eq!(result.breakdown().pay_from_non_dealer(), Some(1300));
    }

    #[test]
    fn rounds_every_payer_before_summing_the_total() {
        assert_eq!(tsumo(240, false).total(), 1100);
        assert_eq!(tsumo(1280, true).total(), 7800);

        assert_ne!(tsumo(240, false).total(), ron(240, false).total());
        assert_ne!(tsumo(1280, true).total(), ron(1280, true).total());
    }

    #[test]
    fn keeps_payments_that_are_already_a_multiple_of_a_hundred() {
        assert_eq!(ron(2000, false).total(), 8000);
        assert_eq!(ron(2000, true).total(), 12000);
        assert_eq!(
            tsumo(2000, false).breakdown(),
            PaymentBreakdown::NonDealerTsumo {
                pay_from_dealer: 4000,
                pay_from_non_dealer: 2000,
            }
        );
        assert_eq!(
            tsumo(2000, true).breakdown(),
            PaymentBreakdown::DealerTsumo {
                pay_from_non_dealer: 4000,
            }
        );
    }

    #[test]
    fn ron_has_no_tsumo_payer() {
        for (is_dealer, pay_ron) in [(false, 5200), (true, 7700)] {
            let breakdown = ron(1280, is_dealer).breakdown();

            assert_eq!(breakdown.pay_ron(), Some(pay_ron), "dealer: {is_dealer}");
            assert_eq!(breakdown.pay_from_dealer(), None, "dealer: {is_dealer}");
            assert_eq!(breakdown.pay_from_non_dealer(), None, "dealer: {is_dealer}");
        }
    }

    #[test]
    fn tsumo_has_no_ron_payment() {
        for is_dealer in [false, true] {
            assert_eq!(
                tsumo(1280, is_dealer).breakdown().pay_ron(),
                None,
                "dealer: {is_dealer}"
            );
        }
    }

    #[test]
    fn a_dealer_tsumo_is_not_paid_by_a_dealer() {
        assert_eq!(tsumo(1280, true).breakdown().pay_from_dealer(), None);
        assert_eq!(tsumo(1280, false).breakdown().pay_from_dealer(), Some(2600));
    }

    #[test]
    fn keeps_the_input_basic_points_and_dealer_fact() {
        for is_dealer in [false, true] {
            for win_method in [WinMethod::Ron, WinMethod::Tsumo] {
                let result = payment(1280, is_dealer, win_method);

                assert_eq!(result.basic_points(), 1280);
                assert_eq!(result.is_dealer(), is_dealer);
            }
        }
    }

    #[test]
    fn treats_basic_points_above_the_normal_hand_range_the_same_way() {
        assert_eq!(ron(16000, false).total(), 64000);
        assert_eq!(ron(16000, true).total(), 96000);
        assert_eq!(tsumo(16000, false).total(), 64000);
        assert_eq!(tsumo(16000, true).total(), 96000);
    }

    #[test]
    fn composes_with_the_basic_points_of_the_normal_score_base() {
        for (han, fu, non_dealer_ron, dealer_ron) in [
            (1, 30, 1000, 1500),
            (3, 40, 5200, 7700),
            (5, 30, 8000, 12000),
            (13, 30, 32000, 48000),
        ] {
            let basic_points = evaluate_normal_score_base(han, fu).unwrap().basic_points();

            assert_eq!(
                ron(basic_points, false).total(),
                non_dealer_ron,
                "han: {han}, fu: {fu}"
            );
            assert_eq!(
                ron(basic_points, true).total(),
                dealer_ron,
                "han: {han}, fu: {fu}"
            );
        }
    }

    #[test]
    fn rejects_basic_points_that_overflow_a_payment() {
        for is_dealer in [false, true] {
            for win_method in [WinMethod::Ron, WinMethod::Tsumo] {
                assert_eq!(
                    evaluate_payment(u32::MAX, is_dealer, win_method),
                    Err(PaymentError::BasicPointsOverflow(u32::MAX)),
                    "dealer: {is_dealer}, win method: {win_method:?}"
                );
            }
        }
    }

    #[test]
    fn rejects_basic_points_whose_total_overflows() {
        let basic_points = 1_000_000_000;

        assert_eq!(
            evaluate_payment(basic_points, true, WinMethod::Tsumo),
            Err(PaymentError::BasicPointsOverflow(basic_points))
        );
    }

    #[test]
    fn evaluations_are_deterministic() {
        for basic_points in [0, 160, 240, 1280, 2000, 8000, 1_000_000_000, u32::MAX] {
            for is_dealer in [false, true] {
                for win_method in [WinMethod::Ron, WinMethod::Tsumo] {
                    assert_eq!(
                        evaluate_payment(basic_points, is_dealer, win_method),
                        evaluate_payment(basic_points, is_dealer, win_method),
                        "basic points: {basic_points}, dealer: {is_dealer}"
                    );
                }
            }
        }
    }
}
