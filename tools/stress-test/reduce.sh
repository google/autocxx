#!/bin/bash
#
# Using this stress test
# 1. cargo build
# 2. Spot what error message appears (because something will)
# 3. Create a new directory and cd into it
# 4. Run this script passing the error message as an argument
# 5. Wait (consider running tail -f nohup.out)
# 6. Several days later, a minimized test case should appear in nohup.out.

set -e

PROBLEM="$1"
SCRIPT_DIR=$(dirname "$(readlink -f "$0")")
TEST_CASE_DIR=$(pwd)

if [ !-n "$PROBLEM" ]; then
  echo "Specify a compile error as an argument"
  exit -1
fi

echo "About to minimize stress test. Problem is '$PROBLEM' and script dir is '$SCRIPT_DIR'. Test case dir is $TEST_CASE_DIR"

REPRO_CASE="$TEST_CASE_DIR/repro.json"

pushd $SCRIPT_DIR
touch src/main.rs
echo Building with repro case
AUTOCXX_REPRO_CASE=$REPRO_CASE cargo build --release || true
echo Built.
popd

echo Building autocxx-reduce and friends
pushd $SCRIPT_DIR/../..
cargo build --all --release
popd

echo Starting reduction
nohup $SCRIPT_DIR/../../target/release/autocxx-reduce --problem "$PROBLEM" -k --clang-arg=-std=c++17 --creduce-arg=--n --creduce-arg=192 repro -r "$REPRO_CASE" &
echo Reduction underway. Consider using tail -f nohup.out.

