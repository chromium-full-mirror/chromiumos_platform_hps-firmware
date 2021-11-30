// Copyright 2021 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef _ENTROPY_H
#define _ENTROPY_H

#include <stdint.h>

uint32_t calculate_entropy(int8_t* buffer);

#endif  // _ENTROBY_H