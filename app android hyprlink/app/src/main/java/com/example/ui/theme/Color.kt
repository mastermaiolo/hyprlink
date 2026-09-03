package com.example.ui.theme

import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

object HyprColors {
    // Surfaces
    val Background = Color(0xFF000000)
    val SurfaceCard = Color(0xFF0B0B0B)
    val SurfaceElevated = Color(0xFF0D0D0D)
    val TerminalSurface = Color(0xFF070707)
    val ClipboardBlock = Color(0xFF060606)

    // Borders
    val BorderNormal = Color(0xFF1C1C1C)
    val BorderElevated = Color(0xFF1E1E1E)
    val BorderHighlight = Color(0xFF262626)
    val BorderTouchpad = Color(0xFF1E3E30)
    val BorderAmber = Color(0xFF3A2C12)
    val BorderRed = Color(0xFF2A1418)

    // Status colors
    val NeonGreen = Color(0xFF3DFF9E)
    val Amber = Color(0xFFFFB020)
    val Red = Color(0xFFFF4757)
    val InactiveGray = Color(0xFF5E5E5E)
    val SubInactiveGray = Color(0xFF6E6E6E)

    // Accented surface tints
    val GreenTintSurface = Color(0xFF071510)
    val AmberTintSurface = Color(0xFF150F03)
    val RedTintSurface = Color(0xFF150809)
    val RedTintBlock = Color(0xFF120609)

    // Text colors
    val TextTitle = Color(0xFFFFFFFF)
    val TextBody = Color(0xFFE4E4E4)
    val TextSecondary = Color(0xFFB4B4B4)
    val TextCaption = Color(0xFF6E6E6E)
    val TextLog = Color(0xFF4E4E4E)
    val TextInstruction = Color(0xFF3A3A3A)

    // Button contrast
    val OnNeonGreen = Color(0xFF001A0E)
}

enum class LinkState {
    DISCONNECTED,
    NEGOTIATING,
    CONNECTED;

    val color: Color
        get() = when (this) {
            CONNECTED -> HyprColors.NeonGreen
            NEGOTIATING -> HyprColors.Amber
            DISCONNECTED -> HyprColors.Red
        }

    val label: String
        get() = when (this) {
            CONNECTED -> "LINK ATIVO"
            NEGOTIATING -> "A NEGOCIAR TLS"
            DISCONNECTED -> "SEM LIGAÇÃO"
        }

    val buttonText: String
        get() = when (this) {
            CONNECTED -> "ON"
            NEGOTIATING -> "···"
            DISCONNECTED -> "LIGAR"
        }
}

enum class ServiceState {
    ACTIVE,
    PENDING,
    INACTIVE,
    ERROR;

    val color: Color
        get() = when (this) {
            ACTIVE -> HyprColors.NeonGreen
            PENDING -> HyprColors.Amber
            INACTIVE -> HyprColors.InactiveGray
            ERROR -> HyprColors.Red
        }

    val label: String
        get() = when (this) {
            ACTIVE -> "ativo"
            PENDING -> "pendente"
            INACTIVE -> "inativo"
            ERROR -> "sem permissão"
        }
}

@Composable
fun BreathingDot(
    color: Color,
    modifier: Modifier = Modifier,
    size: Dp = 8.dp,
    periodMs: Int = if (color == HyprColors.Red) 1200 else 2800
) {
    val infiniteTransition = rememberInfiniteTransition(label = "BreathingDotTransition")

    val scale by infiniteTransition.animateFloat(
        initialValue = 1.0f,
        targetValue = 1.55f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = periodMs / 2, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse
        ),
        label = "BreathingScale"
    )

    val opacity by infiniteTransition.animateFloat(
        initialValue = 0.45f,
        targetValue = 1.0f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = periodMs / 2, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse
        ),
        label = "BreathingOpacity"
    )

    Box(
        modifier = modifier
            .size(size)
            .scale(scale)
            .background(color.copy(alpha = opacity), CircleShape)
    )
}
