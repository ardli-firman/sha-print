using ShaPrint.WpfApp.ViewModels.Pages;
using ShaPrint.WpfApp.ViewModels.Windows;
using Wpf.Ui;
using Xunit;

namespace ShaPrint.Tests
{
    public class WelcomeViewModelTests
    {
        private WelcomeViewModel CreateViewModel()
        {
            // Set up a clean settings singleton just in case
            ShaPrint.WpfApp.Models.AppSettings.Current.NetworkChannel = "TestRoom";
            
            var mainWindowVm = new MainWindowViewModel();
            
            // Pass null for INavigationService as it's not invoked during validation/suggestions tests
            return new WelcomeViewModel(null!, mainWindowVm);
        }

        [Fact]
        public void ValidateChannel_ValidName_SetsNoError()
        {
            var vm = CreateViewModel();
            
            vm.ChannelName = "Room302";
            
            Assert.False(vm.HasValidationError);
            Assert.Empty(vm.ValidationError);
            Assert.True(vm.HasValidationInfo);
            Assert.Equal("✓ Valid channel name", vm.ValidationInfo);
        }

        [Fact]
        public void ValidateChannel_EmptyName_SetsError()
        {
            var vm = CreateViewModel();
            
            vm.ChannelName = "";
            
            Assert.True(vm.HasValidationError);
            Assert.Equal("Channel name is required", vm.ValidationError);
            Assert.False(vm.HasValidationInfo);
            Assert.Empty(vm.ValidationInfo);
        }

        [Fact]
        public void ValidateChannel_TooShort_SetsError()
        {
            var vm = CreateViewModel();
            
            vm.ChannelName = "ab";
            
            Assert.True(vm.HasValidationError);
            Assert.Equal("Channel name must be at least 3 characters", vm.ValidationError);
        }

        [Fact]
        public void ValidateChannel_TooLong_SetsError()
        {
            var vm = CreateViewModel();
            
            vm.ChannelName = new string('a', 51);
            
            Assert.True(vm.HasValidationError);
            Assert.Equal("Channel name is too long (max 50 characters)", vm.ValidationError);
        }

        [Fact]
        public void ValidateChannel_InvalidCharacters_SetsError()
        {
            var vm = CreateViewModel();
            
            vm.ChannelName = "Room@302";
            
            Assert.True(vm.HasValidationError);
            Assert.Equal("Format: Alphanumeric, dash, underscore only", vm.ValidationError);
        }

        [Fact]
        public void ValidateChannel_SpacesAutoFixed_SetsInfo()
        {
            var vm = CreateViewModel();
            
            vm.ChannelName = "Room 302";
            
            // Valid because spaces are removed to "Room302" which passes validation,
            // but sets a validation info explaining the change
            Assert.False(vm.HasValidationError);
            Assert.True(vm.HasValidationInfo);
            Assert.Equal("Spaces removed: 'Room 302' → 'Room302'", vm.ValidationInfo);
        }

        [Fact]
        public void DetectAndSuggestMode_MutuallyExclusiveSuggestions()
        {
            var vm = CreateViewModel();
            
            vm.DetectAndSuggestMode();
            
            // Assert that Server and Client suggestions are mutually exclusive
            Assert.NotEqual(vm.IsServerSuggested, vm.IsClientSuggested);
            
            if (vm.IsServerSuggested)
            {
                Assert.True(vm.DetectedPrinterCount > 0);
                Assert.Contains("printer(s) detected", vm.ServerHintText);
                Assert.Empty(vm.ClientHintText);
            }
            else
            {
                Assert.Equal(0, vm.DetectedPrinterCount);
                Assert.Contains("No local printers", vm.ClientHintText);
                Assert.Empty(vm.ServerHintText);
            }
        }
    }
}
