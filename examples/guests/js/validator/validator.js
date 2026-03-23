function validate(args, out) {
    return 0;
}

function polyplug_init(rt_ctx, host_vtable, ctx) {
    var vtable = {
        contractLo: 0x4C3B2A18,
        contractHi: 0x12F3C106,
        fnCount: 1,
        contractName: "pipeline.Validator@1",
        functions: [validate]
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