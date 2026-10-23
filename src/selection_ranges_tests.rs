use super::{compare, PairwiseRelation, Verdict};
use crate::alien_selection::{
    Availability, PlayerScore, RoundAnalysis, ScoreBreakdown, ScoreRange,
};

fn point(value: i32) -> ScoreRange {
    ScoreRange {
        min: value,
        max: value,
    }
}

fn player(id: u32, fixed: Option<ScoreRange>, eligible: Option<bool>) -> PlayerScore {
    PlayerScore {
        id,
        eligible,
        exclusions: Vec::new(),
        used_mmr: None,
        mmr_source: None,
        host_wins: None,
        successor_wins: None,
        breakdown: ScoreBreakdown {
            fixed_range: fixed,
            fixed_score: fixed
                .filter(|range| range.min == range.max)
                .map(|range| range.min),
            total_range: fixed.and_then(|range| {
                Some(ScoreRange {
                    min: range.min,
                    max: range.max.checked_add(50)?,
                })
            }),
            ..ScoreBreakdown::default()
        },
        reasons: Vec::new(),
    }
}

fn round(players: Vec<PlayerScore>, complete: bool) -> RoundAnalysis {
    RoundAnalysis {
        round: 1,
        target_mmr: None,
        modifier: None,
        fallback: Some(false),
        players,
        availability: if complete {
            Availability::CompleteRanges
        } else {
            Availability::Partial
        },
        reasons: Vec::new(),
        fixed_order: None,
    }
}

fn wins(base: i32, own_roll: i32, other_roll: i32, own_first: bool) -> bool {
    base + own_roll > other_roll || (base + own_roll == other_roll && own_first)
}

#[test]
fn exact_thresholds_match_exhaustive_rolls_at_tie_boundaries() {
    for base in [-51, -50, -49, 0, 49, 50, 51] {
        for (own_id, other_id) in [(1, 2), (2, 1)] {
            let round = round(
                vec![
                    player(other_id, Some(point(0)), Some(true)),
                    player(own_id, Some(point(base)), Some(true)),
                ],
                true,
            );
            let analysis = compare(&round, own_id, None);
            let expected_q = -base + i32::from(own_id > other_id);
            let comparison = analysis.pairwise.first();
            assert_eq!(
                comparison.and_then(|item| item.required_difference),
                Some(point(expected_q))
            );
            let lower = (0..=50)
                .find(|own| (0..=50).any(|other| wins(base, *own, other, own_id < other_id)));
            let guarantee = (0..=50)
                .find(|own| (0..=50).all(|other| wins(base, *own, other, own_id < other_id)));
            assert_eq!(
                analysis.possible_roll,
                lower.map(|min| ScoreRange { min, max: 50 })
            );
            assert_eq!(
                analysis.guaranteed_roll,
                guarantee.map(|min| ScoreRange { min, max: 50 })
            );
            for own in 0..=50 {
                for other in 0..=50 {
                    assert_eq!(
                        wins(base, own, other, own_id < other_id),
                        own - other >= expected_q
                    );
                }
            }
            let expected = if lower.is_none() {
                Verdict::ImpossibleWin
            } else if guarantee == Some(0) {
                Verdict::GuaranteedWin
            } else {
                Verdict::Conditional
            };
            assert_eq!(analysis.verdict, expected, "base {base}, player {own_id}");
        }
    }
}

#[test]
fn negative_scores_are_compared_without_clamping() {
    let round = round(
        vec![
            player(1, Some(point(-100)), Some(true)),
            player(2, Some(point(-151)), Some(true)),
        ],
        true,
    );
    assert_eq!(compare(&round, 1, None).verdict, Verdict::GuaranteedWin);
    assert_eq!(compare(&round, 2, None).verdict, Verdict::ImpossibleWin);
}

#[test]
fn interval_fixed_values_use_best_and_worst_difference_bounds() {
    let round = round(
        vec![
            player(1, Some(ScoreRange { min: 100, max: 102 }), Some(true)),
            player(2, Some(ScoreRange { min: 51, max: 52 }), Some(true)),
        ],
        true,
    );
    let analysis = compare(&round, 1, None);
    assert_eq!(analysis.verdict, Verdict::Conditional);
    assert_eq!(analysis.possible_roll, Some(ScoreRange { min: 0, max: 50 }));
    assert_eq!(
        analysis.guaranteed_roll,
        Some(ScoreRange { min: 2, max: 50 })
    );
    assert_eq!(
        analysis
            .pairwise
            .first()
            .and_then(|item| item.required_difference),
        Some(ScoreRange { min: -51, max: -48 })
    );
    for own_base in 100..=102 {
        for other_base in 51..=52 {
            for own in 2..=50 {
                for other in 0..=50 {
                    assert!(own_base + own >= other_base + other);
                }
            }
        }
    }
}

