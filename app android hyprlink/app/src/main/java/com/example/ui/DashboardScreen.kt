package com.example.ui

import androidx.compose.animation.core.*
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.example.ui.theme.*

@Composable
fun DashboardScreen(
    linkState: LinkState,
    deviceName: String,
    onDeviceClick: () -> Unit,
    onStateButtonClick: () -> Unit,
    mediaTitle: String,
    mediaArtist: String,
    isPlaying: Boolean,
    onPrevClick: () -> Unit,
    onPlayPauseClick: () -> Unit,
    onNextClick: () -> Unit,
    progress: Float,
    elapsedTime: String,
    totalTime: String,
    clipboardContent: String,
    clipboardStateText: String,
    clipboardStateColor: Color,
    onSendClipboardClick: () -> Unit,
    onReceiveClipboardClick: () -> Unit,
    onSendFilesClick: () -> Unit,
    onWebcamClick: () -> Unit,
    webcamActive: Boolean,
    controlService: ServiceState = ServiceState.ACTIVE,
    webcamService: ServiceState = if (webcamActive) ServiceState.ACTIVE else ServiceState.INACTIVE,
    micBridgeService: ServiceState = ServiceState.ACTIVE,
    notifsService: ServiceState = ServiceState.ERROR,
    cpuUsage: Int,
    ramUsageGB: Int,
    tempCelsius: Int,
    batteryPercent: Int,
    onDetailClick: () -> Unit,
    terminalLogs: List<String>,
    modifier: Modifier = Modifier
) {
    val clipboardManager = LocalClipboardManager.current
    var selectedFilter by remember { mutableStateOf("TUDO") }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(HyprColors.Background)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp)
    ) {
        // 1. Cabeçalho colapsado (uma linha)
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = "HYPR",
                    fontFamily = ArchivoBlack,
                    fontWeight = FontWeight.Bold,
                    fontSize = 19.sp,
                    color = if (linkState == LinkState.DISCONNECTED) Color(0xFF5A5A5A) else HyprColors.TextTitle
                )
                Text(
                    text = "LINK",
                    fontFamily = ArchivoBlack,
                    fontWeight = FontWeight.Bold,
                    fontSize = 19.sp,
                    color = linkState.color,
                    modifier = Modifier.shadow(
                        elevation = 0.dp,
                        ambientColor = linkState.color.copy(alpha = 0.6f)
                    )
                )
            }

            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(7.dp)
            ) {
                BreathingDot(
                    color = linkState.color,
                    size = 7.dp,
                    periodMs = if (linkState == LinkState.DISCONNECTED) 1100 else 2800
                )
                Text(
                    text = linkState.label,
                    fontFamily = JetBrainsMono,
                    fontSize = 10.sp,
                    letterSpacing = 0.12.sp,
                    color = linkState.color,
                    fontWeight = FontWeight.Medium
                )
            }
        }

        // 2. Barra da estação
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(11.dp))
                .border(
                    width = 1.dp,
                    color = if (linkState == LinkState.DISCONNECTED) HyprColors.BorderRed else HyprColors.BorderElevated,
                    shape = RoundedCornerShape(11.dp)
                )
                .background(if (linkState == LinkState.DISCONNECTED) HyprColors.RedTintSurface else Color.Transparent)
                .clickable { onDeviceClick() }
                .padding(horizontal = 14.dp, vertical = 12.dp)
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    modifier = Modifier.weight(1f)
                ) {
                    BreathingDot(
                        color = linkState.color,
                        size = 8.dp,
                        periodMs = if (linkState == LinkState.DISCONNECTED) 1300 else 3200
                    )
                    Column {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                text = if (linkState == LinkState.DISCONNECTED) "CONECTAR AO LINUX" else deviceName,
                                fontFamily = JetBrainsMono,
                                fontSize = 14.sp,
                                fontWeight = FontWeight.Bold,
                                color = HyprColors.TextTitle
                            )
                            Spacer(modifier = Modifier.width(4.dp))
                            Text(
                                text = "▾",
                                fontSize = 12.sp,
                                color = HyprColors.TextCaption
                            )
                        }
                        Text(
                            text = if (linkState == LinkState.DISCONNECTED) {
                                "última: $deviceName · há 12 min"
                            } else {
                                "KDEC/1.6 · latência 4 ms · TLS emparelhado"
                            },
                            fontFamily = JetBrainsMono,
                            fontSize = 9.sp,
                            color = if (linkState == LinkState.DISCONNECTED) Color(0xFF8A5A60) else HyprColors.TextCaption,
                            modifier = Modifier.padding(top = 2.dp)
                        )
                    }
                }

                OutlinedButton(
                    onClick = onStateButtonClick,
                    modifier = Modifier.height(34.dp),
                    shape = RoundedCornerShape(8.dp),
                    border = BorderStroke(1.dp, linkState.color),
                    colors = ButtonDefaults.outlinedButtonColors(
                        contentColor = linkState.color,
                        containerColor = Color.Transparent
                    ),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 4.dp)
                ) {
                    Text(
                        text = linkState.buttonText,
                        fontFamily = JetBrainsMono,
                        fontSize = 10.sp,
                        fontWeight = FontWeight.Bold
                    )
                }
            }
        }

        // 3. Player de média (Cartão elevado)
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(16.dp))
                .background(HyprColors.SurfaceElevated)
                .border(1.dp, HyprColors.BorderElevated, RoundedCornerShape(16.dp))
                .padding(14.dp)
        ) {
            Column {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    // Capa quadrada 50dp com raio 12dp
                    Box(
                        modifier = Modifier
                            .size(50.dp)
                            .clip(RoundedCornerShape(12.dp))
                            .background(Color(0xFF141414))
                            .border(1.dp, HyprColors.BorderElevated, RoundedCornerShape(12.dp))
                    ) {
                        // Diagonal placeholder pattern
                        Canvas(modifier = Modifier.fillMaxSize()) {
                            val step = 10f
                            for (x in -size.height.toInt()..size.width.toInt() step step.toInt()) {
                                drawLine(
                                    color = Color(0xFF1E1E1E),
                                    start = androidx.compose.ui.geometry.Offset(x.toFloat(), 0f),
                                    end = androidx.compose.ui.geometry.Offset(x + size.height, size.height),
                                    strokeWidth = 2f
                                )
                            }
                        }
                    }

                    // Título e Artista
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            text = mediaTitle.ifEmpty { "Sem reprodução ativa" },
                            fontFamily = JetBrainsMono,
                            fontSize = 14.sp,
                            color = HyprColors.TextTitle,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                        Text(
                            text = if (mediaArtist.isNotEmpty()) "$mediaArtist · MPRIS" else "artista · aplicação · MPRIS",
                            fontFamily = JetBrainsMono,
                            fontSize = 9.sp,
                            color = HyprColors.TextCaption,
                            modifier = Modifier.padding(top = 2.dp)
                        )
                    }

                    // Botões de controlo
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        IconButton(
                            onClick = onPrevClick,
                            modifier = Modifier.size(32.dp)
                        ) {
                            Text("◀◀", fontSize = 13.sp, color = HyprColors.TextSecondary)
                        }

                        Box(
                            modifier = Modifier
                                .size(38.dp)
                                .clip(CircleShape)
                                .background(HyprColors.NeonGreen)
                                .clickable { onPlayPauseClick() },
                            contentAlignment = Alignment.Center
                        ) {
                            Text(
                                text = if (isPlaying) "❙❙" else "▶",
                                fontSize = 12.sp,
                                color = Color.Black,
                                fontWeight = FontWeight.Bold
                            )
                        }

                        IconButton(
                            onClick = onNextClick,
                            modifier = Modifier.size(32.dp)
                        ) {
                            Text("▶▶", fontSize = 13.sp, color = HyprColors.TextSecondary)
                        }
                    }
                }

                // Barra de progresso de 3dp
                Spacer(modifier = Modifier.height(12.dp))
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(3.dp)
                        .clip(RoundedCornerShape(3.dp))
                        .background(HyprColors.BorderNormal)
                ) {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth(fraction = progress.coerceIn(0f, 1f))
                            .fillMaxHeight()
                            .background(HyprColors.NeonGreen)
                    )
                }

                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = 5.dp),
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    Text(text = elapsedTime, fontSize = 9.sp, color = HyprColors.TextCaption, fontFamily = JetBrainsMono)
                    Text(text = totalTime, fontSize = 9.sp, color = HyprColors.TextCaption, fontFamily = JetBrainsMono)
                }
            }
        }

        // 4. Secção "AQUI MESMO"
        Text(
            text = "AQUI MESMO",
            fontFamily = JetBrainsMono,
            fontSize = 9.sp,
            letterSpacing = 0.16.sp,
            color = HyprColors.TextCaption,
            fontWeight = FontWeight.Bold
        )

        // Cartão de clipboard
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(14.dp))
                .background(HyprColors.SurfaceCard)
                .border(1.dp, HyprColors.BorderNormal, RoundedCornerShape(14.dp))
                .padding(12.dp)
        ) {
            Column {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "CLIPBOARD",
                        fontFamily = JetBrainsMono,
                        fontSize = 9.sp,
                        letterSpacing = 0.16.sp,
                        color = HyprColors.TextCaption,
                        fontWeight = FontWeight.Bold
                    )
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(5.dp)
                    ) {
                        BreathingDot(color = clipboardStateColor, size = 6.dp, periodMs = 1500)
                        Text(
                            text = clipboardStateText,
                            fontFamily = JetBrainsMono,
                            fontSize = 9.sp,
                            color = clipboardStateColor
                        )
                    }
                }

                Spacer(modifier = Modifier.height(8.dp))
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(9.dp))
                        .background(HyprColors.ClipboardBlock)
                        .border(1.dp, HyprColors.BorderNormal, RoundedCornerShape(9.dp))
                        .padding(horizontal = 10.dp, vertical = 8.dp)
                ) {
                    Text(
                        text = clipboardContent.ifEmpty { "(Área de transferência vazia)" },
                        fontFamily = JetBrainsMono,
                        fontSize = 11.sp,
                        color = HyprColors.TextSecondary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                }

                Spacer(modifier = Modifier.height(10.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Button(
                        onClick = onSendClipboardClick,
                        modifier = Modifier
                            .weight(1f)
                            .height(44.dp),
                        shape = RoundedCornerShape(9.dp),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = HyprColors.NeonGreen,
                            contentColor = HyprColors.OnNeonGreen
                        )
                    ) {
                        Text(
                            text = "ENVIAR AO PC ↑",
                            fontFamily = JetBrainsMono,
                            fontSize = 11.sp,
                            fontWeight = FontWeight.Bold
                        )
                    }

                    Box(
                        modifier = Modifier
                            .size(44.dp)
                            .clip(RoundedCornerShape(9.dp))
                            .border(1.dp, HyprColors.BorderHighlight, RoundedCornerShape(9.dp))
                            .clickable { onReceiveClipboardClick() },
                        contentAlignment = Alignment.Center
                    ) {
                        Text(
                            text = "↓",
                            fontFamily = JetBrainsMono,
                            fontSize = 14.sp,
                            color = HyprColors.TextSecondary
                        )
                    }
                }
            }
        }

        // Par de botões compactos (Enviar e Webcam)
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(10.dp)
        ) {
            Box(
                modifier = Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(12.dp))
                    .background(HyprColors.SurfaceCard)
                    .border(1.dp, HyprColors.BorderNormal, RoundedCornerShape(12.dp))
                    .clickable { onSendFilesClick() }
                    .padding(12.dp)
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp)
                ) {
                    Box(
                        modifier = Modifier
                            .size(30.dp)
                            .clip(RoundedCornerShape(8.dp))
                            .border(1.dp, HyprColors.BorderHighlight, RoundedCornerShape(8.dp)),
                        contentAlignment = Alignment.Center
                    ) {
                        Text("↑", fontSize = 13.sp, color = HyprColors.TextSecondary)
                    }
                    Column {
                        Text("Enviar", fontFamily = JetBrainsMono, fontSize = 12.sp, color = HyprColors.TextBody)
                        Text("3 hoje", fontFamily = JetBrainsMono, fontSize = 9.sp, color = HyprColors.TextCaption)
                    }
                }
            }

            Box(
                modifier = Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(12.dp))
                    .background(HyprColors.SurfaceCard)
                    .border(1.dp, HyprColors.BorderNormal, RoundedCornerShape(12.dp))
                    .clickable { onWebcamClick() }
                    .padding(12.dp)
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp)
                ) {
                    Box(
                        modifier = Modifier
                            .size(30.dp)
                            .clip(RoundedCornerShape(8.dp))
                            .border(1.dp, HyprColors.BorderHighlight, RoundedCornerShape(8.dp)),
                        contentAlignment = Alignment.Center
                    ) {
                        Text("◉", fontSize = 13.sp, color = if (webcamActive) HyprColors.NeonGreen else HyprColors.TextSecondary)
                    }
                    Column {
                        Text("Webcam", fontFamily = JetBrainsMono, fontSize = 12.sp, color = HyprColors.TextBody)
                        Text(
                            text = if (webcamActive) "ativa" else "inativa",
                            fontFamily = JetBrainsMono,
                            fontSize = 9.sp,
                            color = if (webcamActive) HyprColors.NeonGreen else HyprColors.TextCaption
                        )
                    }
                }
            }
        }

        // 5. Secção "SERVIÇOS"
        Text(
            text = "SERVIÇOS",
            fontFamily = JetBrainsMono,
            fontSize = 9.sp,
            letterSpacing = 0.16.sp,
            color = HyprColors.TextCaption,
            fontWeight = FontWeight.Bold
        )

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            ServiceItem(title = "CONTROL", state = controlService, modifier = Modifier.weight(1f))
            ServiceItem(title = "WEBCAM", state = webcamService, modifier = Modifier.weight(1f))
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            ServiceItem(title = "MIC BRIDGE", state = micBridgeService, modifier = Modifier.weight(1f))
            ServiceItem(title = "NOTIFS", state = notifsService, modifier = Modifier.weight(1f))
        }

        // 6. Secção "TELEMETRIA DO PC"
        Text(
            text = "TELEMETRIA DO PC",
            fontFamily = JetBrainsMono,
            fontSize = 9.sp,
            letterSpacing = 0.16.sp,
            color = HyprColors.TextCaption,
            fontWeight = FontWeight.Bold
        )

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            val connected = linkState == LinkState.CONNECTED
            TelemetryBox(label = "CPU", value = if (connected) "$cpuUsage%" else "--", progress = if (connected) cpuUsage / 100f else 0f, color = HyprColors.NeonGreen, modifier = Modifier.weight(1f))
            TelemetryBox(label = "RAM", value = if (connected) "${ramUsageGB}G" else "--", progress = if (connected) 0.35f else 0f, color = HyprColors.NeonGreen, modifier = Modifier.weight(1f))
            TelemetryBox(label = "TEMP", value = if (connected) "$tempCelsius°" else "--", progress = if (connected) (tempCelsius / 100f).coerceIn(0f, 1f) else 0f, color = HyprColors.Amber, modifier = Modifier.weight(1f))
            TelemetryBox(label = "BAT", value = if (connected) "$batteryPercent%" else "--", progress = if (connected) batteryPercent / 100f else 0f, color = HyprColors.NeonGreen, modifier = Modifier.weight(1f))

            // Botão Detalhe (fator 0.72)
            Box(
                modifier = Modifier
                    .weight(0.72f)
                    .height(58.dp)
                    .clip(RoundedCornerShape(11.dp))
                    .border(1.dp, HyprColors.BorderHighlight, RoundedCornerShape(11.dp))
                    .clickable { onDetailClick() },
                contentAlignment = Alignment.Center
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center
                ) {
                    Text("＋", fontSize = 14.sp, color = HyprColors.TextSecondary)
                    Text("DETALHE", fontFamily = JetBrainsMono, fontSize = 7.sp, color = HyprColors.TextCaption, letterSpacing = 0.08.sp)
                }
            }
        }

        // 7. Terminal (nunca comprimido)
        TerminalSection(
            linkState = linkState,
            selectedFilter = selectedFilter,
            onFilterSelect = { selectedFilter = it },
            logs = terminalLogs,
            onCopyLogs = {
                clipboardManager.setText(AnnotatedString(terminalLogs.joinToString("\n")))
            }
        )

        Spacer(modifier = Modifier.height(16.dp))
    }
}

