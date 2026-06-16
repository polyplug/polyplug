var polyplug_module = (function(exports) {
	Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
	//#region tests/fixtures/test_plugin_js_generated/generated/guest/contracts.ts
	/** Dispatch mechanism type — determines how function calls are routed. */
	const DispatchType = Object.freeze({
		Native: 0,
		VirtualMachine: 1
	});
	function _ppEncodeUtf8(str) {
		if (typeof TextEncoder !== "undefined") return new TextEncoder().encode(str);
		const out = [];
		for (let i = 0; i < str.length; i++) {
			let code = str.charCodeAt(i);
			if (code >= 55296 && code <= 56319) {
				const low = str.charCodeAt(++i);
				code = 65536 + (code - 55296 << 10) + (low - 56320);
			}
			if (code < 128) out.push(code);
			else if (code < 2048) out.push(192 | code >> 6, 128 | code & 63);
			else if (code < 65536) out.push(224 | code >> 12, 128 | code >> 6 & 63, 128 | code & 63);
			else out.push(240 | code >> 18, 128 | code >> 12 & 63, 128 | code >> 6 & 63, 128 | code & 63);
		}
		return new Uint8Array(out);
	}
	const TEST_ADDER_INTERFACE = {
		contractLo: 2680941750,
		contractHi: 1076121077,
		dispatchType: DispatchType.VirtualMachine,
		fnCount: 4,
		functions: [],
		factory: null,
		contractName: "test.add@1",
		version: 65536
	};
	function test_adder_fn0_abi_wrapper(impl, args_ptr, out_ptr, arena_ptr, bridge) {
		const polyplug = bridge;
		if (!polyplug) return 1;
		if (!impl) return 1;
		if (!args_ptr) return 8;
		if (!out_ptr) return 8;
		var arg_args = {
			a: polyplug.readU32(args_ptr),
			b: polyplug.readU32(args_ptr + 4)
		};
		var result = impl.fn0(arg_args);
		polyplug.writeU32(out_ptr, result);
		return 0;
	}
	function test_adder_fn1_abi_wrapper(impl, args_ptr, out_ptr, arena_ptr, bridge) {
		const polyplug = bridge;
		if (!polyplug) return 1;
		if (!impl) return 1;
		if (!args_ptr) return 8;
		if (!out_ptr) return 8;
		var arg_a = polyplug.readU32(args_ptr);
		var arg_b = polyplug.readU32(args_ptr + 4);
		var result = impl.fn1(arg_a, arg_b);
		polyplug.writeU32(out_ptr, result);
		return 0;
	}
	function test_adder_fn2_abi_wrapper(impl, args_ptr, out_ptr, arena_ptr, bridge) {
		const polyplug = bridge;
		if (!polyplug) return 1;
		if (!impl) return 1;
		if (!out_ptr) return 8;
		const _retBytes = _ppEncodeUtf8(impl.fn2());
		const _retBuf = polyplug.arenaAlloc(_retBytes.length > 0 ? _retBytes.length : 1, arena_ptr);
		const _retPtr = _retBuf[0] + _retBuf[1] * 4294967296;
		for (let _i = 0; _i < _retBytes.length; _i++) polyplug.writeByte(_retPtr + _i, _retBytes[_i]);
		polyplug.writeU32(out_ptr, _retBuf[0]);
		polyplug.writeU32(out_ptr + 4, _retBuf[1]);
		polyplug.writeU32(out_ptr + 8, _retBytes.length);
		polyplug.writeU32(out_ptr + 12, 0);
		return 0;
	}
	function test_adder_fn3_abi_wrapper(impl, args_ptr, out_ptr, arena_ptr, bridge) {
		if (!bridge) return 1;
		if (!impl) return 1;
		impl.fn3();
		return 0;
	}
	function setTestAdderFactory(factory) {
		TEST_ADDER_INTERFACE.factory = factory;
		TEST_ADDER_INTERFACE.functions = [
			test_adder_fn0_abi_wrapper,
			test_adder_fn1_abi_wrapper,
			test_adder_fn2_abi_wrapper,
			test_adder_fn3_abi_wrapper
		];
	}
	//#endregion
	//#region tests/fixtures/test_plugin_js_generated/generated/guest/init.ts
	const AbiErrorCode = {
		Ok: 0,
		Generic: 1,
		InvalidPointer: 8
	};
	/**
	* Initialize plugin with host runtime.
	*
	* Returns `[registrations, abiError]`: the per-contract registration array
	* the loader consumes, plus the canonical AbiError ({ code, message }).
	* Nothing is deposited into any global namespace (Rule 12) — the loader reads
	* BOTH return values. The host vtable and the `bridge` are threaded explicitly
	* to each author factory; no host pointer or bridge is stored in any module.
	*
	* @param host_lo - HostApi pointer (low 32 bits)
	* @param host_hi - HostApi pointer (high 32 bits)
	* @param ctx_lo - BundleInitContext pointer (low 32 bits)
	* @param ctx_hi - BundleInitContext pointer (high 32 bits)
	* @param bridge - Host-capability bridge passed in by the loader
	*/
	function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi, bridge) {
		if (host_lo === 0 && host_hi === 0) return [[], {
			code: AbiErrorCode.Generic,
			message: "null host pointer in polyplug_init"
		}];
		if (ctx_lo === 0 && ctx_hi === 0) return [[], {
			code: AbiErrorCode.Generic,
			message: "null ctx pointer in polyplug_init"
		}];
		if (!bridge || !bridge.alloc) return [[], {
			code: AbiErrorCode.Generic,
			message: "missing bridge in polyplug_init"
		}];
		const registrations = [];
		registrations.push({
			contractLo: TEST_ADDER_INTERFACE.contractLo,
			contractHi: TEST_ADDER_INTERFACE.contractHi,
			interface: TEST_ADDER_INTERFACE,
			fnCount: TEST_ADDER_INTERFACE.fnCount,
			contractName: TEST_ADDER_INTERFACE.contractName,
			version: TEST_ADDER_INTERFACE.version
		});
		return [registrations, {
			code: AbiErrorCode.Ok,
			message: ""
		}];
	}
	//#endregion
	//#region tests/fixtures/test_plugin_js_generated/adder.js
	setTestAdderFactory((bridge, hostLo, hostHi) => ({
		fn0: (args) => args.a + args.b >>> 0,
		fn1: (a, b) => a + b >>> 0,
		fn2: () => "test_adder 1.0.0",
		fn3: () => {}
	}));
	//#endregion
	exports.polyplug_init = polyplug_init;
	return exports;
})({});
globalThis.polyplug_init = polyplug_module.polyplug_init;