#[test]
fn partial_roster_never_proves_global_victory() {
    let round = round(
        vec![
            player(1, Some(point(100)), Some(true)),
            player(2, Some(point(0)), Some(true)),
        ],
        false,
    );
    let analysis = compare(&round, 1, None);
    assert_eq!(analysis.verdict, Verdict::Incomplete);
    assert_eq!(analysis.possible_roll, None);
    assert_eq!(analysis.guaranteed_roll, None);
    assert_eq!(
        analysis.pairwise.first().map(|item| item.relation),
        Some(PairwiseRelation::Guaranteed)
    );
    assert_eq!(compare(&round, 2, None).verdict, Verdict::ImpossibleWin);
}

#[test]
fn unknown_eligibility_cannot_be_used_to_prove_defeat() {
    let round = round(
        vec![
            player(1, Some(point(0)), Some(true)),
            player(2, Some(point(100)), None),
        ],
        false,
    );
    let analysis = compare(&round, 1, None);
    assert_eq!(analysis.verdict, Verdict::Incomplete);
    assert_eq!(
        analysis.pairwise.first().map(|item| item.relation),
        Some(PairwiseRelation::Unknown)
    );
}

#[test]
fn missing_and_overflowing_fixed_values_remain_unknown() {
    for fixed in [
        None,
        Some(point(i32::MAX)),
        Some(ScoreRange { min: 3, max: 2 }),
    ] {
        let round = round(
            vec![
                player(1, Some(point(-100)), Some(true)),
                player(2, fixed, Some(true)),
            ],
            true,
        );
        let analysis = compare(&round, 1, None);
        assert_eq!(analysis.verdict, Verdict::Incomplete);
        assert_eq!(
            analysis
                .pairwise
                .first()
                .and_then(|item| item.required_difference),
            None
        );
        assert_eq!(analysis.guaranteed_roll, None);
    }
}

#[test]
fn observed_winner_constrains_both_rolls_without_inventing_values() {
    let round = round(
        vec![
            player(1, Some(point(0)), Some(true)),
            player(2, Some(point(40)), Some(true)),
        ],
        true,
    );
    let analysis = compare(&round, 2, Some(1));
    let observation = analysis.observed_constraint.as_ref();
    assert_eq!(observation.map(|value| value.conflict), Some(false));
    assert_eq!(
        observation.and_then(|value| value.winner_roll),
        Some(ScoreRange { min: 40, max: 50 })
    );
    assert_eq!(
        observation.and_then(|value| value.selected_player_roll),
        Some(ScoreRange { min: 0, max: 10 })
    );
    assert_eq!(
        observation.and_then(|value| value.winner_required_difference),
        Some(point(40))
    );
    for winner_roll in 0..=50 {
        for loser_roll in 0..=50 {
            if winner_roll >= 40 + loser_roll {
                assert!(winner_roll >= 40);
                assert!(loser_roll <= 10);
            }
        }
    }
}

#[test]
fn impossible_excluded_or_missing_observed_winner_is_a_conflict() {
    let round = round(
        vec![
            player(1, Some(point(0)), Some(true)),
            player(2, Some(point(51)), Some(true)),
            player(3, Some(point(100)), Some(false)),
        ],
        true,
    );
    for winner in [1, 3, 4] {
        let analysis = compare(&round, 2, Some(winner));
        assert_eq!(analysis.verdict, Verdict::Conflict);
        assert_eq!(
            analysis
                .observed_constraint
                .as_ref()
                .map(|value| value.conflict),
            Some(true)
        );
        assert_eq!(
            analysis
                .observed_constraint
                .and_then(|value| value.winner_roll),
            None
        );
    }
}

#[test]
fn multiplayer_thresholds_include_every_opponent_and_ignore_storage_order() {
    let round = round(
        vec![
            player(3, Some(point(20)), Some(true)),
            player(1, Some(point(40)), Some(true)),
            player(2, Some(point(10)), Some(true)),
        ],
        true,
    );
    let analysis = compare(&round, 2, None);
    assert_eq!(
        analysis.possible_roll,
        Some(ScoreRange { min: 31, max: 50 })
    );
    assert_eq!(analysis.guaranteed_roll, None);
    assert_eq!(analysis.verdict, Verdict::Conditional);
}

#[test]
fn no_opponents_still_requires_beating_the_maps_initial_highest_score() {
    let first_round = round(vec![player(1, Some(point(-50_000)), Some(true))], true);
    let analysis = compare(&first_round, 1, None);
    assert_eq!(analysis.possible_roll, Some(ScoreRange { min: 1, max: 50 }));
    assert_eq!(
        analysis.guaranteed_roll,
        Some(ScoreRange { min: 1, max: 50 })
    );
    assert_eq!(analysis.verdict, Verdict::Conditional);
    let round = round(vec![player(1, Some(point(-50_050)), Some(true))], true);
    assert_eq!(compare(&round, 1, None).verdict, Verdict::ImpossibleWin);
}

#[test]
fn duplicate_ids_prevent_a_global_claim() {
    let round = round(
        vec![
            player(1, Some(point(100)), Some(true)),
            player(2, Some(point(0)), Some(true)),
            player(2, Some(point(1)), Some(true)),
        ],
        true,
    );
    assert_eq!(compare(&round, 1, None).verdict, Verdict::Incomplete);
}
