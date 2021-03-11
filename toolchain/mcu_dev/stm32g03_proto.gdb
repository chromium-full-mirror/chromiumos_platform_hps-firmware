source ../../third_party/scripts/gdb-dashboard/.gdbinit

set confirm off
# for sure we are not running the ELF locally.
set auto-connect-native-target off
# connect to OpenOCD
target extended-remote localhost:3333
# https://docs.rs/cortex-m-semihosting/0.3.7/cortex_m_semihosting
monitor arm semihosting enable
