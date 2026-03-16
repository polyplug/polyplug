from polyplug_guest import StringView, to_str, alloc_string

def encode(input: StringView) -> StringView:
    s = to_str(input)
    if s.startswith("TRANSFORMED:"):
        s = s[12:]
    return alloc_string(s.replace('|', ','))

POLYPLUG_FUNCTIONS = {
    'pipeline.Encoder': {'encode': encode},
}
