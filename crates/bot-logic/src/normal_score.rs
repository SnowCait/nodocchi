use thiserror::Error;

const MIN_FU: u8 = 20;
const CHIITOITSU_FU: u8 = 25;
const FU_ROUNDING_UNIT: u8 = 10;

const MANGAN_BASIC_POINTS: u32 = 2000;
const HANEMAN_BASIC_POINTS: u32 = 3000;
const BAIMAN_BASIC_POINTS: u32 = 4000;
const SANBAIMAN_BASIC_POINTS: u32 = 6000;
const KAZOE_YAKUMAN_BASIC_POINTS: u32 = 8000;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum NormalScoreError {
    #[error("no han")]
    NoHan,

    #[error("not a final fu: {0}")]
    InvalidFu(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LimitClass {
    NoLimit,
    Mangan,
    Haneman,
    Baiman,
    Sanbaiman,
    KazoeYakuman,
}

impl LimitClass {
    pub fn is_limit(self) -> bool {
        !matches!(self, Self::NoLimit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NormalScoreBase {
    han: u8,
    fu: u8,
    basic_points: u32,
    limit: LimitClass,
}

impl NormalScoreBase {
    pub fn han(self) -> u8 {
        self.han
    }

    pub fn fu(self) -> u8 {
        self.fu
    }

    pub fn basic_points(self) -> u32 {
        self.basic_points
    }

    pub fn limit(self) -> LimitClass {
        self.limit
    }
}

pub fn evaluate_normal_score_base(han: u8, fu: u8) -> Result<NormalScoreBase, NormalScoreError> {
    if han == 0 {
        return Err(NormalScoreError::NoHan);
    }
    if !is_final_fu(fu) {
        return Err(NormalScoreError::InvalidFu(fu));
    }

    let (limit, basic_points) =
        limit_by_han(han).unwrap_or_else(|| calculated_basic_points(han, fu));

    Ok(NormalScoreBase {
        han,
        fu,
        basic_points,
        limit,
    })
}

fn limit_by_han(han: u8) -> Option<(LimitClass, u32)> {
    match han {
        0..=4 => None,
        5 => Some((LimitClass::Mangan, MANGAN_BASIC_POINTS)),
        6 | 7 => Some((LimitClass::Haneman, HANEMAN_BASIC_POINTS)),
        8..=10 => Some((LimitClass::Baiman, BAIMAN_BASIC_POINTS)),
        11 | 12 => Some((LimitClass::Sanbaiman, SANBAIMAN_BASIC_POINTS)),
        _ => Some((LimitClass::KazoeYakuman, KAZOE_YAKUMAN_BASIC_POINTS)),
    }
}

fn calculated_basic_points(han: u8, fu: u8) -> (LimitClass, u32) {
    let basic_points = u32::from(fu) * 2u32.pow(u32::from(han) + 2);
    if basic_points > MANGAN_BASIC_POINTS {
        (LimitClass::Mangan, MANGAN_BASIC_POINTS)
    } else {
        (LimitClass::NoLimit, basic_points)
    }
}

fn is_final_fu(fu: u8) -> bool {
    fu == CHIITOITSU_FU || (fu >= MIN_FU && fu.is_multiple_of(FU_ROUNDING_UNIT))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(han: u8, fu: u8) -> NormalScoreBase {
        evaluate_normal_score_base(han, fu).unwrap()
    }

    fn result(han: u8, fu: u8) -> (u32, LimitClass) {
        let score = score(han, fu);
        (score.basic_points(), score.limit())
    }

    #[test]
    fn keeps_the_input_han_and_fu() {
        let score = score(3, 40);

        assert_eq!((score.han(), score.fu()), (3, 40));
    }

    #[test]
    fn calculates_basic_points_below_the_limit() {
        assert_eq!(result(1, 20), (160, LimitClass::NoLimit));
        assert_eq!(result(1, 30), (240, LimitClass::NoLimit));
        assert_eq!(result(2, 25), (400, LimitClass::NoLimit));
        assert_eq!(result(2, 30), (480, LimitClass::NoLimit));
        assert_eq!(result(3, 40), (1280, LimitClass::NoLimit));
        assert_eq!(result(4, 30), (1920, LimitClass::NoLimit));
        assert_eq!(result(3, 60), (1920, LimitClass::NoLimit));
    }

    #[test]
    fn caps_calculated_basic_points_at_mangan() {
        assert_eq!(result(4, 40), (2000, LimitClass::Mangan));
        assert_eq!(result(3, 70), (2000, LimitClass::Mangan));
    }

    #[test]
    fn five_han_is_mangan_regardless_of_fu() {
        for fu in [20, 25, 30, 110] {
            assert_eq!(result(5, fu), (2000, LimitClass::Mangan), "fu: {fu}");
        }
    }

    #[test]
    fn six_and_seven_han_are_haneman() {
        assert_eq!(result(6, 30), (3000, LimitClass::Haneman));
        assert_eq!(result(7, 30), (3000, LimitClass::Haneman));
    }

    #[test]
    fn eight_to_ten_han_are_baiman() {
        assert_eq!(result(8, 30), (4000, LimitClass::Baiman));
        assert_eq!(result(9, 30), (4000, LimitClass::Baiman));
        assert_eq!(result(10, 30), (4000, LimitClass::Baiman));
    }

    #[test]
    fn eleven_and_twelve_han_are_sanbaiman() {
        assert_eq!(result(11, 30), (6000, LimitClass::Sanbaiman));
        assert_eq!(result(12, 30), (6000, LimitClass::Sanbaiman));
    }

    #[test]
    fn thirteen_or_more_han_is_a_single_kazoe_yakuman() {
        for han in [13, 14, 26, 39, u8::MAX] {
            assert_eq!(
                result(han, 30),
                (8000, LimitClass::KazoeYakuman),
                "han: {han}"
            );
        }
    }

    #[test]
    fn limits_above_mangan_do_not_depend_on_fu() {
        for han in [6, 8, 11, 13] {
            for fu in [20, 25, 30, 110] {
                assert_eq!(result(han, fu), result(han, 30), "han: {han}, fu: {fu}");
            }
        }
    }

    #[test]
    fn does_not_use_kiriage_mangan() {
        assert_eq!(result(4, 30), (1920, LimitClass::NoLimit));
        assert_eq!(result(3, 60), (1920, LimitClass::NoLimit));
    }

    #[test]
    fn keeps_chiitoitsu_fu_at_twenty_five() {
        assert_eq!(result(2, 25), (400, LimitClass::NoLimit));
        assert_eq!(result(3, 25), (800, LimitClass::NoLimit));
        assert_eq!(result(4, 25), (1600, LimitClass::NoLimit));
    }

    #[test]
    fn keeps_pinfu_tsumo_fu_at_twenty() {
        assert_eq!(result(1, 20), (160, LimitClass::NoLimit));
        assert_eq!(result(2, 20), (320, LimitClass::NoLimit));
    }

    #[test]
    fn matches_the_riichienv_four_han_thirty_fu_fixture() {
        assert_eq!(result(4, 30), (1920, LimitClass::NoLimit));
    }

    #[test]
    fn more_han_is_not_always_more_basic_points() {
        let fewer_han = score(3, 70);
        let more_han = score(4, 30);

        assert!(more_han.han() > fewer_han.han());
        assert!(more_han.basic_points() < fewer_han.basic_points());
        assert_eq!(
            (fewer_han.basic_points(), fewer_han.limit()),
            (2000, LimitClass::Mangan)
        );
        assert_eq!(
            (more_han.basic_points(), more_han.limit()),
            (1920, LimitClass::NoLimit)
        );
    }

    #[test]
    fn no_han_is_not_a_valid_score() {
        for fu in [20, 25, 30, 110] {
            assert_eq!(
                evaluate_normal_score_base(0, fu),
                Err(NormalScoreError::NoHan)
            );
        }
    }

    #[test]
    fn rejects_fu_that_is_not_a_final_fu() {
        for fu in [0, 10, 19, 22, 24, 26, 32, 45] {
            assert_eq!(
                evaluate_normal_score_base(1, fu),
                Err(NormalScoreError::InvalidFu(fu))
            );
        }
    }

    #[test]
    fn accepts_the_fu_produced_by_the_fu_layer() {
        for fu in [20, 25, 30, 40, 50, 60, 70, 80, 90, 100, 110] {
            assert!(evaluate_normal_score_base(1, fu).is_ok(), "fu: {fu}");
        }
    }

    #[test]
    fn only_no_limit_is_not_a_limit() {
        assert!(!LimitClass::NoLimit.is_limit());

        for limit in [
            LimitClass::Mangan,
            LimitClass::Haneman,
            LimitClass::Baiman,
            LimitClass::Sanbaiman,
            LimitClass::KazoeYakuman,
        ] {
            assert!(limit.is_limit(), "limit: {limit:?}");
        }
    }

    #[test]
    fn evaluations_are_deterministic() {
        for han in 1..=u8::MAX {
            for fu in [20, 25, 30, 40, 110] {
                assert_eq!(
                    evaluate_normal_score_base(han, fu),
                    evaluate_normal_score_base(han, fu),
                    "han: {han}, fu: {fu}"
                );
            }
        }
    }
}
