use autopilot_domain::GoalState;

#[test]
fn allows_happy_path_transitions() {
    let path = [
        GoalState::Created,
        GoalState::Planning,
        GoalState::PolicyCheck,
        GoalState::Scheduling,
        GoalState::Executing,
        GoalState::Reviewing,
        GoalState::Verifying,
        GoalState::Publishing,
        GoalState::Completed,
    ];

    for pair in path.windows(2) {
        assert!(pair[0].can_transition_to(&pair[1]), "{:?} -> {:?}", pair[0], pair[1]);
    }
}

#[test]
fn allows_bounded_repair_cycle() {
    assert!(GoalState::Verifying.can_transition_to(&GoalState::Repairing));
    assert!(GoalState::Repairing.can_transition_to(&GoalState::Executing));
}

#[test]
fn rejects_backwards_and_skipped_transitions() {
    assert!(!GoalState::Executing.can_transition_to(&GoalState::Planning));
    assert!(!GoalState::Created.can_transition_to(&GoalState::Executing));
    assert!(!GoalState::Verifying.can_transition_to(&GoalState::Completed));
}

#[test]
fn terminal_states_are_immutable() {
    for terminal in [GoalState::Completed, GoalState::Failed, GoalState::Cancelled] {
        assert!(!terminal.can_transition_to(&GoalState::Planning));
        assert!(!terminal.can_transition_to(&GoalState::Executing));
    }
}

#[test]
fn active_states_can_fail_or_cancel() {
    for state in [
        GoalState::Planning,
        GoalState::PolicyCheck,
        GoalState::Scheduling,
        GoalState::Executing,
        GoalState::Reviewing,
        GoalState::Verifying,
        GoalState::Repairing,
        GoalState::Publishing,
    ] {
        assert!(state.can_transition_to(&GoalState::Failed));
        assert!(state.can_transition_to(&GoalState::Cancelled));
    }
}
