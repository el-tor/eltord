#!/bin/bash

# Release script for eltord
# This script creates a git tag and pushes it to trigger the GitHub Actions build and release workflow

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Function to show usage
show_usage() {
    echo "Usage: $0 <version>"
    echo ""
    echo "Examples:"
    echo "  $0 1.0.0      # Create release v1.0.0"
    echo "  $0 1.2.3-beta # Create release v1.2.3-beta"
    echo ""
    echo "This script will:"
    echo "  1. Validate the version format"
    echo "  2. Check for uncommitted changes"
    echo "  3. Update Cargo.toml version"
    echo "  4. Create a git tag"
    echo "  5. Push the tag to trigger GitHub Actions build & release"
}

# Function to validate version format
validate_version() {
    local version=$1
    if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9]+)?$ ]]; then
        print_error "Invalid version format: $version"
        print_error "Expected format: X.Y.Z or X.Y.Z-suffix (e.g., 1.0.0 or 1.0.0-beta)"
        exit 1
    fi
}

# Function to check for uncommitted changes
check_git_status() {
    if [[ -n $(git status --porcelain) ]]; then
        print_error "There are uncommitted changes in your repository."
        print_error "Please commit or stash them before creating a release."
        git status --short
        exit 1
    fi
}

# Function to check if tag already exists
check_tag_exists() {
    local tag=$1
    if git rev-parse "$tag" >/dev/null 2>&1; then
        print_error "Tag $tag already exists!"
        print_error "Use a different version or delete the existing tag with:"
        print_error "  git tag -d $tag"
        print_error "  git push origin --delete $tag"
        exit 1
    fi
}

# Function to update Cargo.toml version
update_cargo_version() {
    local version=$1
    local cargo_file="Cargo.toml"
    
    if [[ ! -f "$cargo_file" ]]; then
        print_error "Cargo.toml not found in current directory"
        exit 1
    fi
    
    print_status "Updating version in $cargo_file to $version"
    
    # Update version in Cargo.toml
    if command -v sed >/dev/null 2>&1; then
        # Use sed to update the version
        if [[ "$OSTYPE" == "darwin"* ]]; then
            # macOS sed
            sed -i '' "s/^version = \".*\"/version = \"$version\"/" "$cargo_file"
        else
            # Linux sed
            sed -i "s/^version = \".*\"/version = \"$version\"/" "$cargo_file"
        fi
    else
        print_error "sed command not found. Please install sed or update Cargo.toml manually."
        exit 1
    fi
    
    # Verify the change
    if grep -q "version = \"$version\"" "$cargo_file"; then
        print_success "Updated Cargo.toml version to $version"
    else
        print_error "Failed to update Cargo.toml version"
        exit 1
    fi
}

# Function to create and push tag
create_and_push_tag() {
    local version=$1
    local tag="v$version"
    
    print_status "Creating git tag: $tag"
    
    # Add updated Cargo.toml to git
    git add Cargo.toml
    git commit -m "Bump version to $version"
    
    # Create annotated tag
    git tag -a "$tag" -m "Release $tag"
    
    print_status "Pushing tag to origin..."
    git push origin main  # Push the commit first
    git push origin "$tag"  # Then push the tag
    
    print_success "Tag $tag created and pushed successfully!"
    print_success "GitHub Actions should now be building and releasing $tag"
    print_status "Check the progress at: https://github.com/el-tor/eltord/actions"
}

# Main script
main() {
    # Check if version argument is provided
    if [[ $# -eq 0 ]]; then
        show_usage
        exit 1
    fi
    
    # Handle help flags
    if [[ "$1" == "-h" || "$1" == "--help" ]]; then
        show_usage
        exit 0
    fi
    
    local version=$1
    local tag="v$version"
    
    print_status "Starting release process for version $version"
    
    # Validate inputs
    validate_version "$version"
    
    # Check git status
    check_git_status
    
    # Check if tag already exists
    check_tag_exists "$tag"
    
    # Confirm with user
    echo ""
    print_warning "This will create and push tag: $tag"
    print_warning "This will trigger a GitHub Actions build and release."
    echo ""
    read -p "Do you want to continue? (y/N): " -n 1 -r
    echo ""
    
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        print_status "Release cancelled."
        exit 0
    fi
    
    # Update Cargo.toml version
    update_cargo_version "$version"
    
    # Create and push tag
    create_and_push_tag "$version"
    
    echo ""
    print_success "Release $tag initiated successfully!"
    print_status "The GitHub Actions workflow will now:"
    print_status "  1. Build binaries for Linux x86_64"
    print_status "  2. Create a GitHub release"
    print_status "  3. Upload the built binaries as release assets"
    echo ""
    print_status "You can monitor the progress at:"
    print_status "https://github.com/el-tor/eltord/actions"
}

# Run main function with all arguments
main "$@"
