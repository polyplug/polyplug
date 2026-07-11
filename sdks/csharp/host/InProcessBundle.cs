using System;
using System.Threading;

using Polyplug.Abi;

namespace Polyplug.Host;

/// <summary>
/// Complete in-process registration and the managed state that keeps every
/// pointer-bearing registration table and callback target alive.
/// </summary>
public sealed class InProcessBundle
{
    private readonly object _resident;
    private int _released;
    private int _transferred;

    /// <summary>
    /// Creates a bundle registration. The supplied resident is retained by the
    /// owning <see cref="Runtime"/> only after native registration succeeds.
    /// </summary>
    /// <param name="registration">The complete canonical registration input.</param>
    /// <param name="resident">Managed tables, delegates, factories, and implementations referenced by the registration.</param>
    public InProcessBundle(InProcessBundleRegistration registration, object resident)
    {
        ArgumentNullException.ThrowIfNull(resident);
        Registration = registration;
        _resident = resident;
    }

    /// <summary>The canonical registration passed synchronously to the runtime.</summary>
    public InProcessBundleRegistration Registration { get; }

    internal object Resident => _resident;
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
