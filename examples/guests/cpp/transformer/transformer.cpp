#include "generated/guest/init.hpp"
#include <polyplug/helpers.hpp>
#include <string>
#include <sstream>

namespace polyplug_plugin {

class TransformerImpl : public DataTransformerPlugin {
public:
    StringView transform(StringView input) override {
        std::string_view sv = polyplug::abi::strip_prefix(input, "DECODED:");
        std::string s(sv);
        std::istringstream iss(s);
        std::string name, value, count_str;
        if (std::getline(iss, name, '|') && std::getline(iss, value, '|') && std::getline(iss, count_str, '|')) {
            int count = std::stoi(count_str);
            for (auto& c : name) c = ::toupper(c);
            std::ostringstream oss;
            oss << "TRANSFORMED:" << name << "|" << value << " (transformed)|" << (count + 1);
            return polyplug::guest::alloc_string(oss.str());
        }
        return polyplug::guest::alloc_string("ERROR:invalid input format");
    }
};

DataTransformerPlugin* create_transformer_impl() {
    return new TransformerImpl();
}

}  // namespace polyplug_plugin