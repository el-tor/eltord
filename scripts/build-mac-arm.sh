#!/bin/bash

# Build script for macOS ARM64 natively (without Docker)
# This mimics the GitHub Actions workflow but runs locally

# Ensure we're in the correct directory
cd "$(dirname "$0")/.."
WORKSPACE_DIR=$(pwd)

echo "🍎 Building macOS ARM64 binary natively..."
echo ""

# Set environment variables
export PLATFORM_TARGET="aarch64-apple-darwin"
export PLATFORM_BIN="eltord"
export PLATFORM_OS_NAME="macOS-arm64"

# Create shell.nix for the build environment
cat > shell.nix << 'EOF'
{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    # Rust toolchain
    rustc
    cargo
    rustfmt
    clippy
    
    # Build dependencies
    pkg-config
    openssl
    sqlite
    autoconf
    automake
    libtool
    gnumake
    wget
    git
    flex
    bison
    unzip
    
    # macOS specific
    darwin.apple_sdk.frameworks.Security
    darwin.apple_sdk.frameworks.SystemConfiguration
  ];
  
  # Environment variables
  OPENSSL_DIR = "${pkgs.openssl.dev}";
  OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
  OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
  PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig:${pkgs.sqlite.dev}/lib/pkgconfig";
  SQLITE3_LIB_DIR = "${pkgs.sqlite.out}/lib";
  SQLITE3_INCLUDE_DIR = "${pkgs.sqlite.dev}/include";
  
  # Rust target
  CARGO_BUILD_TARGET = "aarch64-apple-darwin";
}
EOF

echo "📦 Created shell.nix environment file"
echo ""

# Install Rust target in Nix environment
echo "🦀 Installing Rust target..."
nix-shell --run "rustup target add aarch64-apple-darwin"
echo ""

# Verify Nix environment
echo "🔍 Verifying Nix environment..."
nix-shell --run "
  echo '=== Nix Environment Setup ==='
  echo 'Rust version:' && rustc --version
  echo 'Cargo version:' && cargo --version
  echo 'OpenSSL version:' && openssl version
  echo
"

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
echo "🌿 Checking out specific branches..."
cd "$BUILD_DIR/eltor" && git checkout lib
cd "$BUILD_DIR/lni" && git checkout search

# Copy shell.nix to build directories for nix-shell context
echo "📄 Copying shell.nix to build directories..."
cp "$WORKSPACE_DIR/shell.nix" "$BUILD_DIR/libeltor-sys/"
cp "$WORKSPACE_DIR/shell.nix" "$BUILD_DIR/eltord/"

echo ""
echo "🔨 Building libeltor-sys..."
cd "$WORKSPACE_DIR"
nix-shell --run "
  cd '$BUILD_DIR/libeltor-sys'
  chmod +x scripts/copy.sh scripts/build.sh
  ./scripts/copy.sh
  mkdir -p patches libtor-src/patches
  touch patches/.keep libtor-src/patches/.keep
  cargo build --release --verbose --target aarch64-apple-darwin --features vendored-openssl
"

echo ""
echo "🔨 Building eltord..."
cd "$WORKSPACE_DIR"
nix-shell --run "
  cd '$BUILD_DIR/eltord'
  cargo build --release --verbose --target aarch64-apple-darwin --features vendored-openssl
"

echo ""
echo "📦 Copying artifacts..."

# Return to workspace directory for artifact handling
cd "$WORKSPACE_DIR"

# Create artifacts directory
mkdir -p "artifacts/$PLATFORM_OS_NAME"
cp "$BUILD_DIR/eltord/target/$PLATFORM_TARGET/release/eltor" "artifacts/$PLATFORM_OS_NAME/$PLATFORM_BIN"

# Copy to persistent cache (same logic as GitHub Actions)
CACHE_DIR="cache/eltord-build-artifacts"
mkdir -p "$CACHE_DIR/$PLATFORM_OS_NAME"
cp "$BUILD_DIR/eltord/target/$PLATFORM_TARGET/release/eltor" "$CACHE_DIR/$PLATFORM_OS_NAME/$PLATFORM_BIN"
echo "Cached artifact to: $(pwd)/$CACHE_DIR/$PLATFORM_OS_NAME/$PLATFORM_BIN"

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
