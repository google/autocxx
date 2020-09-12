#!/bin/bash

set -e

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"

DIRS="$DIR/engine $DIR $DIR/gen/build"

for CRATE in $DIRS; do
  echo "Dry run: $CRATE"
  pushd $CRATE
  cargo publish --dry-run
  popd
done

for CRATE in $DIRS; do
  echo "Publish: $CRATE"
  pushd $CRATE
  cargo publish
  popd
done
