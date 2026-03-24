#!/usr/bin/env bash
# Benchmark pagers vs vmtouch using hyperfine.
#
# Requires: pagers, vmtouch, hyperfine, dd
# Tip: run inside `devshell` to get all three.

set -euo pipefail

PAGERS="${PAGERS:-pagers}"
VMTOUCH="${VMTOUCH:-vmtouch}"

for cmd in "$PAGERS" "$VMTOUCH" hyperfine dd; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "error: $cmd not found in PATH" >&2
    exit 1
  fi
done

export NO_COLOR=1

echo "Arch: $(uname -m)"
echo "OS:   $(uname -s) $(uname -r)"
echo

BENCH_DIR="$(mktemp -d)"
trap 'rm -rf "$BENCH_DIR"' EXIT

BIG_FILES=()
BIG_SIZES_MB=(512 1024 10240)
BIG_LABELS=("500MiB" "1GiB" "10GiB")

TREE_DIR="$BENCH_DIR/tree"
BATCH_FILE="$BENCH_DIR/batch.txt"

echo "Setting up test fixtures in $BENCH_DIR ..."

for i in "${!BIG_SIZES_MB[@]}"; do
  mb="${BIG_SIZES_MB[$i]}"
  label="${BIG_LABELS[$i]}"
  f="$BENCH_DIR/${label}.bin"
  echo "  Creating ${label} file ..."
  dd if=/dev/zero of="$f" bs=1M count="$mb" status=none
  BIG_FILES+=("$f")
done

# Directory tree: 1000 files × 1 MiB = ~1 GiB total
echo "  Creating directory tree (1000 × 1 MiB) ..."
mkdir -p "$TREE_DIR"
for i in $(seq 1 1000); do
  dd if=/dev/zero of="$TREE_DIR/file_$(printf '%04d' "$i").bin" bs=1M count=1 status=none
done

# Batch file listing all tree files
find "$TREE_DIR" -type f -name '*.bin' | sort > "$BATCH_FILE"

echo "Fixtures ready."
echo

# 1. Query time — cached (warm) vs uncached (cold)
for i in "${!BIG_FILES[@]}"; do
  f="${BIG_FILES[$i]}"
  size="${BIG_LABELS[$i]}"

  echo "=== 1a. Query cached: ${size} file ==="
  hyperfine \
    --warmup 2 \
    --min-runs 5 \
    --prepare "$(printf '%q touch -q %q 2>/dev/null' "$PAGERS" "$f")" \
    "$VMTOUCH $f" \
    "$PAGERS query -q $f"

  echo
  echo "=== 1b. Query uncached: ${size} file ==="
  hyperfine \
    --warmup 1 \
    --min-runs 5 \
    --prepare "$(printf '%q evict -q %q 2>/dev/null' "$PAGERS" "$f")" \
    "$VMTOUCH $f" \
    "$PAGERS query -q $f"
  echo
done

# 2. Eviction time
for i in "${!BIG_FILES[@]}"; do
  f="${BIG_FILES[$i]}"
  size="${BIG_LABELS[$i]}"

  echo "=== 2. Evict: ${size} file ==="
  hyperfine \
    --warmup 1 \
    --min-runs 5 \
    --prepare "$(printf '%q touch -q %q 2>/dev/null' "$PAGERS" "$f")" \
    "$VMTOUCH -e $f" \
    "$PAGERS evict -q $f"
  echo
done

echo "=== 2. Evict: directory tree ==="
hyperfine \
  --warmup 1 \
  --min-runs 5 \
  --prepare "$(printf '%q touch -q %q 2>/dev/null' "$PAGERS" "$TREE_DIR")" \
  "$VMTOUCH -e $TREE_DIR" \
  "$PAGERS evict -q $TREE_DIR"
echo

# 3. Touch time — big files
for i in "${!BIG_FILES[@]}"; do
  f="${BIG_FILES[$i]}"
  size="${BIG_LABELS[$i]}"

  echo "=== 3. Touch: ${size} file ==="
  hyperfine \
    --warmup 1 \
    --min-runs 5 \
    --prepare "$(printf '%q evict -q %q 2>/dev/null' "$PAGERS" "$f")" \
    "$VMTOUCH -t $f" \
    "$PAGERS touch -q $f"
  echo
done

# 4. Touch time — large directory tree
echo "=== 4. Touch: directory tree (1000 × 1 MiB) ==="
hyperfine \
  --warmup 1 \
  --min-runs 5 \
  --prepare "$(printf '%q evict -q %q 2>/dev/null' "$PAGERS" "$TREE_DIR")" \
  "$VMTOUCH -t $TREE_DIR" \
  "$PAGERS touch -q $TREE_DIR"
echo

# 5. Touch time — batch mode
echo "=== 5. Touch: batch mode (1000 files from file list) ==="
hyperfine \
  --warmup 1 \
  --min-runs 5 \
  --prepare "$(printf '%q evict -q %q 2>/dev/null' "$PAGERS" "$TREE_DIR")" \
  "$VMTOUCH -t -b $BATCH_FILE" \
  "$PAGERS touch -q -b $BATCH_FILE"
