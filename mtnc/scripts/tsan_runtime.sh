#!/bin/bash
# ThreadSanitizer (and AddressSanitizer + UBSan) run of the C runtime's lock and channel code.
# Usage: bash scripts/tsan_runtime.sh   (from the mtnc directory)
set -u
cd "$(dirname "$0")/.."
out=$(mktemp -d)
status=0
echo "== ThreadSanitizer"
cc -fsanitize=thread -O1 -g -pthread -Wall -Wextra runtime/mtn_rt.c tests/tsan/rt_harness.c -o "$out/tsan" || exit 1
for i in 1 2 3; do
    TSAN_OPTIONS="halt_on_error=0 exitcode=66" "$out/tsan" > "$out/tsan.log" 2>&1
    rc=$?
    warnings=$(grep -c "WARNING: ThreadSanitizer" "$out/tsan.log")
    echo "run $i: exit=$rc, ThreadSanitizer warnings=$warnings"
    [ $rc -ne 0 ] && { status=1; head -60 "$out/tsan.log"; }
done
echo "== AddressSanitizer + UBSan"
cc -fsanitize=address,undefined -fno-sanitize-recover=undefined -O1 -g -pthread -Wall -Wextra runtime/mtn_rt.c tests/tsan/rt_harness.c -o "$out/asan" || exit 1
"$out/asan" > "$out/asan.log" 2>&1
rc=$?
echo "asan/ubsan: exit=$rc"
[ $rc -ne 0 ] && { status=1; head -40 "$out/asan.log"; }
rm -rf "$out"
exit $status
