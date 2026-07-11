import { InProcessBundle } from "../polyplug/mod.js";
import { assertEquals, assertStrictEquals, assertThrows, test } from "../../testing/harness.ts";

test("in-process bundle transfers its rooted resident exactly once", () => {
    const registration = new Uint8Array(64);
    let releases = 0;
    const resident = {
        release(): void {
            releases += 1;
        },
    };
    const bundle = new InProcessBundle(registration, resident);

    assertStrictEquals(bundle._inProcessRegistration(), registration);
    assertStrictEquals(bundle._takeInProcessResident(), resident);
    assertThrows(
        () => bundle._inProcessRegistration(),
        Error,
        "already been registered",
    );
    assertThrows(
        () => bundle._takeInProcessResident(),
        Error,
        "already been transferred",
    );

    resident.release();
    assertEquals(releases, 1);
});
