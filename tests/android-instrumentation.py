"""Run real Android UI/data tests, including a deliberate app-process restart.

Requires an explicitly selected dedicated test device with matching app/test APKs installed.
Uses random synthetic connection IDs and fake file content; does not touch any DSH Host.
"""
import argparse
import re
import subprocess
import sys
import uuid

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--serial', required=True, help='Dedicated emulator/device serial; no implicit default')
parser.add_argument('--suite', choices=['all', 'attachments', 'deletion'], default='all')
parser.add_argument('--tls-fixture-url', help='Optional loopback HTTPS fixture URL; arrange adb reverse first')
args = parser.parse_args()
adb = ['adb', '-s', args.serial]
runner = 'com.zcj.dshclient.test/androidx.test.runner.AndroidJUnitRunner'


def instrument(label, extras):
    result = subprocess.run([*adb, 'shell', 'am', 'instrument', '-w', *extras, runner],
                            capture_output=True, text=True, timeout=150)
    print(f'--- {label} ---\n{result.stdout}', flush=True)
    if result.stderr:
        print(result.stderr, file=sys.stderr)
    # `am instrument` may exit zero even when JUnit failed. Check the runner verdict too.
    if result.returncode or 'FAILURES' in result.stdout or not re.search(r'OK \(\d+ tests?\)', result.stdout):
        raise SystemExit(f'{label} failed; do not report the run as passed')


if args.suite == 'all':
    extras = ['-e', 'class', 'com.zcj.dshclient.ProfileIsolationTest,com.zcj.dshclient.AttachmentSelectionTest']
    if args.tls_fixture_url:
        extras += ['-e', 'tlsFixtureUrl', args.tls_fixture_url]
    else:
        print('TLS fixture not configured: certificate test will be skipped; do not count it as verified.', flush=True)
    instrument('profile/status/attachment checks', extras)
elif args.suite == 'attachments':
    instrument('attachment checks', ['-e', 'class', 'com.zcj.dshclient.AttachmentSelectionTest'])

if args.suite in ['all', 'deletion']:
    identity = str(uuid.uuid4())
    for phase in ['prepare', 'verify']:
        subprocess.run([*adb, 'shell', 'am', 'force-stop', 'com.zcj.dshclient'], check=True)
        instrument(f'deletion lifecycle: {phase}', [
            '-e', 'class', 'com.zcj.dshclient.ProfileDeletionLifecycleTest',
            '-e', 'deletionPhase', phase, '-e', 'deletionId', identity,
        ])
    print('Verified loaded-profile tombstone persistence and next-process deletion.', flush=True)
