import unittest

import numpy as np

from scripts.spatial_review.clear_reference import support


class ClearReferenceTest(unittest.TestCase):
    def test_compares_only_pairs_of_valid_depths(self):
        reference = np.array([2000, 2000, 2000, np.nan, 2000, 2000])
        raw = np.array([1749, 2005, 0, 1600, 1750, 2400])
        result = support(list(range(6)), raw, reference, {"a_mm": 3, "b_mm_per_m2": 1.4})
        self.assertEqual(result, dict(samples=6, comparable=4, near=1, background=1))
