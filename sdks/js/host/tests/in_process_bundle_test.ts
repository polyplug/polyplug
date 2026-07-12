import { InProcessBundle } from "../polyplug/mod.js";
import { assertEquals, assertStrictEquals, assertThrows, test } from "../../testing/harness.ts";

const MANIFEST = 'name = "js.bundle"\nid = 1\nversion = "1.0.0"\nloader = "js-quickjs"\nprovides = []\nfunction_count = {}\nneeds_reinit_on_dep_reload = false\nfile = "plugin.js"\n';

test("in-process bundle retains its manifest and resident until committed transfer", () => {
    let releases = 0;
    let registrations = 0;
    const resident = {
        release(): void {
            releases += 1;
        },
    };
    const bundle = new InProcessBundle(MANIFEST, resident, () => {
        registrations += 1;
    });

    assertStrictEquals(new TextDecoder().decode(bundle._inProcessManifest()), MANIFEST);
    bundle._reserveInProcessTransfer();
    bundle._registerGuestContracts(null as never);
    assertEquals(registrations, 1);
    assertStrictEquals(bundle._takeInProcessResident(), resident);
    assertThrows(
        () => bundle._inProcessManifest(),
        Error,
        "already been registered",
    );
    assertThrows(
        () => bundle._takeInProcessResident(),
        Error,
        "not available",
    );

    resident.release();
    assertEquals(releases, 1);
});

test("in-process bundle cancellation preserves resident ownership for retry", () => {
    const resident = { release(): void {} };
    const bundle = new InProcessBundle(MANIFEST, resident, () => {});

    bundle._reserveInProcessTransfer();
    bundle._cancelInProcessTransfer();
    bundle._reserveInProcessTransfer();
    assertStrictEquals(bundle._takeInProcessResident(), resident);
});
