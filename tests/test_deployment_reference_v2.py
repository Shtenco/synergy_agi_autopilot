import copy, hashlib, json, unittest
from pathlib import Path
from scripts.verify_deployment_reference_v2 import verify, DeploymentReferenceV2Error, canonical

ROOT = Path(__file__).resolve().parents[1]

class DeploymentReferenceV2Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.value = json.loads((ROOT/"evidence"/"deployment_reference_v2.json").read_text(encoding="utf-8"))

    def test_canonical(self):
        self.assertEqual(verify(self.value), "87b9bb8158652dacc02832b465d94fddabfd31c9c6f5aa20bc9687b456bae879")

    def test_reject_upstream_substitution_with_fresh_hash(self):
        for field in (
            "synergynet_location_evidence_v3_digest",
            "synergynet_transport_binding_v2_digest",
            "meta_os_profile_v3_digest",
            "meta_os_admission_v3_digest",
            "production_bootstrap_v4_digest",
            "system_registry_v3_digest",
            "preproduction_rc4_digest",
            "ai_blockchain_preproduction_commitment_v3_digest",
        ):
            c = copy.deepcopy(self.value); c[field] = "0"*64
            p = dict(c); p.pop("deployment_reference_digest", None)
            c["deployment_reference_digest"] = hashlib.sha256(canonical(p)).hexdigest()
            with self.subTest(field=field), self.assertRaises(DeploymentReferenceV2Error): verify(c)

    def test_reject_running_or_authority_escalation(self):
        for field, value in (
            ("current_running", True), ("nexus_network_access", "FULL"),
            ("financial_authority", "ROOT"), ("payment_authority", "ROOT"),
            ("accounting_authority", "ROOT"), ("host_execution_authority", "ROOT"),
            ("blockchain_submission_authority", "ROOT"), ("execution_authority", "ROOT"),
        ):
            c = copy.deepcopy(self.value); c[field] = value
            p = dict(c); p.pop("deployment_reference_digest", None)
            c["deployment_reference_digest"] = hashlib.sha256(canonical(p)).hexdigest()
            with self.subTest(field=field), self.assertRaises(DeploymentReferenceV2Error): verify(c)

if __name__ == "__main__": unittest.main()
