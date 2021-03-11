# Lint as: python3
"""Emits an error telling the user not to use the default toolchain."""

import sys

_ERROR_MESSAGE = """
Error: trying to build a target with the default toolchain

This occurs when a GN target is listed as a dependency outside of a toolchain
group in the root BUILD.gn file.

Make sure that your top-level targets are always instantiated with a toolchain
and that no dependencies are pulled in through the default toolchain.
"""

def main() -> int:
  print(_ERROR_MESSAGE, file=sys.stderr)
  return 1

if __name__ == '__main__':
  sys.exit(main())
