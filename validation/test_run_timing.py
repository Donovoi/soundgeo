"""Benchmark guard tests; these fixtures are not real location validation."""
import copy
import json
import math
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import run_timing as runner


class BenchmarkGuards(unittest.TestCase):
    def setUp(self):
        self.case = {'id': 'fixture', 'settings': {'bounds': {'south': 40., 'north': 42., 'west': -88., 'east': -86.}},
                     'paths': {'media': 'fixture/recording.mkv'},
                     'references': [{'latitude': 41., 'longitude': -87.}],
                     'reference_allowance_m': 200, 'recording_clock_for_evaluation_only': 123456.}
        self.report = {'status': 'candidates_found', 'track_count': 20,
                       'job': {'analysis_method': 'passage_timing', 'start_utc': None, 'bounds': self.case['settings']['bounds']},
                       'timing': {'peaks': list(range(12)), 'region': {'latitude': 41., 'longitude': -87., 'enclosing_radius_m': 100.},
                                  'control_maximum_scores': [10.] * 19, 'control_permutation_seeds': list(range(19)),
                                  'search': {'candidates': [{'log_likelihood_ratio': 30., 'pairs': [{'residual_s': 0.1}] * 12}]}}}

    def test_job_contains_no_reference_or_recording_clock(self):
        with tempfile.TemporaryDirectory() as directory:
            job = runner.make_job(self.case, Path(directory))
        self.assertIsNone(job['start_utc'])
        self.assertEqual(job['analysis_method'], 'passage_timing')
        self.assertNotIn('references', job)
        self.assertNotIn('recording_clock_for_evaluation_only', job)
        self.case['settings']['start_utc'] = 123456.
        with self.assertRaises(ValueError):
            runner.make_job(self.case, Path('.').resolve())

    def test_paths_cannot_escape_the_data_root(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for path in ['../outside.mp4', str(root.resolve() / 'absolute.mp4')]:
                with self.assertRaises(ValueError):
                    runner.under_root(root, path)

    def test_unknown_date_uses_cache_and_removes_date_selected_imports(self):
        self.case['paths'].update(tracks='selected-day/flights.csv', type_catalog='selected-day/types.json')
        self.case['archive'] = {'cache': 'cache', 'provider': 'adsb_lol',
                                'date_from': '2026-11-01', 'date_to': '2026-11-01'}
        with tempfile.TemporaryDirectory() as directory:
            job = runner.make_job(self.case, Path(directory), unknown_date=True, offline=True)
        self.assertEqual(job['archive']['date_from'], '')
        self.assertIsNone(job['archive']['date_to'])
        self.assertNotIn('tracks', job)
        self.assertNotIn('type_catalog', job)
        self.assertTrue(job['offline'])
        self.assertEqual(self.case['archive']['date_from'], '2026-11-01')

    def test_unknown_date_requires_a_cache(self):
        with self.assertRaises(ValueError):
            runner.make_job(self.case, Path('.'), unknown_date=True)
        job = runner.make_job(self.case, Path('.'), unknown_date=True,
                              archive_override={'cache': 'prepared-cache', 'provider': 'adsb_lol'})
        self.assertEqual(job['archive']['cache'], 'prepared-cache')

    def test_plan_records_date_arguments_from_jobs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'fixture').mkdir()
            media = root / self.case['paths']['media']
            media.write_bytes(b'fixed benchmark fixture')
            cli = root / 'cli'
            cli.write_bytes(b'mocked executable')

            def run_cli(command, **kwargs):
                config = Path(command[command.index('--config') + 1])
                report_path = Path(command[command.index('--output') + 1])
                report = copy.deepcopy(self.report)
                report.update(status='insufficient_evidence', job=json.loads(config.read_text(encoding='utf-8')))
                report_path.write_text(json.dumps(report), encoding='utf-8')
                return runner.subprocess.CompletedProcess(command, 3)

            for mode, archive, imported, dated in [
                ('import', None, True, False),
                ('dated', {'cache': 'cache', 'provider': 'adsb_lol', 'date_from': '2026-11-01'}, False, True),
                ('discovery', {'cache': 'cache', 'provider': 'adsb_lol', 'date_from': '', 'date_to': None}, False, False),
            ]:
                with self.subTest(mode=mode):
                    case = copy.deepcopy(self.case)
                    case['media_sha256'] = runner.digest(media)
                    if archive:
                        case['archive'] = archive
                    if imported:
                        case['paths']['tracks'] = 'reference.csv'
                    manifest = root / f'{mode}.json'
                    manifest.write_text(json.dumps({'target_error_m': 1000, 'cases': [case]}), encoding='utf-8')
                    output = root / mode
                    args = ['run_timing.py', '--cli', str(cli), '--data-root', str(root),
                            '--manifest', str(manifest), '--output', str(output), '--offline']
                    with patch.object(sys, 'argv', args), patch.object(runner.subprocess, 'run', side_effect=run_cli):
                        self.assertEqual(runner.main(), 1)
                    for name in ('plan.json', 'results.json'):
                        result = json.loads((output / name).read_text(encoding='utf-8'))
                        self.assertEqual(result['recording_date_supplied'], dated)
                        self.assertEqual(result['date_argument_case_ids'], ['fixture'] if dated else [])
                        self.assertEqual(result['imported_reference_case_ids'], ['fixture'] if imported else [])
                    self.assertEqual(result['processing_errors'], 0)

    def test_unknown_date_and_offline_checks_also_apply_to_abstentions(self):
        self.report['status'] = 'insufficient_evidence'
        self.report['job'].update(offline=True, archive={'date_from': '', 'date_to': None})
        runner.evaluate(self.report, 3, self.case, 1000, unknown_date=True, offline=True)
        for update in [{'archive': {'date_from': '2026-11-01'}}, {'tracks': 'selected-day.csv'}, {'offline': False}]:
            report = copy.deepcopy(self.report)
            report['job'].update(update)
            with self.assertRaises(ValueError):
                runner.evaluate(report, 3, self.case, 1000, unknown_date=True, offline=True)
        self.report['date_search'] = {'complete': False}
        with self.assertRaises(ValueError):
            runner.evaluate(self.report, 3, self.case, 1000, unknown_date=True, offline=True)

    def test_reference_allowance_is_part_of_accuracy(self):
        self.assertEqual(runner.evaluate(self.report, 0, self.case, 1000)['error_m'], 200.)
        self.report['timing']['region']['latitude'] += .008
        row = runner.evaluate(self.report, 0, self.case, 1000)
        self.assertTrue(row['screened'])
        self.assertFalse(row['accepted'])
        self.assertGreater(row['error_m'], 1000)

    def test_wrong_method_clock_bounds_and_exit_code_fail(self):
        for field, value in [('analysis_method', 'doppler'), ('start_utc', 123.), ('bounds', {})]:
            report = copy.deepcopy(self.report)
            report['job'][field] = value
            with self.assertRaises(ValueError):
                runner.evaluate(report, 0, self.case, 1000)
        with self.assertRaises(ValueError):
            runner.evaluate(self.report, 4, self.case, 1000)

    def test_missing_or_invalid_controls_do_not_pass(self):
        for field, value in [('control_maximum_scores', []), ('control_maximum_scores', [math.nan] * 19),
                             ('control_maximum_scores', [30.] * 19), ('control_permutation_seeds', [1] * 19),
                             ('region', None)]:
            report = copy.deepcopy(self.report)
            report['timing'][field] = value
            with self.assertRaises(ValueError):
                runner.evaluate(report, 0, self.case, 1000)

    def test_abstentions_are_not_accuracy_passes(self):
        for status, code in [('insufficient_evidence', 3), ('ambiguous', 4)]:
            report = copy.deepcopy(self.report)
            report['status'] = status
            report['timing']['region'] = None
            row = runner.evaluate(report, code, self.case, 1000)
            self.assertFalse(row['accepted'])
            self.assertFalse(row['screened'])


if __name__ == '__main__':
    unittest.main()
