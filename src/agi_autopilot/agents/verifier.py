from __future__ import annotations

class VerifierAgent:
    def check_evidence(self,p:dict)->dict:
        required=[str(x) for x in (p.get('required') or [])]
        evidence=p.get('evidence') or {}
        if isinstance(evidence,list): evidence={str(x):True for x in evidence}
        present=[k for k in required if evidence.get(k) not in (None,'',False,[])]
        missing=[k for k in required if k not in present]
        return {'schema':'synergy.evidence-verification/v1','required':required,'present':present,'missing':missing,'verified':not missing,'authority':'EVIDENCE_CHECK_ONLY'}
