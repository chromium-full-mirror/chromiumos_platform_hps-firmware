// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! This crate performs very simplistic compression of a bitstream produced by
//! Radiant for running on a Crosslink NX FPGA. It takes advantage of observed
//! properties of these files. The compression isn't nearly as good as proper
//! zip algorithms like gzip, but the code to decompress is very simple and can
//! run on a microcontroller with very little flash.
//!
//! The format is as follows:
//!
//! - The data is divided up into blocks of a similar kind. A block is encoded
//!   with a byte that specifies which type of block it is.
//! - A run of zero-bytes of length 0x01 to 0x3f is encoded as a byte between
//!   0x01 and 0x3f.
//! - A run of 0xff-bytes of length 0x01 to 0x3f is encoded as a byte between
//!   0x40 and 0x7f.
//! - Two special sequences that occur quite a lot are encoded with the values
//!   0x00 and 0x80.
//! - Everything else is encoded as blocks of literal bytes up to 0x7f in length
//!   and preceded by the block length, encoded as a byte in the range 0x81 to
//!   0xff.
//!
//! When supplied with a simple gateware bitstream (that blinked a few GPIO
//! lines), this compression algorithm was able to reduce the file size from
//! 364KB to 11KB. For comparison, gzip was able to reduce the same file to
//! 4.5KB. This algorithm is a win in cases such as factory testing where we
//! need to transfer both the decompression code and the data to decompress,
//! since algorithms like gzip seem to take in the order of several hundred KB.

#![cfg_attr(not(test), no_std)]

use core::cmp::min;

const MAX_DATA_RUN: usize = 127;
const MAX_REPEAT_RUN: usize = 63;
const ZERO_THRESHOLD: usize = 2;
// Note, sequence 1 is followed by a 1-byte repeat count, since this sequence
// often repeats many times.
const SEQUENCE1: &[u8] = &sequence1();
const SEQUENCE1_TOKEN: u8 = 0;
// Note, sequence 2 does not have a repeat count, since it generally does not
// repeat and representing this sequence as 2 bytes instead of 1 would consume a
// significant amount of extra space.
const SEQUENCE2: &[u8] = &sequence2();
const SEQUENCE2_TOKEN: u8 = 0x80;

/// Compresses `input`, calling `out` for each byte in the compressed output.
pub fn compress(input: &[u8], mut out: impl FnMut(u8)) {
    let mut remaining = input;
    while let Some(next) = remaining.first() {
        if let Some(mut after) = remaining.strip_prefix(SEQUENCE1) {
            out(SEQUENCE1_TOKEN);
            let mut count = 1;
            while let Some(a) = after.strip_prefix(SEQUENCE1) {
                count += 1;
                after = a;
                if count == 0xff {
                    break;
                }
            }
            out(count);
            remaining = after;
            continue;
        }
        if let Some(after) = remaining.strip_prefix(SEQUENCE2) {
            out(SEQUENCE2_TOKEN);
            remaining = after;
            continue;
        }
        let mut run_length = 0;
        if *next == 0 {
            // Next byte is zero, output a run of zeros up until the first
            // non-zero, until we reach the maximum block size or we reach the
            // end.
            run_length = repeated_run_length(0, remaining);
            out(run_length as u8);
        } else if *next == 0xff {
            run_length = repeated_run_length(0xff, remaining);
            out(run_length as u8 | 0x40);
        } else {
            // Next byte is non-zero, output a run of data up until we find
            // enough zeros to be worthwhile stopping, until we reach the
            // maximum block size or we reach the end.
            let mut consecutive_zeros = 0;
            let mut consecutive_ff = 0;
            for byte in remaining[..min(MAX_DATA_RUN, remaining.len())].iter() {
                run_length += 1;
                if *byte == 0 {
                    consecutive_zeros += 1;
                } else {
                    consecutive_zeros = 0;
                }
                if *byte == 0xff {
                    consecutive_ff += 1;
                } else {
                    consecutive_ff = 0;
                }
                if consecutive_zeros == ZERO_THRESHOLD || consecutive_ff == ZERO_THRESHOLD {
                    run_length -= consecutive_zeros + consecutive_ff;
                    break;
                }
                if remaining[run_length..].starts_with(SEQUENCE1)
                    || remaining[run_length..].starts_with(SEQUENCE2)
                {
                    break;
                }
            }
            out(run_length as u8 | 0x80);
            for byte in &remaining[..run_length] {
                out(*byte);
            }
        }
        remaining = &remaining[run_length..];
    }
}

/// An iterator that consumes compressed bytes and produces decompressed bytes.
pub struct Decompressor<'a, I: Iterator<Item = &'a u8>> {
    input: I,
    state: CommandState,
}

enum CommandState {
    Data(u8),
    Zeros(u8),
    Ff(u8),
    Sequence1(u8, u8),
    Sequence2(u8),
    End,
}

