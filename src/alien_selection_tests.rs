use super::{
    analyze, average, mmr_penalty, score, second_modifier, target_range, used_mmr, Availability,
    ExclusionReason, HostChoice, MmrModifier, MmrSource, PlayerInput, PlayerScore, RoundAnalysis,
    ScoreRange, SelectionAnalysis, SelectionInput,
};

fn player(id: u32) -> PlayerInput {
    PlayerInput {
        id,
        effective_hmmr: Some(ScoreRange::point(100)),
        effective_ammr: Some(ScoreRange::point(200)),
        host_wins: Some(0),
        successor_wins: Some(0),
        new_player: Some(false),
        new_role: Some(false),
        host_banned: Some(false),
        already_alien: Some(false),
        volunteer: [Some(false), Some(false)],
        deprioritized: Some(false),
        left_last_game: Some(false),
        special_role: Some(false),
    }
}

fn input(players: Vec<PlayerInput>) -> SelectionInput {
    SelectionInput {
        players,
        roster_complete: true,
        enabled: Some(true),
        requirement_penalty: Some(false),
        first_host: None,
        estimate_fixed_arithmetic: true,
    }
}

fn round(analysis: &SelectionAnalysis, number: u8) -> Result<&RoundAnalysis, &'static str> {
    analysis
        .rounds
        .iter()
        .find(|round| round.round == number)
        .ok_or("round missing")
}

fn row(round: &RoundAnalysis, id: u32) -> Result<&PlayerScore, &'static str> {
    round
        .players
        .iter()
        .find(|player| player.id == id)
        .ok_or("player missing")
}

#[test]
fn five_wins_switches_to_alien_mmr() {
    let mut candidate = player(1);
    candidate.host_wins = Some(4);
    assert_eq!(
        used_mmr(&candidate),
        (Some(ScoreRange::point(100)), Some(MmrSource::Human))
    );
    candidate.successor_wins = Some(1);
    assert_eq!(
        used_mmr(&candidate),
        (Some(ScoreRange::point(200)), Some(MmrSource::Alien))
    );
    candidate.host_wins = Some(5);
    candidate.successor_wins = None;
    assert_eq!(
        used_mmr(&candidate),
        (Some(ScoreRange::point(200)), Some(MmrSource::Alien))
    );
    candidate.host_wins = Some(4);
    assert_eq!(
        used_mmr(&candidate),
        (Some(ScoreRange { min: 100, max: 200 }), None)
    );
}

#[test]
fn inactive_requirement_keeps_fixed_99_and_total_99_to_149() {
    let mut candidate = player(1);
    candidate.volunteer = [Some(true), Some(false)];
    let result = score(
        &candidate,
        Some(true),
        Some(ScoreRange::point(110)),
        0,
        Some(false),
    );
    assert_eq!(result.breakdown.mmr_penalty, Some(ScoreRange::point(-1)));
    assert_eq!(result.breakdown.requirement_penalty, Some(0));
    assert_eq!(result.breakdown.fixed_score, Some(99));
    assert_eq!(result.breakdown.fixed_range, Some(ScoreRange::point(99)));
    assert_eq!(
        result.breakdown.total_range,
        Some(ScoreRange { min: 99, max: 149 })
    );
}

#[test]
fn unknown_flags_contribute_ranges_instead_of_zero() {
    let mut candidate = player(1);
    candidate.volunteer = [None, None];
    candidate.deprioritized = None;
    candidate.left_last_game = None;
    let result = score(
        &candidate,
        Some(true),
        Some(ScoreRange::point(100)),
        0,
        None,
    );
    assert_eq!(result.breakdown.preference_bonus, None);
    assert_eq!(result.breakdown.fixed_score, None);
    assert_eq!(
        result.breakdown.fixed_range,
        Some(ScoreRange {
            min: -325,
            max: 100,
        })
    );
    assert_eq!(
        result.breakdown.total_range,
        Some(ScoreRange {
            min: -325,
            max: 150,
        })
    );
}

