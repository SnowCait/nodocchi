//! capture 全 request の self-tsumo soft horizon 12 / 14 / 16 / 18 の production 判断比較。
//!
//! 各 request は scenario / context / legal actions をそのままに、soft horizon だけを差し替えて
//! production と同じ判断経路を通す ([`compare_self_tsumo_horizons`])。どの horizon が正しいかは
//! 判定せず、horizon を変えた場合に production 判断がどの程度・どの局面で変わるかを数える。
//! primary metric は最終 action、secondary metric は通常打牌選択の一致。
//!
//! 将来自摸機会は、通常打牌後・Call 後の baseline `floor(remaining_tiles / 4)` と、Chi / Pon への
//! 反応 request で鳴き判断が評価する Pass 側の値を分けて扱う。局面の時期の bucket は baseline で
//! 1 request = 1 bucket に分類し、Pass 側の値は差分 request の詳細で並べて表示する。

use std::collections::BTreeMap;

use bot_core::{
    AgentActionSource, COMPARED_SELF_TSUMO_HORIZON_TURNS, COMPARED_SELF_TSUMO_HORIZONS,
    LegalAction, PassFutureDraws, PushPullOffenseState, SelfTsumoHorizonComparison,
    SelfTsumoHorizonDecision, compare_self_tsumo_horizons,
};

use crate::cli::CaptureComparisonSpec;
use crate::error::ScenarioError;
use crate::format::{action_label, format_self_tsumo_value};
use crate::replay::load_captured_scenarios;

const HORIZON_COUNT: usize = COMPARED_SELF_TSUMO_HORIZONS.len();
const NOT_EVALUATED: &str = "not evaluated";
const UNKNOWN: &str = "unknown";
const NOT_APPLICABLE: &str = "not applicable";

// 比較する horizon の全 pair。表示順は (12, 14), (12, 16), ... (16, 18)。
const HORIZON_PAIRS: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

/// baseline raw future own draws (`floor(remaining_tiles / 4)`) の集計 bucket。
///
/// 反応 request の Pass 側が使う自摸回数とは異なる場合があるが、分類には使わない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RawDrawBucket {
    UpTo2,
    ThreeToFour,
    FiveToSix,
    SevenToEight,
    NineToTen,
    ElevenOrMore,
    Unknown,
}

impl RawDrawBucket {
    pub const ALL: [Self; 7] = [
        Self::UpTo2,
        Self::ThreeToFour,
        Self::FiveToSix,
        Self::SevenToEight,
        Self::NineToTen,
        Self::ElevenOrMore,
        Self::Unknown,
    ];

    /// 残り山が unknown の request は推測せず [`Self::Unknown`] に入れる。
    pub fn of(raw_future_draws: Option<u32>) -> Self {
        match raw_future_draws {
            None => Self::Unknown,
            Some(0..=2) => Self::UpTo2,
            Some(3..=4) => Self::ThreeToFour,
            Some(5..=6) => Self::FiveToSix,
            Some(7..=8) => Self::SevenToEight,
            Some(9..=10) => Self::NineToTen,
            Some(_) => Self::ElevenOrMore,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::UpTo2 => "0-2",
            Self::ThreeToFour => "3-4",
            Self::FiveToSix => "5-6",
            Self::SevenToEight => "7-8",
            Self::NineToTen => "9-10",
            Self::ElevenOrMore => "11+",
            Self::Unknown => "unknown",
        }
    }
}

pub fn run_capture_comparison(spec: &CaptureComparisonSpec) -> Result<String, ScenarioError> {
    let mut requests = Vec::new();
    for path in &spec.paths {
        for captured in load_captured_scenarios(path)? {
            requests.push(ComparedRequest {
                capture: captured.path.clone(),
                request_id: captured.request_id,
                comparison: compare_self_tsumo_horizons(
                    &captured.scenario.context,
                    &captured.scenario.legal_actions,
                ),
            });
        }
    }
    Ok(format_capture_comparison(spec.paths.len(), &requests))
}

pub struct ComparedRequest {
    pub capture: String,
    pub request_id: u64,
    pub comparison: SelfTsumoHorizonComparison,
}

impl ComparedRequest {
    fn final_actions(&self) -> [&LegalAction; HORIZON_COUNT] {
        self.comparison
            .decisions
            .each_ref()
            .map(|decision| &decision.action)
    }

    fn normal_discards(&self) -> [Option<&LegalAction>; HORIZON_COUNT] {
        self.comparison
            .decisions
            .each_ref()
            .map(|decision| decision.normal_discard.as_ref())
    }

    fn final_actions_agree(&self) -> bool {
        self.comparison.all_final_actions_agree()
    }

    // 通常打牌選択を通らなかった horizon がある request は一致とも不一致とも数えない。
    fn normal_discards_agree(&self) -> Option<bool> {
        let discards = self.normal_discards();
        discards
            .iter()
            .all(Option::is_some)
            .then(|| self.comparison.all_normal_discards_agree())
    }

