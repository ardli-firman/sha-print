using ShaPrint.Core.Ipp.Testing;

namespace ShaPrint.Tests;

public class IppRequestBuilderTests
{
    [Fact]
    public void BuildPrintJobRequest_EndsWithExactDocumentBytes()
    {
        byte[] document = [0x50, 0x44, 0x46, 0x7F];

        byte[] request = IppRequestBuilder.BuildPrintJobRequest("TestPrinter", document);

        Assert.True(request.AsSpan().EndsWith(document));
        Assert.Equal(document.Length, request.AsSpan(request.Length - document.Length).Length);
    }
}
