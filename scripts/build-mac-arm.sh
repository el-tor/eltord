#!/bin/bash

# Build script for macOS ARM64 natively (without Docker)
# This mimics the GitHub Actions workflow but runs locally

# Ensure we're in the correct directory
cd "$(dirname "$0")/.."
WORKSPACE_DIR=$(pwd)

echo "🍎 Building macOS ARM64 binary natively..."
echo ""

# Clean cache and artifacts to ensure fresh build
echo "🧹 Cleaning cache and artifacts..."
rm -rf cache/eltord-build-artifacts/macOS-arm64
rm -rf artifacts/macOS-arm64
echo ""

# Set environment variables
export PLATFORM_TARGET="aarch64-apple-darwin"
export PLATFORM_BIN="eltord"
export PLATFORM_OS_NAME="macOS-arm64"

# Ensure we're using rustup's beta toolchain which supports edition2024
echo "🦀 Configuring Rust toolchain..."
rustup default beta
rustup target add aarch64-apple-darwin --toolchain beta
echo "✅ Rust version: $(rustc --version)"
echo "✅ Cargo version: $(cargo --version)"
echo ""

# Create temporary build directory
export BUILD_DIR="$HOME/tmp/eltord-build-$(date +%s)"
mkdir -p "$BUILD_DIR"
echo "📁 Created build directory: $BUILD_DIR"
echo ""

# Clone dependencies
echo "🔄 Cloning git dependencies..."
git clone https://github.com/el-tor/eltor.git "$BUILD_DIR/eltor"
git clone https://github.com/lightning-node-interface/lni.git "$BUILD_DIR/lni"
git clone https://github.com/el-tor/libeltor-sys.git "$BUILD_DIR/libeltor-sys"
git clone https://github.com/el-tor/libeltor.git "$BUILD_DIR/libeltor"
git clone https://github.com/el-tor/eltord.git "$BUILD_DIR/eltord"

# Checkout specific branches
# echo "🌿 Checking out specific branches..."
# cd "$BUILD_DIR/eltord" && git checkout lib
# cd "$BUILD_DIR/lni" && git checkout search

echo ""
echo "🔨 Building libeltor-sys..."
cd "$BUILD_DIR/libeltor-sys"
chmod +x scripts/copy.sh scripts/build.sh
./scripts/copy.sh
mkdir -p patches libtor-src/patches
touch patches/.keep libtor-src/patches/.keep
cargo build --release --verbose --target aarch64-apple-darwin --features vendored-openssl

echo ""
echo "🔨 Building eltord..."
cd "$WORKSPACE_DIR"
cargo build --release --verbose --target aarch64-apple-darwin --features vendored-openssl

echo ""
echo "📦 Copying artifacts..."

# Return to workspace directory for artifact handling
cd "$WORKSPACE_DIR"

# Check if binary exists
BINARY_PATH="$WORKSPACE_DIR/target/$PLATFORM_TARGET/release/eltor"
if [ ! -f "$BINARY_PATH" ]; then
    echo "❌ Error: Binary not found at $BINARY_PATH"
    echo "Available files in target/$PLATFORM_TARGET/release/:"
    ls -la "$WORKSPACE_DIR/target/$PLATFORM_TARGET/release/" || echo "Directory does not exist"
    exit 1
fi

echo "✅ Found binary at: $BINARY_PATH"
echo "📏 Binary size: $(ls -lh "$BINARY_PATH" | awk '{print $5}')"

# Create artifacts directory
mkdir -p "artifacts/$PLATFORM_OS_NAME"
cp "$BINARY_PATH" "artifacts/$PLATFORM_OS_NAME/$PLATFORM_BIN"
echo "✅ Copied to artifacts/$PLATFORM_OS_NAME/$PLATFORM_BIN"

# Copy to persistent cache (same logic as GitHub Actions)
CACHE_DIR="cache/eltord-build-artifacts"
mkdir -p "$CACHE_DIR/$PLATFORM_OS_NAME"
cp "$BINARY_PATH" "$CACHE_DIR/$PLATFORM_OS_NAME/$PLATFORM_BIN"
echo "✅ Cached artifact to: $(pwd)/$CACHE_DIR/$PLATFORM_OS_NAME/$PLATFORM_BIN"

# Make binaries executable
chmod +x "$CACHE_DIR/$PLATFORM_OS_NAME/$PLATFORM_BIN"
chmod +x "artifacts/$PLATFORM_OS_NAME/$PLATFORM_BIN"

echo ""
echo "🧹 Cleaning up..."
rm -rf "$BUILD_DIR"
echo "Cleaned up temporary build directory: $BUILD_DIR"

echo ""
echo "=== Local Build Artifacts - macOS ARM64 ==="
echo "📁 Workspace artifacts:"
ls -la artifacts/
ls -la "artifacts/$PLATFORM_OS_NAME/"
echo ""

if [ -d "$CACHE_DIR" ]; then
  echo "🗃️  Cached artifacts (persistent between runs):"
  ls -la "$CACHE_DIR"
  if [ -d "$CACHE_DIR/$PLATFORM_OS_NAME" ]; then
    ls -la "$CACHE_DIR/$PLATFORM_OS_NAME/"
  fi
  echo ""
fi

echo "🎉 Build completed successfully!"
echo "📦 Workspace artifact: $(pwd)/artifacts/$PLATFORM_OS_NAME/$PLATFORM_BIN"
if [ -f "$CACHE_DIR/$PLATFORM_OS_NAME/$PLATFORM_BIN" ]; then
  echo "🗃️  Cached artifact: $(pwd)/$CACHE_DIR/$PLATFORM_OS_NAME/$PLATFORM_BIN"
fi
echo "📏 Binary size: $(ls -lh "artifacts/$PLATFORM_OS_NAME/$PLATFORM_BIN" | awk '{print $5}')"
echo "🏗️  Architecture: $PLATFORM_TARGET"
