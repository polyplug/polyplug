// examples/hosts/cpp/host.cpp
// C++ host example using polyplugc-generated bindings.
//
// This host demonstrates the real-world polyplug pattern:
//   1. Generate host bindings: polyplugc --api api.toml --lang cpp --out generated/
//   2. Include generated headers: #include "generated/host/types.hpp"
//   3. Use generated contract IDs instead of hard-coded values
//
// Zero hand-written contract IDs.

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

#include "../../../host-libs/cpp/polyplug/abi.hpp"
#include "generated/host/types.hpp"

static std::string last_error_string() {
    size_t const len = polyplug_runtime_error_message_len();
    if (len == 0U) {
        return std::string("(no details)");
    }
    std::vector<uint8_t> buf(len);
    size_t const written = polyplug_runtime_last_error(buf.data(), buf.size());
    return std::string(reinterpret_cast<const char*>(buf.data()), written);
}

int main(int argc, char* argv[]) {
    std::string plugin_path = (argc > 1) ? std::string(argv[1]) : std::string("examples/plugins");
    
    std::cerr << "plugin directory: " << plugin_path << std::endl;
    
    // Create runtime
    OpaqueRuntime* const rt = polyplug_runtime_create();
    if (rt == nullptr) {
        std::cerr << "polyplug_runtime_create failed: " << last_error_string() << std::endl;
        return 1;
    }
    
    try {
        // Load all bundles
        uint32_t const load_rc = polyplug_runtime_load_bundle(
            rt,
            reinterpret_cast<const uint8_t*>(plugin_path.data()),
            plugin_path.size()
        );
        if (load_rc != 0U) {
            throw std::runtime_error("polyplug_runtime_load_bundle failed: " + last_error_string());
        }
        std::cerr << "Bundle loaded." << std::endl;
        
        std::cout << "\n=== polyplug cpp host example ===" << std::endl;
        
        // Try to find plugins by generated contract IDs
        // Note: This demonstrates using generated constants instead of hard-coded values
        
        uint64_t const decoder_handle = polyplug_runtime_find_by_contract(
            rt, polyplug_generated::PIPELINE_DECODER_CONTRACT_ID, 0U
        );
        if (decoder_handle != UINT64_MAX) {
            std::cout << "[cpp_decoder]                  found decoder plugin" << std::endl;
        }
        
        uint64_t const transformer_handle = polyplug_runtime_find_by_contract(
            rt, polyplug_generated::DATA_TRANSFORMER_CONTRACT_ID, 0U
        );
        if (transformer_handle != UINT64_MAX) {
            std::cout << "[cpp_transformer]              found transformer plugin" << std::endl;
        }
        
        uint64_t const encoder_handle = polyplug_runtime_find_by_contract(
            rt, polyplug_generated::PIPELINE_ENCODER_CONTRACT_ID, 0U
        );
        if (encoder_handle != UINT64_MAX) {
            std::cout << "[cpp_encoder]                  found encoder plugin" << std::endl;
        }
        
        uint64_t const reporter_handle = polyplug_runtime_find_by_contract(
            rt, polyplug_generated::DATA_REPORTER_CONTRACT_ID, 0U
        );
        if (reporter_handle != UINT64_MAX) {
            std::cout << "[cpp_reporter]                 found reporter plugin" << std::endl;
        }
        
        uint64_t const validator_handle = polyplug_runtime_find_by_contract(
            rt, polyplug_generated::PIPELINE_VALIDATOR_CONTRACT_ID, 0U
        );
        if (validator_handle != UINT64_MAX) {
            std::cout << "[cpp_validator]                found validator plugin" << std::endl;
        }
        
        std::cout << "\ncpp pipeline complete" << std::endl;
        
    } catch (const std::exception& ex) {
        std::cerr << "error: " << ex.what() << std::endl;
        polyplug_runtime_destroy(rt);
        return 1;
    }
    
    polyplug_runtime_destroy(rt);
    return 0;
}
