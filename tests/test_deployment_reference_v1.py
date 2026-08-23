import copy
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from verify_deployment_reference_v1 import DeploymentReferenceError, verify_reference  # noqa: E402


class DeploymentReferenceV1Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.reference = json.loads((ROOT / "evidence" / "deployment_reference_v1.json").read_text(encoding="utf-8"))

    def test_canonical_reference(self):
        self.assertEqual(
            verify_reference(self.reference),
            "19375c02a286a4c14e704ad0496d1c6084ddb00e7c4bcf3506a304a6b0946b70",
        )

    def assert_rejected(self, field, value):
        candidate = copy.deepcopy(self.reference)
        candidate[field] = value
        candidate["deployment_reference_digest"] = "0" * 64
        with self.assertRaises(DeploymentReferenceError):
            verify_reference(candidate)

    def test_reject_integration_root_substitution(self):
        self.assert_rejected("integration_manifest_digest", "f" * 64)

    def test_reject_registry_substitution(self):
        self.assert_rejected("system_registry_v2_digest", "f" * 64)

    def test_reject_meta_admission_substitution(self):
        self.assert_rejected("meta_os_admission_v2_digest", "f" * 64)

    def test_reject_transport_substitution(self):
        self.assert_rejected("synergynet_transport_binding_digest", "f" * 64)

    def test_reject_blockchain_commitment_substitution(self):
        self.assert_rejected("ai_blockchain_deployment_commitment_v2_digest", "f" * 64)

    def test_reject_production_gate_substitution(self):
        self.assert_rejected("production_transition_gate_digest", "f" * 64)

    def test_reject_current_running_promotion(self):
        self.assert_rejected("current_running", True)

    def test_reject_production_promotion(self):
        self.assert_rejected("production_status", "RUNNING_PRODUCTION")

    def test_reject_authority_escalation(self):
        for field in (
            "financial_authority",
            "payment_authority",
            "accounting_authority",
            "blockchain_submission_authority",
            "execution_authority",
        ):
            with self.subTest(field=field):
                self.assert_rejected(field, "ROOT")


if __name__ == "__main__":
    unittest.main()
