"""Benchmark guard tests; these fixtures are not real location validation."""
import copy
import math
from pathlib import Path
import tempfile
import unittest

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
