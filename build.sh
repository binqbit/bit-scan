#!/bin/bash
# Build the project in release mode
cargo build --release

# Copy the executable to the bin directory
cp ./target/release/bit-scan ./bin/

# Add the bin directory to the PATH environment variable
export PATH="$PATH:$(pwd)/bin"

echo Build and setup completed.