    fn differs(&self) -> bool {
        !self.final_actions_agree() || self.normal_discards_agree() == Some(false)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PairAgreement {
    pub same: usize,
    pub different: usize,
}

impl PairAgreement {
    fn count(&mut self, same: bool) {
        if same {
            self.same += 1;
        } else {
            self.different += 1;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BucketSummary {
    pub requests: usize,
    pub all_four_same: usize,
    pub not_all_same: usize,
    pub normal_discard_not_all_same: usize,
}

/// capture 全体の集計。最終 action を primary、通常打牌選択を secondary として数える。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HorizonComparisonSummary {
    pub requests: usize,
    pub known_baseline_raw_future_draws: usize,
    pub unknown_baseline_raw_future_draws: usize,
    /// Chi / Pon の反応 request で、Pass 側の自摸回数を production が確定できたもの。
    pub known_pass_raw_future_draws: usize,
    /// Chi / Pon の反応 request で、反応元不明などで Pass 側の自摸回数が unknown のもの。
    pub unknown_pass_raw_future_draws: usize,
    pub final_pairs: [PairAgreement; 6],
    pub final_all_four_same: usize,
    pub final_not_all_same: usize,
    /// 最終 action の horizon 間の分かれ方 ([`partition_label`]) ごとの件数。
    pub final_patterns: BTreeMap<String, usize>,
    /// 両方の horizon で通常打牌選択を通った request だけを数える。
    pub normal_discard_pairs: [PairAgreement; 6],
    pub normal_discard_all_four_same: usize,
    pub normal_discard_not_all_same: usize,
    /// どれかの horizon で通常打牌選択を通らなかった request。
    pub normal_discard_not_evaluated: usize,
    /// 最終 action は4つとも一致したが通常打牌選択は分かれた request。
    pub final_same_but_normal_discard_different: usize,
    pub buckets: BTreeMap<RawDrawBucket, BucketSummary>,
}

impl HorizonComparisonSummary {
    pub fn from_requests(requests: &[ComparedRequest]) -> Self {
        let mut summary = Self {
            requests: requests.len(),
            buckets: RawDrawBucket::ALL
                .into_iter()
                .map(|bucket| (bucket, BucketSummary::default()))
                .collect(),
            ..Self::default()
        };
        for request in requests {
            let raw = request.comparison.baseline_raw_future_draws;
            if raw.is_some() {
                summary.known_baseline_raw_future_draws += 1;
            } else {
                summary.unknown_baseline_raw_future_draws += 1;
            }
            match request.comparison.pass_raw_future_draws {
                PassFutureDraws::Known(_) => summary.known_pass_raw_future_draws += 1,
                PassFutureDraws::Unknown => summary.unknown_pass_raw_future_draws += 1,
                PassFutureDraws::NotApplicable => {}
            }

            let actions = request.final_actions();
            let discards = request.normal_discards();
            for (pair, &(left, right)) in HORIZON_PAIRS.iter().enumerate() {
                summary.final_pairs[pair].count(actions[left] == actions[right]);
                if let (Some(left), Some(right)) = (discards[left], discards[right]) {
                    summary.normal_discard_pairs[pair].count(left == right);
                }
            }

            let final_agree = request.final_actions_agree();
            if final_agree {
                summary.final_all_four_same += 1;
            } else {
                summary.final_not_all_same += 1;
            }
            *summary
                .final_patterns
                .entry(partition_label(&actions))
                .or_default() += 1;

            let normal_discard_agree = request.normal_discards_agree();
            match normal_discard_agree {
                Some(true) => summary.normal_discard_all_four_same += 1,
                Some(false) => summary.normal_discard_not_all_same += 1,
                None => summary.normal_discard_not_evaluated += 1,
            }
            if final_agree && normal_discard_agree == Some(false) {
                summary.final_same_but_normal_discard_different += 1;
            }

            let bucket = summary.buckets.entry(RawDrawBucket::of(raw)).or_default();
            bucket.requests += 1;
            if final_agree {
                bucket.all_four_same += 1;
            } else {
                bucket.not_all_same += 1;
            }
            if normal_discard_agree == Some(false) {
                bucket.normal_discard_not_all_same += 1;
            }
        }
        summary
    }
}

/// horizon 間で同じ値になった組を `=`、別の値の組を ` / ` でつないだ分かれ方。
///
/// 例えば `12=14=16=18` は全一致、`12 / 14=16=18` は 12 だけが違う、`12=14 / 16=18` は
/// 12・14 と 16・18 で分かれたことを表す。
pub fn partition_label<T: PartialEq>(values: &[T; HORIZON_COUNT]) -> String {
    let mut groups: Vec<(usize, Vec<u32>)> = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let turn = COMPARED_SELF_TSUMO_HORIZON_TURNS[index];
        match groups
            .iter_mut()
            .find(|(representative, _)| values[*representative] == *value)
        {
            Some((_, turns)) => turns.push(turn),
            None => groups.push((index, vec![turn])),
        }
    }
    groups
        .iter()
        .map(|(_, turns)| {
            turns
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join("=")
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

pub fn format_capture_comparison(captures: usize, requests: &[ComparedRequest]) -> String {
    let summary = HorizonComparisonSummary::from_requests(requests);
    let horizons = horizon_list();
    let mut lines = vec![
        "Self-tsumo soft horizon comparison over captures".to_string(),
        format!("  captures: {captures}"),
        format!(
            "  horizons: {horizons} (late minimum future draws: {})",
            COMPARED_SELF_TSUMO_HORIZONS[0].late_min_future_draws
        ),
        "  only the soft horizon changes; scenario, context and legal actions stay the same"
            .to_string(),
        "  each horizon runs the production decision path on its own fresh thread with cold memos"
            .to_string(),
        "  the one-shanten Push/Fold threshold value stays until-ryukyoku at every horizon"
            .to_string(),
        "  this observes how the production decision changes; it does not pick a correct horizon"
            .to_string(),
        format!("  requests evaluated: {}", summary.requests),
        "  baseline future draws are floor(remaining_tiles / 4) after a normal discard or a call;"
            .to_string(),
        "  the pass branch of a Chi / Pon reaction counts its own draws from the reaction source"
            .to_string(),
        format!(
            "  requests with known baseline raw future draws: {}",
            summary.known_baseline_raw_future_draws
        ),
        format!(
            "  requests with unknown baseline raw future draws: {}",
            summary.unknown_baseline_raw_future_draws
        ),
        format!(
            "  reaction requests with known pass raw future draws: {}",
            summary.known_pass_raw_future_draws
        ),
        format!(
            "  reaction requests with unknown pass raw future draws: {}",
            summary.unknown_pass_raw_future_draws
        ),
        String::new(),
        "Final action agreement (primary)".to_string(),
    ];
    lines.extend(format_pairs(&summary.final_pairs));
    lines.push(format!("  all four same: {}", summary.final_all_four_same));
    lines.push(format!("  not all same: {}", summary.final_not_all_same));
    lines.push("  patterns:".to_string());
    let mut patterns: Vec<_> = summary.final_patterns.iter().collect();
    patterns.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
    for (pattern, count) in patterns {
        lines.push(format!("    {pattern}: {count}"));
    }

    lines.push(String::new());
    lines.push("Normal discard selection agreement (secondary)".to_string());
    lines.push(
        "  a pair counts only the requests where both horizons ran the normal discard selection"
            .to_string(),
    );
    lines.extend(format_pairs(&summary.normal_discard_pairs));
    lines.push(format!(
        "  all four same: {}",
        summary.normal_discard_all_four_same
    ));
    lines.push(format!(
        "  not all same: {}",
        summary.normal_discard_not_all_same
    ));
    lines.push(format!(
        "  not evaluated at some horizon: {}",
        summary.normal_discard_not_evaluated
    ));
    lines.push(format!(
        "  final action all same but normal discard different: {}",
        summary.final_same_but_normal_discard_different
    ));

    lines.push(String::new());
    lines.push("By baseline raw future own draws".to_string());
    lines.push(
        "  floor(remaining_tiles / 4); the pass branch of a reaction may use a different count"
            .to_string(),
    );
    for (bucket, counts) in &summary.buckets {
        lines.push(format!(
            "  {}: requests {}, all four same {}, not all same {}, normal discard not all same {}",
            bucket.label(),
            counts.requests,
            counts.all_four_same,
            counts.not_all_same,
            counts.normal_discard_not_all_same,
        ));
    }

    let differing: Vec<_> = requests
        .iter()
        .filter(|request| request.differs())
        .collect();
    lines.push(String::new());
    lines.push(format!("Differing requests: {}", differing.len()));
    for request in differing {
        lines.extend(format_differing_request(request));
    }
    lines.join("\n")
}

fn horizon_list() -> String {
    COMPARED_SELF_TSUMO_HORIZON_TURNS
        .map(|turn| turn.to_string())
        .join(" / ")
}

fn format_pairs(pairs: &[PairAgreement; 6]) -> Vec<String> {
    HORIZON_PAIRS
        .iter()
        .zip(pairs)
        .map(|(&(left, right), agreement)| {
            format!(
                "  {} vs {}: same {}, different {}, agreement {}",
                COMPARED_SELF_TSUMO_HORIZON_TURNS[left],
                COMPARED_SELF_TSUMO_HORIZON_TURNS[right],
                agreement.same,
                agreement.different,
                format_percent(agreement.same, agreement.same + agreement.different),
            )
        })
        .collect()
}

fn format_differing_request(request: &ComparedRequest) -> Vec<String> {
    let comparison = &request.comparison;
    let mut lines = vec![
        format!("  {}  request_id={}", request.capture, request.request_id),
        format!(
            "    baseline raw future own draws: {}",
            format_count(comparison.baseline_raw_future_draws)
        ),
        format!(
            "    baseline effective future own draws: {}",
            format_per_horizon(comparison, |decision| format_count(
                decision.baseline_effective_future_draws
            ))
        ),
        format!(
            "    pass raw future own draws: {}",
            format_pass_draws(comparison.pass_raw_future_draws)
        ),
    ];
    // Pass が無い request では Pass 側の effective も無いので、行を増やさない。
    if comparison.pass_raw_future_draws != PassFutureDraws::NotApplicable {
        lines.push(format!(
            "    pass effective future own draws: {}",
            format_per_horizon(comparison, |decision| format_pass_draws(
                decision.pass_effective_future_draws
            ))
        ));
    }
    lines.extend([
        format!(
            "    final action: {}",
            partition_label(&request.final_actions())
        ),
        format!(
            "    normal discard selection: {}",
            partition_label(&request.normal_discards())
        ),
    ]);
    for decision in &comparison.decisions {
        lines.extend(format_decision(decision));
    }
    lines
}

fn format_decision(decision: &SelfTsumoHorizonDecision) -> Vec<String> {
    let mut lines = vec![
        format!("    h{}:", decision.horizon.horizon_turn),
        format!("      final action: {}", action_label(&decision.action)),
        format!(
            "      final action source: {}",
            source_label(decision.source)
        ),
        format!(
            "      push/pull: {}",
            decision
                .push_pull_mode
                .map(|mode| format!("{mode:?}"))
                .unwrap_or_else(|| NOT_EVALUATED.to_string())
        ),
    ];
    let Some(discard) = decision.normal_discard.as_ref() else {
        lines.push(format!("      normal discard selection: {NOT_EVALUATED}"));
        return lines;
    };
    lines.push(format!(
        "      normal discard selection: {}",
        action_label(discard)
    ));
    let Some(offense) = decision.offense.as_ref() else {
        lines.push(format!("      selected discard values: {UNKNOWN}"));
        return lines;
    };
    lines.extend(format_offense(offense));
    lines
}

// 通常打牌選択が押し引き入力へ渡した値そのもの。表示のために評価し直さない。
fn format_offense(offense: &PushPullOffenseState) -> Vec<String> {
    let metrics = offense.iishanten_forward_metrics;
    vec![
        format!(
            "      shanten after discard: {}",
            offense.min_shanten_after_discard
        ),
        format!(
            "      acceptance: {} remaining / {} types",
            offense.acceptance_total_remaining, offense.acceptance_type_count
        ),
        format!(
            "      ExpectedSelfTsumoValue (this horizon): {}",
            metrics.map_or_else(
                || NOT_EVALUATED.to_string(),
                |metrics| format_self_tsumo_value(metrics.expected_self_tsumo_value)
            )
        ),
        format!(
            "      weighted tenpai wait: {}",
            metrics.map_or_else(
                || NOT_EVALUATED.to_string(),
                |metrics| metrics.tenpai_wait.map_or_else(
                    || UNKNOWN.to_string(),
                    |wait| format!(
                        "{} remaining / {} types",
                        wait.weighted_remaining, wait.weighted_type_count
                    )
                )
            )
        ),
        format!(
            "      Push/Fold ExpectedSelfTsumoValue (until ryukyoku): {}",
            offense
                .iishanten_push_pull_expected_self_tsumo_value()
                .map_or_else(
                    || format!("{NOT_EVALUATED} / {UNKNOWN}"),
                    |value| format_self_tsumo_value(Some(value))
                )
        ),
    ]
}

// 防御 fallback の種別もそのまま出し、どの経路の差かを読めるようにする。
fn source_label(source: AgentActionSource) -> String {
    format!("{source:?}")
}

fn format_per_horizon(
    comparison: &SelfTsumoHorizonComparison,
    value: impl Fn(&SelfTsumoHorizonDecision) -> String,
) -> String {
    comparison
        .decisions
        .iter()
        .map(|decision| format!("h{}={}", decision.horizon.horizon_turn, value(decision)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_pass_draws(draws: PassFutureDraws) -> String {
    match draws {
        PassFutureDraws::NotApplicable => NOT_APPLICABLE.to_string(),
        PassFutureDraws::Unknown => UNKNOWN.to_string(),
        PassFutureDraws::Known(draws) => draws.to_string(),
    }
}

fn format_count(value: Option<u32>) -> String {
    value.map_or_else(|| UNKNOWN.to_string(), |value| value.to_string())
}

fn format_percent(part: usize, total: usize) -> String {
    if total == 0 {
        return "n/a".to_string();
    }
    format!("{:.1}%", part as f64 / total as f64 * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_core::{Agent, ShantenAgent};
    use bot_logic::SelfTsumoHorizon;
    use riichilab_client::capture::{self, CaptureDirection};
    use riichilab_client::observation::fixture_base64_with_winds_and_discards;
    use tempfile::TempDir;

    // 34567899m5799p3s + ツモ 4s。北家 (親は1番) の1向聴で、残り山によって horizon ごとの
    // 通常打牌が 9p と 5p の間で変わる synthetic な局面。
    const HAND: [u8; 13] = [8, 12, 17, 20, 24, 28, 32, 33, 53, 60, 68, 69, 80];
    const DRAWN: u8 = 84;
    const DORA_INDICATOR: u8 = 9;
    const ROUND_WIND_EAST: u8 = 0;
    const OYA: u8 = 1;

    // 手牌の受け入れに関わらない字牌・1p・2p・1s・7s・8s・9s。河の枚数で残り山を調整する。
    fn filler_tiles() -> Vec<u8> {
        (108..136)
            .chain(36..44)
            .chain(72..76)
            .chain(96..108)
            .collect()
    }

    // 残り山 = 136 - 王牌14 - 自分の手牌14 - 他家の手牌39 - 河。
    fn discards_for_remaining(remaining_tiles: u32) -> [Vec<u8>; 4] {
        let count = usize::try_from(69 - remaining_tiles).unwrap();
        assert!(count <= filler_tiles().len(), "河に置ける牌が足りない");
        let mut discards: [Vec<u8>; 4] = Default::default();
        for (index, tile) in filler_tiles().into_iter().take(count).enumerate() {
            discards[index % 4].push(tile);
        }
        discards
    }

    fn capture_line(request_id: u64, remaining_tiles: u32, riichi_declared: [bool; 4]) -> String {
        let observation = fixture_base64_with_winds_and_discards(
            0,
            Some(DRAWN),
            HAND.to_vec(),
            vec![DORA_INDICATOR],
            discards_for_remaining(remaining_tiles),
            riichi_declared,
            ROUND_WIND_EAST,
            OYA,
        );
        request_line(request_id, &HAND, DRAWN, &observation)
    }

    fn request_line(request_id: u64, hand: &[u8], drawn: u8, observation: &str) -> String {
        let possible: Vec<_> = hand
            .iter()
            .copied()
            .chain([drawn])
            .map(|id| {
                let pai = bot_logic::TileId::new(id).unwrap().to_mjai_string();
                format!(r#"{{"type":"dahai","pai":"{pai}","tsumogiri":false}}"#)
            })
            .collect();
        let line = capture::record_line(
            CaptureDirection::Server,
            &format!(
                r#"{{"type":"request_action","request_id":{request_id},"actor":0,"possible_actions":[{}],"observation":"{observation}"}}"#,
                possible.join(",")
            ),
        )
        .expect("capture 行を作れる");
        format!("{line}\n")
    }

    fn write_capture(directory: &TempDir, name: &str, lines: &[String]) -> String {
        let path = directory.path().join(name);
        std::fs::write(&path, lines.concat()).expect("capture を書ける");
        path.to_string_lossy().into_owned()
    }

    fn compared_requests(paths: &[String]) -> Vec<ComparedRequest> {
        paths
            .iter()
            .flat_map(|path| load_captured_scenarios(path).expect("capture を読める"))
            .map(|captured| ComparedRequest {
                capture: captured.path.clone(),
                request_id: captured.request_id,
                comparison: compare_self_tsumo_horizons(
                    &captured.scenario.context,
                    &captured.scenario.legal_actions,
                ),
            })
            .collect()
    }

    // 1 -> 1p, 2 -> 1s, 3 -> 東。
    fn dahai(kind: u8) -> LegalAction {
        LegalAction::Dahai {
            tile: bot_logic::TileId::new(kind * 36).unwrap(),
        }
    }

    // production の判断経路を通さずに集計だけを確かめるための horizon ごとの判断。
    fn synthetic_request(
        request_id: u64,
        raw_future_draws: Option<u32>,
        finals: [u8; 4],
        normal_discards: [Option<u8>; 4],
    ) -> ComparedRequest {
        synthetic_reaction(
            request_id,
            raw_future_draws,
            PassFutureDraws::NotApplicable,
            finals,
            normal_discards,
        )
    }

    // Pass 側の自摸回数も指定する。effective は production の soft horizon helper で作る。
    fn synthetic_reaction(
        request_id: u64,
        raw_future_draws: Option<u32>,
        pass_raw_future_draws: PassFutureDraws,
        finals: [u8; 4],
        normal_discards: [Option<u8>; 4],
    ) -> ComparedRequest {
        let decisions = [0, 1, 2, 3].map(|index| {
            let horizon = COMPARED_SELF_TSUMO_HORIZONS[index];
            SelfTsumoHorizonDecision {
                horizon,
                baseline_effective_future_draws: raw_future_draws
                    .map(|raw| horizon.effective_future_draws(raw)),
                pass_effective_future_draws: match pass_raw_future_draws {
                    PassFutureDraws::Known(raw) => {
                        PassFutureDraws::Known(horizon.effective_future_draws(raw))
                    }
                    other => other,
                },
                action: dahai(finals[index]),
                source: if normal_discards[index] == Some(finals[index]) {
                    AgentActionSource::NormalDiscard
                } else {
                    AgentActionSource::LegalDahaiFallback
                },
                normal_discard: normal_discards[index].map(dahai),
                push_pull_mode: None,
                offense: None,
            }
        });
        ComparedRequest {
            capture: format!("synthetic-{request_id}.jsonl"),
            request_id,
            comparison: SelfTsumoHorizonComparison {
                baseline_raw_future_draws: raw_future_draws,
                pass_raw_future_draws,
                decisions,
            },
        }
    }

    #[test]
    fn the_raw_future_draws_are_bucketed_without_guessing_an_unknown_wall() {
        for (raw, bucket) in [
            (Some(0), RawDrawBucket::UpTo2),
            (Some(2), RawDrawBucket::UpTo2),
            (Some(3), RawDrawBucket::ThreeToFour),
            (Some(4), RawDrawBucket::ThreeToFour),
            (Some(5), RawDrawBucket::FiveToSix),
            (Some(6), RawDrawBucket::FiveToSix),
            (Some(7), RawDrawBucket::SevenToEight),
            (Some(8), RawDrawBucket::SevenToEight),
            (Some(9), RawDrawBucket::NineToTen),
            (Some(10), RawDrawBucket::NineToTen),
            (Some(11), RawDrawBucket::ElevenOrMore),
            (Some(17), RawDrawBucket::ElevenOrMore),
            (None, RawDrawBucket::Unknown),
        ] {
            assert_eq!(RawDrawBucket::of(raw), bucket, "{raw:?}");
        }
    }

    #[test]
    fn the_partition_label_names_how_the_horizons_split() {
        assert_eq!(partition_label(&[1, 1, 1, 1]), "12=14=16=18");
        assert_eq!(partition_label(&[1, 2, 2, 2]), "12 / 14=16=18");
        assert_eq!(partition_label(&[1, 1, 2, 2]), "12=14 / 16=18");
        assert_eq!(partition_label(&[1, 1, 1, 2]), "12=14=16 / 18");
        assert_eq!(partition_label(&[1, 2, 1, 2]), "12=16 / 14=18");
        assert_eq!(partition_label(&[1, 2, 3, 4]), "12 / 14 / 16 / 18");
    }

    #[test]
    fn the_summary_counts_the_pairs_the_patterns_and_the_buckets() {
        let requests = [
            synthetic_request(1, Some(11), [1, 1, 1, 1], [Some(1); 4]),
            synthetic_request(
                2,
                Some(7),
                [1, 2, 2, 2],
                [Some(1), Some(2), Some(2), Some(2)],
            ),
            synthetic_request(
                3,
                Some(5),
                [1, 1, 2, 2],
                [Some(1), Some(1), Some(2), Some(2)],
            ),
            // 最終 action は同じ防御牌だが、通常打牌選択は horizon で分かれる。
            synthetic_request(
                4,
                Some(7),
                [3, 3, 3, 3],
                [Some(1), Some(2), Some(2), Some(2)],
            ),
            // 通常打牌選択を通らなかった horizon がある request は secondary では数えない。
            synthetic_request(5, None, [3, 3, 3, 3], [None, None, Some(2), Some(2)]),
        ];
        let summary = HorizonComparisonSummary::from_requests(&requests);

        assert_eq!(summary.requests, 5);
        assert_eq!(summary.known_baseline_raw_future_draws, 4);
        assert_eq!(summary.unknown_baseline_raw_future_draws, 1);
        let same_different =
            |pairs: &[PairAgreement; 6]| pairs.map(|pair| (pair.same, pair.different));
        // 12v14, 12v16, 12v18, 14v16, 14v18, 16v18
        assert_eq!(
            same_different(&summary.final_pairs),
            [(4, 1), (3, 2), (3, 2), (4, 1), (4, 1), (5, 0)]
        );
        assert_eq!(summary.final_all_four_same, 3);
        assert_eq!(summary.final_not_all_same, 2);
        assert_eq!(
            summary.final_patterns,
            BTreeMap::from([
                ("12=14=16=18".to_string(), 3),
                ("12 / 14=16=18".to_string(), 1),
                ("12=14 / 16=18".to_string(), 1),
            ])
        );

        assert_eq!(
            same_different(&summary.normal_discard_pairs),
            [(2, 2), (1, 3), (1, 3), (3, 1), (3, 1), (5, 0)]
        );
        assert_eq!(summary.normal_discard_all_four_same, 1);
        assert_eq!(summary.normal_discard_not_all_same, 3);
        assert_eq!(summary.normal_discard_not_evaluated, 1);
        assert_eq!(summary.final_same_but_normal_discard_different, 1);

        let bucket = |bucket| summary.buckets[&bucket];
        assert_eq!(
            bucket(RawDrawBucket::SevenToEight),
            BucketSummary {
                requests: 2,
                all_four_same: 1,
                not_all_same: 1,
                normal_discard_not_all_same: 2,
            }
        );
        assert_eq!(bucket(RawDrawBucket::FiveToSix).not_all_same, 1);
        assert_eq!(bucket(RawDrawBucket::ElevenOrMore).all_four_same, 1);
        assert_eq!(bucket(RawDrawBucket::Unknown).requests, 1);
        assert_eq!(bucket(RawDrawBucket::UpTo2).requests, 0);
        assert_eq!(summary.buckets.len(), RawDrawBucket::ALL.len());
    }

    #[test]
    fn the_buckets_follow_the_baseline_and_not_the_pass_draws() {
        // baseline 15 (floor(63 / 4)) と Pass 16 は同じ 11+ だが、baseline 10 と Pass 11 は
        // baseline の 9-10 だけに入り、Pass 側で別 bucket へ二重計上しない。
        let requests = [
            synthetic_reaction(
                1,
                Some(10),
                PassFutureDraws::Known(11),
                [1, 2, 2, 2],
                [None; 4],
            ),
            synthetic_reaction(2, Some(15), PassFutureDraws::Unknown, [1; 4], [None; 4]),
            synthetic_request(3, Some(10), [1; 4], [Some(1); 4]),
        ];
        let summary = HorizonComparisonSummary::from_requests(&requests);

        assert_eq!(summary.buckets[&RawDrawBucket::NineToTen].requests, 2);
        assert_eq!(summary.buckets[&RawDrawBucket::NineToTen].not_all_same, 1);
        assert_eq!(summary.buckets[&RawDrawBucket::ElevenOrMore].requests, 1);
        assert_eq!(
            summary
                .buckets
                .values()
                .map(|bucket| bucket.requests)
                .sum::<usize>(),
            requests.len()
        );
        assert_eq!(summary.known_baseline_raw_future_draws, 3);
        assert_eq!(summary.known_pass_raw_future_draws, 1);
        assert_eq!(summary.unknown_pass_raw_future_draws, 1);
    }

    #[test]
    fn a_differing_reaction_shows_the_baseline_and_the_pass_draws_apart() {
        let requests = [
            synthetic_reaction(
                7,
                Some(15),
                PassFutureDraws::Known(16),
                [1, 2, 2, 2],
                [None; 4],
            ),
            synthetic_reaction(
                8,
                Some(15),
                PassFutureDraws::Unknown,
                [1, 2, 2, 2],
                [None; 4],
            ),
        ];
        let output = format_capture_comparison(1, &requests);

        assert!(
            output.contains("  synthetic-7.jsonl  request_id=7\n    baseline raw future own draws: 15\n    baseline effective future own draws: h12=9 h14=11 h16=13 h18=15\n    pass raw future own draws: 16\n    pass effective future own draws: h12=10 h14=12 h16=14 h18=16\n"),
            "{output}"
        );
        assert!(
            output.contains("  synthetic-8.jsonl  request_id=8\n    baseline raw future own draws: 15\n    baseline effective future own draws: h12=9 h14=11 h16=13 h18=15\n    pass raw future own draws: unknown\n    pass effective future own draws: h12=unknown h14=unknown h16=unknown h18=unknown\n"),
            "{output}"
        );
        assert!(
            output.contains("  reaction requests with known pass raw future draws: 1\n  reaction requests with unknown pass raw future draws: 1\n"),
            "{output}"
        );
        assert!(
            output.contains("By baseline raw future own draws\n"),
            "{output}"
        );
    }

    #[test]
    fn only_the_differing_requests_are_listed_with_their_capture_and_request_id() {
        let requests = [
            synthetic_request(1, Some(11), [1, 1, 1, 1], [Some(1); 4]),
            synthetic_request(
                2,
                Some(7),
                [1, 2, 2, 2],
                [Some(1), Some(2), Some(2), Some(2)],
            ),
            synthetic_request(
                4,
                Some(7),
                [3, 3, 3, 3],
                [Some(1), Some(2), Some(2), Some(2)],
            ),
            synthetic_request(5, None, [3, 3, 3, 3], [None, None, Some(2), Some(2)]),
        ];
        let output = format_capture_comparison(2, &requests);

        assert!(output.contains("  captures: 2\n"), "{output}");
        assert!(output.contains("Differing requests: 2\n"), "{output}");
        assert!(
            output.contains("  synthetic-2.jsonl  request_id=2\n    baseline raw future own draws: 7\n    baseline effective future own draws: h12=2 h14=3 h16=5 h18=7\n    pass raw future own draws: not applicable\n    final action: 12 / 14=16=18\n    normal discard selection: 12 / 14=16=18\n"),
            "{output}"
        );
        assert!(
            output.contains("  synthetic-4.jsonl  request_id=4\n    baseline raw future own draws: 7\n    baseline effective future own draws: h12=2 h14=3 h16=5 h18=7\n    pass raw future own draws: not applicable\n    final action: 12=14=16=18\n    normal discard selection: 12 / 14=16=18\n"),
            "{output}"
        );
        assert!(!output.contains("request_id=1\n"), "{output}");
        assert!(!output.contains("request_id=5\n"), "{output}");
        // 値を持たない判断は推測せず not evaluated と出す。
        assert!(
            output.contains("    h12:\n      final action: E\n      final action source: LegalDahaiFallback\n      push/pull: not evaluated\n      normal discard selection: 1p\n      selected discard values: unknown\n"),
            "{output}"
        );
    }

    // 1枚待ちの軽いテンパイ。horizon によらず同じ判断になる。
    const TENPAI_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 44, 48, 89, 90];
    const TENPAI_DRAWN: u8 = 116;

    fn tenpai_capture_line(request_id: u64) -> String {
        let observation = riichilab_client::observation::fixture_base64_with_dora(
            0,
            Some(TENPAI_DRAWN),
            TENPAI_HAND.to_vec(),
            vec![36],
        );
        request_line(request_id, &TENPAI_HAND, TENPAI_DRAWN, &observation)
    }

    #[test]
    fn several_captures_and_an_unknown_wall_are_summed_up_together() {
        let directory = TempDir::new().expect("一時 directory を作れる");
        let paths = vec![
            write_capture(&directory, "first.jsonl", &[tenpai_capture_line(10)]),
            write_capture(&directory, "second.jsonl", &[tenpai_capture_line(20)]),
        ];
        let mut requests = compared_requests(&paths);

        // 残り山を推測しない inline scenario。他家の河を指定すると残り山は unknown のまま。
        let args = [
            "--hand",
            "234m455p789s1123z",
            "--draw",
            "N",
            "--discards-shimocha",
            "1z",
        ];
        let scenario = match crate::cli::CliArgs::parse(args.iter().map(|arg| arg.to_string()))
            .expect("option を読める")
            .source
        {
            crate::cli::ScenarioSource::Inline(spec) => {
                bot_analysis::Scenario::resolve(&spec).expect("局面を組み立てられる")
            }
            other => panic!("inline scenario ではない: {other:?}"),
        };
        assert_eq!(scenario.context.remaining_tiles(), None);
        requests.push(ComparedRequest {
            capture: "unknown-wall".to_string(),
            request_id: 30,
            comparison: compare_self_tsumo_horizons(&scenario.context, &scenario.legal_actions),
        });

        // capture の context は production の horizon のままで、h12 の判断は production の
        // act() と一致する。比較は production の horizon を書き換えない。
        let mut originals: Vec<_> = paths
            .iter()
            .flat_map(|path| load_captured_scenarios(path).unwrap())
            .map(|captured| captured.scenario)
            .collect();
        originals.push(scenario);
        for (original, request) in originals.iter().zip(&requests) {
            assert_eq!(
                original.context.self_tsumo_horizon(),
                SelfTsumoHorizon::PRODUCTION
            );
            assert_eq!(
                request.comparison.decisions[0].action,
                ShantenAgent.act(&original.context, &original.legal_actions)
            );
        }
        assert_eq!(
            SelfTsumoHorizon::PRODUCTION,
            SelfTsumoHorizon {
                horizon_turn: 12,
                late_min_future_draws: 2,
            }
        );

        let output = format_capture_comparison(paths.len() + 1, &requests);
        assert!(output.contains("  captures: 3\n"), "{output}");
        assert!(output.contains("  requests evaluated: 3\n"), "{output}");
        assert!(
            output.contains("  requests with known baseline raw future draws: 2\n"),
            "{output}"
        );
        assert!(
            output.contains("  requests with unknown baseline raw future draws: 1\n"),
            "{output}"
        );
        assert!(
            output.contains("  11+: requests 2, all four same 2, not all same 0"),
            "{output}"
        );
        assert!(
            output.contains("  unknown: requests 1, all four same 1, not all same 0"),
            "{output}"
        );
        assert!(output.contains("  all four same: 3\n"), "{output}");
        assert!(output.contains("Differing requests: 0"), "{output}");
        // 通常ツモ番の baseline は従来どおり floor(remaining_tiles / 4) で、Pass は存在しない。
        for (original, request) in originals.iter().zip(&requests) {
            assert_eq!(
                request.comparison.baseline_raw_future_draws,
                original
                    .context
                    .remaining_tiles()
                    .map(|remaining| remaining / 4)
            );
            assert_eq!(
                request.comparison.pass_raw_future_draws,
                PassFutureDraws::NotApplicable
            );
        }
        assert!(!output.contains("  pass raw future own draws"), "{output}");
        let unknown = &requests[2].comparison;
        assert_eq!(unknown.baseline_raw_future_draws, None);
        for decision in &unknown.decisions {
            assert_eq!(decision.baseline_effective_future_draws, None);
        }
    }

    #[test]
    #[ignore = "heavy capture E2E; run by the slow-tests workflow with --run-ignored=only"]
    fn the_capture_comparison_reports_where_the_horizon_changes_the_discard() {
        let directory = TempDir::new().expect("一時 directory を作れる");
        let first = write_capture(
            &directory,
            "first.jsonl",
            &[
                capture_line(1, 44, [false; 4]),
                capture_line(2, 30, [false; 4]),
            ],
        );
        let second = write_capture(
            &directory,
            "second.jsonl",
            &[capture_line(3, 22, [false; 4])],
        );
        let paths = vec![first.clone(), second.clone()];
        let requests = compared_requests(&paths);

        let effective = |request: &ComparedRequest| {
            request
                .comparison
                .decisions
                .each_ref()
                .map(|decision| decision.baseline_effective_future_draws.unwrap())
        };
        assert_eq!(requests[0].comparison.baseline_raw_future_draws, Some(11));
        assert_eq!(effective(&requests[0]), [5, 7, 9, 11]);
        assert_eq!(requests[1].comparison.baseline_raw_future_draws, Some(7));
        assert_eq!(effective(&requests[1]), [2, 3, 5, 7]);
        assert_eq!(requests[2].comparison.baseline_raw_future_draws, Some(5));
        assert_eq!(effective(&requests[2]), [2, 2, 3, 5]);

        let labels = |request: &ComparedRequest| {
            request
                .comparison
                .decisions
                .each_ref()
                .map(|decision| action_label(&decision.action))
        };
        assert_eq!(labels(&requests[0]), ["5p", "5p", "5p", "5p"]);
        assert_eq!(labels(&requests[1]), ["9p", "5p", "5p", "5p"]);
        assert_eq!(labels(&requests[2]), ["9p", "9p", "5p", "5p"]);
        for request in &requests {
            for decision in &request.comparison.decisions {
                assert_eq!(decision.source, AgentActionSource::NormalDiscard);
                assert_eq!(decision.normal_discard.as_ref(), Some(&decision.action));
            }
        }

        let output = format_capture_comparison(paths.len(), &requests);
        for expected in [
            "  captures: 2\n",
            "  requests evaluated: 3\n",
            "  requests with known baseline raw future draws: 3\n",
            "  requests with unknown baseline raw future draws: 0\n",
            "  12 vs 14: same 2, different 1, agreement 66.7%\n",
            "  12 vs 16: same 1, different 2, agreement 33.3%\n",
            "  12 vs 18: same 1, different 2, agreement 33.3%\n",
            "  14 vs 16: same 2, different 1, agreement 66.7%\n",
            "  14 vs 18: same 2, different 1, agreement 66.7%\n",
            "  16 vs 18: same 3, different 0, agreement 100.0%\n",
            "  all four same: 1\n  not all same: 2\n",
            "    12 / 14=16=18: 1\n",
            "    12=14 / 16=18: 1\n",
            "    12=14=16=18: 1\n",
            "  5-6: requests 1, all four same 0, not all same 1, normal discard not all same 1\n",
            "  7-8: requests 1, all four same 0, not all same 1, normal discard not all same 1\n",
            "  11+: requests 1, all four same 1, not all same 0, normal discard not all same 0\n",
            "Differing requests: 2\n",
        ] {
            assert!(output.contains(expected), "{expected:?}\n{output}");
        }
        assert!(
            output.contains(&format!(
                "  {first}  request_id=2\n    baseline raw future own draws: 7\n    baseline effective future own draws: h12=2 h14=3 h16=5 h18=7\n    pass raw future own draws: not applicable\n"
            )),
            "{output}"
        );
        assert!(
            output.contains(&format!(
                "  {second}  request_id=3\n    baseline raw future own draws: 5\n    baseline effective future own draws: h12=2 h14=2 h16=3 h18=5\n    pass raw future own draws: not applicable\n"
            )),
            "{output}"
        );
        assert!(!output.contains("request_id=1\n"), "{output}");
        // 差分 request の通常打牌の値は production selection が持つ値をそのまま出す。
        assert!(
            output.contains("    h12:\n      final action: 9p\n      final action source: NormalDiscard\n      push/pull: Push\n      normal discard selection: 9p\n      shanten after discard: 1\n      acceptance: 16 remaining / 4 types\n      ExpectedSelfTsumoValue (this horizon): "),
            "{output}"
        );
        assert!(
            output.contains(
                "      Push/Fold ExpectedSelfTsumoValue (until ryukyoku): not evaluated / unknown\n"
            ),
            "{output}"
        );
    }

    #[test]
    #[ignore = "heavy capture E2E; run by the slow-tests workflow with --run-ignored=only"]
    fn the_push_pull_value_stays_until_ryukyoku_at_every_horizon() {
        let directory = TempDir::new().expect("一時 directory を作れる");
        let reached = [false, true, false, false];
        let path = write_capture(
            &directory,
            "reach.jsonl",
            &[capture_line(1, 66, reached), capture_line(2, 30, reached)],
        );
        let requests = compared_requests(&[path]);

        for request in &requests {
            let values = request.comparison.decisions.each_ref().map(|decision| {
                let offense = decision.offense.expect("offense state がある");
                (
                    offense
                        .iishanten_selection_expected_self_tsumo_value()
                        .unwrap(),
                    offense
                        .iishanten_push_pull_expected_self_tsumo_value()
                        .unwrap(),
                )
            });
            // 選択用の値は configured horizon に追従し、horizon 18 では Push/Fold 用の値と一致する。
            for pair in values.windows(2) {
                assert!(pair[0].0 < pair[1].0, "{values:?}");
            }
            assert_eq!(values[3].0, values[3].1);
            // Push/Fold 用の値は選んだ候補の UNTIL_RYUKYOKU 値で、同じ候補なら horizon によらない。
            let discards = request
                .comparison
                .decisions
                .each_ref()
                .map(|decision| decision.normal_discard.clone().expect("通常打牌選択を通る"));
            for (index, discard) in discards.iter().enumerate() {
                if *discard == discards[3] {
                    assert_eq!(values[index].1, values[3].1, "{values:?}");
                }
            }
            // リーチ者に対しては horizon によらず同じ防御牌で降りる。
            assert!(request.comparison.all_final_actions_agree());
            for decision in &request.comparison.decisions {
                assert!(
                    matches!(decision.source, AgentActionSource::DefenseFallback(_)),
                    "{:?}",
                    decision.source
                );
            }
        }
        assert!(requests[0].comparison.all_normal_discards_agree());
        // 残り山が少ない局面では通常打牌だけが horizon で分かれ、最終 action は同じまま。
        assert!(!requests[1].comparison.all_normal_discards_agree());

        let output = format_capture_comparison(1, &requests);
        assert!(output.contains("  all four same: 2\n"), "{output}");
        assert!(
            output.contains("  final action all same but normal discard different: 1\n"),
            "{output}"
        );
        assert!(output.contains("Differing requests: 1\n"), "{output}");
        assert!(output.contains("request_id=2\n"), "{output}");
    }
}
