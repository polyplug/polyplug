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
	const POLYPLUG_MANIFEST = new Uint8Array([
		35,
		32,
		84,
		72,
		73,
		83,
		32,
		70,
		73,
		76,
		69,
		32,
		73,
		83,
		32,
		65,
		85,
		84,
		79,
		45,
		71,
		69,
		78,
		69,
		82,
		65,
		84,
		69,
		68,
		32,
		66,
		89,
		32,
		112,
		111,
		108,
		121,
		112,
		108,
		117,
		103,
		99,
		46,
		32,
		68,
		79,
		32,
		78,
		79,
		84,
		32,
		69,
		68,
		73,
		84,
		46,
		10,
		110,
		97,
		109,
		101,
		32,
		61,
		32,
		34,
		116,
		101,
		115,
		116,
		95,
		106,
		115,
		95,
		103,
		101,
		110,
		101,
		114,
		97,
		116,
		101,
		100,
		95,
		98,
		117,
		110,
		100,
		108,
		101,
		34,
		10,
		105,
		100,
		32,
		61,
		32,
		49,
		48,
		52,
		52,
		57,
		48,
		52,
		55,
		48,
		52,
		55,
		53,
		56,
		50,
		53,
		51,
		50,
		49,
		49,
		50,
		10,
		118,
		101,
		114,
		115,
		105,
		111,
		110,
		32,
		61,
		32,
		34,
		49,
		46,
		48,
		46,
		48,
		34,
		10,
		108,
		111,
		97,
		100,
		101,
		114,
		32,
		61,
		32,
		34,
		106,
		115,
		45,
		113,
		117,
		105,
		99,
		107,
		106,
		115,
		34,
		10,
		112,
		114,
		111,
		118,
		105,
		100,
		101,
		115,
		32,
		61,
		32,
		91,
		34,
		116,
		101,
		115,
		116,
		46,
		97,
		100,
		100,
		64,
		49,
		34,
		93,
		10,
		102,
		117,
		110,
		99,
		116,
		105,
		111,
		110,
		95,
		99,
		111,
		117,
		110,
		116,
		32,
		61,
		32,
		123,
		32,
		34,
		116,
		101,
		115,
		116,
		46,
		97,
		100,
		100,
		64,
		49,
		34,
		32,
		61,
		32,
		52,
		32,
		125,
		10,
		110,
		101,
		101,
		100,
		115,
		95,
		114,
		101,
		105,
		110,
		105,
		116,
		95,
		111,
		110,
		95,
		100,
		101,
		112,
		95,
		114,
		101,
		108,
		111,
		97,
		100,
		32,
		61,
		32,
		102,
		97,
		108,
		115,
		101,
		10,
		102,
		105,
		108,
		101,
		32,
		61,
		32,
		34,
		112,
		108,
		117,
		103,
		105,
		110,
		46,
		106,
		115,
		34,
		10
	]);
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
	exports.POLYPLUG_MANIFEST = POLYPLUG_MANIFEST;
	exports.polyplug_init = polyplug_init;
	return exports;
})({});
globalThis.polyplug_init = polyplug_module.polyplug_init;
globalThis.POLYPLUG_MANIFEST = polyplug_module.POLYPLUG_MANIFEST;
