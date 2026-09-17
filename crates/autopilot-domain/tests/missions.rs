use autopilot_domain::{MissionRecord, MissionState};

#[test]
fn queued_mission_has_stable_initial_state() {
    let mission = MissionRecord::queued("fix checkout", "C:/repo");
    assert_eq!(mission.state, MissionState::Queued);
    assert_eq!(mission.phase, "Queued");
    assert_eq!(mission.attempt, 0);
    assert!(mission.lease_owner.is_none());
}

#[test]
fn terminal_mission_states_cannot_reenter_execution() {
    for state in [MissionState::Completed, MissionState::Failed, MissionState::Cancelled] {
        assert!(state.is_terminal());
        assert!(!state.can_transition_to(MissionState::Queued));
        assert!(!state.can_transition_to(MissionState::Running));
    }
}

#[test]
fn lease_and_recovery_transitions_are_explicit() {
    assert!(MissionState::Queued.can_transition_to(MissionState::Leased));
    assert!(MissionState::Leased.can_transition_to(MissionState::Running));
    assert!(MissionState::Leased.can_transition_to(MissionState::Queued));
    assert!(MissionState::Running.can_transition_to(MissionState::Waiting));
    assert!(MissionState::Waiting.can_transition_to(MissionState::Queued));
}
