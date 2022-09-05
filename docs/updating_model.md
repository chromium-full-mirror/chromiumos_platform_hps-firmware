# Updating model

To update to a newer model:
* Copy the new model to model/shared.tflite.
* Update model/manifest.txt with the new model's verison information.
* Update test_data/presence.expected and test_data/second.expected with the new
  expected scores for each of the test images.
* Run `./scripts/proto2-openocd` in one terminal.
* Run `./scripts/test-models` in a second terminal.

The output from test-models should look something like this:

```
MCU> INFO: Found expected flash chip
MCU> INFO: Default HM01B0 configuration applied
MCU> INFO: MCU application started. CPU is running at 60MHz
MCU> INFO: FPGA reported boot #1
FPGA> INFO: Hello from the Rust FPGA
FPGA> INFO: Classifier status: InitOk
FPGA> INFO: Testing TFLM on canned data
FPGA> INFO:      Presence results: [-91, 127, 127, -83, -30, 127, 126] - PASS
FPGA> INFO: Second person results: [-128, -128, 127, -128, -128, -128, -8] - PASS
```

If you'd like to interactively check the behavior of the model, press enter to
activate the hps-mon prompt, then run:

```
>> set_feature_enable 3set_feature_enable 3
FPGA> INFO: scores:(-112, -122) wait:  77ms capture:  41ms
```

The first score (-112 above) is the presence score. The second score (-122) is
the second person score.
