// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub mod nucleo;
pub mod proto1;
pub mod proto2;

#[cfg(any(
    feature = "nucleo",
    all(
        not(feature = "proto1"),
        not(feature = "nucleo"),
        not(feature = "proto2")
    )
))]
pub use nucleo as board;

#[cfg(feature = "proto1")]
pub use proto1 as board;

#[cfg(feature = "proto2")]
pub use proto2 as board;
