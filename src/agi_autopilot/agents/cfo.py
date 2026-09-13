from __future__ import annotations
from typing import Any


def _f(v: Any) -> float:
    try: return float(v or 0)
    except (TypeError, ValueError): return 0.0


class CFOAgent:
    """Analytical CFO only. It never books entries, sends funds, or authorizes spend."""
    def analyze_budget(self, p: dict) -> dict:
        revenue = _f(p.get("revenue_usd"))
        costs = p.get("costs", {}) or {}
        if isinstance(costs, list): total_cost = sum(_f(x.get("amount_usd")) for x in costs if isinstance(x, dict))
        else: total_cost = sum(_f(v) for v in costs.values()) if isinstance(costs, dict) else _f(costs)
        budget = _f(p.get("budget_usd"))
        ad_spend = _f(p.get("ad_spend_usd"))
        margin = revenue - total_cost
        return {
            "schema":"synergy.cfo-analysis/v1", "revenue_usd": revenue, "cost_usd": total_cost,
            "operating_margin_usd": margin, "margin_pct": (margin/revenue*100 if revenue else None),
            "budget_usd": budget, "budget_variance_usd": (budget-total_cost if budget else None),
            "roas": (revenue/ad_spend if ad_spend else None), "authority":"ANALYTICAL_PROPOSAL_ONLY",
            "warnings": (["negative operating margin"] if margin < 0 else []) + (["budget exceeded"] if budget and total_cost > budget else []),
        }

    def unit_economics(self, p: dict) -> dict:
        spend=_f(p.get("acquisition_spend_usd")); customers=_f(p.get("new_customers")); arpu=_f(p.get("arpu_usd")); gross=_f(p.get("gross_margin_pct"))/100
        churn=_f(p.get("monthly_churn_pct"))/100; other=_f(p.get("variable_cost_per_customer_usd"))
        cac=spend/customers if customers else None
        ltv=(arpu*gross/churn if churn > 0 else None)
        contrib=arpu*gross-other
        return {"schema":"synergy.unit-economics/v1","cac_usd":cac,"ltv_usd":ltv,"ltv_cac":(ltv/cac if ltv is not None and cac else None),"monthly_contribution_usd":contrib,"payback_months":(cac/contrib if cac is not None and contrib>0 else None),"authority":"ANALYTICAL_PROPOSAL_ONLY"}

    def capital_proposal(self, p: dict) -> dict:
        cash=_f(p.get("available_cash_usd")); reserve=_f(p.get("minimum_reserve_usd")); requested=max(0,_f(p.get("requested_spend_usd")))
        capacity=max(0,cash-reserve)
        return {"schema":"synergy.capital-proposal/v1","requested_spend_usd":requested,"observed_capacity_usd":capacity,"within_observed_capacity":requested<=capacity,"authority":"NONE","note":"Financial OS remains canonical for capacity/authorization/accounting."}
