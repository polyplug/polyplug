// examples/hosts/cpp/host.cpp
// C++ host example using polyplugc-generated bindings.
//
// This host demonstrates the real-world polyplug pattern:
//   1. Generate host bindings: polyplugc --api api.toml --lang cpp --out generated/
//   2. Include generated headers: #include "generated/host/host_callers.hpp"
//   3. Use type-safe contract wrappers instead of manual vtable dispatch
//
// Zero hand-written contract IDs, zero manual unsafe dispatch.

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

#include "../../../host-libs/cpp/polyplug/abi.hpp"
#include "generated/host/host_callers.hpp"

static std::string last_error_string() {
    size_t const len = polyplug_runtime_error_message_len();
    if (len == 0U) {
        return std::string("(no details)");
    }
    std::vector<uint8_t> buf(len);
    size_t const written = polyplug_runtime_last_error(buf.data(), buf.size());
    return std::string(reinterpret_cast<const char*>(buf.data()), written);
}

static std::string string_view_to_str(const StringView& sv) {
    if (sv.ptr == nullptr || sv.len == 0) {
        return std::string();
    }
    return std::string(reinterpret_cast<const char*>(sv.ptr), sv.len);
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
        
        // Get host vtable for contract calls
        const HostVTable* host = polyplug_runtime_get_host_vtable(rt);
        
        // Find and call decoder plugin using generated caller
        uint64_t const decoder_handle = polyplug_runtime_find_by_contract(
            rt, polyplug_generated::PIPELINE_DECODER_CONTRACT_ID, 0U
        );
        if (decoder_handle != UINT64_MAX) {
            std::cout << "[cpp_decoder] found decoder plugin" << std::endl;
            
            // Use generated caller for type-safe invocation
            polyplug_generated::PipelineDecoderContract decoder(decoder_handle, host);
            StringView input_sv = StringView::from_string("name,value,42");
            StringView result = decoder.decode(input_sv);
            std::cout << "  decode result: " << string_view_to_str(result) << std::endl;
        }
        
        // Find and call transformer plugin
        uint64_t const transformer_handle = polyplug_runtime_find_by_contract(
            rt, polyplug_generated::DATA_TRANSFORMER_CONTRACT_ID, 0U
        );
        if (transformer_handle != UINT64_MAX) {
            std::cout << "[cpp_transformer] found transformer plugin" << std::endl;
            
            polyplug_generated::DataTransformerContract transformer(transformer_handle, host);
            StringView data_sv = StringView::from_string("test,data,123");
            StringView result = transformer.transform(data_sv);
            std::cout << "  transform result: " << string_view_to_str(result) << std::endl;
        }
        
        // Find and call encoder plugin
        uint64_t const encoder_handle = polyplug_runtime_find_by_contract(
            rt, polyplug_generated::PIPELINE_ENCODER_CONTRACT_ID, 0U
        );
        if (encoder_handle != UINT64_MAX) {
            std::cout << "[cpp_encoder] found encoder plugin" << std::endl;
            
            polyplug_generated::PipelineEncoderContract encoder(encoder_handle, host);
            StringView data_sv = StringView::from_string("name|value|42");
            StringView result = encoder.encode(data_sv);
            std::cout << "  encode result: " << string_view_to_str(result) << std::endl;
        }
        
        std::cout << "\n=== done ===" << std::endl;
        
    } catch (const std::exception& e) {
        std::cerr << "error: " << e.what() << std::endl;
        polyplug_runtime_destroy(rt);
        return 1;
    }
    
    polyplug_runtime_destroy(rt);
    return 0;
}
