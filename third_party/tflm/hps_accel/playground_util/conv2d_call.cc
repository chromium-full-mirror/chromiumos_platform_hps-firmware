/*
 * Copyright 2021 The CFU-Playground Authors
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#include "conv2d_call.h"

#include <cstdio>

#include "tensorflow/lite/kernels/internal/reference/integer_ops/conv.h"
#include "third_party/tflm/capi/shim.h"
#include "third_party/tflm/hps_accel/playground_util/dump.h"

bool test_conv2d(const Conv2DData* data) {
  // Copy input arena
  int8_t* arena_input = tflite_get_input();
  auto input_shape =
      *(reinterpret_cast<const tflite::RuntimeShape*>(data->input_shape));
  for (int i = 0; i < input_shape.FlatSize(); i++) {
    arena_input[i] = data->input_data[i];
  }
  int8_t* arena_output = arena_input + 128 * 1024;

  const tflite::RuntimeShape& output_shape =
      *(reinterpret_cast<const tflite::RuntimeShape*>(data->output_shape));
  tflite::reference_integer_ops::ConvPerChannel(
      *(reinterpret_cast<const tflite::ConvParams*>(data->params)),
      reinterpret_cast<const int32_t*>(data->output_multiplier),
      reinterpret_cast<const int32_t*>(data->output_shift), input_shape,
      reinterpret_cast<const int8_t*>(arena_input),
      *(reinterpret_cast<const tflite::RuntimeShape*>(data->filter_shape)),
      reinterpret_cast<const int8_t*>(data->filter_data),
      *(reinterpret_cast<const tflite::RuntimeShape*>(data->bias_shape)),
      reinterpret_cast<const int32_t*>(data->bias_data), output_shape,
      arena_output);

  const int32_t* arena_words = reinterpret_cast<const int32_t*>(arena_output);
  const int32_t arena_hash =
      murmurhash3_32(reinterpret_cast<const uint8_t*>(arena_words),
                     output_shape.FlatSize());

  return arena_hash == data->output_data_hash;
}
