/* Copyright 2021 The ChromiumOS Authors

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
==============================================================================*/

// Access to TfLM functions for evaluating models
//
#include <stdint.h>
#include <stdarg.h>
#ifndef THIRD_PARTY_TFLM_CAPI_SHIM_H_
#define THIRD_PARTY_TFLM_CAPI_SHIM_H_

#ifdef __cplusplus
extern "C" {
#endif

// Possible status from tflite_init
enum TfLiteInitStatus : int32_t {
  TLITE_INIT_OK = 0,
  TLITE_INIT_ERR_MODEL_1 = 1,
  TLITE_INIT_ERR_MODEL_2 = 2,
  TLITE_INIT_CFU_BUG = 3,
  TLITE_INIT_ERR_OTHER = -1,
};

// Set up TfLM environment
// Returns a value from enum TfLiteInitStatus
int tflite_init(const uint8_t* model_bytes);

// Obtain a pointer to the model input vector
// This is the tensor input data. It is assumed to be a 320x240 greyscale image.
// Note that data is signed with black being -128 and white being +127
int8_t* tflite_get_input();

// Run a classification and write the results into `score1` and `score2`.
// Returns true on successful model run, false if error detected.
bool tflite_classify(int8_t* score1, int8_t* score2);

// Execute custom layer test for accelerator correctness.
bool tflite_layer_test();

#ifdef __cplusplus
}
#endif

#endif  // THIRD_PARTY_TFLM_CAPI_SHIM_H_
