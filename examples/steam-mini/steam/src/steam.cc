// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

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