@Composable
private fun ServiceItem(
    title: String,
    state: ServiceState,
    modifier: Modifier = Modifier
) {
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(10.dp))
            .background(HyprColors.SurfaceCard)
            .border(
                1.dp,
                if (state == ServiceState.ERROR) HyprColors.BorderRed else HyprColors.BorderNormal,
                RoundedCornerShape(10.dp)
            )
            .padding(horizontal = 10.dp, vertical = 9.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(7.dp)
            ) {
                BreathingDot(color = state.color, size = 6.dp, periodMs = if (state == ServiceState.ERROR) 1200 else 2400)
                Text(
                    text = title,
                    fontFamily = JetBrainsMono,
                    fontSize = 11.sp,
                    color = HyprColors.TextBody
                )
            }
            Text(
                text = state.label,
                fontFamily = JetBrainsMono,
                fontSize = 9.sp,
                color = state.color
            )
        }
    }
}

@Composable
private fun TelemetryBox(
    label: String,
    value: String,
    progress: Float,
    color: Color,
    modifier: Modifier = Modifier
) {
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(11.dp))
            .background(HyprColors.SurfaceCard)
            .border(1.dp, HyprColors.BorderNormal, RoundedCornerShape(11.dp))
            .padding(horizontal = 8.dp, vertical = 9.dp)
    ) {
        Column {
            Text(text = label, fontFamily = JetBrainsMono, fontSize = 8.sp, color = HyprColors.TextCaption)
            Text(
                text = value,
                fontFamily = JetBrainsMono,
                fontSize = 14.sp,
                color = if (value == "--") HyprColors.TextCaption else color,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(vertical = 2.dp)
            )
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(2.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(HyprColors.BorderNormal)
            ) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth(progress.coerceIn(0f, 1f))
                        .fillMaxHeight()
                        .background(color)
                )
            }
        }
    }
}

