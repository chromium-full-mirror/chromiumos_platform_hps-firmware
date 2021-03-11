# How to regenerate TfLM

HPS uses Tensorflow Lite for Microcontrollers.

The instructions for building a TfLM library are in the [TfLM
documentation](https://www.tensorflow.org/lite/microcontrollers/library#generate_projects_for_other_platforms).
We use the `person_detection_int8` example as our reference.

To update the version of tflm used:

1. Run:
```
make -f tensorflow/lite/micro/tools/make/Makefile generate_projects
```

2. Copy
   `tensorflow/lite/micro/tools/make/gen/linux_x86_64/prj/person_detection_int8/make`
   into the `third_party/tflm_gen` directory.

3. Update the `README_VERSION.md` file to `third_party/tflm_gen` explicitly
   identifying the TfLM version used.

4. Merge the Makefile changes, so that TfLM is built with the RISCV toolchain.


## The 'litex\_shim' directory

The litex\_shim directory contains additional files to allow TfLM to work with the hps\_soc.

In particular, it contains example data files for the pdti8 person detection model.


## To do

1.  Work out a scheme for overlaying accelerated ops onto TfLM dirs or
    otherwise keeping track of patches.
