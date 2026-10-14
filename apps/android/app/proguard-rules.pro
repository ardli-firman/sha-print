# ShaPrint Android Client ProGuard / R8 Rules

# Keep ShaPrint domain models and data structures
-keep class io.shaprint.client.core.domain.model.** { *; }
-keep class io.shaprint.client.core.ipp.** { *; }
-keep class io.shaprint.client.core.network.** { *; }
-keep class io.shaprint.client.core.security.** { *; }
-keep class io.shaprint.client.core.raster.** { *; }

# Keep system services and components
-keep class io.shaprint.client.features.printservice.** { *; }
-keep class io.shaprint.client.features.print.PrintJobForegroundService { *; }

# Keep AndroidX Navigation & Compose arguments
-keepclassmembers class * extends androidx.lifecycle.ViewModel {
    <init>(...);
}

# Preserve line numbers and source file names for Play Console crash deobfuscation
-renamesourcefileattribute SourceFile
-keepattributes SourceFile,LineNumberTable,Signature,InnerClasses,EnclosingMethod
