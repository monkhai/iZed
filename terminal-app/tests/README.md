# SSH smoke fixture

`ssh_fixture.py` performs real SSH authentication and channel traffic, but its
terminal responses are scripted: Enter returns `PHONE_SSH_OK`. It never executes
received commands or starts a real shell/PTY process.

From `terminal-app`, install Paramiko in an isolated environment, then run:

```sh
python3 -m venv /tmp/ized-ssh-fixture-venv
/tmp/ized-ssh-fixture-venv/bin/python -m pip install paramiko
/tmp/ized-ssh-fixture-venv/bin/python tests/ssh_fixture.py \
  --key /path/to/device-public-key.pub --events /tmp/ized-ssh-events.jsonl
```

Supply only the test client's OpenSSH public key. The default listener is
`127.0.0.1:22222`; `--bind` and `--port` can override it. Binding beyond loopback
exposes the server on that interface. Tailscale routing is separate; loopback
alone does not make it reachable by the production iZed client.

Compare the printed SHA-256 host fingerprint with the client's trust prompt.
The existing host key is generated in memory on each start, so restarting changes
its fingerprint. Never blindly accept a changed-host warning.

The event file records authentication results, PTY/resize requests, input and
Ctrl-C. Use synthetic input only; logs persist until you remove them. Stop with
Ctrl-C. Do not commit keys, logs or virtual environments.

The fixture echoes bytes, handles CRLF as one submission and ignores Backspace
on empty input. It is not a full Unicode editor or normal shell. ANSI/UTF-8 banner
writes are split deliberately, but SSH/TCP may coalesce them.
