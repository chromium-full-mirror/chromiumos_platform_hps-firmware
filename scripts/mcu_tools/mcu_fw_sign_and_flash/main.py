#!/usr/bin/env python3

import argparse
import hashlib
from ecdsa import SigningKey, VerifyingKey

## FIXME: make parser nicer so this script does all the things!
# gen private key
# $ openssl ecparam -name secp256k1 -genkey -noout -out private.pem
# gen public key
# $ openssl ec -in private.pem -pubout -out public.pem

def get_verify_key(vk_pem):
    """
    Does the same as $ openssl ec -in private.pem -text -noout
    """
    with open(vk_pem, "r") as f:
        vk = VerifyingKey.from_pem(f.read())
    print("verification key to embed in MCU OTP")
    #print(" > uncompressed: {0}".format(vk.to_string("uncompressed").hex()))
    print(" > compressed: {0}".format(vk.to_string("compressed").hex()))
    # format with | sed 's/.\{2\}/0x&, /g'

def verify_binary_signature(data, sig, vk_pem):
    """
    Does the same as $ openssl dgst -sha256 -verify public.pem -signature fw.sig fw.bin
    """
    with open(vk_pem, "r") as f:
        vk = VerifyingKey.from_pem(f.read())
    assert vk.verify(sig, data, hashfunc=hashlib.sha256) # SHA256 is our digest hash.

def gen_binary_signature(data, sk_pem):
    """
    Does the same as $ openssl dgst -sha256 -sign private.pem -out fw.sig fw.bin
    """
    with open(sk_pem, "r") as f:
        sk = SigningKey.from_pem(f.read())
    sig = sk.sign_deterministic(
            data,
            hashfunc=hashlib.sha256 # SHA256 is our digest hash.
            )
    return sig

def patch_binary_payload(binf, sk_pem):
    """
    Takes the generated signature and embeds it into the header of fw.bin INPLACE!

    MUST align with the following definition:
    pub struct ImageHeader {
        magic: u32,
        version: u16,
        vector_addr: &'static u32,
        git_hash: [u8; 16],
        sig: [u8; 64],
    }
    taken from `mcu_rom/stage1/src/main.rs`.

    MUST align with the following definition:
    MEMORY
    {
        HDR   (rx) : ORIGIN = 0x08002000, LENGTH = 128
    }
    taken from `/mcu_rom/stage1/memory.x`.
    """
    with open(binf, "r+b") as f:
        f.seek(128, 0) # advance past HDR [128 bytes].
        data = f.read()
        signature = gen_binary_signature(data, sk_pem)
        f.seek(4, 0) # advance past magic [4 bytes]
        f.seek(2+2, 1) # advance past version [2 bytes] + [2byte alignment padding]
        f.seek(4, 1) # advance past vector_addr [4 bytes]
        f.seek(16, 1) # advance past git_hash [7 bytes]+[sizeof("-modified"):=9]
        f.write(signature)

def verify_patched_binary(binf, vk_pem):
    """
    Takes the generated signature embeded in the header of fw.bin and verifies the data

    MUST align with the following definition:
    pub struct ImageHeader {
        magic: u32,
        version: u16,
        vector_addr: &'static u32,
        git_hash: [u8; 16],
        sig: [u8; 64],
    }
    taken from `mcu_rom/stage1/src/main.rs`.

    MUST align with the following definition:
    MEMORY
    {
        HDR   (rx) : ORIGIN = 0x08002000, LENGTH = 128
    }
    taken from `/mcu_rom/stage1/memory.x`.
    """
    with open(binf, "r+b") as f:
        f.seek(4, 0) # advance past magic [4 bytes]
        f.seek(2+2, 1) # advance past version [2 bytes] + [2byte alignment padding]
        f.seek(4, 1) # advance past vector_addr [4 bytes]
        f.seek(16, 1) # advance past git_hash [7 bytes]+[sizeof("-modified"):=9]
        sig = f.read(64) # extract signature
        f.seek(128, 0) # advance past HDR [128 bytes].
        data = f.read()
        verify_binary_signature(data, sig, vk_pem)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        formatter_class=argparse.RawDescriptionHelpFormatter,
        description=__doc__,
    )
    parser.add_argument("bin", action="store")
    parser.add_argument("sk_pem", action="store")
    parser.add_argument("vk_pem", action="store")
    args = parser.parse_args()

    get_verify_key(args.vk_pem)
    patch_binary_payload(args.bin, args.sk_pem)
    verify_patched_binary(args.bin, args.vk_pem)
