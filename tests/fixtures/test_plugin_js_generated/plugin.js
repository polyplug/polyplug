var polyplug_module = (function(exports) {
	Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
	const TEST_ADDER_INTERFACE = {
		contractLo: 2680941750,
		contractHi: 1076121077,
		dispatchType: Object.freeze({
			Native: 0,
			VirtualMachine: 1
		}).VirtualMachine,
		fnCount: 4,
		functions: [],
		factory: null,
		contractName: "test.add@1",
		version: 65536
	};
	function test_adder_fn0_abi_wrapper(impl, args_ptr, out_ptr) {
		var polyplug = globalThis.polyplug;
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
	function test_adder_fn1_abi_wrapper(impl, args_ptr, out_ptr) {
		var polyplug = globalThis.polyplug;
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
	function test_adder_fn2_abi_wrapper(impl, args_ptr, out_ptr) {
		var polyplug = globalThis.polyplug;
		if (!polyplug) return 1;
		if (!impl) return 1;
		if (!out_ptr) return 8;
		var result = impl.fn2();
		polyplug.writeU32(out_ptr, result.ptr_lo);
		polyplug.writeU32(out_ptr + 4, result.ptr_hi);
		polyplug.writeU32(out_ptr + 8, result.len);
		polyplug.writeU32(out_ptr + 12, 0);
		return 0;
	}
	function test_adder_fn3_abi_wrapper(impl, args_ptr, out_ptr) {
		if (!globalThis.polyplug) return 1;
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
	function storeHostVtable(lo, hi) {
		globalThis.polyplug._hostVtableLo = lo;
		globalThis.polyplug._hostVtableHi = hi;
	}
	const AbiErrorCode = {
		Ok: 0,
		Generic: 1,
		InvalidPointer: 8
	};
	/**
	* Initialize plugin with host runtime.
	* @param host_lo - HostApi pointer (low 32 bits)
	* @param host_hi - HostApi pointer (high 32 bits)
	* @param ctx_lo - BundleInitContext pointer (low 32 bits)
	* @param ctx_hi - BundleInitContext pointer (high 32 bits)
	*/
	function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {
		if (host_lo === 0 && host_hi === 0) return {
			code: AbiErrorCode.Generic,
			message: {
				ptr: 0,
				len: 0
			}
		};
		if (ctx_lo === 0 && ctx_hi === 0) return {
			code: AbiErrorCode.Generic,
			message: {
				ptr: 0,
				len: 0
			}
		};
		storeHostVtable(host_lo, host_hi);
		const polyplug = globalThis.polyplug;
		if (!polyplug || !polyplug.registerVtable) return {
			code: AbiErrorCode.Generic,
			message: {
				ptr: 0,
				len: 0
			}
		};
		polyplug.registerVtable(TEST_ADDER_INTERFACE.contractLo, TEST_ADDER_INTERFACE.contractHi, TEST_ADDER_INTERFACE, TEST_ADDER_INTERFACE.fnCount, TEST_ADDER_INTERFACE.contractName, TEST_ADDER_INTERFACE.version);
		return {
			code: AbiErrorCode.Ok,
			message: {
				ptr: 0,
				len: 0
			}
		};
	}
	//#endregion
	//#region sdks/js/guest/polyplug_guest.js
	/**
	* Write bytes to host memory.
	* 
	* @param {bigint} ptr - Pointer to memory (as BigInt)
	* @param {Uint8Array} data - Bytes to write
	* @returns {void}
	* 
	* @example
	* writeBytes(0x1234n, new TextEncoder().encode("hello"));
	*/
	function writeBytes(ptr, data) {
		const ptrNum = Number(ptr);
		for (let i = 0; i < data.length; i++) globalThis.polyplug.writeByte(ptrNum + i, data[i]);
	}
	/**
	* Allocate a string in host memory.
	* 
	* @param {string} str - JavaScript string to allocate
	* @returns {{ ptr: bigint, len: number }} Pointer and length of allocated string
	* 
	* @example
	* const { ptr, len } = allocString("hello");
	*/
	function _encodeUtf8(str) {
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
	/**
	* Allocate a return-value string from the current call arena.
	*
	* Use this for strings RETURNED from a contract function: the bytes are served
	* from the host's per-call {@link CallArena} and stay valid until the next call
	* on the same caller, so the guest never frees them. When no arena is active
	* (`polyplug.arenaAlloc` falls back to `polyplug.alloc`), this behaves like
	* {@link allocString}. For data that must outlive the call, use
	* {@link allocString} and free it explicitly with {@link freeBytes}.
	*
	* @param {string} str - JavaScript string to allocate.
	* @returns {{ ptr: bigint, len: number }} Pointer and length of the bytes.
	*/
	function allocStringArena(str) {
		const bytes = typeof TextEncoder !== "undefined" ? new TextEncoder().encode(str) : _encodeUtf8(str);
		const ptrArr = globalThis.polyplug.arenaAlloc(bytes.length);
		const ptr = (BigInt(ptrArr[1]) << 32n) + BigInt(ptrArr[0]);
		writeBytes(ptr, bytes);
		return {
			ptr,
			len: bytes.length
		};
	}
	//#endregion
	//#region tests/fixtures/test_plugin_js_generated/adder.js
	function add(args) {
		return args.a + args.b >>> 0;
	}
	function addPrimitive(a, b) {
		return a + b >>> 0;
	}
	function version() {
		const result = allocStringArena("test_adder 1.0.0");
		return {
			ptr_lo: Number(result.ptr & 4294967295n),
			ptr_hi: Number(result.ptr >> 32n & 4294967295n),
			len: result.len
		};
	}
	function reset() {}
	setTestAdderFactory(() => ({
		fn0: add,
		fn1: addPrimitive,
		fn2: version,
		fn3: reset
	}));
	//#endregion
	exports.polyplug_init = polyplug_init;
	return exports;
})({});
globalThis.polyplug_init = polyplug_module.polyplug_init;
