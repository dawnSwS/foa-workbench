//! Integer inequalities for the map's bounded selection term.
//!
//! These are constraints, not reconstructed random draws or probabilities.
//! Player-group traversal follows the ascending-ID convention used by the model;
//! the script itself retains the first visited player on a tied total score.

use super::{Availability, PlayerScore, RoundAnalysis, ScoreRange};

const ROLL_MAX: i64 = 50;
const LOWEST_WINNING_SCORE: i64 = -49_999;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    GuaranteedWin,
    ImpossibleWin,
    Conditional,
    Incomplete,
    Excluded,
    Conflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairwiseRelation {
    Guaranteed,
    Impossible,
    Conditional,
    Unknown,
}

#[derive(Debug)]
pub struct PairwiseComparison {
    pub opponent_id: u32,
    pub opponent_eligible: Option<bool>,
    /// The selected player must satisfy r_self - r_opponent >= this threshold.
    /// A non-point threshold encloses uncertainty in the fixed contributions.
    pub required_difference: Option<ScoreRange>,
    pub relation: PairwiseRelation,
}

#[derive(Debug)]
pub struct ObservedConstraint {
    pub winner_id: u32,
    /// Necessary bounds implied by the winner fact, never a recorded draw.
    pub winner_roll: Option<ScoreRange>,
    pub selected_player_roll: Option<ScoreRange>,
    /// The observed winner must satisfy r_winner - r_selected >= this value.
    pub winner_required_difference: Option<ScoreRange>,
    pub conflict: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug)]
pub struct ComparisonAnalysis {
    pub verdict: Verdict,
    /// A necessary condition; an enclosed value need not be reachable by the PRNG.
    pub possible_roll: Option<ScoreRange>,
    /// Sufficient against every opponent and every fixed-value realization.
    pub guaranteed_roll: Option<ScoreRange>,
    pub pairwise: Vec<PairwiseComparison>,
    pub observed_constraint: Option<ObservedConstraint>,
    pub reasons: Vec<String>,
}

fn unique_player(round: &RoundAnalysis, id: u32) -> Option<&PlayerScore> {
    let mut players = round.players.iter().filter(|player| player.id == id);
    let player = players.next()?;
    players.next().is_none().then_some(player)
}

fn valid_range(range: Option<ScoreRange>) -> Option<ScoreRange> {
    range.filter(|range| range.min <= range.max)
}

fn threshold(player: &PlayerScore, opponent: &PlayerScore) -> Option<ScoreRange> {
    let own = valid_range(player.breakdown.fixed_range)?;
    let other = valid_range(opponent.breakdown.fixed_range)?;
    let tie = i64::from(player.id > opponent.id);
    Some(ScoreRange {
        min: i32::try_from(i64::from(other.min) - i64::from(own.max) + tie).ok()?,
        max: i32::try_from(i64::from(other.max) - i64::from(own.min) + tie).ok()?,
    })
}

fn pairwise(player: &PlayerScore, opponent: &PlayerScore) -> PairwiseComparison {
    let required_difference = threshold(player, opponent);
    let relation = if player.eligible != Some(true) || opponent.eligible != Some(true) {
        PairwiseRelation::Unknown
    } else {
        required_difference.map_or(PairwiseRelation::Unknown, |range| {
            if i64::from(range.max) <= -ROLL_MAX {
                PairwiseRelation::Guaranteed
            } else if i64::from(range.min) > ROLL_MAX {
                PairwiseRelation::Impossible
            } else {
                PairwiseRelation::Conditional
            }
        })
    };
    PairwiseComparison {
        opponent_id: opponent.id,
        opponent_eligible: opponent.eligible,
        required_difference,
        relation,
    }
}

fn roll_interval(lower: i64, upper: i64) -> Option<ScoreRange> {
    let min = lower.max(0);
    let max = upper.min(ROLL_MAX);
    if min > max {
        return None;
    }
    Some(ScoreRange {
        min: i32::try_from(min).ok()?,
        max: i32::try_from(max).ok()?,
    })
}

