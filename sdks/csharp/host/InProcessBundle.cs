using System;
using System.Threading;

using Polyplug.Abi;

namespace Polyplug.Host;

/// <summary>
/// Canonical manifest, contract registrar, and managed state for one in-process
/// bundle. The resident remains owned by its creator until the staged transaction
/// commits successfully.
/// </summary>
public sealed class InProcessBundle
{
    private readonly object _resident;
    private readonly Func<nint, AbiError> _registerContracts;
    private int _released;
    private int _transferred;

    /// <summary>
    /// Creates a bundle backed by canonical manifest bytes and existing
    /// <see cref="PluginDescriptor"/> / <see cref="GuestContractInterface"/> pairs.
    /// </summary>
    /// <param name="manifest">Canonical manifest bytes for the bundle.</param>
    /// <param name="resident">Managed delegates, factories, interfaces, and implementation objects.</param>
    /// <param name="registerContracts">Registers every descriptor/interface pair with the active staging transaction.</param>
    public InProcessBundle(byte[] manifest, object resident, Func<nint, AbiError> registerContracts)
    {
        ArgumentNullException.ThrowIfNull(manifest);
        ArgumentNullException.ThrowIfNull(resident);
        ArgumentNullException.ThrowIfNull(registerContracts);
        if (manifest.Length == 0)
        {
            throw new ArgumentException("In-process manifest must not be empty.", nameof(manifest));
        }

        Manifest = manifest;
        _resident = resident;
        _registerContracts = registerContracts;
    }

    internal byte[] Manifest { get; }
    internal AbiError RegisterContracts(nint host) => _registerContracts(host);
    internal bool TryReserveTransfer() =>
        Interlocked.CompareExchange(ref _transferred, 1, 0) == 0;

    internal void CancelTransfer() =>
        Interlocked.CompareExchange(ref _transferred, 0, 1);


    internal void Release()
    {
        if (Interlocked.Exchange(ref _released, 1) == 0 && _resident is IDisposable disposable)
        {
            disposable.Dispose();
        }
    }
}
