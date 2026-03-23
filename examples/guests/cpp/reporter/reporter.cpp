#include "generated/guest/init.hpp"
#include <polyplug/helpers.hpp>
#include <string>
#include <sstream>

namespace polyplug_plugin {

class ReporterImpl : public DataReporterPlugin {
public:
    StringView report(StringView input) override {
        std::string_view sv = polyplug::abi::strip_prefix(input, "TRANSFORMED:");
        std::string s(sv);
        std::istringstream iss(s);
        std::string name, value, count_str;
        if (std::getline(iss, name, '|') && std::getline(iss, value, '|') && std::getline(iss, count_str, '|')) {
            std::ostringstream oss;
            oss << "Report: " << name << " has value '" << value << "' with count " << count_str;
            return polyplug::guest::alloc_string(oss.str());
        }
        return polyplug::guest::alloc_string("ERROR:invalid input format");
    }
};

DataReporterPlugin* create_reporter_impl() {
    return new ReporterImpl();
}

}  // namespace polyplug_plugin