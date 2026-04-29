# This workflow was adapted from
# https://github.com/tjeerdintveen/rust-mobile-multiplatform-example/blob/main/build_ios.sh

#!/bin/bash
set -e

BINDGEN_TARGET="libguyos_core.dylib"
IOS_PROJECT_NAME="GuyOs"
OUTDIR="bindings"
RUST_LIB_NAME="guyos_core"
RUST_PROJECT_DIR="$RUST_LIB_NAME"
SWIFT_PACKAGE_DIR="$SWIFT_PKG_NAME"
SWIFT_PKG_NAME="GuyOSClient"

echo "Building swift package for iOS..."

pushd $RUST_PROJECT_DIR

# Build all targets in parallel for efficiency
echo "Building for all targets..."

# Optional cargo clean for a clean build, slower but recommended
# cargo clean

# Build for host (Mac) in debug mode. Faster, only needed for UniFFI API inspection
cargo build 

# Build for iOS targets in release mode. These go into the actual app
cargo build --release --target=aarch64-apple-ios
cargo build --release --target=aarch64-apple-ios-sim

echo "Generating Swift bindings..."

# Generate Swift bindings
cargo run --bin uniffi-bindgen generate \
      --library ./target/debug/$BINDGEN_TARGET \
      --language swift \
      --out-dir $OUTDIR

echo "Preparing XCFramework..."

# Rename modulemap for Xcode
mv bindings/${RUST_LIB_NAME}FFI.modulemap bindings/module.modulemap

# Remove old framework
rm -rf ios/${SWIFT_PKG_NAME}.xcframework

# Create XCFramework
xcodebuild -create-xcframework \
           -library ./target/aarch64-apple-ios-sim/release/lib${RUST_LIB_NAME}.a -headers ./bindings \
           -library ./target/aarch64-apple-ios/release/lib${RUST_LIB_NAME}.a -headers ./bindings \
           -output "ios/${SWIFT_PKG_NAME}.xcframework"

echo "Copying files to Swift package..."

# Copy files to Swift package
mkdir -p ../${SWIFT_PKG_NAME}/Sources/${SWIFT_PKG_NAME}
cp -r ios/${SWIFT_PKG_NAME}.xcframework ../${SWIFT_PKG_NAME}/
cp -r $OUTDIR/${RUST_LIB_NAME}.swift ../${SWIFT_PKG_NAME}/Sources/${SWIFT_PKG_NAME}/

popd

echo "Cleaning swift package:"
pushd ${SWIFT_PKG_NAME}
swift package clean

popd

echo "iOS files generated successfully!"
echo "XCFramework: ${SWIFT_PKG_NAME}/ios/${SWIFT_PKG_NAME}.xcframework" 
echo "Swift bindings: ${SWIFT_PKG_NAME}/bindings/${RUST_LIB_NAME}.swift"
echo "Files copied to ${SWIFT_PKG_NAME}/package"

