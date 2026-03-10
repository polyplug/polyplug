#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "Building csv_decoder (Rust)..."
cargo build -p csv_decoder --release
cp target/release/libcsv_decoder.so showcase/fixtures/libcsv_decoder.so

echo "Building uppercase_transformer (C++)..."
make -C showcase/plugins/uppercase_transformer/
cp showcase/plugins/uppercase_transformer/libuppercase_transformer.so showcase/fixtures/libuppercase_transformer.so

echo "Building csv_encoder (C#)..."
dotnet build -c Release showcase/plugins/csv_encoder/
cp showcase/plugins/csv_encoder/bin/Release/net10.0/CsvEncoder.dll showcase/fixtures/csv_encoder/CsvEncoder.dll
cp showcase/plugins/csv_encoder/bin/Release/net10.0/Polyplug.Guest.dll showcase/fixtures/csv_encoder/Polyplug.Guest.dll
cp showcase/plugins/csv_encoder/bin/Release/net10.0/CsvEncoder.deps.json showcase/fixtures/csv_encoder/CsvEncoder.deps.json
cp showcase/plugins/csv_encoder/bin/Release/net10.0/CsvEncoder.runtimeconfig.json showcase/fixtures/csv_encoder/CsvEncoder.runtimeconfig.json

echo "Copying summary_reporter (Python)..."
cp showcase/plugins/summary_reporter/summary_reporter.py showcase/fixtures/summary_reporter/summary_reporter.py

echo "Copying reverse_transformer (Lua)..."
cp showcase/plugins/reverse_transformer/reverse_transformer.lua showcase/fixtures/reverse_transformer/reverse_transformer.lua

echo "Copying field_validator (JS)..."
cp showcase/plugins/field_validator/bundle.js showcase/fixtures/field_validator/bundle.js

echo "All plugins built and copied to showcase/fixtures/."
