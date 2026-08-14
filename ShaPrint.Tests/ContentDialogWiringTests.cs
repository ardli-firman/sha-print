namespace ShaPrint.Tests;

public class ContentDialogWiringTests
{
    [Fact]
    public void DriverDialogs_UseConfiguredApplicationDialogHost()
    {
        string repositoryRoot = FindRepositoryRoot();
        string mainWindowXaml = File.ReadAllText(Path.Combine(
            repositoryRoot, "ShaPrint.WpfApp", "Views", "Windows", "MainWindow.xaml"));
        string mainWindowCode = File.ReadAllText(Path.Combine(
            repositoryRoot, "ShaPrint.WpfApp", "Views", "Windows", "MainWindow.xaml.cs"));
        string clientViewModel = File.ReadAllText(Path.Combine(
            repositoryRoot, "ShaPrint.WpfApp", "ViewModels", "Pages", "ClientViewModel.cs"));

        Assert.Contains("<ui:ContentDialogHost x:Name=\"RootContentDialogHost\"", mainWindowXaml);
        Assert.Contains("IContentDialogService contentDialogService", mainWindowCode);
        Assert.Contains("contentDialogService.SetDialogHost(RootContentDialogHost);", mainWindowCode);

        Assert.Contains("_contentDialogService.ShowAsync(picker", clientViewModel);
        Assert.Contains("_contentDialogService.ShowAsync(dialog", clientViewModel);
        Assert.DoesNotContain("await picker.ShowAsync(", clientViewModel);
        Assert.DoesNotContain("await dialog.ShowAsync(", clientViewModel);
    }

    private static string FindRepositoryRoot()
    {
        DirectoryInfo? directory = new(AppContext.BaseDirectory);

        while (directory != null && !File.Exists(Path.Combine(directory.FullName, "ShaPrint.sln")))
        {
            directory = directory.Parent;
        }

        return directory?.FullName
            ?? throw new DirectoryNotFoundException("Could not locate the ShaPrint repository root.");
    }
}
