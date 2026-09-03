package com.example.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.example.ui.theme.*

data class AudioSinkItem(
    val id: Long,
    val name: String,
    val description: String,
    val volume: Int, // 0 to 150
    val isMuted: Boolean,
    val isDefault: Boolean,
    val isPhone: Boolean = false
)

data class AudioAppItem(
    val id: Long,
    val name: String,
    val media: String,
    val volume: Int,
    val isMuted: Boolean,
    val isPlaying: Boolean = true,
    val idleMinutes: Int = 0
)

@Composable
fun AudioScreen(
    onBack: () -> Unit,
    mediaAppSource: String,
    mediaTitle: String,
    mediaArtist: String,
    elapsedTime: String,
    totalTime: String,
    isPlaying: Boolean,
    onPrevClick: () -> Unit,
    onPlayPauseClick: () -> Unit,
    onNextClick: () -> Unit,
    isListenOnPhone: Boolean,
    onToggleListenOnPhone: (Boolean) -> Unit,
    sinks: List<AudioSinkItem>,
    onSelectDefaultSink: (String) -> Unit,
    onSinkVolumeChange: (Long, Int) -> Unit,
    onSinkMuteToggle: (Long, Boolean) -> Unit,
    apps: List<AudioAppItem>,
    onAppVolumeChange: (Long, Int) -> Unit,
    onAppMuteToggle: (Long, Boolean) -> Unit,
    modifier: Modifier = Modifier
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .background(HyprColors.Background)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        // Cabeçalho com seta de voltar e o título "ÁUDIO" (19sp Archivo)
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            Box(
                modifier = Modifier
                    .size(36.dp)
                    .clip(CircleShape)
                    .border(1.dp, HyprColors.BorderNormal, CircleShape)
                    .clickable { onBack() },
                contentAlignment = Alignment.Center
            ) {
                Text("←", fontSize = 16.sp, color = HyprColors.TextBody, fontWeight = FontWeight.Bold)
            }

            Text(
                text = "ÁUDIO",
                fontFamily = ArchivoBlack,
                fontWeight = FontWeight.Bold,
                fontSize = 19.sp,
                color = HyprColors.TextTitle
            )
        }

        // 1. Controlo de média
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
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "CONTROLO DE MÉDIA",
                        fontFamily = JetBrainsMono,
                        fontSize = 9.sp,
                        letterSpacing = 0.16.sp,
                        color = HyprColors.TextCaption,
                        fontWeight = FontWeight.Bold
                    )
                    Text(
                        text = mediaAppSource.ifEmpty { "Spotify" },
                        fontFamily = JetBrainsMono,
                        fontSize = 10.sp,
                        color = HyprColors.NeonGreen,
                        fontWeight = FontWeight.Bold
                    )
                }

                Spacer(modifier = Modifier.height(12.dp))

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    // Capa quadrada 50dp
                    Box(
                        modifier = Modifier
                            .size(50.dp)
                            .clip(RoundedCornerShape(12.dp))
                            .background(Color(0xFF141414))
                            .border(1.dp, HyprColors.BorderElevated, RoundedCornerShape(12.dp))
                    ) {
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

                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            text = mediaTitle.ifEmpty { "Sem reprodução" },
                            fontFamily = JetBrainsMono,
                            fontSize = 14.sp,
                            color = HyprColors.TextTitle,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                        Text(
                            text = if (mediaArtist.isNotEmpty()) "$mediaArtist · $elapsedTime / $totalTime" else "artista · $elapsedTime / $totalTime",
                            fontFamily = JetBrainsMono,
                            fontSize = 9.sp,
                            color = HyprColors.TextCaption,
                            modifier = Modifier.padding(top = 2.dp)
                        )
                    }

                    // Trio de botões
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        IconButton(onClick = onPrevClick, modifier = Modifier.size(32.dp)) {
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
                        IconButton(onClick = onNextClick, modifier = Modifier.size(32.dp)) {
                            Text("▶▶", fontSize = 13.sp, color = HyprColors.TextSecondary)
                        }
                    }
                }
            }
        }

        // 2. Ouvir no telemóvel
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(14.dp))
                .background(HyprColors.SurfaceCard)
                .border(1.dp, HyprColors.BorderNormal, RoundedCornerShape(14.dp))
                .padding(horizontal = 16.dp, vertical = 14.dp)
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = "Ouvir no telemóvel",
                        fontFamily = ArchivoBlack,
                        fontWeight = FontWeight.Bold,
                        fontSize = 14.sp,
                        color = HyprColors.TextTitle
                    )
                    Text(
                        text = "Toca o áudio do PC diretamente aqui",
                        fontFamily = JetBrainsMono,
                        fontSize = 10.sp,
                        color = HyprColors.TextCaption,
                        modifier = Modifier.padding(top = 2.dp)
                    )
                }

                Switch(
                    checked = isListenOnPhone,
                    onCheckedChange = { onToggleListenOnPhone(it) },
                    colors = SwitchDefaults.colors(
                        checkedThumbColor = Color.Black,
                        checkedTrackColor = HyprColors.NeonGreen,
                        uncheckedThumbColor = HyprColors.InactiveGray,
                        uncheckedTrackColor = Color(0xFF141414)
                    )
                )
            }
        }

        // 3. Secção "SAÍDA DE SOM"
        Text(
            text = "SAÍDA DE SOM",
            fontFamily = JetBrainsMono,
            fontSize = 9.sp,
            letterSpacing = 0.16.sp,
            color = HyprColors.TextCaption,
            fontWeight = FontWeight.Bold
        )

        val defaultSinks = if (sinks.isEmpty()) {
            listOf(
                AudioSinkItem(1, "alsa_output.pci-0000_03_00.6.analog-stereo", "Ryzen HD Audio", 74, false, true),
                AudioSinkItem(2, "bluez_output.84.1", "Headphones Bluetooth", 50, false, false),
                AudioSinkItem(9000, "hyprlink-speaker", "HyprLink-Phone", 100, false, false, true)
            )
        } else sinks

        defaultSinks.forEach { sink ->
            val isActive = sink.isDefault
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(14.dp))
                    .background(if (isActive) HyprColors.GreenTintSurface else HyprColors.SurfaceCard)
                    .border(
                        1.dp,
                        if (isActive) HyprColors.NeonGreen else HyprColors.BorderNormal,
                        RoundedCornerShape(14.dp)
                    )
                    .clickable { onSelectDefaultSink(sink.name) }
                    .padding(14.dp)
            ) {
                Column {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(10.dp)
                        ) {
                            // Radio button
                            Box(
                                modifier = Modifier
                                    .size(16.dp)
                                    .clip(CircleShape)
                                    .border(1.5.dp, if (isActive) HyprColors.NeonGreen else HyprColors.TextCaption, CircleShape),
                                contentAlignment = Alignment.Center
                            ) {
                                if (isActive) {
                                    Box(modifier = Modifier.size(8.dp).background(HyprColors.NeonGreen, CircleShape))
                                }
                            }

                            Text(
                                text = if (sink.isPhone) "📱 Este telemóvel" else sink.description,
                                fontFamily = ArchivoBlack,
                                fontWeight = FontWeight.Bold,
                                fontSize = 14.sp,
                                color = if (isActive) HyprColors.NeonGreen else HyprColors.TextTitle
                            )
                        }

                        if (isActive) {
                            Text(
                                text = "ATIVO",
                                fontFamily = JetBrainsMono,
                                fontSize = 9.sp,
                                letterSpacing = 0.1.sp,
                                color = HyprColors.NeonGreen,
                                fontWeight = FontWeight.Bold
                            )
                        }
                    }

                    // Slider customizado
                    AudioCustomSlider(
                        volume = sink.volume,
                        isMuted = sink.isMuted,
                        onVolumeChange = { onSinkVolumeChange(sink.id, it) },
                        onMuteToggle = { onSinkMuteToggle(sink.id, !sink.isMuted) },
                        showScaleMarks = isActive
                    )
                }
            }
        }

        // 4. Secção "APLICAÇÕES"
        Text(
            text = "APLICAÇÕES",
            fontFamily = JetBrainsMono,
            fontSize = 9.sp,
            letterSpacing = 0.16.sp,
            color = HyprColors.TextCaption,
            fontWeight = FontWeight.Bold
        )

        val defaultApps = if (apps.isEmpty()) {
            listOf(
                AudioAppItem(101, "Spotify", "Spotify Music Player", 69, false, true),
                AudioAppItem(102, "Firefox", "YouTube - Cyberpunk Radio", 90, false, true),
                AudioAppItem(103, "Discord", "Voice Chat", 0, true, false, 3)
            )
        } else apps

        defaultApps.forEach { app ->
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(14.dp))
                    .background(HyprColors.SurfaceCard)
                    .border(1.dp, HyprColors.BorderNormal, RoundedCornerShape(14.dp))
                    .padding(14.dp)
            ) {
                Column {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Column {
                            Text(
                                text = app.name,
                                fontFamily = ArchivoBlack,
                                fontWeight = FontWeight.Bold,
                                fontSize = 14.sp,
                                color = HyprColors.TextTitle
                            )
                            if (!app.isPlaying && app.idleMinutes > 0) {
                                Text(
                                    text = "sem som há ${app.idleMinutes} min",
                                    fontFamily = JetBrainsMono,
                                    fontSize = 9.sp,
                                    color = HyprColors.TextCaption,
                                    modifier = Modifier.padding(top = 2.dp)
                                )
                            }
                        }

                        if (app.isPlaying) {
                            Text(
                                text = "a reproduzir",
                                fontFamily = JetBrainsMono,
                                fontSize = 9.sp,
                                color = HyprColors.NeonGreen,
                                fontWeight = FontWeight.Bold
                            )
                        } else {
                            OutlinedButton(
                                onClick = { onAppMuteToggle(app.id, !app.isMuted) },
                                shape = RoundedCornerShape(8.dp),
                                border = BorderStroke(1.dp, HyprColors.BorderHighlight),
                                colors = ButtonDefaults.outlinedButtonColors(contentColor = HyprColors.TextSecondary),
                                contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                                modifier = Modifier.height(28.dp)
                            ) {
                                Text("MUDO", fontFamily = JetBrainsMono, fontSize = 9.sp)
                            }
                        }
                    }

                    if (app.isPlaying) {
                        AudioCustomSlider(
                            volume = app.volume,
                            isMuted = app.isMuted,
                            onVolumeChange = { onAppVolumeChange(app.id, it) },
                            onMuteToggle = { onAppMuteToggle(app.id, !app.isMuted) },
                            showScaleMarks = false
                        )
                    }
                }
            }
        }

        Spacer(modifier = Modifier.height(16.dp))
    }
}

