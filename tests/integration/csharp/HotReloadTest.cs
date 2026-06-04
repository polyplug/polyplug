// tests/integration/csharp/HotReloadTest.cs
// Unit tests for ReloadPhase and RuntimeConfig types.
//
// Run with: dotnet test

using System;
using Polyplug.Host;

namespace Polyplug.Tests;

/// <summary>
/// Unit tests for ReloadPhaseType enum constants.
/// </summary>
public static class ReloadPhaseTypeTests
{
    public static void TypePreparingIs0()
    {
        if ((uint)ReloadPhaseType.Preparing != 0U)
        {
            throw new Exception("TYPE_PREPARING should be 0");
        }
    }

    public static void TypeReloadedIs1()
    {
        if ((uint)ReloadPhaseType.Reloaded != 1U)
        {
            throw new Exception("TYPE_RELOADED should be 1");
        }
    }

    public static void TypeFailedIs2()
    {
        if ((uint)ReloadPhaseType.Failed != 2U)
        {
            throw new Exception("TYPE_FAILED should be 2");
        }
    }
}

/// <summary>
/// Unit tests for ReloadPhase class.
/// </summary>
public static class ReloadPhaseTests
{
    public static void ConstructorSetsAllProperties()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Preparing,
            12345UL,
            "TestBundle",
            2u,
            "Test reason"
        );

        if (phase.Type != ReloadPhaseType.Preparing)
        {
            throw new Exception("phase.Type should be Preparing");
        }
        if (phase.BundleId != 12345UL)
        {
            throw new Exception("phase.BundleId should be 12345");
        }
        if (phase.BundleName != "TestBundle")
        {
            throw new Exception("phase.BundleName should be TestBundle");
        }
        if (phase.RetryCount != 2u)
        {
            throw new Exception("phase.RetryCount should be 2");
        }
        if (phase.Reason != "Test reason")
        {
            throw new Exception("phase.Reason should be Test reason");
        }
    }

    public static void ConstructorUsesDefaultValues()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Reloaded,
            999UL,
            "MyBundle"
        );

        if (phase.Type != ReloadPhaseType.Reloaded)
        {
            throw new Exception("phase.Type should be Reloaded");
        }
        if (phase.BundleId != 999UL)
        {
            throw new Exception("phase.BundleId should be 999");
        }
        if (phase.BundleName != "MyBundle")
        {
            throw new Exception("phase.BundleName should be MyBundle");
        }
        if (phase.RetryCount != 0u)
        {
            throw new Exception("phase.RetryCount should default to 0");
        }
        if (phase.Reason != "")
        {
            throw new Exception("phase.Reason should default to empty string");
        }
    }

    public static void ConstructorHandlesNullBundleName()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Failed,
            1UL,
            null!,
            0u,
            "Error"
        );

        if (phase.BundleName != "")
        {
            throw new Exception("null bundleName should default to empty string");
        }
    }

    public static void ConstructorHandlesNullReason()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Failed,
            1UL,
            "Bundle",
            0u,
            null!
        );

        if (phase.Reason != "")
        {
            throw new Exception("null reason should default to empty string");
        }
    }

    public static void IsPreparingReturnsTrueForPreparing()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Preparing, 1UL, "Bundle");
        if (!phase.IsPreparing())
        {
            throw new Exception("IsPreparing should return true for Preparing");
        }
    }

    public static void IsPreparingReturnsFalseForReloaded()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Reloaded, 1UL, "Bundle");
        if (phase.IsPreparing())
        {
            throw new Exception("IsPreparing should return false for Reloaded");
        }
    }

    public static void IsReloadedReturnsTrueForReloaded()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Reloaded, 1UL, "Bundle");
        if (!phase.IsReloaded())
        {
            throw new Exception("IsReloaded should return true for Reloaded");
        }
    }

    public static void IsReloadedReturnsFalseForPreparing()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Preparing, 1UL, "Bundle");
        if (phase.IsReloaded())
        {
            throw new Exception("IsReloaded should return false for Preparing");
        }
    }

    public static void IsFailedReturnsTrueForFailed()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Failed, 1UL, "Bundle");
        if (!phase.IsFailed())
        {
            throw new Exception("IsFailed should return true for Failed");
        }
    }

    public static void IsFailedReturnsFalseForPreparing()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Preparing, 1UL, "Bundle");
        if (phase.IsFailed())
        {
            throw new Exception("IsFailed should return false for Preparing");
        }
    }

    public static void ToStringIncludesAllRelevantFields()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Preparing,
            42UL,
            "TestBundle",
            3u,
            "Ignored"
        );
        var result = phase.ToString();

        if (!result.Contains("Preparing"))
        {
            throw new Exception("ToString should include Preparing");
        }
        if (!result.Contains("42"))
        {
            throw new Exception("ToString should include BundleId");
        }
        if (!result.Contains("TestBundle"))
        {
            throw new Exception("ToString should include BundleName");
        }
        if (!result.Contains("3"))
        {
            throw new Exception("ToString should include RetryCount");
        }
    }
}

