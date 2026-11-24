import unittest

import numpy as np

from score import compare, reference


class SilhouetteScores(unittest.TestCase):
    def test_ambiguous_and_invalid_cells_are_excluded(self):
        depth = np.ones((21, 21), dtype=np.uint16)
        depth[10, 10] = 0
        inside, outside = reference(depth, [(4,4),(16,4),(16,16),(4,16)], 2)
        self.assertFalse(inside[10, 10] or outside[10, 10])
        self.assertFalse(inside[4, 8] or outside[4, 8])
        self.assertTrue(inside[10, 9] and outside[1, 1])

    def test_separates_leakage_fragmentation_and_missing_support(self):
        inside = np.array([[True, True, True, False, False]])
        outside = np.array([[False, False, False, True, False]])
        counts, _ = compare([(1, [0,3,4]), (2, [1])], inside, outside)
        self.assertEqual(counts, dict(target=3, recovered=1, background=1,
                                      other_tracks=1, missed=1))


if __name__ == "__main__":
    unittest.main()
