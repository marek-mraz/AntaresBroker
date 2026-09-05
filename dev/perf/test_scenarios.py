#!/usr/bin/env python3
"""Unit tests for scenario-check.py and scenario orchestration logic."""

import unittest
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import importlib
scenario_check = importlib.import_module("scenario-check")


class TestScenarioHelpers(unittest.TestCase):

    def test_http_req_handling(self):
        # http_req handles bad urls gracefully returning code 0
        code, body, headers = scenario_check.http_req("http://127.0.0.1:65530/test", timeout=0.1)
        self.assertEqual(code, 0)
        self.assertIsInstance(body, bytes)

    def test_no_fabricated_numbers_in_scenarios_sh(self):
        scenarios_path = os.path.join(os.path.dirname(__file__), "scenarios.sh")
        import re
        pat_ms = re.compile(r"[0-9]+\.[0-9]+ ms \|")
        pat_pct = re.compile(r"100\.0%")
        with open(scenarios_path, "r", encoding="utf-8") as f:
            for idx, line in enumerate(f, 1):
                if line.strip().startswith('echo "|') or 'echo "|' in line:
                    self.assertFalse(
                        pat_ms.search(line),
                        f"Line {idx} in scenarios.sh contains fabricated latency pattern: {line.strip()}"
                    )
                    self.assertFalse(
                        pat_pct.search(line),
                        f"Line {idx} in scenarios.sh contains fabricated percentage pattern: {line.strip()}"
                    )

    def test_what_this_shows_lines_carry_a_measured_value(self):
        # a reading of the numbers is either conditional (indented under if/else) or names a value
        scenarios_path = os.path.join(os.path.dirname(__file__), "scenarios.sh")
        with open(scenarios_path, "r", encoding="utf-8") as f:
            for idx, line in enumerate(f, 1):
                if line.startswith('      echo "- '):
                    self.assertIn("$", line, f"Line {idx} in scenarios.sh claims a result it did not measure: {line.strip()}")

    def test_every_scenario_calls_check_in_both_modes(self):
        scenarios_path = os.path.join(os.path.dirname(__file__), "scenarios.sh")
        with open(scenarios_path, "r", encoding="utf-8") as f:
            content = f.read()
        import re
        scenarios = [
            "hot_entity", "noisy_tenant", "slow_subscriber", "fan_in",
            "hub_sources", "collision", "loop", "distributed_subscription", "ha_pair"
        ]
        for s in scenarios:
            pattern = rf"scenario_{s}\(\)\s*\{{(.*?)\n\}}"
            m = re.search(pattern, content, re.DOTALL)
            self.assertIsNotNone(m, f"scenario_{s} function not found in scenarios.sh")
            fn_body = m.group(1)
            check_calls = re.findall(r"\$CHECK\s+", fn_body)
            self.assertGreaterEqual(
                len(check_calls), 2,
                f"scenario_{s} must call $CHECK in both check and load modes, found {len(check_calls)} calls"
            )


if __name__ == "__main__":
    unittest.main()
