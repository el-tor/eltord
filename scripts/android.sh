#!/bin/bash
set -euo pipefail

# === Configuration ===
ANDROID_NDK_VERSION=28.0.12433566
ANDROID_API=24
TARGET=${1:-aarch64-linux-android}  # Allow target to be passed as argument

# Validate target
case "$TARGET" in
    aarch64-linux-android|armv7-linux-androideabi|i686-linux-android|x86_64-linux-android)
        ;;
    *)
        echo "❌ Unsupported target: $TARGET"
        echo "Supported targets: aarch64-linux-android, armv7-linux-androideabi, i686-linux-android, x86_64-linux-android"
        exit 1
        ;;
esac

# === Detect OS and Architecture ===
detect_host_os_arch() {
    local os_name
    local arch_name
    
    # Detect OS
    case "$(uname -s)" in
        Darwin*)
            os_name="darwin"
            ;;
        Linux*)
            os_name="linux"
            ;;
        MINGW64*|MSYS_NT*|CYGWIN*)
            os_name="windows"
            ;;
        *)
            echo "❌ Unsupported OS: $(uname -s)"
            exit 1
            ;;
    esac
    
    # Detect architecture
    case "$(uname -m)" in
        x86_64|amd64)
            arch_name="x86_64"
            ;;
        arm64|aarch64)
            arch_name="x86_64"  # NDK uses x86_64 toolchain even on ARM Macs
            ;;
        *)
            echo "❌ Unsupported architecture: $(uname -m)"
            exit 1
            ;;
    esac
    
    echo "${os_name}-${arch_name}"
}

HOST_ARCH=$(detect_host_os_arch)

# === NDK Paths ===
# Check if NDK_HOME is already set, otherwise use default paths
if [[ -n "${NDK_HOME:-}" && -d "$NDK_HOME" ]]; then
    echo " Using existing NDK_HOME: $NDK_HOME"
    NDK="$NDK_HOME"
    TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$HOST_ARCH"
else
    echo " NDK_HOME not set or invalid, using default paths"
    if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" ]]; then
        # Windows paths
        NDK="$USERPROFILE/AppData/Local/Android/Sdk/ndk/$ANDROID_NDK_VERSION"
        TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/windows-x86_64"
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        # Linux paths
        NDK="$HOME/Android/Sdk/ndk/$ANDROID_NDK_VERSION"
        TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$HOST_ARCH"
    else
        # macOS paths
        NDK="$HOME/Library/Android/sdk/ndk/$ANDROID_NDK_VERSION"
        TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$HOST_ARCH"
    fi
fi

# === Environment setup (minimal working config) ===
# Only export NDK_HOME if it wasn't already set
if [[ -z "${NDK_HOME:-}" ]]; then
    export NDK_HOME="$NDK"
fi

# Set target-specific compiler
case "$TARGET" in
    aarch64-linux-android)
        export CC_aarch64_linux_android="$TOOLCHAIN/bin/${TARGET}${ANDROID_API}-clang"
        ;;
    armv7-linux-androideabi)
        export CC_armv7_linux_androideabi="$TOOLCHAIN/bin/armv7a-linux-androideabi${ANDROID_API}-clang"
        ;;
    i686-linux-android)
        export CC_i686_linux_android="$TOOLCHAIN/bin/${TARGET}${ANDROID_API}-clang"
        ;;
    x86_64-linux-android)
        export CC_x86_64_linux_android="$TOOLCHAIN/bin/${TARGET}${ANDROID_API}-clang"
        ;;
esac

export TARGET_AR="llvm-ar"
export RUSTFLAGS="-C panic=abort -C link-arg=-fuse-ld=lld"
export CFLAGS_${TARGET//-/_}="-DOPENSSL_RAND_SEED_NONE"
export PATH="$TOOLCHAIN/bin:$PATH"

# Set Android-specific linker flags
case "$TARGET" in
    aarch64-linux-android)
        export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN/bin/${TARGET}${ANDROID_API}-clang"
        ;;
    armv7-linux-androideabi)
        export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$TOOLCHAIN/bin/armv7a-linux-androideabi${ANDROID_API}-clang"
        ;;
    i686-linux-android)
        export CARGO_TARGET_I686_LINUX_ANDROID_LINKER="$TOOLCHAIN/bin/${TARGET}${ANDROID_API}-clang"
        ;;
    x86_64-linux-android)
        export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$TOOLCHAIN/bin/${TARGET}${ANDROID_API}-clang"
        ;;
esac

# Verify NDK toolchain exists
if [ ! -d "$TOOLCHAIN" ]; then
    echo "❌ Android NDK not found at: $TOOLCHAIN"
    echo "Please install Android NDK $ANDROID_NDK_VERSION"
    exit 1
fi

# Verify Rust target is installed
if ! rustup target list --installed | grep -q "$TARGET"; then
    echo "Installing Rust target: $TARGET"
    rustup target add "$TARGET"
fi

echo "Building for $TARGET (API $ANDROID_API)..."
echo "Detected host: $HOST_ARCH"
echo "Using NDK: $NDK"
echo "Using toolchain: $TOOLCHAIN"

# Show the active compiler
CC_VAR="CC_${TARGET//-/_}"
echo "CC: ${!CC_VAR}"

# Debug build (faster for development)
# cargo build --target "$TARGET" --features=vendored-openssl

# Release build (optimized for production)
cargo build --release --target "$TARGET" --features=vendored-openssl

echo "✅ Done. Output at: target/${TARGET}/release/"
