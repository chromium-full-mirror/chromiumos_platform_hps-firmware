# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

"""Generates version.h containing information about the current build"""

import sys
import os
import time
import datetime
import subprocess

build_hash = subprocess.check_output(
    ['git', 'describe', '--dirty', '--always'],
    universal_newlines=True).strip()
build_user = os.environ['USER']
build_date = datetime.datetime.utcfromtimestamp(
    int(os.environ.get('SOURCE_DATE_EPOCH', time.time())))

output = f"""
#ifndef HPS_VERSION_H_
#define HPS_VERSION_H_

#define HPS_VERSION "{build_hash}"
#define HPS_BUILD_USER "{build_user}"
#define HPS_BUILD_DATE "{build_date.strftime('%Y-%m-%d %H:%M:%S')} UTC"

#endif /* HPS_VERSION_H_ */
"""

if len(sys.argv) > 1:
    os.makedirs(os.path.dirname(sys.argv[1]), mode=0o755, exist_ok=True)
    with open(sys.argv[1], 'w') as f:
        f.write(output)
else:
    print(output)
