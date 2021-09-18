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

// This example shows Rust subclasses of C++ classes.
// See messages.h and main.rs for most of the interesting code.

#include "messages.h"
#include <ctime>
#include <iostream>
#include <sstream>
#include <vector>
#include <functional>
 
class CppExampleProducer : public MessageProducer {
public:
    CppExampleProducer() {}
    std::string get_message() const {
        std::time_t result = std::time(nullptr);
        std::ostringstream st;
        st << std::asctime(std::localtime(&result))
           << result << " seconds since the Epoch";
        return st.str();
    }
};

class CppExampleDisplayer : public MessageDisplayer {
public:
    CppExampleDisplayer() {}
    void display_message(const std::string& msg) const {
        std::cout << "Message: " << msg << std::endl;
    }
};

std::vector<std::reference_wrapper<const MessageProducer>> producers;
std::vector<std::reference_wrapper<const MessageDisplayer>> displayers;
CppExampleProducer cpp_producer;
CppExampleDisplayer cpp_displayer;


// Maybe we should use a language which tracks lifetimes
// better than this. If only such a language existed.
void register_displayer(const MessageDisplayer& displayer) {
    displayers.push_back(displayer);
}

void register_producer(const MessageProducer& producer) {
    producers.push_back(producer);
}

void register_cpp_thingies() {
    register_producer(cpp_producer);
    register_displayer(cpp_displayer);
}

void run_demo() {
    for (auto& producer: producers) {
        auto msg = producer.get().get_message();
        for (auto& displayer: displayers) {
            displayer.get().display_message(msg);
            std::cout << std::endl;
        }
        std::cout << std::endl;
    }
}

