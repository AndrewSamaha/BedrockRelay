#!/bin/bash
# Quick test script to inspect log file format

LOG_FILE="$1"
if [ -z "$LOG_FILE" ]; then
    echo "Usage: $0 <log_file.bin.gz>"
    exit 1
fi

echo "Inspecting: $LOG_FILE"
echo "Compressed size: $(stat -f%z "$LOG_FILE" 2>/dev/null || stat -c%s "$LOG_FILE") bytes"

# Decompress and inspect
gunzip -c "$LOG_FILE" > /tmp/log_inspect.bin 2>/dev/null || gzip -dc "$LOG_FILE" > /tmp/log_inspect.bin 2>/dev/null
if [ $? -ne 0 ]; then
    echo "Failed to decompress"
    exit 1
fi

SIZE=$(stat -f%z /tmp/log_inspect.bin 2>/dev/null || stat -c%s /tmp/log_inspect.bin)
echo "Decompressed size: $SIZE bytes"
echo ""
echo "First 64 bytes (hex):"
hexdump -C /tmp/log_inspect.bin | head -4
echo ""
echo "First 4 bytes as u32 (little-endian):"
hexdump -C /tmp/log_inspect.bin | head -1 | awk '{print $2 $3 $4 $5}' | python3 -c "import sys; data=bytes.fromhex(sys.stdin.read().strip().replace(' ', '')); print(int.from_bytes(data[:4], 'little'))" 2>/dev/null || echo "Could not parse"

rm -f /tmp/log_inspect.bin
