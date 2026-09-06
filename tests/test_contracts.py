import pytest

from agi_autopilot import AutopilotContractError, AutopilotPolicyDecision


def test_require_approval_needs_named_authority():
    with pytest.raises(AutopilotContractError):
        AutopilotPolicyDecision(
            proposal_id="p1",
            decision="REQUIRE_APPROVAL",
            policy_id="finance-v1",
            reason="capital at risk",
        )


def test_autopilot_cannot_mint_financial_authority():
    with pytest.raises(AutopilotContractError):
        AutopilotPolicyDecision(
            proposal_id="p1",
            decision="ALLOW",
            policy_id="finance-v1",
            capabilities=("accounting-authority",),
            reason="invalid authority escalation",
        )


def test_bounded_approval_flow():
    decision = AutopilotPolicyDecision(
        proposal_id="p1",
        decision="REQUIRE_APPROVAL",
        policy_id="finance-v1",
        capabilities=("submit-finance-proposal",),
        required_approvals=("financial_authority",),
        reason="domain approval required",
    )
    assert decision.as_payload()["required_approvals"] == ["financial_authority"]
