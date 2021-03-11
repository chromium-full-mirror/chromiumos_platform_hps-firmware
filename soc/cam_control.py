from migen import *

from litex.soc.interconnect.csr import *

# Control bits for HM01B0


class CameraControl(Module, AutoCSR):
    """All non-I2C signals for the camera
    """

    def __init__(self, pads, sys_clk_freq):
        self.pads = pads
        self.sys_clk_freq = sys_clk_freq
        self._w = CSRStorage(fields=[
            CSRField("trig", size=1, offset=1),
            CSRField("mclk_en", size=1, offset=2),
        ],
            name="w")
        self._r = CSRStatus(fields=[
            CSRField("lvld", size=1, offset=0),
            CSRField("fvld", size=1, offset=1),
            CSRField("pclk", size=1, offset=2),
        ],
            name="r")
        self._dat = CSRStatus(fields=[
            CSRField("data", size=8, offset=0),
        ],
            name="dat")
        self.connect_mclk(pads)
        self.connect(pads)

    def connect_mclk(self, pads):
        """Handle the MCLK signal
        """
        # Only change enabled state while clock is low - prevents
        # short clock pulses.
        clk = Signal()
        enabled = Signal()
        self.sync += If(~clk, enabled.eq(self._w.fields.mclk_en))

        # Clock divider - always running
        # TODO: Use a PLL to generate clock
        # Divide sys_clk by constant to get master clock of ~10MHz
        div = int(self.sys_clk_freq / 10e6 / 2)
        count = Signal((div-1).bit_length())
        self.sync += If(count == 0,
                        clk.eq(~clk),
                        count.eq(div-1)
                        ).Else(
            count.eq(count-1)
        )
        # output clock only when enabled.
        self.sync += pads.mclk.eq(enabled & clk)

    def connect(self, pads):
        """Connect all the straight-through signals.
        """
        self.sync += [
            pads.trig.eq(self._w.fields.trig),
        ]

        self.sync += [
            self._r.fields.lvld.eq(pads.lvld),
            self._r.fields.fvld.eq(pads.fvld),
            self._r.fields.pclk.eq(pads.pclk),
        ]
        self.sync += [
            self._dat.fields.data.eq(pads.data)
        ]
