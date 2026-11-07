"""Repeat the fixed 1 km real-video benchmark. Media is not redistributed."""
import argparse
import concurrent.futures
import hashlib
import json
import math
from pathlib import Path
import subprocess
import time

from run import distance_m


SETTINGS = {
    'bounds', 'ground_m', 'ground_datum', 'smooth_tracks', 'temperature_c',
    'aircraft_filter', 'timing_options',
}
PATHS = {'media', 'tracks', 'terrain', 'type_catalog'}


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def under_root(root, relative):
    path = Path(relative)
    if path.is_absolute() or not path.parts:
        raise ValueError('Benchmark paths must be relative to --data-root')
    resolved = (root / path).resolve()
    if not resolved.is_relative_to(root.resolve()):
        raise ValueError('Benchmark path leaves --data-root')
    return resolved


def make_job(case, data_root, *, unknown_date=False, offline=False, archive_override=None):
    """Copy only solver settings. References and source clocks stay outside."""
    if set(case['settings']) - SETTINGS:
        raise ValueError('Unexpected benchmark solver setting')
    if set(case['paths']) - PATHS:
        raise ValueError('Unexpected benchmark path setting')
    job = dict(case['settings'])
    job.update(analysis_method='passage_timing', start_utc=None)
    job.update({key: str(under_root(data_root, value))
                for key, value in case['paths'].items()})
    if case.get('archive'):
        archive = case['archive']
        if set(archive) - {'date_from', 'date_to', 'provider', 'cache'}:
            raise ValueError('Unexpected benchmark archive setting')
        job['archive'] = {**archive, 'cache': str(under_root(data_root, archive['cache']))}
    if archive_override is not None:
        job['archive'] = dict(archive_override)
    if unknown_date:
        if not job.get('archive'):
            raise ValueError('Unknown-date tests need a provider cache; use --cache and --provider')
        job['archive'].update(date_from='', date_to=None)
        # A date-selected import must not override or supplement the cache search.
        job.pop('tracks', None)
        job.pop('type_catalog', None)
    if offline:
        job['offline'] = True
    return job


