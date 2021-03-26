#!/bin/sh

set -e

git submodule update --init --recursive
pushd or-tools
make third_party
make cc
popd

cargo run
