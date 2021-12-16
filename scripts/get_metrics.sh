#!/bin/bash

# Copyright 2021 The Chromium OS Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.

PROJECT_ROOT=$(realpath "$(dirname "${BASH_SOURCE[0]}")"/..)

output_file="${PROJECT_ROOT}/build/hps_platform/gateware/hps_platform_build.metadata"
log_file="${PROJECT_ROOT}/build/hps_platform/gateware/hps_platform_output.log"
area_names=("OXIDE_FF" "OXIDE_COMB" "OXIDE_EBR" "LRAM_CORE")

for name in "${area_names[@]}"
do
  area_metrics="$(grep ".*${name}.*[[:digit:]]%$" "${log_file}")"
  num="$(echo "${area_metrics}" | grep -o -E "[0-9]+%$")"
  echo "${name}_PERCENT: ${num}" | tr -d '%' >> "${output_file}"
done

fmax_name="Max frequency for clock"
fmax="$(grep "${fmax_name}" "${log_file}" | tail -n 1 )"
fmax_num="$(echo "${fmax}" | grep -o -E "[0-9]+\.[0-9]+ MHz " | tr -d 'MHz ')"
echo "fMAX_MHz: ${fmax_num}" >> "${output_file}"

echo "Metrics:"
cat "${output_file}"