/// <summary>
/// Test runner entry point.
/// </summary>
public static class TestRunner
{
    private static int _passed = 0;
    private static int _failed = 0;

    private static void RunTest(Action test, string name)
    {
        try
        {
            test();
            _passed++;
            Console.WriteLine($"  PASS: {name}");
        }
        catch (Exception e)
        {
            _failed++;
            Console.WriteLine($"  FAIL: {name}");
            Console.WriteLine($"        {e.Message}");
        }
    }

    public static int Main()
    {
        Console.WriteLine("=== ReloadPhase Type Constants ===");
        RunTest(ReloadPhaseTypeTests.TypePreparingIs0, "TypePreparingIs0");
        RunTest(ReloadPhaseTypeTests.TypeReloadedIs1, "TypeReloadedIs1");
        RunTest(ReloadPhaseTypeTests.TypeFailedIs2, "TypeFailedIs2");

        Console.WriteLine("\n=== ReloadPhase Constructor ===");
        RunTest(ReloadPhaseTests.ConstructorSetsAllProperties, "ConstructorSetsAllProperties");
        RunTest(ReloadPhaseTests.ConstructorUsesDefaultValues, "ConstructorUsesDefaultValues");
        RunTest(ReloadPhaseTests.ConstructorHandlesNullBundleName, "ConstructorHandlesNullBundleName");
        RunTest(ReloadPhaseTests.ConstructorHandlesNullReason, "ConstructorHandlesNullReason");

        Console.WriteLine("\n=== ReloadPhase Helper Methods ===");
        RunTest(ReloadPhaseTests.IsPreparingReturnsTrueForPreparing, "IsPreparingReturnsTrueForPreparing");
        RunTest(ReloadPhaseTests.IsPreparingReturnsFalseForReloaded, "IsPreparingReturnsFalseForReloaded");
        RunTest(ReloadPhaseTests.IsReloadedReturnsTrueForReloaded, "IsReloadedReturnsTrueForReloaded");
        RunTest(ReloadPhaseTests.IsReloadedReturnsFalseForPreparing, "IsReloadedReturnsFalseForPreparing");
        RunTest(ReloadPhaseTests.IsFailedReturnsTrueForFailed, "IsFailedReturnsTrueForFailed");
        RunTest(ReloadPhaseTests.IsFailedReturnsFalseForPreparing, "IsFailedReturnsFalseForPreparing");

        Console.WriteLine("\n=== ReloadPhase ToString ===");
        RunTest(ReloadPhaseTests.ToStringIncludesAllRelevantFields, "ToStringIncludesAllRelevantFields");

        Console.WriteLine("\n========================================");
        Console.WriteLine($"Tests passed: {_passed}");
        Console.WriteLine($"Tests failed: {_failed}");
        Console.WriteLine("========================================");

        return _failed > 0 ? 1 : 0;
    }
}