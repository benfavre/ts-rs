#!/usr/bin/env python3
"""Compare complete site-check responses and API process counts on generated projects."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--projects', type=int, default=128)
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--rounds', type=int, default=5)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if min(args.projects, args.jobs, args.rounds) < 1:
        parser.error('projects, jobs and rounds must be positive')
    native = os.environ.get('TSRS_BINARY')
    if not native:
        version = (Path(__file__).resolve().parent.parent / 'TSRS_VERSION').read_text().strip()
        for prefix in [Path('/usr/local/share/tsc-rs/tsrs'), Path.home() / '.local/share/tsc-rs/tsrs']:
            candidate = prefix / version / 'tsrs'
            if candidate.is_file():
                native = str(candidate)
                break
    if not native:
        parser.error('install the pinned checker or set TSRS_BINARY to its native executable')
    env = {**os.environ, 'TSRS_BINARY': native, 'RAYON_NUM_THREADS': '4'}
    env.pop('TSC_RS_CHECKER', None)
    env.pop('TSC_RS_EMIT', None)
    timings = {'before': [], 'after': []}
    counts = {}
    binaries = {'before': args.before.resolve(), 'after': args.after.resolve()}
    with tempfile.TemporaryDirectory(prefix='tsrs-site-bench-') as temporary:
        root = Path(temporary)
        sites = []
        for number in range(args.projects):
            site = root / f'project-{number:04}'
            (site / 'lib').mkdir(parents=True)
            (site / 'src').mkdir()
            (site / 'tsconfig.json').write_text(json.dumps({
                'compilerOptions': {'target': 'ES2022', 'module': 'ESNext',
                    'moduleResolution': 'bundler', 'strict': True,
                    'paths': {'@data/*': ['./lib/*']}}, 'include': ['src/*.ts'],
            }))
            value = "'wrong'" if number % 7 == 0 else '1'
            (site / 'lib/value.ts').write_text(f'export const value = {value};\n')
            (site / 'src/input.ts').write_text("import {value} from '@data/value'; export const result: number = value;\n")
            sites.append(str(site))
        reference = None

        def run(label, instrument=False):
            nonlocal reference
            settings = env.copy()
            if instrument:
                settings.update(TSRS_BINARY=str(root / 'checker-proxy'),
                                NATIVE_TSRS=native, API_START_LOG=str(root / 'api-starts'))
                (root / 'api-starts').write_text('')
            started = time.perf_counter()
            output = subprocess.run([str(binaries[label]), 'check-site', *sites,
                '--json', '--jobs', str(args.jobs)], env=settings,
                capture_output=True, text=True, timeout=120, check=True)
            elapsed = time.perf_counter() - started
            reports = [json.loads(line) for line in output.stdout.splitlines()]
            assert len(reports) == args.projects, (label, len(reports))
            for report in reports:
                assert not report.get('skipped'), report
                report.pop('elapsed_ms', None)
            if reference is None:
                reference = reports
            assert reports == reference, f'{label}: site-check responses changed'
            return elapsed

        # Timed runs use the native executable directly, without instrumentation.
        for number in range(args.rounds):
            for label in (['before', 'after'] if number % 2 == 0 else ['after', 'before']):
                timings[label].append(run(label))
        proxy = root / 'checker-proxy'
        proxy.write_text('#!/bin/sh\nif [ "$1" = --api ]; then printf "api\\n" >> "$API_START_LOG"; fi\nexec "$NATIVE_TSRS" "$@"\n')
        proxy.chmod(0o755)
        for label in binaries:
            run(label, instrument=True)
            counts[label] = len((root / 'api-starts').read_text().splitlines())
    medians = {name: statistics.median(values) for name, values in timings.items()}
    result = {
        'projects': args.projects, 'jobs': args.jobs, 'rounds': args.rounds,
        'seconds': timings, 'median_seconds': medians,
        'median_change_percent': 100 * (medians['after'] / medians['before'] - 1),
        'api_processes': counts, 'responses_equal': True,
        'sha256': {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in binaries.items()},
        'notes': ['Process-count runs use a separate instrumented invocation.',
                  'This fixture measures small independent projects, not large monorepo checking.'],
    }
    args.out.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
