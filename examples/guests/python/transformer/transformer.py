from polyplug_guest import StringView, to_str, alloc_string

def transform(input: StringView) -> StringView:
    s = to_str(input)
    if s.startswith("DECODED:"):
        s = s[8:]
    parts = s.split('|')
    if len(parts) >= 3:
        name = parts[0].upper()
        value = f"{parts[1]} (transformed)"
        count = int(parts[2]) + 1
        return alloc_string(f"TRANSFORMED:{name}|{value}|{count}")
    return alloc_string("INVALID:format")

POLYPLUG_FUNCTIONS = {
    'data.Transformer': {'transform': transform},
}
