// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#pragma once
#include <memory>
#include <string>
#include <vector>

// This is supposed to be a _fairly_ faithful representation of a few
// Chromium codebase APIs. Just enough that we can start to experiment
// with ownership patterns.

namespace content {

class RenderFrameHost {
public:
  static RenderFrameHost *FromId(int process_id, int frame_id);
  virtual int GetRoutingID() = 0;

  /// Returns the assigned name of the frame, the name of the iframe tag
  /// declaring it. For example, <iframe name="framename">[...]</iframe>. It is
  /// quite possible for a frame to have no name, in which case GetFrameName
  /// will return an empty string.
  virtual std::string GetFrameName() = 0;
  virtual ~RenderFrameHost() {}
};

class CreateParams {
public:
  CreateParams(const std::string &);
  std::string main_frame_name_;
};

class WebContentsObserver;

class WebContents {
public:
  static std::unique_ptr<WebContents> Create(const CreateParams &params);

  static WebContents *FromFrameTreeNodeId(int frame_tree_node_id);

  // TODO - should not be in WebContents, just WebContentsImpl
  virtual void AddObserver(WebContentsObserver *) {}
  virtual void RemoveObserver(WebContentsObserver *) {}

  virtual ~WebContents(){};

  virtual const std::string &GetTitle() = 0;
};

class WebContentsObserver {
public:
  virtual void RenderFrameCreated(RenderFrameHost *) {}
  virtual void RenderFrameDeleted(RenderFrameHost *) {}
  virtual ~WebContentsObserver() {}
};

class WebContentsImpl : public WebContents {
public:
  void AddObserver(WebContentsObserver *);
  void RemoveObserver(WebContentsObserver *);
  const std::string &GetTitle();
  WebContentsImpl(const CreateParams &);
  void DeleteRFH();

private:
  std::string title_;
  std::vector<WebContentsObserver *> observers_;
  std::vector<std::unique_ptr<RenderFrameHost>> rfhs_;
};
} // namespace content

void SimulateRendererShutdown(int frame_id);