@Composable
fun AudioCustomSlider(
    volume: Int, // 0 to 150
    isMuted: Boolean,
    onVolumeChange: (Int) -> Unit,
    onMuteToggle: () -> Unit,
    showScaleMarks: Boolean = false
) {
    var sliderVal by remember(volume) { mutableFloatStateOf(volume.toFloat()) }

    Column(modifier = Modifier.padding(top = 10.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(11.dp)
        ) {
            // Botão de mudo 30dp com borda e glifo "◀))"
            Box(
                modifier = Modifier
                    .size(30.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .border(
                        1.dp,
                        if (isMuted) HyprColors.BorderRed else HyprColors.BorderTouchpad,
                        RoundedCornerShape(8.dp)
                    )
                    .clickable { onMuteToggle() },
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = "◀))",
                    fontSize = 11.sp,
                    color = if (isMuted) HyprColors.Red else HyprColors.NeonGreen
                )
            }

            // Track do slider
            Box(
                modifier = Modifier
                    .weight(1f)
                    .height(22.dp)
                    .pointerInput(Unit) {
                        detectDragGestures { change, _ ->
                            val width = size.width
                            val x = change.position.x.coerceIn(0f, width.toFloat())
                            val pct = (x / width) * 150f
                            sliderVal = pct
                            onVolumeChange(pct.toInt())
                        }
                    },
                contentAlignment = Alignment.CenterStart
            ) {
                // Background Track
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(5.dp)
                        .clip(RoundedCornerShape(5.dp))
                        .background(Color(0xFF141414))
                ) {
                    Row(modifier = Modifier.fillMaxSize()) {
                        // Segmento 0 a 100% ocupa 2/3 da largura
                        Box(
                            modifier = Modifier
                                .weight(2f)
                                .fillMaxHeight()
                                .background(Color(0xFF141414))
                        ) {
                            val greenFraction = (sliderVal.coerceIn(0f, 100f) / 100f)
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth(fraction = greenFraction)
                                    .fillMaxHeight()
                                    .background(HyprColors.NeonGreen)
                            )
                        }

                        // Segmento 100 a 150% ocupa 1/3 com fundo rgba(255,176,32,.16)
                        Box(
                            modifier = Modifier
                                .weight(1f)
                                .fillMaxHeight()
                                .background(Color(0x28FFB020))
                        ) {
                            if (sliderVal > 100f) {
                                val amberFraction = ((sliderVal - 100f) / 50f).coerceIn(0f, 1f)
                                Box(
                                    modifier = Modifier
                                        .fillMaxWidth(fraction = amberFraction)
                                        .fillMaxHeight()
                                        .background(HyprColors.Amber)
                                )
                            }
                        }
                    }
                }

                // Entalhe vertical de 1dp e 18dp de altura em #4E4E4E nos 100% (posição 2/3)
                BoxWithConstraints(modifier = Modifier.fillMaxWidth()) {
                    val twoThirdsPos = maxWidth * 0.666f
                    Box(
                        modifier = Modifier
                            .offset(x = twoThirdsPos)
                            .width(1.dp)
                            .height(18.dp)
                            .background(HyprColors.TextLog)
                    )

                    // Thumb: círculo de 12dp em verde com sombra de brilho
                    val thumbFraction = (sliderVal / 150f).coerceIn(0f, 1f)
                    val thumbX = maxWidth * thumbFraction - 6.dp
                    Box(
                        modifier = Modifier
                            .offset(x = thumbX)
                            .size(12.dp)
                            .shadow(8.dp, CircleShape, ambientColor = HyprColors.NeonGreen, spotColor = HyprColors.NeonGreen)
                            .background(HyprColors.NeonGreen, CircleShape)
                    )
                }
            }

            // Valor em percentagem (largura fixa 38dp, alinhado à direita)
            Text(
                text = "${sliderVal.toInt()}%",
                fontFamily = JetBrainsMono,
                fontSize = 12.sp,
                color = HyprColors.TextBody,
                modifier = Modifier.width(38.dp),
                textAlign = TextAlign.End
            )
        }

        if (showScaleMarks) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(start = 41.dp, end = 38.dp, top = 4.dp),
                horizontalArrangement = Arrangement.SpaceBetween
            ) {
                Text("0", fontSize = 8.sp, color = HyprColors.TextLog, fontFamily = JetBrainsMono)
                Text("100 · seguro", fontSize = 8.sp, color = HyprColors.TextLog, fontFamily = JetBrainsMono)
                Text("150", fontSize = 8.sp, color = HyprColors.TextLog, fontFamily = JetBrainsMono)
            }
        }
    }
}
