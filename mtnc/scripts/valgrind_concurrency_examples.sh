#!/usr/bin/env bash
# Phase 11c C7: build every program in examples/concurrency/ and run it under valgrind.
# Fails (exit 1) if any program leaks definitely-lost memory, touches invalid memory,
# double-frees, or exits with a non-zero code.  Usage: scripts/valgrind_memory_examples.sh [path/to/mtnc]
set -u
MTNC="${1:-target/debug/mtnc}"
OUT="$(mktemp -d)"
fail=0
for src in examples/concurrency/*.mtn; do
  name="$(basename "$src" .mtn)"
  if ! "$MTNC" build "$src" -o "$OUT/$name" > "$OUT/$name.build.log" 2>&1; then
    echo "BUILD FAILED: $src"; cat "$OUT/$name.build.log"; fail=1; continue
  fi
  valgrind --leak-check=full --errors-for-leak-kinds=definite --error-exitcode=99 -q \
    "$OUT/$name" > "$OUT/$name.out" 2> "$OUT/$name.vg"
  code=$?
  if [ "$code" -ne 0 ]; then
    echo "VALGRIND/RUN FAILED ($code): $src"; cat "$OUT/$name.vg"; fail=1
  else
    echo "valgrind clean: $src"
  fi
done
exit $fail
