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

use autocxx::include_cpp;
mod render_frame_host;
use autocxx::{c_int, PinMut};
use render_frame_host::RenderFrameHostForWebContents;
use render_frame_host::RenderFrameHostHandle;

include_cpp! {
    #include "fake-chromium-header.h"
    safety!(unsafe) // unsafety policy; see docs
    generate!("content::WebContents")
    generate!("content::RenderFrameHost")
    generate!("content::CreateParams")
    generate!("SimulateRendererShutdown")
    subclass!("content::WebContentsObserver",RenderFrameHostForWebContents)
}

use ffi::ToCppString;

fn main() {
    // Create some fake toy WebContents.
    let create_params = ffi::content::CreateParams::make_unique(&"silly-frame".into_cpp());
    let mut frame = ffi::content::WebContents::Create(&create_params);

    // This object is a memory-safe handle to a RenderFrameHost.
    // On creation, we pass it the WebContents, such that it can register
    // to be informed of the destruction of the RenderFrameHost.
    // It also happens to store a reference to that WebContents,
    // so the compiler will prove that this RenderFrameHostHandle
    // can't outlive the WebContents. That's nice. But currently
    // it stores an exclusive (a.k.a. mutable) reference, and we may
    // well want to relax that in future.
    // (If we are still using autocxx by then, this would be
    // https://github.com/google/autocxx/issues/622)
    let mut rfh_handle = RenderFrameHostHandle::from_id(c_int(3), c_int(0), frame.pin_mut());

    // We can directly call methods on the RFH.
    // (If this were a 'const' method, the `.pin_mut()` wouldn't be necessary).
    let frame_name = rfh_handle.pin_mut().GetFrameName();
    println!("Frame name is {}", frame_name.to_str().unwrap());

    {
        // We can also borrow the RFH and use Rust's borrow checker to ensure
        // no other code can do so. This also gives us a chance to explicitly
        // handle the case where the RFH was already destroyed, in case
        // we want to do something smarter than panicking.
        let mut rfh_borrowed = rfh_handle
            .try_borrow_mut()
            .expect("Oh! The RFH was already destroyed!");
        // Nobody else can borrow it during this time...
        //   let mut rfh_borrowed_again = rfh_handle.try_borrow_mut().unwrap();
        // Gives compile-time error "second mutable borrow occurs here..."
        let frame_name = rfh_borrowed.pin_mut().GetFrameName();
        println!("Frame name is {}", frame_name.to_str().unwrap());
        let frame_name = rfh_borrowed.pin_mut().GetFrameName();
        println!("Frame name is {}", frame_name.to_str().unwrap());

        // Supposing we end up calling some code deep in the Chrome C++
        // stack which destroys the RFH whilst it's still borrowed.
        // That will result in a runtime panic...
        //  ffi::SimulateRendererShutdown(c_int(0)); // would panic
    }

    // But let's assume we've now returned to the event loop.
    // None of the previous borrows still exist. It's perfectly OK to now
    // delete the RFH.
    ffi::SimulateRendererShutdown(c_int(0));
}
