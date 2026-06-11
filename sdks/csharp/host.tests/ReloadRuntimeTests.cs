using System;
using System.Collections.Generic;
using System.IO;
using Polyplug.Host;
using Polyplug.Loaders.Native;
using Xunit;

namespace Polyplug.Host.Tests
{
    /// <summary>
    /// REAL-runtime hot-reload notification test (mirrors
    /// sdks/lua/host/tests/test_reload_runtime.lua and
    /// sdks/js/host/tests/reload_runtime_test.ts).
    ///
    /// <see cref="RuntimeCreateTests.RuntimeCreateWithHotReloadConfig"/> only
    /// proves that BUILDING with an OnReload callback survives the FFI boundary —
    /// it can never catch a broken delivery path. This test drives the actual
    /// flow: build a runtime with an OnReload callback through the builder (which
    /// marshals the <c>OnReloadNative</c> trampoline into <c>RuntimeConfig</c>),
    /// register the native loader, load the native reload fixture bundle, trigger
    /// a reload through the runtime, and assert the callback fired with REAL
    /// phase data delivered across the C ABI.
    ///
    /// Skip-honestly policy (matches the lua/js reload runtime tests): the core
    /// cdylib resolution (<see cref="TestNativeLibraries"/>) and the fixture
    /// checks below FAIL LOUDLY with instructions when the environment is
    /// missing — a runtime test that silently passes hides exactly the never-run
    /// breakage class it exists to catch.
    ///
    /// Run from repo root:
    ///   cargo build --release -p polyplug -p polyplug_native
    ///   bash tests/fixtures/build_all.sh
    ///   dotnet test sdks/csharp/host.tests/Polyplug.Host.Tests.csproj
    /// </summary>
    public sealed class ReloadRuntimeTests
    {
        /// <summary>
        /// id from tests/fixtures/reload_plugin_v1/manifest.toml — the FNV-1a 64
        /// hash of "reload_plugin_v1" (TRUST_MODEL §2), enforced by the manifest
        /// parser. Mirrors V1_BUNDLE_ID in the js reload runtime test.
        /// </summary>
        private const ulong V1BundleId = 16808897324254478442UL;

        static ReloadRuntimeTests()
        {
            TestNativeLibraries.EnsureInstalled();
        }

        /// <summary>
        /// Resolve a reload fixture path, failing loudly with build instructions
        /// when the fixture cdylibs have not been produced yet.
        /// </summary>
        private static string RequireFixture(params string[] relative)
        {
            string path = Path.Combine(TestNativeLibraries.RepoRoot, "tests", "fixtures");
            foreach (string part in relative)
            {
                path = Path.Combine(path, part);
            }

            if (!File.Exists(path))
            {
                throw new InvalidOperationException(
                    $"reload fixture missing: {path} — run `bash tests/fixtures/build_all.sh` first.");
            }

            return path;
        }

        [Fact]
        public void OnReloadFiresWithRealPhaseDataOnRealRuntimeReload()
        {
            string v1Lib = TestNativeLibraries.CdylibFileName("reload_plugin_v1");
            string v2Lib = TestNativeLibraries.CdylibFileName("reload_plugin_v2");

            RequireFixture("reload_plugin_v1", "manifest.toml");
            string v1Dir = Path.GetDirectoryName(RequireFixture("reload_plugin_v1", v1Lib))!;
            // The reload target is the v2 cdylib INSIDE its bundle dir — the
            // runtime reads the sibling manifest.toml during reload (mirrors
            // integration_reload.rs).
            string v2So = RequireFixture("reload_plugin_v2", v2Lib);

            List<ReloadPhase> phases = new List<ReloadPhase>();
            // The builder sets HotReloadEnabled = true whenever OnReload is given.
            Runtime runtime = new RuntimeBuilder()
                .OnReload(phases.Add)
                .Build();
            runtime.RegisterNativeLoader();

            runtime.LoadBundle(v1Dir);
            Assert.Empty(phases);

            runtime.ReloadBundle(v2So);

            Assert.True(
                phases.Count >= 2,
                $"reload must deliver at least Preparing + Reloaded, got {phases.Count}: " +
                string.Join(", ", phases));

            ReloadPhase preparing = phases[0];
            Assert.True(preparing.IsPreparing(), $"first phase must be Preparing, got: {preparing}");
            Assert.Equal(V1BundleId, preparing.BundleId);
            Assert.Equal("reload_plugin_v1", preparing.BundleName);
            // Non-Failed phases carry the null-view reason; the SDK surfaces it
            // as the empty string.
            Assert.Equal(string.Empty, preparing.Reason);

            Assert.Contains(phases, phase => phase.IsReloaded());
            GC.KeepAlive(runtime);
        }
    }
}