#[test]
fn mmr_distance_bounds_include_overlap_and_division_edges() {
    assert_eq!(
        mmr_penalty(ScoreRange { min: 90, max: 110 }, ScoreRange::point(100)),
        Some(ScoreRange { min: -1, max: 0 })
    );
    assert_eq!(
        mmr_penalty(
            ScoreRange { min: 120, max: 129 },
            ScoreRange { min: 100, max: 110 },
        ),
        Some(ScoreRange { min: -2, max: -1 })
    );
    assert_eq!(
        mmr_penalty(ScoreRange::point(91), ScoreRange::point(100)),
        Some(ScoreRange::point(0))
    );
}

#[test]
fn room_average_uses_integer_endpoint_division() {
    let mut first = player(1);
    first.effective_hmmr = Some(ScoreRange { min: 100, max: 101 });
    let mut second = player(2);
    second.effective_hmmr = Some(ScoreRange { min: 102, max: 103 });
    assert_eq!(
        average(&input(vec![first, second])),
        Some(ScoreRange { min: 101, max: 102 })
    );
}

#[test]
fn target_bounds_preserve_half_conversion_and_branch_uncertainty() {
    assert_eq!(
        target_range(ScoreRange::point(101), MmrModifier::Full),
        Some(ScoreRange::point(101))
    );
    assert_eq!(
        target_range(ScoreRange::point(101), MmrModifier::Half),
        Some(ScoreRange { min: 50, max: 51 })
    );
    assert_eq!(
        target_range(ScoreRange::point(100), MmrModifier::Half),
        Some(ScoreRange::point(50))
    );
    assert_eq!(
        target_range(ScoreRange::point(101), MmrModifier::FullOrHalf),
        Some(ScoreRange { min: 50, max: 101 })
    );
}

#[test]
fn modifier_uses_last_candidate_and_retains_fixed_boundaries() -> Result<(), &'static str> {
    let mut first = player(1);
    first.effective_hmmr = Some(ScoreRange::point(100));
    let mut last = player(12);
    last.effective_hmmr = Some(ScoreRange::point(200));
    let mut settings = input(vec![last, first]);
    settings.first_host = Some(HostChoice::Observed(1));
    let analysis = analyze(&settings);
    let first_round = round(&analysis, 1)?;
    assert_eq!(
        second_modifier(&settings, first_round, Some(ScoreRange::point(100))),
        Some(MmrModifier::Half)
    );
    assert_eq!(
        second_modifier(&settings, first_round, Some(ScoreRange::point(159))),
        Some(MmrModifier::FullOrHalf)
    );
    assert_eq!(
        second_modifier(&settings, first_round, Some(ScoreRange::point(160))),
        Some(MmrModifier::FullOrHalf)
    );
    assert_eq!(
        second_modifier(&settings, first_round, Some(ScoreRange::point(161))),
        Some(MmrModifier::Full)
    );
    Ok(())
}

#[test]
fn participant_count_controls_second_round() {
    assert_eq!(
        analyze(&input((1..=9).map(player).collect())).rounds.len(),
        1
    );
    assert_eq!(
        analyze(&input((1..=10).map(player).collect())).rounds.len(),
        2
    );
}

#[test]
fn empty_candidate_fallback_retains_penalties_then_removes_existing_aliens(
) -> Result<(), &'static str> {
    let mut first = player(1);
    first.new_player = Some(true);
    let mut second = player(2);
    second.new_role = Some(true);
    second.already_alien = Some(true);
    let analysis = analyze(&input(vec![first, second]));
    let first_round = round(&analysis, 1)?;
    assert_eq!(first_round.fallback, Some(true));
    assert_eq!(row(first_round, 1)?.eligible, Some(true));
    assert_eq!(row(first_round, 1)?.breakdown.fixed_score, Some(-5000));
    assert_eq!(row(first_round, 2)?.eligible, Some(false));
    assert_eq!(row(first_round, 2)?.breakdown.total_range, None);
    assert!(row(first_round, 2)?
        .exclusions
        .iter()
        .any(|reason| reason.reason == ExclusionReason::AlreadyAlien));
    Ok(())
}

