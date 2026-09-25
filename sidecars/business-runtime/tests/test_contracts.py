import pytest
from agi_autopilot.contracts import AutopilotContractError,AutopilotPolicyDecision

def test_forbidden_authority():
    with pytest.raises(AutopilotContractError): AutopilotPolicyDecision('x','ALLOW','p',('accounting-authority',),(), 'no')
