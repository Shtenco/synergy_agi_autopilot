from __future__ import annotations
from dataclasses import asdict, dataclass
from enum import StrEnum
from typing import Any, Callable


class Effect(StrEnum):
    READ = "read"
    LOCAL_COMPUTE = "local_compute"
    EXTERNAL_WRITE = "external_write"
    CRM_WRITE = "crm_write"
    ADS_WRITE = "ads_write"
    PAID_API = "paid_api"
    PAYMENT = "payment"
    ACCOUNTING = "accounting"
    BLOCKCHAIN = "blockchain"
    HOST_EXEC = "host_exec"


@dataclass(frozen=True)
class CapabilitySpec:
    name: str
    agent: str
    effect: Effect
    description: str
    requires_approval: bool = False
    enabled: bool = True
    source_lineage: str = "new-runtime"

    def payload(self) -> dict[str, Any]:
        d = asdict(self); d["effect"] = self.effect.value; return d


class CapabilityRegistry:
    def __init__(self) -> None:
        self._specs: dict[str, CapabilitySpec] = {}
        self._handlers: dict[str, Callable[[dict], Any]] = {}

    def register(self, spec: CapabilitySpec, handler: Callable[[dict], Any]) -> None:
        if spec.name in self._specs:
            raise ValueError(f"duplicate capability: {spec.name}")
        self._specs[spec.name] = spec
        self._handlers[spec.name] = handler

    def spec(self, name: str) -> CapabilitySpec:
        if name not in self._specs: raise KeyError(name)
        return self._specs[name]

    def handler(self, name: str):
        if name not in self._handlers: raise KeyError(name)
        return self._handlers[name]

    def list(self) -> list[dict]:
        return [self._specs[k].payload() for k in sorted(self._specs)]
