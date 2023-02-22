#!/bin/bash

# Upgrade third-party deps under third-party/bazel.
bazel run //third-party:vendor
