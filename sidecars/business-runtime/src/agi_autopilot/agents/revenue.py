from __future__ import annotations

class RevenueAgent:
    """Evidence-first opportunity validator. It does not contact buyers or claim revenue."""
    def validate(self,p:dict)->dict:
        o=p.get('opportunity') or p
        title=str(o.get('title','')).strip(); source=str(o.get('source','')).strip(); url=str(o.get('url','')).strip()
        evidence=[str(x) for x in (o.get('evidence') or []) if str(x).strip()]
        expected=float(o.get('expected_revenue_usd',0) or 0); cost=float(o.get('estimated_cost_usd',0) or 0)
        probability=max(0.0,min(1.0,float(o.get('probability',0) or 0)))
        missing=[]
        if not title: missing.append('title')
        if not source: missing.append('source')
        if not (url or evidence): missing.append('buyer_or_source_evidence')
        if expected <= 0: missing.append('budget_or_revenue_evidence')
        confidence=max(0.0,min(1.0, probability + min(0.3,0.08*len(evidence)) + (0.1 if url else 0.0) - 0.12*len(missing)))
        return {
            'schema':'synergy.revenue-validation/v1',
            'buyer': o.get('buyer') or None,
            'source': source,
            'source_url': url or None,
            'budget_evidence': expected if expected>0 else None,
            'expected_revenue_usd': expected,
            'estimated_cost_usd': cost,
            'expected_margin_usd': expected-cost,
            'deliverable': o.get('description') or title,
            'deadline': o.get('deadline'),
            'confidence': round(confidence,4),
            'missing_evidence': missing,
            'next_action': 'prepare_bounded_proposal' if not missing and confidence>=0.5 else 'collect_more_evidence',
            'revenue_authority':'NONE',
        }
