#!/bin/bash

set -e

echo "📦 Releasing..."

# Check if we're in nix-shell, if not, enter it
if [ -z "$IN_NIX_SHELL" ]; then
    echo "🔧 Entering nix-shell..."
    nix-shell --run "$0 $*"
    exit $?
fi

echo "✅ Running in nix-shell"

# Parse command line arguments
NO_BUILD=false
NO_RELEASE=false
VERSION=""

while [ $# -gt 0 ]; do
    case $1 in
        --no-build)
            NO_BUILD=true
            shift
            ;;
        --no-release)
            NO_RELEASE=true
            shift
            ;;
        *)
            if [ -z "$VERSION" ]; then
                VERSION="$1"
            fi
            shift
            ;;
    esac
done

# Read version from VERSION file, fallback to Cargo.toml, then command line arg, then git tag, then default
if [ -z "$VERSION" ]; then
    if [ -f "VERSION" ]; then
        VERSION=$(cat VERSION | tr -d '\n\r')
    elif [ -f "Cargo.toml" ]; then
        VERSION=$(grep '^version = ' Cargo.toml | head -n1 | sed 's/version = "\(.*\)"/v\1/')
    else
        VERSION=$(git describe --tags --abbrev=0 2>/dev/null || echo "v0.0.X")
    fi
fi

RELEASE_DIR="release/eltord-${VERSION}"

echo "🚀 Creating release ${VERSION}"

# Clean and create release directory
rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR"

echo "📦 Collecting artifacts..."

if [ "$NO_BUILD" = false ]; then
    ########################################
    ### 1. Build macOS ARM64 locally
    ########################################
    echo "🍎 Building macOS ARM64 locally..."
    ./scripts/build-mac-arm.sh
    cp cache/eltord-build-artifacts/macOS-arm64/eltord "$RELEASE_DIR/eltord-macos-arm64"

    ########################################
    ### 2. Build Linux ARM64 locally via act
    ########################################
    echo "🐧 Building Linux ARM64 locally..."
    ./scripts/build-linux-arm.sh  
    cp cache/eltord-build-artifacts/linux-arm64/eltord "$RELEASE_DIR/eltord-linux-arm64"
else
    echo "⏭️  Skipping local builds (--no-build specified)"
    
    # Copy existing artifacts if they exist
    if [ -f "cache/eltord-build-artifacts/macOS-arm64/eltord" ]; then
        echo "🍎 Using existing macOS ARM64 artifact..."
        cp cache/eltord-build-artifacts/macOS-arm64/eltord "$RELEASE_DIR/eltord-macos-arm64"
    else
        echo "⚠️  No existing macOS ARM64 artifact found in cache"
    fi
    
    if [ -f "cache/eltord-build-artifacts/linux-arm64/eltord" ]; then
        echo "🐧 Using existing Linux ARM64 artifact..."
        cp cache/eltord-build-artifacts/linux-arm64/eltord "$RELEASE_DIR/eltord-linux-arm64"
    else
        echo "⚠️  No existing Linux ARM64 artifact found in cache"
    fi
fi

#########################################################
### 3. Download artifacts from latest GitHub Actions run
#########################################################
echo "☁️  Downloading GitHub Actions artifacts..."
gh run list --workflow=build.yml --limit=1 --json databaseId --jq '.[0].databaseId' > /tmp/run_id
RUN_ID=$(cat /tmp/run_id)

# Download each platform
gh run download $RUN_ID --name "eltord-Linux-x86_64" --dir "$RELEASE_DIR/temp-linux"
gh run download $RUN_ID --name "eltord-Windows-x86_64" --dir "$RELEASE_DIR/temp-windows"
gh run download $RUN_ID --name "eltord-macOS-x86_64" --dir "$RELEASE_DIR/temp-macos"

# Move to final locations
if [ -f "$RELEASE_DIR/temp-linux/Linux-x86_64/eltord" ]; then
    mv "$RELEASE_DIR/temp-linux/Linux-x86_64/eltord" "$RELEASE_DIR/eltord-linux-x86_64"
