from __future__ import annotations

from dataclasses import asdict, dataclass, field
from typing import Literal


class AutopilotContractError(ValueError):
    pass


@dataclass(frozen=True)
class AutopilotPolicyDecision:
    proposal_id: str
    decision: Literal["ALLOW", "DENY", "REQUIRE_APPROVAL", "DEFER"]
    policy_id: str
    capabilities: tuple[str, ...] = field(default_factory=tuple)
    required_approvals: tuple[str, ...] = field(default_factory=tuple)
    reason: str = ""

    def __post_init__(self) -> None:
        if not self.policy_id:
            raise AutopilotContractError("policy_id is required")
        if not self.reason:
            raise AutopilotContractError("decision reason is required")
        if self.decision == "ALLOW" and self.required_approvals:
            raise AutopilotContractError("ALLOW cannot retain unresolved approvals")
        if self.decision == "REQUIRE_APPROVAL" and not self.required_approvals:
            raise AutopilotContractError("REQUIRE_APPROVAL needs at least one authority")
        if any(cap in {"accounting-authority", "payment-signing-authority", "blockchain-admin-authority"} for cap in self.capabilities):
            raise AutopilotContractError("AUTOPILOT cannot mint domain authority through capabilities")

    def as_payload(self) -> dict:
        payload = asdict(self)
        payload["capabilities"] = list(self.capabilities)
        payload["required_approvals"] = list(self.required_approvals)
        return payload
