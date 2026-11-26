#!/bin/bash
# Whole-program ThreadSanitizer run of Mountain programs (Phase 13 M5). NOT part of CI.
#   usage: bash scripts/tsan_program.sh target/debug/mtnc file.mtn [file.mtn ...]
#
# How: `mtnc build --emit-ir` prints the LLVM IR; every function definition gets the `sanitize_thread`
# attribute; clang compiles the IR with -fsanitize=thread (this inserts the __tsan_* calls, including the
# __tsan_atomic* ones for Atomic<T> operations); `cc` links it with the C runtime (also built with
# -fsanitize=thread) and GCC's libtsan (clang-18 has no TSAN runtime library in this environment; the
# instrumentation interface is the same). The program must exit 0 with no "WARNING: ThreadSanitizer".
# tests/tsan/race_control.mtn is a deliberate data race: running this script on it must FAIL (control).
set -u
MTNC=${1:?usage: tsan_program.sh target/debug/mtnc file.mtn...}; shift
HERE=$(cd "$(dirname "$0")/.." && pwd)
TMP=$(mktemp -d /tmp/mtn_tsan_prog.XXXXXX)
trap 'rm -rf "$TMP"' EXIT
rc=0
for f in "$@"; do
  b=$(basename "$f" .mtn)
  "$MTNC" build "$f" --emit-ir > "$TMP/$b.ll" 2> "$TMP/$b.err" || { echo "FAIL (emit-ir): $f"; cat "$TMP/$b.err"; rc=1; continue; }
  sed -E '/^define /s/ \{$/ sanitize_thread {/' "$TMP/$b.ll" > "$TMP/$b.t.ll"
  clang-18 -fsanitize=thread -O1 -g -c "$TMP/$b.t.ll" -o "$TMP/$b.o" \
    && cc -fsanitize=thread -O1 -g -pthread "$TMP/$b.o" "$HERE/runtime/mtn_rt.c" -o "$TMP/$b.bin" \
    || { echo "FAIL (build): $f"; rc=1; continue; }
  TSAN_OPTIONS="exitcode=66" "$TMP/$b.bin" > "$TMP/$b.out" 2> "$TMP/$b.log"; ec=$?
  w=$(grep -c "WARNING: ThreadSanitizer" "$TMP/$b.log")
  if [ $ec -eq 0 ] && [ "$w" -eq 0 ]; then echo "tsan clean: $f"; else echo "TSAN FINDINGS: $f (exit $ec, $w warning(s))"; head -30 "$TMP/$b.log"; rc=1; fi
done
exit $rc
