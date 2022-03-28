// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#include <iostream>
#include "steam.h"

// This is a simulation of _something like_ the way the steam API works.
// None of this code is really from Steam.

class SteamEngine : public IEngine {
    int ConnectToGlobalUser(int user_id) {
        std::cout << "ConnectToGlobalUser, passed " << user_id << std::endl;
        return 42;
    }
    void DisconnectGlobalUser(int user_id) {
        std::cout << "DisconnectGlobalUser, passed " << user_id << std::endl;
    }
};

void* GetSteamEngine() {
    return new SteamEngine();
}

