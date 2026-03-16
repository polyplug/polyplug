from polyplug_guest import StringView, to_str, alloc_string

def validate(input: StringView) -> StringView:
    s = to_str(input)
    if s.startswith("DECODED:"):
        s = s[8:]
    parts = s.split('|')
    if len(parts) >= 3 and parts[0] and parts[1]:
        try:
            int(parts[2])
            return alloc_string(f"VALID:{s}")
        except ValueError:
            pass
    return alloc_string("INVALID:expected format is name|value|count")

POLYPLUG_FUNCTIONS = {
    'pipeline.Validator': {'validate': validate},
}
