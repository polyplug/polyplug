from polyplug_guest import StringView, to_str, alloc_string

def report(input: StringView) -> StringView:
    s = to_str(input)
    if s.startswith("TRANSFORMED:"):
        s = s[12:]
    parts = s.split('|')
    if len(parts) >= 3:
        return alloc_string(f"Report: {parts[0]} has value '{parts[1]}' with count {parts[2]}")
    return alloc_string("INVALID:format")

POLYPLUG_FUNCTIONS = {
    'data.Reporter': {'report': report},
}
