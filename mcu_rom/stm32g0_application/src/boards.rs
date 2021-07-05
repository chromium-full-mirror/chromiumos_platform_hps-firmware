// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#[cfg(any(
    feature = "nucleo",
    all(not(feature = "nucleo"))
))]
pub mod nucleo;

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
pub mod proto1;

#[cfg(feature = "proto1")]
pub use proto1 as board;

#[cfg(feature = "proto2")]
pub mod proto2;

#[cfg(feature = "proto2")]
pub use proto2 as board;
