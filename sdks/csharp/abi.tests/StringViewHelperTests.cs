// Edge-case tests for the validated StringView helpers (docs/SDK_HELPERS.md).

using System;
using System.Runtime.InteropServices;
using Xunit;

namespace Polyplug.Abi.Tests
{
    public class StringViewHelperTests
    {
        private static string[] SplitPinned(string input, string delimiter)
        {
            (StringView view, GCHandle handle) = StringViewHelper.FromStringPinned(input);
            try
            {
                return StringViewHelper.Split(view, delimiter);
            }
            finally
            {
                if (handle.IsAllocated)
                    handle.Free();
            }
        }

        [Fact]
        public void ToStringNullViewIsEmpty()
        {
            StringView sv = new StringView { Ptr = IntPtr.Zero, Len = 5 };
            Assert.Equal(string.Empty, StringViewHelper.ToString(sv));
        }

        [Fact]
        public void ToStringZeroLenViewIsEmpty()
        {
            (StringView view, GCHandle handle) = StringViewHelper.FromStringPinned("x");
            try
            {
                StringView truncated = new StringView { Ptr = view.Ptr, Len = 0 };
                Assert.Equal(string.Empty, StringViewHelper.ToString(truncated));
            }
            finally
            {
                handle.Free();
            }
        }

        [Fact]
        public void ToStringRoundTripsUtf8()
        {
            (StringView view, GCHandle handle) = StringViewHelper.FromStringPinned("héllo wörld");
            try
            {
                Assert.Equal("héllo wörld", StringViewHelper.ToString(view));
            }
            finally
            {
                handle.Free();
            }
        }

        [Fact]
        public void SplitKeepsConsecutiveEmptySegments()
        {
            Assert.Equal(new[] { "a", "", "b" }, SplitPinned("a||b", "|"));
        }

        [Fact]
        public void SplitKeepsLeadingAndTrailingEmptySegments()
        {
            Assert.Equal(new[] { "", "a", "" }, SplitPinned("|a|", "|"));
        }

        [Fact]
        public void SplitEmptyViewIsEmptyArray()
        {
            StringView sv = new StringView { Ptr = IntPtr.Zero, Len = 0 };
            Assert.Empty(StringViewHelper.Split(sv, "|"));
        }

        [Fact]
        public void SplitEmptyDelimiterReturnsWholeString()
        {
            Assert.Equal(new[] { "ab" }, SplitPinned("ab", ""));
        }

        [Fact]
        public void SplitMultiByteLiteralDelimiter()
        {
            Assert.Equal(new[] { "a", "b", "c" }, SplitPinned("a::b::c", "::"));
        }
    }
}
