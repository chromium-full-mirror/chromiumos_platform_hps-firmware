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
#include "litex_shim/shim.h"

#include "tensorflow/lite/micro/micro_error_reporter.h"
#include "tensorflow/lite/micro/micro_interpreter.h"
#include "tensorflow/lite/micro/micro_mutable_op_resolver.h"

#include "keras_324_244_1.h"
#include "tensorflow/lite/micro/examples/person_detection_experimental/model_settings.h"
#include "tensorflow/lite/micro/examples/person_detection_experimental/person_detect_model_data.h"

#include "tensorflow/lite/version.h"

// Globals
namespace {
tflite::ErrorReporter* error_reporter = nullptr;
const tflite::Model* model = nullptr;

tflite::MicroInterpreter* interpreter = nullptr;

TfLiteTensor* input = nullptr;

// An area of memory to use for input, output, and intermediate arrays.
constexpr int kTensorArenaSize = 160 * 1024;
static uint8_t tensor_arena[kTensorArenaSize] __attribute__((aligned(16)));
}  // namespace

class CallbackErrorReporter : public tflite::ErrorReporter {
 public:
  CallbackErrorReporter(report_error_callback_t error_callback)
      : error_callback_(error_callback) {}
  virtual ~CallbackErrorReporter() {}
  virtual int Report(const char* format, va_list args) {
    error_callback_(format, args);
    return 0;
  }

 private:
  report_error_callback_t error_callback_;
};

void tflite_init(enum Model model_enum,
                 report_error_callback_t error_callback) {
  static CallbackErrorReporter error_reporter_instance(error_callback);
  error_reporter = &error_reporter_instance;

  // Map the model into a usable data structure. This doesn't involve any
  // copying or parsing, it's a very lightweight operation.
  model = tflite::GetModel(
      model_enum == model_keras ? keras_324_244_1 : g_person_detect_model_data
      );
  if (model->version() != TFLITE_SCHEMA_VERSION) {
    TF_LITE_REPORT_ERROR(error_reporter,
                         "Model provided is schema version %d not equal "
                         "to supported version %d.",
                         model->version(), TFLITE_SCHEMA_VERSION);
    return;
  }

  // Pull in only the operation implementations we need.
  // Alternatively, we could pull in all the operations with:
  //   tflite::AllOpsResolver resolver;
  static tflite::MicroMutableOpResolver<7> micro_op_resolver;
  micro_op_resolver.AddAveragePool2D();
  micro_op_resolver.AddConv2D();
  micro_op_resolver.AddDepthwiseConv2D();
  micro_op_resolver.AddFullyConnected();
  micro_op_resolver.AddMaxPool2D();
  micro_op_resolver.AddReshape();
  micro_op_resolver.AddSoftmax();

  // Build an interpreter to run the model with.
  static tflite::MicroInterpreter static_interpreter(
      model, micro_op_resolver, tensor_arena, kTensorArenaSize, error_reporter);
  interpreter = &static_interpreter;

  // Allocate memory from the tensor_arena for the model's tensors.
  TfLiteStatus allocate_status = interpreter->AllocateTensors();
  if (allocate_status != kTfLiteOk) {
    TF_LITE_REPORT_ERROR(error_reporter, "AllocateTensors() failed");
    return;
  }

  // Get information about the memory area to use for the model's input.
  input = interpreter->input(0);
}

int8_t* tflite_get_input() {
  return input->data.int8;
}

void tflite_classify(int8_t* person_score, int8_t* no_person_score) {
  // Run inference
  if (kTfLiteOk != interpreter->Invoke()) {
    TF_LITE_REPORT_ERROR(error_reporter, "Invoke failed.");
  }

  // Fetch output
  TfLiteTensor* output = interpreter->output(0);
  *person_score = output->data.uint8[kPersonIndex];
  *no_person_score = output->data.uint8[kNotAPersonIndex];
}