fn valid_ids(round: &RoundAnalysis) -> bool {
    let mut ids = std::collections::BTreeSet::new();
    round
        .players
        .iter()
        .all(|player| (1..=12).contains(&player.id) && ids.insert(player.id))
}

fn empty_analysis(reason: &str) -> ComparisonAnalysis {
    ComparisonAnalysis {
        verdict: Verdict::Incomplete,
        possible_roll: None,
        guaranteed_roll: None,
        pairwise: Vec::new(),
        observed_constraint: None,
        reasons: vec![reason.into()],
    }
}

fn base_comparison(round: &RoundAnalysis, player_id: u32) -> ComparisonAnalysis {
    let Some(player) = unique_player(round, player_id) else {
        return empty_analysis("玩家未唯一出现在本轮名单中，不能比较胜负区间。");
    };
    if !valid_ids(round) {
        return empty_analysis("本轮玩家编号重复或超出 1–12，遍历顺序与同分规则无法确认。");
    }
    let pairwise: Vec<_> = round
        .players
        .iter()
        .filter(|opponent| opponent.id != player.id && opponent.eligible != Some(false))
        .map(|opponent| pairwise(player, opponent))
        .collect();
    let fixed = valid_range(player.breakdown.fixed_range);
    let complete = round.availability == Availability::CompleteRanges
        && player.eligible == Some(true)
        && fixed.is_some()
        && pairwise
            .iter()
            .all(|item| item.opponent_eligible == Some(true) && item.required_difference.is_some());
    let possible_lower = fixed.map(|fixed| {
        pairwise
            .iter()
            .filter(|item| item.opponent_eligible == Some(true))
            .filter_map(|item| item.required_difference.map(|range| i64::from(range.min)))
            .fold(LOWEST_WINNING_SCORE - i64::from(fixed.max), i64::max)
            .max(0)
    });
    let guaranteed_lower = complete.then(|| {
        pairwise
            .iter()
            .filter_map(|item| {
                item.required_difference
                    .map(|range| i64::from(range.max) + ROLL_MAX)
            })
            .fold(
                fixed.map_or(0, |fixed| LOWEST_WINNING_SCORE - i64::from(fixed.min)),
                i64::max,
            )
            .max(0)
    });
    let verdict = classify(
        player,
        complete,
        possible_lower,
        guaranteed_lower,
        &pairwise,
    );
    let possible_roll = if complete {
        possible_lower.and_then(|lower| roll_interval(lower, ROLL_MAX))
    } else {
        None
    };
    let guaranteed_roll = guaranteed_lower.and_then(|lower| roll_interval(lower, ROLL_MAX));
    let mut reasons = vec![
        "S = B + r，r 为 0–50 的整数项；本人胜过对手须满足 r本人 − r对手 ≥ B对手 − B本人 + 同分修正。".into(),
        "按模型中的玩家编号升序遍历；同分保留先遍历者。本人编号较大时，门槛额外加 1。".into(),
    ];
    reasons.push(verdict_reason(verdict).into());
    if fixed.is_some_and(|range| range.min != range.max) {
        reasons.push("固定分含数值边界或未知分项，差值门槛也显示区间；必要条件取最宽边界，保证条件取最不利边界。".into());
    }
    ComparisonAnalysis {
        verdict,
        possible_roll,
        guaranteed_roll,
        pairwise,
        observed_constraint: None,
        reasons,
    }
}

