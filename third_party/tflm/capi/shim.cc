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
#include "riscv.h"

#include "tensorflow/lite/micro/micro_error_reporter.h"
#include "tensorflow/lite/micro/micro_allocator.h"
#include "tensorflow/lite/micro/micro_interpreter.h"
#include "tensorflow/lite/micro/micro_mutable_op_resolver.h"

// Globals
namespace {

// Ignores errors messages.
// Many of the "errors" reported here are actually debugging messages.
class IgnoringErrorReporter : public tflite::ErrorReporter {
 public:
  IgnoringErrorReporter() {}
  virtual ~IgnoringErrorReporter() {}
  virtual int Report(const char* format, va_list args) {
    // ignore
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

int check_post_cfu_instruction(int input)
    __attribute__((section(".text_cfu_check"))) __attribute__((noinline));

// Returns `input`... if the CFU plugin doesn't have a particular bug that
// messes up the value of the instruction after CFU instructions.
int check_post_cfu_instruction(int input) {
  register int output;
  int tmp;
  asm volatile(
      // Some padding. The alignment of the CFU instruction seems to matter.
      "nop \n\t"
      "nop \n\t"
      "nop \n\t"
      "nop \n\t"
      "nop \n\t"
      "nop \n\t"

      // Set up an input to our CFU instruction.
      "li a6, 0; \n\t"

      // These four instructions seem to need to be stores.
      "sw %[input],%[tmp] \n\t"
      "sw %[input],%[tmp] \n\t"
      "sw %[input],%[tmp] \n\t"
      "sw %[input],%[tmp] \n\t"

      // This can be any instruction.
      "nop \n\t"

      // Call CFU function 0.
      ".word ((CUSTOM0) | (regnum_a7 << 7) | (regnum_a6 << 15) | (regnum_a6 << "
      "20) | (0 << 12) | (5 << 25)) \n\t"

      // If the bug is present, the value stored into %[output] will be wrong.
      "mv %[output],%[input] \n\t"

      "nop \n\t"

      "nop \n\t"
      "nop \n\t"
      : [output] "=r"(output), [tmp] "=m"(tmp)
      : [input] "r"(input)
      : "a6", "a7"

  );
  return output;
}

bool has_cfu_bug() {
  // Note, it's important that we call check_post_cfu_instruction with more than
  // one value, since otherwise GCC "inlines" the argument into the function
  // itself, which affects the alignment of the instructions within the
  // function.
  return check_post_cfu_instruction(41) != 41 ||
         check_post_cfu_instruction(42) != 42;
}

}  // namespace

int tflite_init(const uint8_t* model1_bytes, const uint8_t* model2_bytes) {
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

  auto* allocator = tflite::MicroAllocator::Create(
      tensor_arena, kTensorArenaSize, error_reporter);

  if (has_cfu_bug()) {
    TF_LITE_REPORT_ERROR(error_reporter, "CFU interface is faulty");
    return TLITE_INIT_CFU_BUG;
  }

  // Model one is presence sensor
  auto model1 = tflite::GetModel(model1_bytes);
  static tflite::MicroInterpreter interpreter1_instance(
      model1, resolver_instance, allocator, error_reporter);
  TfLiteStatus allocate_status = interpreter1_instance.AllocateTensors();
  if (allocate_status != kTfLiteOk) {
    TF_LITE_REPORT_ERROR(error_reporter, "Model 1 AllocateTensors() failed");
    return TLITE_INIT_ERR_MODEL_1;
  }
  interpreter1 = &interpreter1_instance;

  // Model two is second person detector
  auto model2 = tflite::GetModel(model2_bytes);
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