def evaluate(report, exit_code, case, target, *, unknown_date=False, offline=False):
    """A green model status alone does not pass the real-location benchmark."""
    job = report['job']
    if job.get('start_utc') is not None or job.get('analysis_method') != 'passage_timing':
        raise ValueError('The report used a recording clock or the wrong method')
    if job.get('bounds') != case['settings']['bounds']:
        raise ValueError('The report used a different search area')
    if unknown_date:
        archive = job.get('archive')
        if not archive or archive.get('date_from') or archive.get('date_to') or job.get('tracks'):
            raise ValueError('The report used a supplied date or date-selected flight import')
        coverage = report.get('date_search')
        if coverage is not None and not coverage.get('complete'):
            raise ValueError('The date search did not finish')
    if offline and not job.get('offline'):
        raise ValueError('The report did not use offline mode')
    timing = report['timing']
    row = {'status': report['status'], 'accepted': False, 'screened': False,
           'sound_peaks': len(timing['peaks']), 'track_count': report['track_count']}
    expected_exit = {'candidates_found': 0, 'insufficient_evidence': 3, 'ambiguous': 4}
    if expected_exit.get(report['status']) != exit_code:
        raise ValueError('Report status and strict CLI exit code disagree')
    if report['status'] != 'candidates_found':
        return row
    region = timing.get('region')
    if not region:
        raise ValueError('The accepted timing report has no region')
    if not all(math.isfinite(region[key]) for key in ['latitude', 'longitude', 'enclosing_radius_m']):
        raise ValueError('The report region is not finite')
    if not (-90 <= region['latitude'] <= 90 and -180 <= region['longitude'] <= 180
            and 0 <= region['enclosing_radius_m'] <= 1000):
        raise ValueError('The report region is invalid')
    controls = timing['control_maximum_scores']
    seeds = timing['control_permutation_seeds']
    first = timing['search']['candidates'][0]
    sigma = timing['search'].get('options', {}).get('timing_sigma_s', 1.)
    if not math.isfinite(sigma) or not 0.5 <= sigma <= 10:
        raise ValueError('Invalid timing width in report')
    score = first['log_likelihood_ratio']
    if (len(controls) != 19 or len(seeds) != 19 or len(set(seeds)) != 19
            or not math.isfinite(score)
            or any(not math.isfinite(value) or value + 1e-7 >= score for value in controls)
            or sum(abs(pair['residual_s']) <= 3 * sigma for pair in first['pairs']) < 6):
        raise ValueError('The report does not pass the required timing screens')
    # Reference positions enter only this evaluator, after the CLI has exited.
    error = max(distance_m(region, point) for point in case['references']) + case['reference_allowance_m']
    row.update(screened=True, region=region, error_m=error, accepted=error <= target,
               matched_flights=len(first['pairs']), score=score, controls=19)
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--data-root', type=Path, required=True,
                        help='Folder containing the original local media and reference caches')
    parser.add_argument('--output', type=Path, required=True, help='New folder; existing results are preserved')
    parser.add_argument('--manifest', type=Path, default=Path(__file__).with_name('timing-cases.json'),
                        help='Case manifest; defaults to the preserved 25-case benchmark')
    parser.add_argument('--case', action='append', help='Case ID; repeat to select several, omit for all')
    parser.add_argument('--unknown-date', action='store_true', help='Search cached dates without supplying a recording date')
    parser.add_argument('--offline', action='store_true', help='Disable network access in the app')
    parser.add_argument('--cache', type=Path, help='Override the flight cache for selected cases')
    parser.add_argument('--provider', choices=['adsb_lol', 'adsb_exchange_sample', 'webtrak_chicago', 'webtrak_lax'])
    parser.add_argument('--workers', type=int, choices=range(1, 5), default=2)
    parser.add_argument('--timeout', type=float, default=3600, help='Maximum seconds per case, including downloads')
    args = parser.parse_args()
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error('--timeout must be a positive number of seconds')
    manifest_path = args.manifest.resolve(strict=True)
    manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
    selected = [case for case in manifest['cases'] if not args.case or case['id'] in args.case]
    if not selected or (args.case and set(args.case) - {case['id'] for case in selected}):
        parser.error('Unknown case ID')
    if bool(args.cache) != bool(args.provider):
        parser.error('--cache and --provider must be supplied together')
    if args.cache and not args.unknown_date:
        parser.error('A cache override requires --unknown-date')
    if args.output.exists():
        parser.error('--output already exists; choose a new folder')
    cli = args.cli.resolve(strict=True)
    data_root = args.data_root.resolve(strict=True)
    archive_override = {'cache': str(args.cache.resolve(strict=True)), 'provider': args.provider} if args.cache else None
    prepared = []
    for case in selected:
        if not case['id'] or any(c not in 'abcdefghijklmnopqrstuvwxyz0123456789-' for c in case['id']):
            raise ValueError('Invalid benchmark case ID')
        job = make_job(case, data_root, unknown_date=args.unknown_date, offline=args.offline,
                       archive_override=archive_override)
        if digest(Path(job['media'])) != case['media_sha256']:
            raise ValueError(f"{case['id']}: media differs from the fixed capture")
        prepared.append((case, job))
    args.output.mkdir(parents=True, exist_ok=False)
    plan = {'cli_sha256': digest(cli), 'manifest_sha256': digest(manifest_path),
            'runner_sha256': digest(Path(__file__)),
            'target_error_m': manifest['target_error_m'], 'selected_cases': [c['id'] for c in selected],
            'all_cases_selected': len(selected) == len(manifest['cases']),
            'workers': args.workers, 'timeout_s': args.timeout,
            'recording_clock_supplied': False, 'recording_date_supplied': not args.unknown_date,
            'offline': args.offline, 'archive_override': archive_override, 'all_cases_are_replays': True}
    (args.output / 'plan.json').write_text(json.dumps(plan, indent=2) + '\n', encoding='utf-8')

    def run_case(item):
        case, job = item
        folder = args.output / case['id']
        folder.mkdir()
        config = folder / 'job.json'
        report_path = folder / 'report.json'
        config.write_text(json.dumps(job, indent=2) + '\n', encoding='utf-8')
        row = {'case': case['id'], 'accepted': False, 'screened': False,
               'job_sha256': digest(config), 'media_sha256': case['media_sha256']}
        command = [str(cli), 'analyze', '--config', str(config.resolve()),
                   '--output', str(report_path.resolve()), '--require-unambiguous']
        started = time.monotonic()
        try:
            with (folder / 'run.log').open('x', encoding='utf-8') as log:
                process = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=args.timeout)
            row['exit_code'] = process.returncode
            if not report_path.is_file():
                raise ValueError('The CLI did not write a report; see run.log')
            report = json.loads(report_path.read_text(encoding='utf-8'))
            row['report_sha256'] = digest(report_path)
            row.update(evaluate(report, process.returncode, case, manifest['target_error_m'],
                                unknown_date=args.unknown_date, offline=args.offline))
            row['date_search'] = report.get('date_search')
        except (OSError, ValueError, KeyError, IndexError, TypeError, subprocess.TimeoutExpired) as error:
            row['error'] = str(error)
        row['elapsed_s'] = time.monotonic() - started
        (folder / 'evaluation.json').write_text(json.dumps(row, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(row), flush=True)
        return row

    rows = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(run_case, item) for item in prepared]
        for future in concurrent.futures.as_completed(futures):
            rows.append(future.result())
            result = {**plan, 'completed_cases': len(rows), 'planned_cases': len(selected),
                      'accepted': sum(row['accepted'] for row in rows),
                      'wrong_screened': sum(row['screened'] and not row['accepted'] for row in rows),
                      'processing_errors': sum('error' in row for row in rows),
                      'cases': sorted(rows, key=lambda row: row['case'])}
            (args.output / 'results.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    return 0 if all(row['accepted'] for row in rows) else 1


if __name__ == '__main__':
    raise SystemExit(main())
