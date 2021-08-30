// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// This module receives data from the camera then sends that data out over the
// SPI bus to the MCU. A heartbeat signal is emitted so that the MCU knows we're
// running even if the camera isn't connected properly.

module top(
    input gsrn,
    input wire [7:0] data,
    input wire pclk,
    input wire fvld,
    input wire lvld,
    output wire sclk,
    output wire copi,
    output wire cs,
    output wire lvld_out,
    output wire heartbeat,
    output wire led
  );

  wire clk;

  OSC_CORE #(
    .HF_CLK_DIV("10")
  ) osc_i (
    .HFOUTEN(1'b1),
    .HFCLKOUT(clk)
  );

  // heartbeat
  reg [23:0] hb_count;
  always @(posedge clk)
    hb_count <= hb_count + 1'b1;
  assign heartbeat = hb_count[16];

  // LED is just for debugging on dev boards.
  assign led = hb_count[23];

  // LVLD and FLVLD pass through
  assign lvld_out = lvld;
  assign cs = ~fvld;

  // Buffer data and clk
  reg [7:0] data_b1;
  reg [7:0] data_b2;
  reg pclk_b1;
  reg pclk_b2;

  always @(posedge clk) begin
    data_b1 <= data;
    data_b2 <= data_b1;
    pclk_b1 <= pclk;
    pclk_b2 <= pclk_b1;
  end

  reg [7:0] sr; // shift register
  reg [4:0] sr_count;
  reg last_pclk;

  reg sclk_out;
  assign sclk = sclk_out;
  reg copi_out;
  assign copi = copi_out;

  always @(posedge clk) begin
    last_pclk <= pclk_b2;
    // Receive next byte, but only if we've finished sending the previous byte.
    // If we haven't finished sending the previous byte, then we drop the new
    // byte. We don't care about dropped bytes. We do however care that data pin
    // 0 is always bit 0, data pin 1 is always bit 1 etc when the data is
    // received by the MCU.
    if (pclk_b2 != 0 && last_pclk == 0 && sr_count == 0) begin
      sr <= data_b2;
      sr_count <= 8;
    end

    // Mode 0: normally low, output on falling edge
    if (sr_count >= 0 && hb_count[1:0] == 0) begin
      sclk_out <= 0;
      sr[6:0] <= sr[7:1];
      copi_out <= sr[0];
      sr_count <= sr_count - 1;
    end
    if (hb_count[1:0] == 2) begin
      sclk_out <= 1;
    end
  end

endmodule
