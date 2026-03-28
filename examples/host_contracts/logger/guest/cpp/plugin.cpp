#include <polyplug_guest.hpp>
#include "generated/guest/contracts.hpp"
#include "generated/guest/host_contract_callers.hpp"
#include "generated/guest/types.hpp"

#include <string>
#include <algorithm>

extern "C" {

uint32_t polyplug_abi_version() {
    return POLYPLUG_ABI_VERSION;
}

void polyplug_user_init() {
    Contracts::set_worker_impl([](const StringView& input) -> StringView {
        std::string s = StringView_to_string(input);

        auto logger = HostLoggerCaller::from_host(get_host_vtable(), 1);

        if (logger && logger->is_valid()) {
            logger->log("Processing input: " + s);
            logger->log("Step 1: Analyzing input");
            logger->log("Step 2: Transforming data");
            logger->log("Step 3: Generating output");
        }

        std::transform(s.begin(), s.end(), s.begin(), ::toupper);
        return alloc_string("WORKED: " + s);
    });
}

}