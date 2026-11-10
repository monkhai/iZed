#!/usr/bin/env python3
"""Apply Android IDE additions to an explicitly selected, iOS-patched Zed checkout."""
import argparse
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('zed_source', type=Path, help='isolated checkout used by this build; not a shared Cargo cache')
args = parser.parse_args()
source = args.zed_source.resolve()
revision = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
if revision != '5688167d224b5eca54875d49afb8bfd73a07915a':
    parser.error('Zed source does not match the pinned revision')
patch = Path(__file__).resolve().parents[1] / 'patches/android-ide.patch'
command = ['git', '-C', str(source), 'apply']
if subprocess.run(command + ['--reverse', '--check', str(patch)], capture_output=True).returncode == 0:
    print('Android IDE patch is already applied')
else:
    subprocess.run(command + ['--check', str(patch)], check=True)
    subprocess.run(command + [str(patch)], check=True)
    print('Applied Android IDE patch; existing iOS behavior is retained')
