// Deno test: the host-contract PROVIDER (buildHostContractInterface) gives each
// instance its own state.
//
// # Why this exists
//
// A host contract registered by a Deno host is provided through
// `buildHostContractInterface`, which builds a real C `HostContractInterface`
// with native dispatch backed by `Deno.UnsafeCallback`s. For a non-singleton
// contract the runtime calls `create_instance` once per caller; each call must
// build a FRESH implementation and dispatch must route to the right one by the
// instance handle. This test drives the generated C function pointers directly
// (the exact path the runtime takes) and asserts two instances never share state.
//
// This surface has no cargo coverage (the Deno host SDK runs only under Deno), so
// this test is the per-instance floor; the end-to-end path is covered by the
// example host (examples/hosts/js) under `just verify-examples`.

import { buildHostContractInterface } from "../polyplug/mod.js";
import {
  HOST_CONTRACT_INTERFACE_CREATE_INSTANCE_OFFSET,
  HOST_CONTRACT_INTERFACE_DESTROY_INSTANCE_OFFSET,
  HOST_CONTRACT_INTERFACE_DISPATCH_OFFSET,
  NATIVE_DISPATCH_FUNCTIONS_OFFSET,
} from "../../abi/abi.ts";

const HOST_CONTRACT_INSTANCE_STRUCT = { struct: ["pointer"] } as const;

const CREATE_DEF = {
  parameters: ["pointer", "pointer", "pointer"],
  result: "void",
} as const;
const DESTROY_DEF = {
  parameters: ["pointer", HOST_CONTRACT_INSTANCE_STRUCT],
  result: "void",
} as const;
const DISPATCH_DEF = {
  parameters: [HOST_CONTRACT_INSTANCE_STRUCT, "pointer", "pointer", "pointer"],
  result: "void",
} as const;

function ptrField<T extends Deno.ForeignFunction>(
  view: Deno.UnsafePointerView,
  offset: number,
): Deno.PointerObject<T> {
  const p = Deno.UnsafePointer.create(view.getBigUint64(offset));
  if (p === null) {
    throw new Error(`null pointer field at offset ${offset}`);
  }
  return p as Deno.PointerObject<T>;
}

Deno.test("host-contract provider gives each instance independent state", () => {
  // Stateful contract: a counter with a void `inc()` (fn 0). The factory records
  // every impl it builds so the test can inspect their counts directly.
  const built: { count: number }[] = [];
  const factory = () => {
    const impl = { count: 0 };
    built.push(impl);
    return impl;
  };

  const { interfacePtr } = buildHostContractInterface({
    contractIdLo: 0x1234,
    contractIdHi: 0x5678,
    major: 1,
    minor: 0,
    singleton: false,
    factory,
    // fn 0 = inc(): advance THIS instance's own counter.
    methods: [
      (impl: object) => {
        (impl as { count: number }).count += 1;
        return 0;
      },
    ],
  });
  if (interfacePtr === null) {
    throw new Error("interfacePtr is null");
  }
  // built[0] is the per-contract default impl (id 0), constructed at build time.

  const ifaceView = new Deno.UnsafePointerView(interfacePtr);
  const createFn = new Deno.UnsafeFnPointer(
    ptrField<typeof CREATE_DEF>(ifaceView, HOST_CONTRACT_INTERFACE_CREATE_INSTANCE_OFFSET),
    CREATE_DEF,
  );
  const destroyFn = new Deno.UnsafeFnPointer(
    ptrField<typeof DESTROY_DEF>(ifaceView, HOST_CONTRACT_INTERFACE_DESTROY_INSTANCE_OFFSET),
    DESTROY_DEF,
  );
  const fnsArrayPtr = ptrField<Deno.ForeignFunction>(
    ifaceView,
    HOST_CONTRACT_INTERFACE_DISPATCH_OFFSET + NATIVE_DISPATCH_FUNCTIONS_OFFSET,
  );
  const incFn = new Deno.UnsafeFnPointer(
    ptrField<typeof DISPATCH_DEF>(new Deno.UnsafePointerView(fnsArrayPtr), 0),
    DISPATCH_DEF,
  );

  // create_instance writes an 8-byte HostContractInstance { data } we read back.
  const createInstance = (): Uint8Array => {
    const inst = new Uint8Array(8);
    createFn.call(null, null, Deno.UnsafePointer.of(inst));
    return inst;
  };
  const errBuf = new Uint8Array(24);
  const errPtr = Deno.UnsafePointer.of(errBuf);
  const inc = (inst: Uint8Array) => {
    incFn.call(inst, null, null, errPtr);
  };

  const a = createInstance();
  const b = createInstance();

  // Distinct, non-zero ids.
  const idA = new DataView(a.buffer).getBigUint64(0, true);
  const idB = new DataView(b.buffer).getBigUint64(0, true);
  if (idA === 0n || idB === 0n) {
    throw new Error(`instance ids must be non-zero (a=${idA}, b=${idB})`);
  }
  if (idA === idB) {
    throw new Error(`instances must have distinct ids (both ${idA})`);
  }

  // Advance A three times, B once.
  inc(a);
  inc(a);
  inc(a);
  inc(b);

  // built[1] = instance A, built[2] = instance B, built[0] = default (untouched).
  if (built[1].count !== 3) {
    throw new Error(`instance A must keep its own count of 3, got ${built[1].count}`);
  }
  if (built[2].count !== 1) {
    throw new Error(`instance B must keep its own count of 1, got ${built[2].count}`);
  }
  if (built[0].count !== 0) {
    throw new Error(`default instance must be independent (0), got ${built[0].count}`);
  }

  // destroy_instance drops the impl from the registry (no throw).
  destroyFn.call(null, a);
  destroyFn.call(null, b);
});
