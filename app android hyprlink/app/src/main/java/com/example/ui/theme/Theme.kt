package com.example.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable

// Compatibility aliases for any legacy references
val DarkBackground = HyprColors.Background
val DarkSurface = HyprColors.SurfaceCard
val DarkOutline = HyprColors.BorderNormal
val PrimaryPurple = HyprColors.NeonGreen
val PrimaryViolet = HyprColors.BorderHighlight
val CyanActive = HyprColors.NeonGreen
val AmberWarning = HyprColors.Amber
val RedError = HyprColors.Red
val TextPrimary = HyprColors.TextTitle
val TextSecondary = HyprColors.TextSecondary
val TerminalBlack = HyprColors.TerminalSurface

private val DarkColorScheme = darkColorScheme(
    primary = HyprColors.NeonGreen,
    onPrimary = HyprColors.OnNeonGreen,
    primaryContainer = HyprColors.SurfaceElevated,
    secondary = HyprColors.NeonGreen,
    tertiary = HyprColors.Amber,
    error = HyprColors.Red,
    background = HyprColors.Background,
    surface = HyprColors.SurfaceCard,
    outline = HyprColors.BorderNormal,
    onBackground = HyprColors.TextBody,
    onSurface = HyprColors.TextBody,
    onSurfaceVariant = HyprColors.TextSecondary
)

@Composable
fun MyApplicationTheme(
    content: @Composable () -> Unit
) {
    MaterialTheme(
        colorScheme = DarkColorScheme,
        typography = Typography,
        content = content
    )
}
