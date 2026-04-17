# Deferred Items - Phase 05-03

## Out-of-Scope Issues

**2026-04-04**: C# SDK abi project has pre-existing compilation errors:
- `sdks/csharp/abi/StringViewHelper.cs`: Missing `StringView` type (14 errors)
- `sdks/csharp/abi/StringHelpers.cs`: Missing `StringView` type
- This is a pre-existing ABI sync issue from earlier phase work
- Not related to RuntimeConfigC or PluginGuard changes in this plan
- Requires separate plan to sync C# abi with polyplug_abi updates

## Resolution Needed

Future plan should:
1. Define `StringView` struct in `sdks/csharp/abi/`
2. Update all abi types to match polyplug_abi v1.1 layout