// Simple C++ host example for polyplug.
// Build: make host    Run: ./host [bundle_dir]

#include <cstdint>
#include <cstdio>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>

#include "../../../host-libs/cpp/polyplug/abi.hpp"

// DataRecord: 40 bytes, align 8 — mirrors abi_types.md canonical layout.
struct DataRecord {
    StringView name;
    StringView value;
    uint32_t   count;
    uint32_t   _pad;
};

static_assert(sizeof(DataRecord) == 40U, "DataRecord ABI size mismatch");
static_assert(sizeof(StringView) == 16U, "StringView ABI size mismatch");

static constexpr uint64_t DECODER_CONTRACT_ID =
    polyplug::fnv1a_contract_id("pipeline.decoder", 1U);

static std::string last_error_string() {
    size_t const len = polyplug_runtime_error_message_len();
    if (len == 0U) {
        return std::string("(no details)");
    }
    std::vector<uint8_t> buf(len);
    size_t const written = polyplug_runtime_last_error(buf.data(), buf.size());
    return std::string(reinterpret_cast<const char*>(buf.data()), written);
}

static void free_abi_error(const AbiError& err) {
    if (err.code == ABI_OK || err.message.ptr == nullptr || err.message.len == 0U) {
        return;
    }
    // SAFETY: ABI contract guarantees message.ptr is allocated via polyplug_host_alloc
    // when code != ABI_OK. We are the sole call-site owner and free it exactly once.
    polyplug_host_free(const_cast<uint8_t*>(err.message.ptr), err.message.len, 1U);
}

static void require_ok(const AbiError& err, const char* const stage) {
    if (err.code == ABI_OK) {
        return;
    }
    std::string msg;
    if (err.message.ptr != nullptr && err.message.len > 0U) {
        msg.assign(reinterpret_cast<const char*>(err.message.ptr), err.message.len);
    } else {
        msg = "(no message)";
    }
    free_abi_error(err);
    throw std::runtime_error(
        std::string(stage) + " failed [code=" + std::to_string(err.code) + "]: " + msg
    );
}

int main(int argc, char* argv[]) {
    std::string const bundle_dir = (argc > 1)
        ? std::string(argv[1])
        : std::string("examples/guests/rust/decoder");

    std::cout << "=== polyplug C++ host (simple example) ===" << std::endl;
    std::cout << "Bundle dir: " << bundle_dir << std::endl;

    OpaqueRuntime* const rt = polyplug_runtime_create();
    if (rt == nullptr) {
        std::cerr << "polyplug_runtime_create failed: " << last_error_string() << std::endl;
        return 1;
    }

    try {
        uint32_t const load_rc = polyplug_runtime_load_bundle(
            rt,
            reinterpret_cast<const uint8_t*>(bundle_dir.data()),
            bundle_dir.size()
        );
        if (load_rc != 0U) {
            throw std::runtime_error("polyplug_runtime_load_bundle failed: " + last_error_string());
        }
        std::cout << "Bundle loaded." << std::endl;

        uint64_t const packed_handle =
            polyplug_runtime_find_by_contract(rt, DECODER_CONTRACT_ID, 0U);
        if (packed_handle == std::numeric_limits<uint64_t>::max()) {
            char id_buf[19];
            std::snprintf(id_buf, sizeof(id_buf), "0x%016llX",
                static_cast<unsigned long long>(DECODER_CONTRACT_ID));
            throw std::runtime_error(
                std::string("no plugin for pipeline.decoder@1 (id=") + id_buf + ")"
            );
        }
        std::cout << "Plugin found." << std::endl;

        OpaqueGuard* const guard = polyplug_runtime_resolve_plugin(rt, packed_handle);
        if (guard == nullptr) {
            throw std::runtime_error(
                "polyplug_runtime_resolve_plugin failed: " + last_error_string()
            );
        }

        const void* const vt_raw = polyplug_runtime_plugin_vtable(guard);
        if (vt_raw == nullptr) {
            polyplug_runtime_plugin_release(guard);
            throw std::runtime_error("polyplug_runtime_plugin_vtable returned null");
        }

        const PluginVTable* const vtable = static_cast<const PluginVTable*>(vt_raw);
        std::cout << "Vtable: contract_id=0x" << std::hex << vtable->contract_id
                  << std::dec << " functions=" << vtable->function_count << std::endl;

        if (vtable->functions == nullptr || vtable->function_count == 0U) {
            polyplug_runtime_plugin_release(guard);
            throw std::runtime_error("vtable has no functions");
        }

        std::string const csv_input = "Alice,hello,3\n";
        Buffer input_buf{};
        input_buf.ptr = const_cast<void*>(static_cast<const void*>(csv_input.data()));
        input_buf.len = csv_input.size();
        input_buf.cap = csv_input.size();

        DataRecord out{};
        out.name  = StringView{nullptr, 0U};
        out.value = StringView{nullptr, 0U};
        out.count = 0U;
        out._pad  = 0U;

        using DecodeFn = AbiError (*)(const void*, void*);
        // SAFETY: functions[0] conforms to the ABI signature (const void* args, void* out)
        // per the pipeline.decoder contract. The vtable was resolved from a successfully
        // loaded bundle and verified non-null in the checks above.
        auto const decode_fn = reinterpret_cast<DecodeFn>(vtable->functions[0U]);

        AbiError const call_err = decode_fn(&input_buf, &out);
        require_ok(call_err, "decode");

        std::string const name_str(reinterpret_cast<const char*>(out.name.ptr), out.name.len);
        std::string const value_str(reinterpret_cast<const char*>(out.value.ptr), out.value.len);

        std::cout << "Result:" << std::endl;
        std::cout << "  name  = " << name_str  << std::endl;
        std::cout << "  value = " << value_str << std::endl;
        std::cout << "  count = " << out.count  << std::endl;

        polyplug_runtime_plugin_release(guard);
        polyplug_runtime_destroy(rt);
        std::cout << "Done." << std::endl;
        return 0;

    } catch (const std::exception& ex) {
        std::cerr << "Error: " << ex.what() << std::endl;
        polyplug_runtime_destroy(rt);
        return 1;
    }
}
