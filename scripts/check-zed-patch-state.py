#!/usr/bin/env python3
"""Exit successfully when a Zed checkout already contains the complete patch stack.

Later patches can change lines introduced by earlier ones, so checking each patch
in reverse against the final tree is not enough. Build the expected final diff in
a temporary Git index, then check that the checkout can reverse that combined diff.
"""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys
import tempfile


def git(
    source: Path, args: list[str], *, env: dict[str, str], input: bytes | None = None
) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        ["git", "-C", str(source), *args],
        env=env,
        input=input,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def main() -> int:
    source = Path(sys.argv[1])
    patches = [Path(path) for path in sys.argv[2:]]
    with tempfile.TemporaryDirectory(prefix="ized-zed-patches-") as directory:
        objects = Path(directory) / "objects"
        objects.mkdir()
        original_objects = git(source, ["rev-parse", "--path-format=absolute", "--git-path", "objects"], env=os.environ)
        if original_objects.returncode:
            sys.stderr.buffer.write(original_objects.stderr)
            return 2
        alternates = [original_objects.stdout.decode().strip()]
        if extra_alternates := os.environ.get("GIT_ALTERNATE_OBJECT_DIRECTORIES"):
            alternates.append(extra_alternates)
        env = {
            **os.environ,
            "GIT_INDEX_FILE": str(Path(directory) / "index"),
            "GIT_OBJECT_DIRECTORY": str(objects),
            "GIT_ALTERNATE_OBJECT_DIRECTORIES": os.pathsep.join(alternates),
        }
        if (result := git(source, ["read-tree", "HEAD"], env=env)).returncode:
            sys.stderr.buffer.write(result.stderr)
            return 2
        for patch in patches:
            if (result := git(source, ["apply", "--cached", str(patch)], env=env)).returncode:
                sys.stderr.buffer.write(result.stderr)
                return 2
        # Cargo can update its lockfile after preparation without changing the
        # source patches. Ignore that generated drift when checking the state.
        expected = git(source, ["diff", "--cached", "--binary", "HEAD", "--", ".", ":!Cargo.lock"], env=env)
        if expected.returncode:
            sys.stderr.buffer.write(expected.stderr)
            return 2
        return git(source, ["apply", "--reverse", "--check", "-"], env=env, input=expected.stdout).returncode


if __name__ == "__main__":
    raise SystemExit(main())
