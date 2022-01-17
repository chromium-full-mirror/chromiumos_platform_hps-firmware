/* Copyright 2019 The TensorFlow Authors. All Rights Reserved.
   Copyright 2021 The ChromiumOS Authors. All Rights Reserved.

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
// Access to TfLM functions for a pure C based LiteX client
//
// Based on
// tensorflow/lite/micro/examples/person_detection_experimental/main_functions.h
#include "capi/shim.h"

#include "tensorflow/lite/micro/micro_error_reporter.h"
#include "tensorflow/lite/micro/micro_allocator.h"
#include "tensorflow/lite/micro/micro_interpreter.h"
#include "tensorflow/lite/micro/micro_mutable_op_resolver.h"

#include "models/hps_model_2021_09_20_tiled.h"
#include "models/hps_second_2021_11_18_tiled.h"

// Globals
namespace {

// Ignores errors messages.
// Many of the "errors" reported here are actually debugging messages.
class IgnoringErrorReporter : public tflite::ErrorReporter {
 public:
  IgnoringErrorReporter() {}
  virtual ~IgnoringErrorReporter() {}
  virtual int Report(const char* format, va_list args) {
    //ignore
    return 0;
  }
};

IgnoringErrorReporter* error_reporter = nullptr;
tflite::MicroInterpreter* interpreter1 = nullptr;
tflite::MicroInterpreter* interpreter2 = nullptr;

tflite::MicroInterpreter* get_interpreter(int model_number) {
  return model_number == 1 ? interpreter1 : interpreter2;
}

// An area of memory to use for input, output, and intermediate arrays.
constexpr int kTensorArenaSize = 256 * 1024;

// The arena is specially placed in memory
static uint8_t tensor_arena[kTensorArenaSize]
        __attribute__((section(".arena")));


}  // namespace

int tflite_init() {
  static IgnoringErrorReporter error_reporter_instance;
  error_reporter = &error_reporter_instance;

  // Pull in only the operation implementations we need.
  // Alternatively, we could pull in all the operations with:
  //   tflite::AllOpsResolver resolver;
  static tflite::MicroMutableOpResolver<8> resolver_instance;
  resolver_instance.AddConcatenation();
  resolver_instance.AddConv2D();
  resolver_instance.AddFullyConnected();
  resolver_instance.AddLogistic();
  resolver_instance.AddMaxPool2D();
  resolver_instance.AddReshape();
  resolver_instance.AddStridedSlice();
  resolver_instance.AddPad();

  // Model one is presence sensor
  auto* allocator = tflite::MicroAllocator::Create(
      tensor_arena, kTensorArenaSize, error_reporter);
  auto model1 = tflite::GetModel(hps_model_2021_09_20_tiled);
  static tflite::MicroInterpreter interpreter1_instance(
      model1, resolver_instance, allocator, error_reporter);
  TfLiteStatus allocate_status = interpreter1_instance.AllocateTensors();
  if (allocate_status != kTfLiteOk) {
    TF_LITE_REPORT_ERROR(error_reporter, "Model 1 AllocateTensors() failed");
    return TLITE_INIT_ERR_MODEL_1;
  }
  interpreter1 = &interpreter1_instance;

  // Model two is second person detector
  auto model2 = tflite::GetModel(hps_second_2021_11_18_tiled);
  static tflite::MicroInterpreter interpreter2_instance(
      model2, resolver_instance, allocator, error_reporter);
  allocate_status = interpreter2_instance.AllocateTensors();
  if (allocate_status != kTfLiteOk) {
    TF_LITE_REPORT_ERROR(error_reporter, "Model 2 AllocateTensors() failed");
    return TLITE_INIT_ERR_MODEL_2;
  }
  interpreter2 = &interpreter2_instance;

  // If seen no errors, then we are all good
  return TLITE_INIT_OK;
}

// Allocate memory from the tensor_arena for the model's tensors.
int8_t* tflite_get_input(int model_number) {
  return get_interpreter(model_number)->input(0)->data.int8;
}

bool tflite_classify(int model_number, int8_t* score) {
  // Run inference
  if (kTfLiteOk != get_interpreter(model_number)->Invoke()) {
    TF_LITE_REPORT_ERROR(error_reporter, "Invoke failed.");
    return false;
  }

  // Fetch output
  TfLiteTensor* output = get_interpreter(model_number)->output(0);
  *score = output->data.int8[0];

  return true;
}
