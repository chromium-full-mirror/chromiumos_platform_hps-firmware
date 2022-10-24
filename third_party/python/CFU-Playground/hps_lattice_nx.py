# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
#
# This file is part of LiteX.
#
# Copyright (c) 2022 The Chromium OS Authors. All rights reserved.
# Copyright (c) 2020 David Corrigan <davidcorrigan714@gmail.com>
# Copyright (c) 2019 William D. Jones <thor0505@comcast.net>
# Copyright (c) 2019 Tim 'mithro' Ansell <me@mith.ro>
# Copyright (c) 2019 Florent Kermarrec <florent@enjoy-digital.fr>
# SPDX-License-Identifier: BSD-2-Clause
#
"""NX family-specific Wishbone interface to the LRAM primitive.

Each LRAM is 64kBytes arranged in 32 bit wide words.

##################################################################
 This is a modified version of litex/soc/cores/ram/lattice_nx.py
 Modified by CFU Playground Authors to:
  * optionally use the dual port LRAM primitive
  * optionally stripe consecutive addresses across the banks
##################################################################

 When the striping option is used:

 The RAM from which data is fetched consists of 4 32 bit wide, 16K words deep
 LRAMs, with word addresses in rows across the LRAMs:
 (this is how the "arena" address space looks from the Wishbone bus)

 +--------+--------+--------+--------+
 | LRAM 0 | LRAM 1 | LRAM 2 | LRAM 3 |
 +--------+--------+--------+--------+
 |    0   |    1   |    2   |    3   |
 +--------+--------+--------+--------+
 |    4   |    5   |    6   |    7   |
 +--------+--------+--------+--------+
 |    8   |    9   |   10   |   11   |
 +--------+--------+--------+--------+
 |  ...   |  ...   |  ...   |  ...   |
 +--------+--------+--------+--------+

  In addition, each LRAM has a second port that gets connected directly to the
  CFU. From each of these ports, the connected LRAM is word addressed
  consecutively.

  Thus, "arena" offset 4 from the Wishbone bus can also be accessed at
  address 0x1 from the LRAM-0 port connected to the CFU.
"""

from litex.soc.interconnect import wishbone
from migen import ClockSignal
from migen import If
from migen import Instance
from migen import Module
from migen import Mux
from migen import Signal


kB = 1024


def initval_parameters(contents, width):
    """Convert contents to INITVAL format.

    In Radiant, initial values for LRAM are passed a sequence of parameters
    named INITVAL_00 ... INITVAL_7F. Each parameter value contains 4096 bits
    of data, encoded as a 1280-digit hexadecimal number, with
    alternating sequences of 8 bits of padding and 32 bits of real data,
    making up 64KiB altogether.
    """
    assert width in [32, 64]
    # Each LRAM is 64KiB == 524288 bits
    assert len(contents) == 524288 // width
    chunk_size = 4096 // width
    parameters = []
    for i in range(0x80):
        name = f'INITVAL_{i:02X}'
        offset = chunk_size * i
        if width == 32:
            value = '0x' + ''.join(f'00{contents[offset + j]:08X}'
                                   for j in range(chunk_size - 1, -1, -1))
        elif width == 64:
            value = '0x' + ''.join(f'00{contents[offset + j] >> 32:08X}'
                                   + f'00{contents[offset + j] | 0xFFFFFF:08X}'
                                   for j in range(chunk_size - 1, -1, -1))
        parameters.append(Instance.Parameter(name, value))
    return parameters


