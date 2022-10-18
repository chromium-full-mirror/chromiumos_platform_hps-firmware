// Copyright 2021 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub mod proto2;

#[cfg(feature = "proto2")]
pub use proto2 as board;
