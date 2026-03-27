# SDK Validator

Cross-language SDK consistency validator for polyplug. Ensures all language SDKs implement the same set of methods with consistent naming conventions.

## Overview

This tool validates that SDK implementations across different languages (Rust, Python, C#, C++, TypeScript, JavaScript, Lua) are consistent with a golden method set defined in a YAML configuration file.

The validator uses [ast-grep](https://ast-grep.github.io/) for AST-based code analysis (except Lua, which uses tree-sitter).

## Installation

```bash
cargo install --path crates/sdk-validator
```

Or run directly from the workspace:

```bash
cargo run --bin sdk-validator -- --config sdk-validator.yaml
```

## Quick Start

1. Create a configuration file (`sdk-validator.yaml`):

```yaml
version: 1

methods:
  StringView:
    - to_str
    - starts_with
    - ends_with
    - strip_prefix
    - split

naming:
  rust: snake_case
  python: snake_case
  csharp: PascalCase
  js: camelCase
  cpp: snake_case
  lua: snake_case

targets:
  rust:
    - crates/polyplug_guest/src/lib.rs
  python:
    - sdks/python/polyplug_abi/polyplug_abi/helpers.py
  csharp:
    - sdks/csharp/abi/StringViewHelper.cs
  cpp:
    - sdks/cpp/abi/polyplug/helpers.hpp
  js:
    - sdks/js/abi/helpers.js
  lua:
    - sdks/lua/abi/helpers.lua
```

2. Run the validator:

```bash
sdk-validator --config sdk-validator.yaml
```

## CLI Usage

### Basic Validation

```bash
# Validate all structs against the golden method set
sdk-validator --config sdk-validator.yaml
```

### JSON Output (for CI/CD)

```bash
# Output as JSON for programmatic consumption
sdk-validator --config sdk-validator.yaml --json
```

### Filter to Specific Struct

```bash
# Validate only StringView methods
sdk-validator --config sdk-validator.yaml --struct StringView

# Validate only Buffer methods
sdk-validator --config sdk-validator.yaml --struct Buffer
```

### Fail on Missing Methods (CI Mode)

```bash
# Exit with code 1 if any methods are missing
sdk-validator --config sdk-validator.yaml --fail-on-missing
```

### All Options

```bash
sdk-validator --help

Options:
  -c, --config <FILE>           Path to YAML configuration file [required]
  -j, --json                    Output as JSON instead of human-readable table
  -s, --struct <NAME>           Filter validation to a specific struct
  -f, --fail-on-missing         Exit with code 1 if any methods are missing
  -h, --help                    Print help
  -V, --version                 Print version
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All methods implemented (or no `--fail-on-missing` flag) |
| 1 | Some methods are missing (only with `--fail-on-missing`) |
| 2 | Configuration or runtime error (e.g., missing config file, ast-grep not installed) |

## Configuration File Format

The configuration file is YAML with the following structure:

### `version` (required)

Configuration format version. Currently only `1` is supported.

```yaml
version: 1
```

### `methods` (required)

The golden method set. Maps struct names to lists of method names. Method names should use **snake_case** as the canonical form.

```yaml
methods:
  StringView:
    - to_str
    - starts_with
    - ends_with
    - strip_prefix
    - split
  
  Buffer:
    - as_slice
    - as_mut_slice
    - len
```

### `naming` (required)

Naming conventions for each language. Used to transform snake_case method names to the target language's convention.

Supported conventions:
- `snake_case` (Rust, Python, C++, Lua)
- `PascalCase` (C#)
- `camelCase` (JavaScript, TypeScript)

```yaml
naming:
  rust: snake_case
  python: snake_case
  csharp: PascalCase
  js: camelCase
  cpp: snake_case
  lua: snake_case
```

### `targets` (required)

File paths to validate for each language. Each language can have multiple target files.

```yaml
targets:
  rust:
    - crates/polyplug_guest/src/lib.rs
  python:
    - sdks/python/polyplug_abi/polyplug_abi/helpers.py
  csharp:
    - sdks/csharp/abi/StringViewHelper.cs
  cpp:
    - sdks/cpp/abi/polyplug/helpers.hpp
  js:
    - sdks/js/abi/helpers.ts
    - sdks/js/abi/helpers.js
  lua:
    - sdks/lua/abi/helpers.lua
```

## Example Output

### Human-Readable Table

```
StringView Validation Report
============================
Overall Completion: 78%

Method          | rust | python | csharp | cpp | js  | lua  | Status
----------------|------|--------|--------|-----|-----|------|--------
to_str          |  ✓   |   ✓    |   ✓    |  ✓  |  ✓  |  ✓   | Complete
starts_with     |  ✓   |   ✓    |   ✗    |  ✓  |  ✓  |  ✗   | Partial
ends_with       |  ✗   |   ✗    |   ✗    |  ✗  |  ✗  |  ✗   | Missing
strip_prefix    |  ✓   |   ✓    |   ✗    |  ✓  |  ✓  |  ✓   | Partial
split           |  ✗   |   ✗    |   ✗    |  ✗  |  ✗  |  ✗   | Missing

Missing in rust: ends_with, split
Missing in python: ends_with, split
Missing in csharp: starts_with, ends_with, strip_prefix, split
```

### JSON Output

```json
{
  "is_complete": false,
  "completion_percentage": 78,
  "per_struct": {
    "StringView": {
      "methods": {
        "to_str": {
          "found_in": ["rust", "python", "csharp", "cpp", "js", "lua"],
          "missing_in": []
        },
        "ends_with": {
          "found_in": [],
          "missing_in": ["rust", "python", "csharp", "cpp", "js", "lua"]
        }
      }
    }
  },
  "per_language": {
    "rust": {
      "structs": {
        "StringView": {
          "found": 3,
          "total": 5
        }
      }
    }
  }
}
```

## Adding New Methods

To add a new method to the validation set:

1. **Update the configuration file** - Add the method name to the appropriate struct in `methods`:

```yaml
methods:
  StringView:
    - to_str
    - starts_with
    - ends_with
    - strip_prefix
    - split
    - contains  # New method added here
```

2. **Implement the method in each language SDK** - Add the implementation to each target file listed in `targets`.

3. **Run the validator** - Check which languages are missing the new method:

```bash
sdk-validator --config sdk-validator.yaml --struct StringView
```

4. **Fix missing implementations** - Implement the method in any languages that don't have it yet.

## Adding New Structs

To add validation for a new struct:

1. **Add the struct to the configuration**:

```yaml
methods:
  StringView:
    - to_str
    - starts_with
  
  Buffer:  # New struct
    - as_slice
    - as_mut_slice
    - len
    - is_empty
```

2. **Add target files for each language** (if not already present):

```yaml
targets:
  rust:
    - crates/polyplug_guest/src/lib.rs
  # ... other languages
```

3. **Implement the methods in each language SDK**.

4. **Run validation**:

```bash
sdk-validator --config sdk-validator.yaml --struct Buffer
```

## Adding New Languages

To add validation for a new language:

1. **Add the language to the `naming` section**:

```yaml
naming:
  rust: snake_case
  go: PascalCase  # New language
```

2. **Add target files in the `targets` section**:

```yaml
targets:
  go:
    - sdks/go/abi/helpers.go
```

3. **Implement a validator** for the new language in `src/languages/`. See existing validators (e.g., `rust.rs`, `python.rs`) for examples.

4. **Register the validator** in `src/languages/mod.rs`.

5. **Run validation** to check the new language:

```bash
sdk-validator --config sdk-validator.yaml
```

## Prerequisites

### ast-grep

Most language validators require [ast-grep](https://ast-grep.github.io/) to be installed:

```bash
# macOS
brew install ast-grep

# Linux
curl -sS https://ast-grep.github.io/install.sh | bash

# Windows (winget)
winget install ast-grep

# Cargo
cargo install ast-grep
```

Verify installation:

```bash
sg --version
```

### tree-sitter (for Lua only)

The Lua validator uses tree-sitter instead of ast-grep. The required dependencies are included in the crate.

## CI/CD Integration

### GitHub Actions

```yaml
- name: Validate SDK consistency
  run: |
    cargo run --bin sdk-validator -- \
      --config sdk-validator.yaml \
      --fail-on-missing \
      --json > sdk-validation-report.json
    
- name: Upload validation report
  uses: actions/upload-artifact@v4
  with:
    name: sdk-validation-report
    path: sdk-validation-report.json
```

### GitLab CI

```yaml
validate-sdk:
  stage: test
  script:
    - cargo run --bin sdk-validator -- --config sdk-validator.yaml --fail-on-missing
  artifacts:
    reports:
      sdk-validation: sdk-validation-report.json
```

## Troubleshooting

### "ast-grep not found" error

Install ast-grep (see [Prerequisites](#prerequisites)) or ensure it's in your PATH.

### "Config file not found" error

Provide the absolute path to the config file:

```bash
sdk-validator --config /absolute/path/to/sdk-validator.yaml
```

### "Unsupported config version" error

Ensure your config file has `version: 1` at the top.

### Methods not detected

Check that:
1. The method name in the config matches the actual implementation (after applying naming convention transformation)
2. The target file path is correct
3. ast-grep can parse the file (try running `sg --pattern 'function_name' path/to/file`)

## License

MIT License — see the main [polyplug LICENSE](../../LICENSE) for details.