#[test]
fn second_round_removes_special_roles_but_fallback_can_restore_them() -> Result<(), &'static str> {
    let mut settings = input((1..=10).map(player).collect());
    settings.first_host = Some(HostChoice::Observed(1));
    for candidate in settings.players.iter_mut().filter(|player| player.id <= 2) {
        candidate.special_role = Some(true);
    }
    let analysis = analyze(&settings);
    let second_round = round(&analysis, 2)?;
    assert_eq!(second_round.fallback, Some(false));
    assert_eq!(row(second_round, 1)?.eligible, Some(false));
    assert_eq!(row(second_round, 2)?.eligible, Some(false));
    assert_eq!(row(second_round, 3)?.eligible, Some(true));
    assert!(row(second_round, 2)?
        .exclusions
        .iter()
        .any(|reason| reason.reason == ExclusionReason::SpecialRole(1)));

    for candidate in &mut settings.players {
        if candidate.id > 2 {
            candidate.new_player = Some(true);
        }
    }
    let analysis = analyze(&settings);
    let second_round = round(&analysis, 2)?;
    assert_eq!(second_round.fallback, Some(true));
    assert_eq!(row(second_round, 1)?.eligible, Some(false));
    assert_eq!(row(second_round, 2)?.eligible, Some(true));
    assert_eq!(row(second_round, 3)?.eligible, Some(true));
    assert_eq!(
        row(second_round, 3)?.breakdown.new_player_penalty,
        Some(-5000)
    );
    Ok(())
}

#[test]
fn incomplete_roster_never_proves_empty_candidate_fallback() -> Result<(), &'static str> {
    let mut first = player(1);
    first.new_player = Some(true);
    let mut settings = input(vec![first]);
    settings.roster_complete = false;
    let analysis = analyze(&settings);
    let first_round = round(&analysis, 1)?;
    assert_eq!(analysis.average_hmmr, None);
    assert_eq!(first_round.fallback, None);
    assert_eq!(row(first_round, 1)?.eligible, None);
    assert_ne!(first_round.availability, Availability::CompleteRanges);
    assert_eq!(first_round.fixed_order, None);
    assert_eq!(analysis.rounds.len(), 2);
    Ok(())
}

#[test]
fn interval_fixed_scores_do_not_create_a_point_ranking() -> Result<(), &'static str> {
    let mut candidate = player(1);
    candidate.volunteer = [None, None];
    let analysis = analyze(&input(vec![candidate]));
    let first_round = round(&analysis, 1)?;
    assert_eq!(first_round.availability, Availability::CompleteRanges);
    assert_eq!(first_round.fixed_order, None);
    assert_eq!(
        row(first_round, 1)?.breakdown.fixed_range,
        Some(ScoreRange { min: 0, max: 100 })
    );
    Ok(())
}

#[test]
fn invalid_mmr_ranges_and_duplicate_players_block_room_average() {
    let mut candidate = player(1);
    candidate.effective_hmmr = Some(ScoreRange { min: 200, max: 100 });
    assert_eq!(average(&input(vec![candidate])), None);
    assert_eq!(average(&input(vec![player(1), player(1)])), None);
    assert_eq!(average(&input(Vec::new())), None);
}

#[test]
fn disabled_mode_has_no_selection_rounds() {
    let mut settings = input(vec![player(1)]);
    settings.enabled = Some(false);
    let analysis = analyze(&settings);
    assert!(analysis.rounds.is_empty());
    assert_eq!(analysis.average_hmmr, None);
}

#[test]
fn impossible_observed_host_blocks_second_round_causal_inference() -> Result<(), &'static str> {
    let mut settings = input((1..=10).map(player).collect());
    settings.first_host = Some(HostChoice::Observed(1));
    for candidate in &mut settings.players {
        candidate.volunteer = [Some(candidate.id != 1), Some(false)];
    }
    let analysis = analyze(&settings);
    let second_round = round(&analysis, 2)?;
    assert_eq!(second_round.modifier, None);
    assert_eq!(second_round.target_mmr, None);
    assert_eq!(second_round.fallback, None);
    assert_eq!(row(second_round, 1)?.eligible, Some(false));
    assert_eq!(row(second_round, 2)?.eligible, None);
    assert_eq!(row(second_round, 2)?.breakdown.total_range, None);
    assert!(second_round
        .reasons
        .iter()
        .any(|reason| reason.contains("首轮模型与录像冲突")));
    Ok(())
}
