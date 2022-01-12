# tflm

This directory contains TensorflowLite for Microcontrollers code as used
by HPS. The contents of this directory are copy of parts of CFU Playground

Contents:

- tflite-micro: A copy of CFU-Playground/third_party/tflite-micro
- hps_accel: Modified TfLite source files that implement accelerated
  operations. Copied from CFU-Playground/proj/hps_accel/src/tensorflow.
- capi: C API used by Rust code to initialize tflm and run models.
- models: repository of tflite micro models.
- BUILD.gn: the local build file.

The build file produces a single static library, tflite-micro.a, which is then
available to be linked with Rust code.