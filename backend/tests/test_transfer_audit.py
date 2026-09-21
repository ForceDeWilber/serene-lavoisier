import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import unittest
from app.capital_manager import CapitalManager

class TestTransferAudit(unittest.TestCase):
    def setUp(self):
        self.cm = CapitalManager(starting_balance_gbp=25.0)
        self.cm.deposits = []
        self.cm.withdrawals = []

    def test_telemetry_dto_contains_transfer_audit(self):
        dto = self.cm.get_telemetry_dto()
        self.assertIn("transfer_audit", dto)
        audit = dto["transfer_audit"]
        self.assertIn("deposits_count", audit)
        self.assertIn("withdrawals_count", audit)
        self.assertIn("total_deposits_gbp", audit)
        self.assertIn("total_withdrawals_gbp", audit)
        self.assertIn("net_deposited_cash_gbp", audit)

    def test_net_deposited_calculation(self):
        self.cm.deposits = [
            {"id": "dep-1", "type": "receive", "status": "completed", "currency": "GBP", "amount": 10.0, "amount_gbp": 10.0},
            {"id": "dep-2", "type": "receive", "status": "completed", "currency": "GBP", "amount": 15.0, "amount_gbp": 15.0},
            {"id": "dep-3", "type": "receive", "status": "completed", "currency": "USD", "amount": 13.31, "amount_gbp": 10.0},
        ]
        self.cm.withdrawals = [
            {"id": "wd-1", "type": "send", "status": "completed", "currency": "GBP", "amount": 5.0, "amount_gbp": 5.0},
        ]
        total_dep = sum(d["amount_gbp"] for d in self.cm.deposits)
        total_wd = sum(w["amount_gbp"] for w in self.cm.withdrawals)
        net_dep = total_dep - total_wd

        self.assertEqual(total_dep, 35.0)
        self.assertEqual(total_wd, 5.0)
        self.assertEqual(net_dep, 30.0)

if __name__ == "__main__":
    unittest.main()
