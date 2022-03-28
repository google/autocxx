// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#include "fake-chromium-header.h"
#include <map>
#include <algorithm>

using namespace content;

// This is all appalling. None of this is real Chromium code.
// It's just designed to be the bare minimum required
// to knock together a quick Rust-side demo. In some future realities, all
// this is replaced with real Chromium code.

int latest_rfh_id = 0;
std::map<int, RenderFrameHost *> render_frame_hosts;
WebContentsImpl *the_only_web_contents; // for this daft demo

CreateParams::CreateParams(const std::string &main_frame_name)
    : main_frame_name_(main_frame_name) {}

RenderFrameHost *RenderFrameHost::FromId(int, int frame_id) {
  return render_frame_hosts.at(frame_id);
}

class RenderFrameHostImpl : public RenderFrameHost {
public:
  RenderFrameHostImpl(const std::string name, int routing_id)
      : routing_id_(routing_id), name_(name) {}
  virtual int GetRoutingID() { return routing_id_; }
  virtual std::string GetFrameName() { return name_; }

private:
  int routing_id_;
  std::string name_;
};

std::unique_ptr<WebContents> WebContents::Create(const CreateParams &params) {
  auto wc = std::make_unique<WebContentsImpl>(params);
  the_only_web_contents = wc.get();
  return wc;
}

WebContentsImpl::WebContentsImpl(const CreateParams &params)
    : title_(params.main_frame_name_) {
  int id = latest_rfh_id++;
  std::unique_ptr<RenderFrameHost> new_rfh(
      new RenderFrameHostImpl(params.main_frame_name_, id));
  render_frame_hosts.insert(
      std::pair<int, RenderFrameHost *>(id, new_rfh.get()));
  for (auto obs : observers_) {
    obs->RenderFrameCreated(new_rfh.get());
  }
  rfhs_.push_back(std::move(new_rfh));
}

void WebContentsImpl::AddObserver(WebContentsObserver *observer) {
  observers_.push_back(observer);
}
void WebContentsImpl::RemoveObserver(WebContentsObserver *observer) {
  std::remove(std::begin(observers_), std::end(observers_), observer);
}

void WebContentsImpl::DeleteRFH() {
  for (auto obs : observers_) {
    obs->RenderFrameDeleted(rfhs_[0].get());
  }
  rfhs_.clear();
}

const std::string &WebContentsImpl::GetTitle() { return title_; }

void SimulateRendererShutdown(int frame_id) {
  render_frame_hosts.erase(frame_id);
  the_only_web_contents->DeleteRFH();
}
