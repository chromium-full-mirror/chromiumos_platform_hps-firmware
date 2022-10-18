# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Interface to test data.

We store data in pickle files rather than referrring to it directly, as the
large .py files containing the raw data are slow and inconvenient to use in
IDEs.
"""

from collections import namedtuple
from pathlib import Path
import pickle


Conv2DData = namedtuple(
    "Conv2DData",
    [
        "input_dims",
        "output_dims",
        "filter_dims",
        "input_offset",
        "output_offset",
        "output_min",
        "output_max",
        "stride",
        "output_multipliers",
        "output_shifts",
        "output_biases",
        "raw_input_data",  # as bytes
        "input_data",  # as words
        "filter_data",  # as words
        "expected_output_data",  # as words
    ],
)


def fetch_data(name):
    """Fetches data with the given name.

    Data is stored in a file namemed name.pickle in the current directory.
    """
    filename = Path(__file__).parent / (name + ".pickle")
    with open(filename, "rb") as f:
        return pickle.load(f)


def save_data(name, data):
    """Saves data into a pickle file."""
    with open(Path(__file__).parent / (name + ".pickle"), "wb") as f:
        pickle.dump(data, f)