class NXLRAM(Module):
    """Physical LRAM block with bus interface."""

    def __init__(self, width=32, size=128*kB, dual_port=False, init=None):
        self.bus = wishbone.Interface(width)
        assert width in [32, 64]
        self.width = width
        self.size = size

        if width == 32:
            assert size in [64*kB, 128*kB, 192*kB, 256*kB, 320*kB]
            self.depth_cascading = size//(64*kB)
            self.width_cascading = 1
        if width == 64:
            assert size in [128*kB, 256*kB]
            self.depth_cascading = size//(128*kB)
            self.width_cascading = 2

        self.lram_blocks = []
        sel_bits = (self.depth_cascading-1).bit_length()

        # currently tie "sel_uses_lsb" to "dual_port"
        #   (they *can* be independent)
        sel_uses_lsb = dual_port

        if sel_uses_lsb:
            #
            # REQUIRES that self.depth_cascading be a power of 2
            #
            assert 2**sel_bits == self.depth_cascading, (
                f'Memory size {size} results in depth of ' +
                f'{self.depth_cascading}, but depth must be a power of 2.')
            sel_bits_start = 0
            adr_bits_start = sel_bits
        else:
            sel_bits_start = 14
            adr_bits_start = 0

        print('sel_bits ', sel_bits)
        print('sel_bits_start ', sel_bits_start)
        print('adr_bits_start ', adr_bits_start)

        self.a_clk_ens = []
        if dual_port:
            self.b_addrs = []
            self.b_douts = []
            self.b_clk_ens = []

        # Combine RAMs to increase Depth.
        for d in range(self.depth_cascading):
            self.lram_blocks.append([])
            # Combine RAMs to increase Width.
            for w in range(self.width_cascading):
                datain = Signal(32)
                dataout = Signal(32)
                cs = Signal()
                wren = Signal()

                if sel_bits > 0:
                    bank = self.bus.adr[sel_bits_start:sel_bits_start+sel_bits]
                    self.comb += [
                        datain.eq(self.bus.dat_w[32*w:32*(w+1)]),
                        If(bank == d,
                           cs.eq(1),
                           wren.eq(self.bus.we & self.bus.stb & self.bus.cyc),
                           self.bus.dat_r[32*w:32*(w+1)].eq(dataout)),
                    ]
                else:
                    self.comb += [
                        datain.eq(self.bus.dat_w[32*w:32*(w+1)]),
                        cs.eq(1),
                        wren.eq(self.bus.we & self.bus.stb & self.bus.cyc),
                        self.bus.dat_r[32*w:32*(w+1)].eq(dataout)
                    ]

                byte_enable_n = ~self.bus.sel[4*w:4*(w+1)]
                addr = self.bus.adr[adr_bits_start:adr_bits_start+14]
                a_clk_en = Signal(reset=1)
                if dual_port:
                    b_clk_en = Signal(reset=1)
                    b_addr = Signal(14)
                    b_dout = Signal(32)

                    # Special care needs to be taken since the port B clock
                    # enable signal can be turned on and off at any point. Port
                    # B output needs to lag 2 cycles behind the port B address
                    # input as though the clock enable had never been turned
                    # off.

                    latcher = GatedPassThrough()
                    self.submodules += latcher
                    lram_block = Instance('DPSC512K',
                                          p_ECC_BYTE_SEL='BYTE_EN',
                                          p_OUTREG_A='OUT_REG',
                                          p_OUTREG_B='OUT_REG',
                                          i_DIA=datain,
                                          i_ADA=addr,
                                          i_CLK=ClockSignal(),
                                          i_CEA=a_clk_en,
                                          i_WEA=wren,
                                          i_CSA=cs,
                                          i_RSTA=0b0,
                                          i_CEOUTA=0b1,
                                          i_BENA_N=byte_enable_n,
                                          o_DOA=dataout,
                                          # port B read only
                                          i_ADB=b_addr,
                                          o_DOB=b_dout,
                                          i_CEB=b_clk_en,
                                          i_WEB=0b0,
                                          i_CSB=0b1,
                                          i_RSTB=0b0,
                                          i_CEOUTB=1,
                                          )
                    self.comb += latcher.clk_en.eq(b_clk_en)
                    self.comb += latcher.data_in.eq(b_dout)
                    self.b_clk_ens.append(b_clk_en)
                    self.b_addrs.append(b_addr)
                    self.b_douts.append(latcher.data_out)

                else:
                    a_clk_en = Signal(reset=1)
                    lram_block = Instance('SP512K',
                                          p_ECC_BYTE_SEL='BYTE_EN',
                                          p_OUTREG='OUT_REG',
                                          i_DI=datain,
                                          i_AD=addr,
                                          i_CLK=ClockSignal(),
                                          i_CE=a_clk_en,
                                          i_WE=wren,
                                          i_CS=cs,
                                          i_RSTOUT=0b0,
                                          i_CEOUT=0b1,
                                          i_BYTEEN_N=byte_enable_n,
                                          o_DO=dataout
                                          )
                self.a_clk_ens.append(a_clk_en)
                self.lram_blocks[d].append(lram_block)
                self.specials += lram_block

        ack = Signal()
        self.comb += ack.eq(self.bus.stb & self.bus.cyc)
        ack_delayed = Signal()
        # If we're reading (~wren) then ACK is delayed by one cycle.
        self.sync += ack_delayed.eq(ack & ~wren & ~ack_delayed & ~self.bus.ack)
        self.sync += self.bus.ack.eq((ack & wren & ~self.bus.ack) | ack_delayed)

        if init is not None:
            self.add_init(init)

    def add_init(self, data):
        # Pad it out to make slicing easier below.
        data += [0] * (self.size // self.width * 8 - len(data))
        for d in range(self.depth_cascading):
            for w in range(self.width_cascading):
                offset = d * self.width_cascading * 64*kB + w * 64*kB
                chunk = data[offset:offset + 64*kB]
                self.lram_blocks[d][w].items += initval_parameters(
                    chunk, self.width)


class GatedPassThrough(Module):
    """A pass-through that keeps previous value when disabled.

    Wires data_in to data_out except when clk_en was disabled on the previous
    cycle, in which case the previous value continues to be output. This behaves
    the same as if:
    self.sync += If(self.clk_en, self.data_in.eq(some_signal));
    self.comb += self.data_out.eq(self.data_in);

    But can be used when we can't control how/when data_in gets updated.
    """
    def __init__(self):
        self.data_in = Signal(32)
        self.clk_en = Signal()
        self.data_out = Signal(32)

        prev_clk_en = Signal()
        saved = Signal(32)
        self.sync += If(prev_clk_en, saved.eq(self.data_in))
        self.sync += prev_clk_en.eq(self.clk_en)
        self.comb += self.data_out.eq(Mux(prev_clk_en, self.data_in, saved))
