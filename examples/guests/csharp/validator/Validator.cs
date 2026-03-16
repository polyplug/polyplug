// Validator.cs — Validator plugin. ZERO unsafe. Pure business logic.
using System.Text;
using Polyplug.Guest;

namespace Validator;

// Business logic — pure safe C#.
public static class ValidatorImpl
{
    // pipeline.Validator@1 contract ID
    public const ulong VALIDATOR_CONTRACT_ID = 0xA553FAB5D11C7AF0UL;

    public static byte[] Validate(string value)
    {
        // Expected format: "DECODED:name|value|42"
        if (!value.StartsWith("DECODED:"))
        {
            byte[] errBytes = Encoding.UTF8.GetBytes("INVALID:missing DECODED prefix");
            return errBytes;
        }

        string payload = value.Substring(8); // skip "DECODED:"
        string[] parts = payload.Split('|');

        if (parts.Length != 3)
        {
            byte[] errBytes = Encoding.UTF8.GetBytes("INVALID:expected 3 pipe-separated fields");
            return errBytes;
        }

        string name = parts[0];
        string val = parts[1];
        string numStr = parts[2];

        if (string.IsNullOrEmpty(name) || string.IsNullOrEmpty(val))
        {
            byte[] errBytes = Encoding.UTF8.GetBytes("INVALID:empty name or value field");
            return errBytes;
        }

        bool isNumeric = int.TryParse(numStr, out _);
        if (!isNumeric)
        {
            byte[] errBytes = Encoding.UTF8.GetBytes($"INVALID:third field is not a number: {numStr}");
            return errBytes;
        }

        byte[] okBytes = Encoding.UTF8.GetBytes("VALID:ok");
        return okBytes;
    }
}