@Composable
private fun TerminalSection(
    linkState: LinkState,
    selectedFilter: String,
    onFilterSelect: (String) -> Unit,
    logs: List<String>,
    onCopyLogs: () -> Unit
) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(HyprColors.TerminalSurface)
            .border(1.dp, HyprColors.BorderNormal, RoundedCornerShape(14.dp))
    ) {
        Column {
            // Header
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(width = 0.5.dp, color = Color(0xFF161616))
                    .padding(horizontal = 12.dp, vertical = 10.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    Text(
                        text = if (linkState == LinkState.CONNECTED) "TERMINAL · SESSÃO 001" else "TERMINAL · À PROCURA",
                        fontFamily = JetBrainsMono,
                        fontSize = 9.sp,
                        letterSpacing = 0.14.sp,
                        color = HyprColors.TextSecondary
                    )
                    if (linkState != LinkState.CONNECTED) {
                        BreathingDot(color = HyprColors.Amber, size = 5.dp, periodMs = 1400)
                        Text(
                            text = "scan",
                            fontFamily = JetBrainsMono,
                            fontSize = 8.sp,
                            color = HyprColors.Amber
                        )
                    }
                }

                Row(
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "copiar ⧉",
                        fontFamily = JetBrainsMono,
                        fontSize = 9.sp,
                        color = HyprColors.TextCaption,
                        modifier = Modifier.clickable { onCopyLogs() }
                    )
                    Text(
                        text = "☰",
                        fontFamily = JetBrainsMono,
                        fontSize = 11.sp,
                        color = HyprColors.TextCaption
                    )
                }
            }

            // Filtros
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(width = 0.5.dp, color = Color(0xFF161616))
                    .padding(horizontal = 12.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp)
            ) {
                listOf("TUDO", "TX/RX", "AVISOS", "ERROS").forEach { filter ->
                    val isSelected = selectedFilter == filter
                    Box(
                        modifier = Modifier
                            .clip(RoundedCornerShape(6.dp))
                            .background(if (isSelected) HyprColors.NeonGreen else Color.Transparent)
                            .border(
                                1.dp,
                                if (isSelected) HyprColors.NeonGreen else HyprColors.BorderHighlight,
                                RoundedCornerShape(6.dp)
                            )
                            .clickable { onFilterSelect(filter) }
                            .padding(horizontal = 8.dp, vertical = 3.dp)
                    ) {
                        Text(
                            text = filter,
                            fontFamily = JetBrainsMono,
                            fontSize = 8.sp,
                            fontWeight = FontWeight.Bold,
                            color = if (isSelected) HyprColors.OnNeonGreen else HyprColors.TextSecondary
                        )
                    }
                }
            }

            // Linhas de log
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 12.dp, vertical = 10.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp)
            ) {
                val filteredLogs = logs.filter { line ->
                    when (selectedFilter) {
                        "TX/RX" -> line.contains("tx", ignoreCase = true) || line.contains("rx", ignoreCase = true)
                        "AVISOS" -> line.contains("warn", ignoreCase = true)
                        "ERROS" -> line.contains("erro", ignoreCase = true) || line.contains("fail", ignoreCase = true)
                        else -> true
                    }
                }.takeLast(14)

                if (filteredLogs.isEmpty()) {
                    Text(
                        text = "03:04:40  mdns   a anunciar _hyprlink._tcp",
                        fontFamily = JetBrainsMono,
                        fontSize = 9.sp,
                        color = HyprColors.TextLog
                    )
                } else {
                    filteredLogs.forEach { logLine ->
                        TerminalLine(logLine)
                    }
                }
            }
        }
    }
}

@Composable
private fun TerminalLine(line: String) {
    val levelColor = when {
        line.contains("erro", ignoreCase = true) || line.contains("fail", ignoreCase = true) -> HyprColors.Red
        line.contains("warn", ignoreCase = true) || line.contains("retry", ignoreCase = true) -> HyprColors.Amber
        line.contains("ok", ignoreCase = true) || line.contains("link", ignoreCase = true) || line.contains("tx", ignoreCase = true) -> HyprColors.NeonGreen
        else -> HyprColors.TextCaption
    }

    Row(modifier = Modifier.fillMaxWidth()) {
        Text(
            text = line,
            fontFamily = JetBrainsMono,
            fontSize = 9.sp,
            color = levelColor
        )
    }
}
