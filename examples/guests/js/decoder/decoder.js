function decode(args, out) {
    return 0;
}

function polyplug_init(rt_ctx, host_vtable, ctx) {
    var vtable = {
        contractLo: 0xB0C3DC1E,
        contractHi: 0x12F3C106,
        fnCount: 1,
        contractName: "pipeline.Decoder@1",
        functions: [decode]
    };
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName
    );
    return { code: 0, message: null };
}