// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub mod nucleo;
pub mod proto2;

#[cfg(any(
    feature = "nucleo",
    all(not(feature = "nucleo"), not(feature = "proto2"))
))]
pub use nucleo as board;

#[cfg(feature = "proto2")]
pub use proto2 as board;
