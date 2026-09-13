from __future__ import annotations

class CoderAgent:
    def change_plan(self,p:dict)->dict:
        objective=str(p.get('objective') or p.get('goal') or '').strip()
        repo=str(p.get('repository') or '').strip()
        files=[str(x) for x in (p.get('files') or [])]
        return {
            'schema':'synergy.code-change-plan/v1',
            'repository':repo or None,
            'objective':objective,
            'candidate_files':files,
            'steps':['inspect authoritative contracts','make additive bounded change','run focused tests','record evidence'],
            'acceptance_criteria':p.get('acceptance_criteria') or ['tests pass','authority boundaries preserved','no secrets committed'],
            'execution_authority':'NONE',
            'shell_authority':'NONE',
        }