fn classify(
    player: &PlayerScore,
    complete: bool,
    possible_lower: Option<i64>,
    guaranteed_lower: Option<i64>,
    opponents: &[PairwiseComparison],
) -> Verdict {
    if player.eligible == Some(false) {
        Verdict::Excluded
    } else if player.eligible != Some(true) {
        Verdict::Incomplete
    } else if possible_lower.is_some_and(|value| value > ROLL_MAX)
        || opponents
            .iter()
            .any(|item| item.relation == PairwiseRelation::Impossible)
    {
        Verdict::ImpossibleWin
    } else if !complete {
        Verdict::Incomplete
    } else if guaranteed_lower == Some(0) {
        Verdict::GuaranteedWin
    } else {
        Verdict::Conditional
    }
}

fn verdict_reason(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::GuaranteedWin => {
            "在本轮已知完整候选及全部区间边界内，本人取任何 0–50 值都能胜出。"
        }
        Verdict::ImpossibleWin => {
            "即使本人取最有利边界，仍无法越过至少一项必要门槛；区间可以证明落选。"
        }
        Verdict::Conditional => {
            "区间仍允许不同胜负，需要同时满足对所有对手的差值门槛；固定分最高不等于必定当选。"
        }
        Verdict::Incomplete => {
            "候选或数值信息不完整，只能核查已知对手，不能给出全局保证或完整可胜范围。"
        }
        Verdict::Excluded => "本人已被筛除，不进入本轮分数竞争；具体原因见候选筛选记录。",
        Verdict::Conflict => {
            "录像当选事实与当前候选或分数区间矛盾，不能据此编造具体取值或落选原因。"
        }
    }
}

fn observed_constraint(
    round: &RoundAnalysis,
    player_id: u32,
    winner_id: u32,
) -> ObservedConstraint {
    let winner_analysis = base_comparison(round, winner_id);
    let winner = unique_player(round, winner_id);
    let selected = unique_player(round, player_id);
    let conflict = winner.is_none()
        || matches!(
            winner_analysis.verdict,
            Verdict::Excluded | Verdict::ImpossibleWin
        );
    let winner_roll = if conflict {
        None
    } else {
        winner_analysis.possible_roll
    };
    let winner_required_difference = winner.zip(selected).and_then(|(winner, selected)| {
        (winner.id != selected.id && selected.eligible == Some(true))
            .then(|| threshold(winner, selected))
            .flatten()
    });
    let selected_player_roll = if conflict {
        None
    } else if player_id == winner_id {
        winner_roll
    } else {
        winner_required_difference
            .and_then(|range| roll_interval(0, ROLL_MAX - i64::from(range.min)))
    };
    let mut reasons =
        vec!["以下范围由录像当选事实与不等式反推，是必要条件；不代表录像记录了具体附加值。".into()];
    if conflict {
        reasons.push("录像赢家不在已知候选中，或其区间已被确定对手压过；应复查输入状态与适用地图规则，不能将矛盾解释为随机波动。".into());
    } else if winner_analysis.verdict == Verdict::Incomplete {
        reasons.push("赢家的完整比较条件尚不齐全，只展示能由已知分项约束的关系。".into());
    } else if player_id != winner_id && selected_player_roll.is_some() {
        reasons.push(
            "落选者范围仅利用赢家至多取 50 的必要上界；仍须同时满足赢家与所有候选的差值关系。"
                .into(),
        );
    }
    ObservedConstraint {
        winner_id,
        winner_roll,
        selected_player_roll,
        winner_required_difference,
        conflict,
        reasons,
    }
}

pub fn compare(
    round: &RoundAnalysis,
    player_id: u32,
    observed_winner: Option<u32>,
) -> ComparisonAnalysis {
    let mut analysis = base_comparison(round, player_id);
    if let Some(winner_id) = observed_winner {
        let constraint = observed_constraint(round, player_id, winner_id);
        if constraint.conflict {
            analysis.verdict = Verdict::Conflict;
            analysis
                .reasons
                .push(verdict_reason(Verdict::Conflict).into());
        }
        analysis.observed_constraint = Some(constraint);
    }
    analysis
}

#[cfg(test)]
#[path = "selection_ranges_tests.rs"]
mod tests;
