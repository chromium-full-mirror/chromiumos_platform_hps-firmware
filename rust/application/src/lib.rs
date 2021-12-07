// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#![cfg_attr(not(test), no_std)]

mod application_state;
pub mod fpga;
mod host_interface;
pub mod trace;

pub use application_state::ApplicationState;
pub use application_state::Configuration;
pub use application_state::Rotation;
pub use application_state::Status;
pub use host_interface::Event;
pub use host_interface::HostInterface;