else
    echo "⚠️  Linux x86_64 artifact not found"
fi

if [ -f "$RELEASE_DIR/temp-windows/Windows-x86_64/eltord.exe" ]; then
    mv "$RELEASE_DIR/temp-windows/Windows-x86_64/eltord.exe" "$RELEASE_DIR/eltord-windows-x86_64"
else
    echo "⚠️  Windows x86_64 artifact not found"
fi

if [ -f "$RELEASE_DIR/temp-macos/macOS-x86_64/eltord" ]; then
    mv "$RELEASE_DIR/temp-macos/macOS-x86_64/eltord" "$RELEASE_DIR/eltord-macos-x86_64"
else
    echo "⚠️  macOS x86_64 artifact not found"
fi
# Cleanup temp directories
rm -rf "$RELEASE_DIR/temp-"*

########################################
### 4. Copy torrc files and README
########################################
# Copy torrc file to each platform folder
cp torrc "$RELEASE_DIR/"
cp "readme.md" "$RELEASE_DIR/"


########################################
# 5. Create zip bundles with torrc files
########################################
cd "$RELEASE_DIR"
for platform in macOS-arm64 macos-x86_64 linux-arm64 linux-x86_64 windows-x86_64; do
    if [ -f "eltord-$platform" ]; then
        if [[ "$platform" == *"windows"* ]]; then
            # For Windows platforms, rename to eltord.exe
            mkdir -p "temp-$platform"
            cp "eltord-$platform" "temp-$platform/eltord.exe"
            cp "torrc" "readme.md" "temp-$platform/"
            cd "temp-$platform"
            zip -r "../eltord-$platform.zip" .
            cd ..
            rm -rf "temp-$platform"
        else
            # For non-Windows platforms, rename to just eltord
            mkdir -p "temp-$platform"
            cp "eltord-$platform" "temp-$platform/eltord"
            cp "torrc" "readme.md" "temp-$platform/"
            cd "temp-$platform"
            zip -r "../eltord-$platform.zip" .
            cd ..
            rm -rf "temp-$platform"
        fi
    else
        echo "⚠️  eltord-$platform artifact not found"
    fi
done
cd ..

########################################
# 6. Create checksums
########################################
cd "$RELEASE_DIR"
shasum -a 256 eltord-*.zip > checksums.txt
cd ..

########################################
# 6. Ship GitHub release
########################################
if [ "$NO_RELEASE" = false ]; then
    echo "🏷️  Creating GitHub release..."
    
    # Read release notes from CHANGELOG.md or use default
    RELEASE_NOTES="eltord release $VERSION"
    if [ -f "CHANGELOG.md" ]; then
        # Extract section for this version from CHANGELOG.md
        RELEASE_NOTES=$(awk "/^## \[?$VERSION\]?|^## \[?${VERSION#v}\]?/{flag=1; next} /^## /{flag=0} flag" CHANGELOG.md)
        if [ -n "$RELEASE_NOTES" ]; then
            echo "📝 Using release notes from CHANGELOG.md"
        else
            echo "⚠️  No section found for $VERSION in CHANGELOG.md, using default notes"
            RELEASE_NOTES="eltord release $VERSION"
        fi
    else
        echo "📝 No CHANGELOG.md found, using default release notes"
    fi
    
    gh release create "$VERSION" \
      --title "eltord release $VERSION" \
      --notes "$RELEASE_NOTES" \
      --draft \
      "$RELEASE_DIR"/*.zip \
      "$RELEASE_DIR"/checksums.txt
    
    echo "✅ Release $VERSION created successfully!"
    echo "🌐 GitHub release: https://github.com/$(gh repo view --json owner,name --jq '.owner.login + "/" + .name')/releases"
else
    echo "⏭️  Skipping GitHub release creation (--no-release specified)"
fi

echo "📁 Artifacts in: $RELEASE_DIR"
echo "📦 Zip bundles created with torrc files included"
