from polyplug_guest import StringView, to_str, alloc_string

def decode(input: StringView) -> StringView:
    s = to_str(input).replace(',', '|')
    return alloc_string(f"DECODED:{s}")

POLYPLUG_FUNCTIONS = {
    'pipeline.Decoder': {'decode': decode},
}
