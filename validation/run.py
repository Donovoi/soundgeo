"""Real-video acceptance runner. Ground truth is never passed to SoundGeo.

Requires the locally retained recordings listed by SHA-256 in cases.json.
Videos are not redistributed. Each uncached reference day is several GB.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess


def distance_m(a, b):
    lat1, lat2 = map(math.radians, [a['latitude'], b['latitude']])
    dlat = lat2 - lat1
    dlon = math.radians(b['longitude'] - a['longitude'])
    h = math.sin(dlat / 2) ** 2 + math.cos(lat1) * math.cos(lat2) * math.sin(dlon / 2) ** 2
    return 6371008.8 * 2 * math.asin(min(1, math.sqrt(h)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--media-root', type=Path, required=True)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', help='One case ID; omit to run every case')
    args = parser.parse_args()
    manifest = json.loads(Path(__file__).with_name('cases.json').read_text())
    cases = [c for c in manifest['cases'] if not args.case or c['id'] == args.case]
    if not cases:
        parser.error('Unknown case ID')
    args.output.mkdir(parents=True, exist_ok=True)
    rows = []
    for case in cases:
        media = (args.media_root / case['id'] / 'recording.mp4').resolve()
        with media.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        if digest != case['sha256']:
            raise ValueError(f"{case['id']}: media differs from the benchmark capture")
        job = {
            'media': str(media), 'tracks': '', 'start_utc': None,
            'archive': {'date_from': case['utc_day'], 'cache': str(args.cache.resolve())},
            'bounds': case['bounds'], 'ground_m': case['ground_m'],
            'search_mode': 'flight_corridors', 'max_hypotheses': 5000000,
        }
        config = args.output / f"{case['id']}-job.json"
        report_path = args.output / f"{case['id']}-report.json"
        config.write_text(json.dumps(job, indent=2))
        result = subprocess.run([str(args.cli.resolve()), 'analyze', '--config', str(config),
                                 '--output', str(report_path), '--quiet'],
                                capture_output=True, text=True)
        row = {'case': case['id'], 'exit_code': result.returncode, 'passed': False}
        if result.returncode:
            row['error'] = result.stderr.strip()
        else:
            report = json.loads(report_path.read_text())
            assert report['job']['start_utc'] is None, 'Recording clock leaked into solver'
            row.update(status=report['status'], events=len(report['events']),
                       candidates=len(report['candidates']),
                       hypotheses=report['hypotheses'],
                       reference_sources=report['reference_sources'])
            if report['candidates']:
                error = distance_m(report['candidates'][0], case['ground_truth'])
                row['top_error_m'] = error
                row['passed'] = (error <= manifest['accuracy_target_m']
                                 and report['status'] == 'candidates_found')
        rows.append(row)
        print(json.dumps(row), flush=True)
    (args.output / 'acceptance.json').write_text(json.dumps(rows, indent=2))
    return 0 if all(row['passed'] for row in rows) else 1


if __name__ == '__main__':
    raise SystemExit(main())
