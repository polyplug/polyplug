function report(args, out) {
    return 0;
}

function polyplug_init(rt_ctx, host_vtable, ctx) {
    var vtable = {
        contractLo: 0x7D6E5F4A,
        contractHi: 0x12F3C106,
        fnCount: 1,
        contractName: "pipeline.Reporter@1",
        functions: [report]
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