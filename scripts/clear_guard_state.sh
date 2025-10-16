#!/bin/bash

# Script to clear Tor Guard state and rate-limiting cache
# This forces Tor to select new Guards and clears any rate-limiting state
# 
# Usage: ./scripts/clear_guard_state.sh [tor_data_dir]
# 
# If no directory is provided, defaults to ./tmp/client

set -e

# Default to tmp/client if no argument provided
TOR_DATA_DIR="${1:-./tmp/prod/client}"

echo "======================================="
echo "Clearing Tor Guard State"
echo "======================================="
echo "Data directory: $TOR_DATA_DIR"
echo ""

# Check if directory exists
if [ ! -d "$TOR_DATA_DIR" ]; then
    echo "Error: Directory $TOR_DATA_DIR does not exist"
    exit 1
fi

# Files and directories to clear:
# - state: Contains Guard selection and circuit failure history
# - keys/: Contains identity keys that Guards use to recognize you
# - cached-certs: Cached authority certificates
# - cached-microdesc-consensus: Cached consensus
# - cached-microdescs.new: Cached relay descriptors
# - diff-cache/: Consensus diffs

echo "Removing files that affect Guard rate-limiting:"

# Remove state file
if [ -f "$TOR_DATA_DIR/state" ]; then
    echo "  ✓ Removing state file (Guard selection, circuit history)"
    rm -f "$TOR_DATA_DIR/state"
else
    echo "  - state file not found (already clean)"
fi

# Remove keys directory
if [ -d "$TOR_DATA_DIR/keys" ]; then
    echo "  ✓ Removing keys/ directory (identity, Guards won't recognize you)"
    rm -rf "$TOR_DATA_DIR/keys"
else
    echo "  - keys/ directory not found (already clean)"
fi

# Remove cached consensus and descriptors
if [ -f "$TOR_DATA_DIR/cached-certs" ]; then
    echo "  ✓ Removing cached-certs"
    rm -f "$TOR_DATA_DIR/cached-certs"
fi

if [ -f "$TOR_DATA_DIR/cached-microdesc-consensus" ]; then
    echo "  ✓ Removing cached-microdesc-consensus"
    rm -f "$TOR_DATA_DIR/cached-microdesc-consensus"
fi

if [ -f "$TOR_DATA_DIR/cached-microdescs.new" ]; then
    echo "  ✓ Removing cached-microdescs.new"
    rm -f "$TOR_DATA_DIR/cached-microdescs.new"
fi

if [ -d "$TOR_DATA_DIR/diff-cache" ]; then
    echo "  ✓ Removing diff-cache/ directory"
    rm -rf "$TOR_DATA_DIR/diff-cache"
fi

echo ""
echo "======================================="
echo " ✅ Guard state cleared successfully!"
echo "======================================="
echo ""
echo "What this does:"
echo "  • Clears Guard selection (Tor will pick new Guards)"
echo "  • Removes identity keys (Guards won't recognize you)"
echo "  • Clears circuit failure history"
echo "  • Forces fresh consensus/descriptor download"
echo ""
echo "Next steps:"
echo "  1. Restart your Tor client"
echo "  2. Tor will select new Guards"
echo "  3. Rate-limiting counters will be reset"
echo ""
