import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class RosterImportTests(unittest.TestCase):
    def test_imports_computers_without_credentials_or_mobile_devices(self):
        status = {
            "PrivateKey": "must-not-copy",
            "Self": {"HostName": "Studio", "DNSName": "studio.example.ts.net.",
                     "OS": "macOS", "Online": True, "TailscaleIPs": ["100.64.0.1"]},
            "Peer": {
                "phone": {"HostName": "Phone", "OS": "iOS", "TailscaleIPs": ["100.64.0.2"]},
                "server": {"HostName": "Server", "DNSName": "server.example.ts.net.",
                           "OS": "linux", "Online": False, "TailscaleIPs": ["100.64.0.3"]},
            },
        }
        with tempfile.TemporaryDirectory() as directory:
            source, output = Path(directory) / "status.json", Path(directory) / "roster.json"
            source.write_text(json.dumps(status))
            subprocess.run([sys.executable, str(Path(__file__).parents[1] / "scripts/import-tailnet.py"),
                            str(source), str(output), "--username", "me"], check=True, capture_output=True)
            roster = json.loads(output.read_text())
            self.assertEqual(roster["source"], "me@100.64.0.1")
            self.assertEqual([m["name"] for m in roster["machines"]], ["Studio", "Server"])
            self.assertEqual(roster["machines"][0]["hostname"], "studio.example.ts.net")
            self.assertFalse(roster["machines"][1]["online"])
            self.assertNotIn("PrivateKey", output.read_text())
            self.assertNotIn("must-not-copy", output.read_text())


if __name__ == "__main__":
    unittest.main()
