/* Copyright 2021 The ChromiumOS Authors. All Rights Reserved.

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
#ifndef _TFLITE_LITEX_H_
#define _TFLITE_LITEX_H_

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*report_error_callback_t)(const char*, va_list);

// In this version, the only model evaluated is pdti8
//
// Set up TfLM environment
void tflite_init(report_error_callback_t error_callback);

// Obtain a pointer to the model input vector
// This is the tensor input data. It is assumed to be a 96x96 greyscale image.
// Note that data is signed with black being -128 and white being +127
int8_t *tflite_get_input();

// Load test data into input vector from ROM
void tflite_load_person();

// Load test data into input vector from ROM
void tflite_load_no_person();

// Run a classification and return result
void tflite_classify(int8_t *person_score, int8_t *no_person_score);

#ifdef __cplusplus
}
#endif

#endif // _TFLITE_LITEX_H_
