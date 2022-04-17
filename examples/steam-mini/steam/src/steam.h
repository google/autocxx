// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#pragma once

// This is a simulation of _something like_ the way the steam API works.

class IEngine {
public:
	virtual int ConnectToGlobalUser(int) = 0;
    virtual void DisconnectGlobalUser(int user_id) = 0;
};

void* GetSteamEngine(); // return an IEngine*
