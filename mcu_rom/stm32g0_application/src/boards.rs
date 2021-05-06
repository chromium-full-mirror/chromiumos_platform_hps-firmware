// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub mod nucleo;
pub mod proto0;

#[cfg(feature = "proto0")]
pub use proto0 as board;

#[cfg(any(
    feature = "nucleo",
    all(not(feature = "proto0"), not(feature = "nucleo"))
))]
pub use nucleo as board;
