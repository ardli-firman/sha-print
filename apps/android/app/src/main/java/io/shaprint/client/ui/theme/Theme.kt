package io.shaprint.client.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext

private val DarkColorScheme = darkColorScheme(
    primary = ShaPrintPrimaryDark,
    onPrimary = ShaPrintOnPrimaryDark,
    primaryContainer = ShaPrintPrimaryContainerDark,
    onPrimaryContainer = ShaPrintOnPrimaryContainerDark,
    secondary = ShaPrintSecondaryDark,
    onSecondary = ShaPrintOnSecondaryDark,
    secondaryContainer = ShaPrintSecondaryContainerDark,
    onSecondaryContainer = ShaPrintOnSecondaryContainerDark,
    background = BackgroundDark,
    surface = SurfaceDark,
    surfaceVariant = SurfaceVariantDark,
    outline = OutlineDark,
    outlineVariant = OutlineVariantDark,
    error = ErrorRose,
    errorContainer = ErrorRoseContainer
)

private val LightColorScheme = lightColorScheme(
    primary = ShaPrintPrimaryLight,
    onPrimary = ShaPrintOnPrimaryLight,
    primaryContainer = ShaPrintPrimaryContainerLight,
    onPrimaryContainer = ShaPrintOnPrimaryContainerLight,
    secondary = ShaPrintSecondaryLight,
    onSecondary = ShaPrintOnSecondaryLight,
    secondaryContainer = ShaPrintSecondaryContainerLight,
    onSecondaryContainer = ShaPrintOnSecondaryContainerLight,
    background = BackgroundLight,
    surface = SurfaceLight,
    surfaceVariant = SurfaceVariantLight,
    outline = OutlineLight,
    outlineVariant = OutlineVariantLight,
    error = ErrorRose,
    errorContainer = ErrorRoseContainer
)

@Composable
fun ShaPrintTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = false, // Use ShaPrint brand identity by default for consistent desktop-client visual coherence
    content: @Composable () -> Unit
) {
    val colorScheme = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }
        darkTheme -> DarkColorScheme
        else -> LightColorScheme
    }

    MaterialTheme(
        colorScheme = colorScheme,
        typography = Typography,
        content = content
    )
}
