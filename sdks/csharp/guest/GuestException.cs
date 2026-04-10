namespace Polyplug.Guest;

/// <summary>
/// Exception thrown when a polyplug ABI call returns a non-zero error code.
/// Carries the numeric ABI error code alongside the UTF-8 message.
/// </summary>
public sealed class GuestException : Exception
{
    public uint Code { get; }

    public GuestException(uint code, string message) : base(message)
    {
        Code = code;
    }
}