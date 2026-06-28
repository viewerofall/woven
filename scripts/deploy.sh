#!/bin/bash
# Deploy woven to Cloudflare Pages
# Usage: ./scripts/deploy.sh [version]

set -e

VERSION="${1:-2.6}"
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="$PROJECT_ROOT/target/release"
DIST_DIR="$PROJECT_ROOT/dist"
TARBALL_NAME="v${VERSION}.tar.gz"

cd "$PROJECT_ROOT"

echo "==> Building woven v${VERSION}..."
cargo build --release -p woven-sys -p woven-ctrl 2>&1 | grep -E "(Compiling woven|Finished)" || true

echo "==> Packaging release..."
mkdir -p "$DIST_DIR"
rm -f "$DIST_DIR/$TARBALL_NAME"

# Create release structure
RELEASE_DIR="$(mktemp -d)"
trap "rm -rf $RELEASE_DIR" EXIT

mkdir -p "$RELEASE_DIR/exec" "$RELEASE_DIR/runtime" "$RELEASE_DIR/plugins"

# Copy binaries
cp "$BUILD_DIR/woven" "$BUILD_DIR/woven-ctrl" "$RELEASE_DIR/exec/"

# Copy runtime and plugins
cp -r "$PROJECT_ROOT/runtime" "$RELEASE_DIR/"
cp "$PROJECT_ROOT"/*.lua "$RELEASE_DIR/" 2>/dev/null || true
cp "$PROJECT_ROOT/plugins"/*.lua "$RELEASE_DIR/plugins/" 2>/dev/null || true

# Copy service and desktop files
cp "$PROJECT_ROOT"/*.service "$RELEASE_DIR/" 2>/dev/null || true
cp "$PROJECT_ROOT"/*.desktop "$RELEASE_DIR/" 2>/dev/null || true
cp "$PROJECT_ROOT"/*_icon.png "$RELEASE_DIR/" 2>/dev/null || true

# Create tarball
tar -czf "$DIST_DIR/$TARBALL_NAME" -C "$(dirname "$RELEASE_DIR")" "$(basename "$RELEASE_DIR")"

echo "✓ Built: $DIST_DIR/$TARBALL_NAME"
ls -lh "$DIST_DIR/$TARBALL_NAME"

echo ""
echo "==> To deploy to Cloudflare Pages:"
echo "    1. Push dist/ to your Pages repo, or"
echo "    2. Upload via wrangler:"
echo ""
echo "    wrangler pages deploy dist/"
echo ""
