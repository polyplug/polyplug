import { InternalPluginBundle } from "../polyplug/mod.js";
import { assertEquals, assertStrictEquals, assertThrows, test } from "../../testing/harness.ts";

const MANIFEST = 'name = "js.bundle"\nid = 1\nversion = "1.0.0"\nloader = "js-quickjs"\nprovides = []\nfunction_count = {}\nneeds_reinit_on_dep_reload = false\nfile = "plugin.js"\n';

test("internal-plugin bundle retains its manifest and resident until committed transfer", () => {
    let releases = 0;
    let registrations = 0;
    const resident = {
        release(): void {
            releases += 1;
        },
    };
    const bundle = new InternalPluginBundle(MANIFEST, resident, () => {
        registrations += 1;
    });

    assertStrictEquals(new TextDecoder().decode(bundle._internalPluginManifest()), MANIFEST);
    bundle._reserveInternalPluginTransfer();
    bundle._registerGuestContracts(null as never);
    assertEquals(registrations, 1);
    assertStrictEquals(bundle._takeInternalPluginResident(), resident);
    assertThrows(
        () => bundle._internalPluginManifest(),
        Error,
        "already been registered",
    );
    assertThrows(
        () => bundle._takeInternalPluginResident(),
        Error,
        "not available",
    );

    resident.release();
    assertEquals(releases, 1);
});

test("internal-plugin bundle cancellation preserves resident ownership for retry", () => {
    const resident = { release(): void {} };
    const bundle = new InternalPluginBundle(MANIFEST, resident, () => {});

    bundle._reserveInternalPluginTransfer();
    bundle._cancelInternalPluginTransfer();
    bundle._reserveInternalPluginTransfer();
    assertStrictEquals(bundle._takeInternalPluginResident(), resident);
});
