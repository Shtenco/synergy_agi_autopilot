from __future__ import annotations
import os
from .capabilities import CapabilitySpec, Effect
from .contracts import AutopilotPolicyDecision


HARD_DENY = {Effect.PAYMENT, Effect.ACCOUNTING, Effect.BLOCKCHAIN, Effect.HOST_EXEC}
APPROVAL_EFFECTS = {Effect.EXTERNAL_WRITE, Effect.CRM_WRITE, Effect.ADS_WRITE, Effect.PAID_API}


class PolicyEngine:
    policy_id = "synergy.autopilot.policy/v1"

    def decide(self, proposal_id: str, spec: CapabilitySpec, approved: bool = False) -> AutopilotPolicyDecision:
        if not spec.enabled:
            return AutopilotPolicyDecision(proposal_id, "DENY", self.policy_id, (spec.name,), (), "capability disabled")
        if spec.effect in HARD_DENY:
            return AutopilotPolicyDecision(proposal_id, "DENY", self.policy_id, (spec.name,), (), f"{spec.effect.value} authority is outside AUTOPILOT")
        needs = spec.requires_approval or spec.effect in APPROVAL_EFFECTS
        if needs and not approved:
            return AutopilotPolicyDecision(proposal_id, "REQUIRE_APPROVAL", self.policy_id, (spec.name,), ("company-operator",), "external/cost-bearing side effect requires explicit approval")
        return AutopilotPolicyDecision(proposal_id, "ALLOW", self.policy_id, (spec.name,), (), "bounded capability allowed")

    @staticmethod
    def external_effects_enabled() -> bool:
        return os.getenv("AUTOPILOT_EXTERNAL_EFFECTS", "0").strip().lower() in {"1","true","yes","on"}
