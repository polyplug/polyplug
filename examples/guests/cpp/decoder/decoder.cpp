#include "generated/guest/init.hpp"
#include <algorithm>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <string>
#include <string_view>

namespace polyplug_plugin {

// The decode transformation itself, factored so the registered `decode` contract
// AND the raw-FFI baseline export (`polyplug_bench_decode`) run byte-identical
// work — the only thing that differs between them is the calling mechanism
// (polyplug dispatch vs a hand-rolled `dlsym` call), which is exactly what the
// cross-language matrix baseline isolates.
static std::string decode_body(std::string_view input) {
    std::string s(input);
    std::replace(s.begin(), s.end(), ',', '|');
    return "DECODED:" + s;
}

class DecoderImpl : public PipelineDecoderGuestContract {
public:
    explicit DecoderImpl(const HostApi* host) : host_(host) {}

    StringView decode(StringView input) override {
        return polyplug::alloc_string(host_, decode_body(polyplug::abi::to_string(input)));
    }

private:
    // Host handle for this runtime, captured at instance creation.
    const HostApi* host_;
};

// Factory called by the generated create_instance for every host-created
// instance. Ownership of the returned object transfers to the instance.
PipelineDecoderGuestContract* polyplug_create_decoder(const HostApi* host) {
    return new DecoderImpl(host);
}

}  // namespace polyplug_plugin

// Raw-FFI baseline exports: the SAME `decode_body` the registered `decode`
// contract runs, reached by `dlsym` instead of polyplug dispatch. Allocates the
// result with `malloc` (a plain plugin author's allocator) and returns it through
// out-params; the caller reads it and calls `polyplug_bench_decode_free`. This is
// the "what you'd hand-write WITHOUT polyplug" floor the cross-language matrix is
// measured against. Empty/invalid input writes null/0.
extern "C" void polyplug_bench_decode(const uint8_t* in_ptr, size_t in_len,
                                      uint8_t** out_ptr, size_t* out_len) {
    if (in_ptr == nullptr) {
        *out_ptr = nullptr;
        *out_len = 0;
        return;
    }
    const std::string result = polyplug_plugin::decode_body(
        std::string_view(reinterpret_cast<const char*>(in_ptr), in_len));
    uint8_t* buf = static_cast<uint8_t*>(std::malloc(result.size()));
    if (buf == nullptr) {
        *out_ptr = nullptr;
        *out_len = 0;
        return;
    }
    std::memcpy(buf, result.data(), result.size());
    *out_ptr = buf;
    *out_len = result.size();
}

// Free a buffer returned by `polyplug_bench_decode`.
extern "C" void polyplug_bench_decode_free(uint8_t* ptr, size_t /*len*/) {
    std::free(ptr);
}