impl<'a, I: Iterator<Item = &'a u8>> Decompressor<'a, I> {
    pub fn new<T: core::iter::IntoIterator<IntoIter = I>>(input: T) -> Decompressor<'a, I> {
        let mut decompressor = Decompressor {
            input: input.into_iter(),
            // Temporarily set this to End. It'll be updated when we call
            // advance below.
            state: CommandState::End,
        };
        decompressor.advance();
        decompressor
    }

    fn advance(&mut self) {
        if let Some(&byte) = self.input.next() {
            self.state = match byte {
                0 => {
                    if let Some(&count) = self.input.next() {
                        CommandState::Sequence1(0, count)
                    } else {
                        CommandState::End
                    }
                }
                1..=0x3f => CommandState::Zeros(byte),
                0x40 => {
                    // Currently unused and thus invalid.
                    CommandState::End
                }
                0x41..=0x7f => CommandState::Ff(byte & 0x3f),
                0x80 => CommandState::Sequence2(0),
                0x81..=0xff => CommandState::Data(byte & 0x7f),
            };
        } else {
            self.state = CommandState::End;
        }
    }
}

impl<'a, I: Iterator<Item = &'a u8>> Iterator for Decompressor<'a, I> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        match self.state {
            CommandState::Data(count) => {
                let value = self.input.next().cloned();
                if count <= 1 {
                    self.advance();
                } else {
                    self.state = CommandState::Data(count - 1);
                }
                value
            }
            CommandState::Zeros(count) => {
                if count <= 1 {
                    self.advance();
                } else {
                    self.state = CommandState::Zeros(count - 1);
                }
                Some(0)
            }
            CommandState::Ff(count) => {
                if count <= 1 {
                    self.advance();
                } else {
                    self.state = CommandState::Ff(count - 1);
                }
                Some(0xff)
            }
            CommandState::Sequence1(offset, count) => {
                if offset + 1 == SEQUENCE1.len() as u8 {
                    if count <= 1 {
                        self.advance();
                    } else {
                        self.state = CommandState::Sequence1(0, count - 1);
                    }
                } else {
                    self.state = CommandState::Sequence1(offset + 1, count);
                }
                Some(SEQUENCE1[offset as usize])
            }
            CommandState::Sequence2(offset) => {
                if offset + 1 == SEQUENCE2.len() as u8 {
                    self.advance();
                } else {
                    self.state = CommandState::Sequence2(offset + 1);
                }
                Some(SEQUENCE2[offset as usize])
            }
            CommandState::End => None,
        }
    }
}

const fn sequence1() -> [u8; 47] {
    let mut result = [0u8; 47];
    result[0] = 0x73;
    result[1] = 0x01;
    result[2] = 0xff;
    result
}

const fn sequence2() -> [u8; 49] {
    let mut result = [0u8; 49];
    result[0] = 0x3b;
    result[1] = 0xe1;
    result[2] = 0xeb;
    result[3] = 0x0f;
    result[4] = 0xff;
    result
}

fn repeated_run_length(to_repeat: u8, bytes: &[u8]) -> usize {
    bytes
        .iter()
        .take(MAX_REPEAT_RUN)
        .position(|byte| *byte != to_repeat)
        .unwrap_or_else(|| min(MAX_REPEAT_RUN, bytes.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks that `input` compresses to `expected` and that when decompressed,
    /// we get back to `input`.
    #[track_caller]
    fn check(input: &[u8], expected: &[u8]) {
        let mut compressed = Vec::new();
        compress(input, |byte| compressed.push(byte));
        assert_eq!(&compressed, expected);
        let decompressed: Vec<u8> = Decompressor::new(&compressed).collect();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn empty() {
        check(&[], &[]);
    }

    #[test]
    fn trivial_data() {
        check(&[1], &[0x81, 1]);
        check(&[10, 20], &[0x82, 10, 20]);
        check(&[10, 20, 30], &[0x83, 10, 20, 30]);
    }

    #[test]
    fn only_zeros() {
        check(&[0], &[1]);
        check(&[0, 0], &[2]);
        check(&[0, 0, 0], &[3]);
    }

    #[test]
    fn data_then_zeros() {
        check(&[1, 0, 0, 0], &[0x81, 1, 0x3]);
    }

    #[test]
    fn zeros_then_data() {
        check(&[0, 0, 0, 1], &[0x3, 0x81, 1]);
    }

    #[test]
    fn single_zero() {
        check(&[1, 2, 3, 0, 4, 5, 6], &[0x87, 1, 2, 3, 0, 4, 5, 6]);
    }

    #[test]
    fn data_then_ff() {
        check(&[1, 0xff, 0xff, 0xff], &[0x81, 1, 0x43]);
    }

    #[test]
    fn multiple_zero_blocks() {
        check(&[0; 128], &[63, 63, 2]);
    }

    #[test]
    fn multiple_data_blocks() {
        let mut expected = vec![0xff];
        expected.extend_from_slice(&[42; 127]);
        expected.push(0x81);
        expected.push(42);
        check(&[42; 128], &expected);
    }

    #[test]
    fn sequence1() {
        check(SEQUENCE1, &[0, 1]);
        let mut input = vec![10, 20];
        input.extend_from_slice(SEQUENCE1);
        input.extend_from_slice(SEQUENCE1);
        input.extend_from_slice(SEQUENCE1);
        input.push(30);
        check(&input, &[0x82, 10, 20, 0, 3, 0x81, 30]);
    }

    #[test]
    fn sequence2() {
        check(SEQUENCE2, &[0x80]);
        let mut input = vec![10, 20];
        input.extend_from_slice(SEQUENCE2);
        input.extend_from_slice(SEQUENCE2);
        input.push(30);
        check(&input, &[0x82, 10, 20, 0x80, 0x80, 0x81, 30]);
    }
}
