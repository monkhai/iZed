"""Android equivalent of the iOS SSH fixture smoke test.

Run only on an approved test emulator after Tailscale sign-in and fixture setup.
Requires its exact printed host fingerprint. Never reads private key material.
"""
import argparse
import json
import re
import subprocess
import time
import xml.etree.ElementTree as ET
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adb", default="adb")
    parser.add_argument("--serial", required=True)
    parser.add_argument("--address", required=True)
    parser.add_argument("--machine", default="ized-ssh-fixture")
    parser.add_argument("--fingerprint", required=True)
    parser.add_argument("--events", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not args.serial.startswith("emulator-"):
        parser.error("This test is restricted to emulators")
    if not re.fullmatch(r"[a-zA-Z0-9_.:@\[\]-]+", args.address):
        parser.error("Use a plain user@host[:port] fixture address")
    if not re.fullmatch(r"SHA256:[A-Za-z0-9+/]+", args.fingerprint):
        parser.error("Supply the fixture's exact SHA256 host fingerprint")
    args.output.mkdir(parents=True, exist_ok=True)
    app = "com.monkhai.ized"

    def adb(*command, binary=False):
        return subprocess.check_output([args.adb, "-s", args.serial, *command],
                                       text=not binary, timeout=30)

    def tree():
        adb("shell", "uiautomator", "dump", "/sdcard/ized-terminal-smoke.xml")
        return ET.fromstring(adb("exec-out", "cat", "/sdcard/ized-terminal-smoke.xml"))

    def wait(predicate, timeout=35):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            result = predicate(tree())
            if result is not None and result is not False:
                return result
            time.sleep(0.3)
        raise AssertionError("Timed out waiting for Android terminal UI")

    def find(root, text):
        return next((n for n in root.iter("node")
                     if n.get("text", "").casefold() == text.casefold()
                     or n.get("content-desc", "").casefold() == text.casefold()), None)

    def bounds(node):
        return tuple(map(int, re.findall(r"\d+", node.get("bounds"))))

    def tap(node):
        left, top, right, bottom = bounds(node)
        adb("shell", "input", "tap", str((left + right) // 2), str((top + bottom) // 2))

    def click(text):
        tap(wait(lambda root: find(root, text)))

    def screen(root):
        return next((n for n in root.iter("node")
                     if n.get("content-desc", "").startswith("ssh-terminal-screen\n")), None)

    def screen_contains(value):
        def predicate(root):
            node = screen(root)
            return node if node is not None and value in node.get("content-desc", "") else None
        return wait(predicate)

    def capture(name):
        (args.output / (name + ".png")).write_bytes(adb("exec-out", "screencap", "-p", binary=True))

    def restart():
        adb("shell", "am", "force-stop", app)
        adb("shell", "am", "start", "-W", "-n", app + "/.platform.GpuiActivity")
        wait(lambda root: find(root, "Machines"))
        wait(lambda root: find(root, "Save device name"), timeout=60)

    def connect():
        click(args.machine)
        root = tree()
        if find(root, "Your account on this machine") is not None:
            capture("00-connection-editor")
            click("Your account on this machine")
            adb("shell", "input", "text", args.address.split("@", 1)[0])
            click("Open terminal  →")

    def verify_prompt():
        root = wait(lambda root: root if find(root, "Verify host identity") is not None
                    or find(root, "Host identity changed") is not None else None)
        assert args.fingerprint in "\n".join(n.get("text", "") for n in root.iter("node")), \
            "Refusing trust: fixture fingerprint does not match"
        return root

    def events():
        if not args.events.exists():
            return []
        return [json.loads(line) for line in args.events.read_text().splitlines() if line.strip()]

    baseline = len(events())
    try:
        restart()
        print("Tailscale ready", flush=True)
        capture("00-machines")
        public_key = adb("exec-out", "run-as", app, "cat", "no_backup/device-public-key.pub")
        assert public_key.startswith("ssh-ed25519 ")
        connect()
        verify_prompt()
        capture("01-host-verification")
        click("Cancel")
        time.sleep(1)
        assert not any(e["kind"] == "auth" and e.get("accepted") for e in events()[baseline:]), \
            "Trust rejection must not authenticate"
        print("Host trust rejection passed", flush=True)
        restart()
        assert public_key == adb("exec-out", "run-as", app, "cat", "no_backup/device-public-key.pub"), \
            "SSH identity changed across process restart"
        connect()
        root = verify_prompt()
        click("Trust new identity" if find(root, "Host identity changed") is not None else "Trust and connect")
        print("Host fingerprint verified; trust accepted", flush=True)
        screen_contains("SSH ready")
        grid = screen_contains("UTF-8: ✓")
        capture("02-terminal-keyboard")
        print("SSH and UTF-8 output passed", flush=True)
        adb("shell", "input", "text", "phone_ssh_smoke")
        adb("shell", "input", "keyevent", "66")
        grid = screen_contains("PHONE_SSH_OK")
        capture("03-command")
        print("Typed command passed", flush=True)
        density = adb("shell", "wm", "density")
        scale = int(re.findall(r"density:\s*(\d+)", density)[-1]) / 160
        left, top, right, bottom = bounds(grid)
        # Same shared seven-button accessory geometry as TerminalUITests.swift.
        content_left = left - 10 * scale
        content_width = right - left + 20 * scale
        width = (content_width - 24 * scale) / 7
        x = content_left + 6 * scale + 2 * (width + 2 * scale) + width / 2
        y = bottom + 36 * scale
        adb("shell", "input", "tap", str(round(x)), str(round(y)))
        adb("shell", "input", "text", "c")
        screen_contains("^C")
        capture("04-control-c")
        print("Ctrl-C passed", flush=True)
        adb("shell", "input", "keycombination", "113", "31")
        wait(lambda root: n if (n := screen(root)) is not None and n.get("content-desc", "").count("^C") >= 2 else None)
        assert sum(e["kind"] == "control_c" for e in events()[baseline:]) >= 2
        print("Hardware Ctrl-C passed", flush=True)
        # Hiding and reopening the IME must resize the real remote PTY.
        adb("shell", "input", "keyevent", "4")
        expanded = wait(lambda root: n if (n := screen(root)) is not None and bounds(n)[3] > bottom else None)
        tap(expanded)
        wait(lambda root: n if (n := screen(root)) is not None and bounds(n)[3] <= bottom else None)
        capture("05-resize")
        print("PTY resize passed", flush=True)
        adb("shell", "input", "keyevent", "3")
        adb("shell", "am", "start", "-W", "-n", app + "/.platform.GpuiActivity")
        screen_contains("PHONE_SSH_OK")
        adb("shell", "input", "text", "resume_smoke")
        adb("shell", "input", "keyevent", "66")
        screen_contains("resume_smoke")
        assert any(e["kind"] == "command" and e.get("text") == "resume_smoke" for e in events()[baseline:])
        capture("06-resume")
        print("Resume passed", flush=True)
        observed = events()[baseline:]
        assert any(e["kind"] == "auth" and e.get("accepted") for e in observed)
        assert any(e["kind"] == "command" and e.get("text") == "phone_ssh_smoke" for e in observed)
        assert any(e["kind"] == "control_c" for e in observed)
        sizes = {(e["columns"], e["rows"]) for e in observed if e["kind"] == "resize"}
        assert len(sizes) >= 2, "Expected distinct remote PTY sizes after IME resize"
        result = {"passed": ["trust rejection", "fingerprint verification", "identity persistence",
                  "SSH public-key authentication", "ANSI/UTF-8 output", "typed input", "accessory Ctrl-C", "hardware Ctrl-C", "PTY resize", "resume"],
                  "address": args.address, "fingerprint": args.fingerprint, "resize_sizes": sorted(sizes)}
        (args.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result, indent=2))
    finally:
        adb("shell", "rm", "-f", "/sdcard/ized-terminal-smoke.xml")


if __name__ == "__main__":
    main()
