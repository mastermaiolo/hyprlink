package com.example.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.example.ui.theme.*

@Composable
fun TouchpadScreen(
    deviceName: String,
    onMouseMove: (Float, Float) -> Unit,
    onMouseClick: (Int) -> Unit, // 1: left, 2: middle, 3: right
    onScroll: (Float, Float) -> Unit,
    onSendKey: (String) -> Unit,
    onSendText: (String) -> Unit,
    onToggleKeyboardDialog: () -> Unit,
    modifier: Modifier = Modifier
) {
    var textInput by remember { mutableStateOf("") }
    var cursorPosition by remember { mutableStateOf<Offset?>(null) }
    var showInstructions by remember { mutableStateOf(true) }
    var hasMovedOnce by remember { mutableStateOf(false) }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(HyprColors.Background)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        // Cabeçalho: "RATO & TECLADO" (19sp Archivo), subtítulo em verde e botão circular com "⌨"
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Column {
                Text(
                    text = "RATO & TECLADO",
                    fontFamily = ArchivoBlack,
                    fontWeight = FontWeight.Bold,
                    fontSize = 19.sp,
                    color = HyprColors.TextTitle
                )
                Text(
                    text = "touchpad sensível · $deviceName",
                    fontFamily = JetBrainsMono,
                    fontSize = 10.sp,
                    color = HyprColors.NeonGreen,
                    modifier = Modifier.padding(top = 2.dp)
                )
            }

            Box(
                modifier = Modifier
                    .size(36.dp)
                    .clip(CircleShape)
                    .border(1.dp, HyprColors.BorderNormal, CircleShape)
                    .clickable { onToggleKeyboardDialog() },
                contentAlignment = Alignment.Center
            ) {
                Text("⌨", fontSize = 16.sp, color = HyprColors.TextBody, fontWeight = FontWeight.Bold)
            }
        }

        // Superfície do touchpad: weight(1f), borda #1E3E30, raio 18dp, brilho radial suave rgba(61,255,158,.06)
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f)
                .clip(RoundedCornerShape(18.dp))
                .background(
                    brush = Brush.radialGradient(
                        colors = listOf(
                            Color(0x0F3DFF9E),
                            Color(0x00000000)
                        )
                    )
                )
                .border(1.dp, HyprColors.BorderTouchpad, RoundedCornerShape(18.dp))
                .pointerInput(Unit) {
                    detectTapGestures(
                        onTap = {
                            onMouseClick(1)
                            if (!hasMovedOnce) {
                                hasMovedOnce = true
                                showInstructions = false
                            }
                        },
                        onDoubleTap = {
                            onMouseClick(1)
                            onMouseClick(1)
                        },
                        onLongPress = {
                            // Toque longo faz reaparecer a ajuda
                            showInstructions = true
                        }
                    )
                }
                .pointerInput(Unit) {
                    detectDragGestures(
                        onDragStart = { offset ->
                            cursorPosition = offset
                            if (!hasMovedOnce) {
                                hasMovedOnce = true
                                showInstructions = false
                            }
                        },
                        onDragEnd = {
                            cursorPosition = null
                        },
                        onDragCancel = {
                            cursorPosition = null
                        },
                        onDrag = { change, dragAmount ->
                            change.consume()
                            cursorPosition = change.position
                            onMouseMove(dragAmount.x, dragAmount.y)
                        }
                    )
                }
        ) {
            // Ponto de cursor de 10dp em verde com sombra de brilho que segue o dedo
            cursorPosition?.let { pos ->
                Box(
                    modifier = Modifier
                        .offset(x = (pos.x - 5).dp, y = (pos.y - 5).dp)
                        .size(10.dp)
                        .shadow(12.dp, CircleShape, ambientColor = HyprColors.NeonGreen, spotColor = HyprColors.NeonGreen)
                        .background(HyprColors.NeonGreen, CircleShape)
                )
            }

            if (showInstructions) {
                Text(
                    text = "1 dedo move · 2 dedos scroll · toque longo para ajuda",
                    fontFamily = JetBrainsMono,
                    fontSize = 9.sp,
                    color = HyprColors.TextInstruction,
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .padding(bottom = 12.dp)
                )
            }
        }

        // Fila de modificadores: pílulas com scroll horizontal — ESC, TAB, SUPER, CTRL, ALT, ↑ — borda #262626, texto verde 10sp
        val modifierKeys = listOf("ESC", "TAB", "SUPER", "CTRL", "ALT", "↑", "↓", "←", "→", "ENTER", "BACKSPACE")
        LazyRow(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            items(modifierKeys) { key ->
                Box(
                    modifier = Modifier
                        .clip(RoundedCornerShape(8.dp))
                        .border(1.dp, HyprColors.BorderHighlight, RoundedCornerShape(8.dp))
                        .clickable { onSendKey(key) }
                        .padding(horizontal = 12.dp, vertical = 7.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        text = key,
                        fontFamily = JetBrainsMono,
                        fontSize = 10.sp,
                        fontWeight = FontWeight.Bold,
                        color = HyprColors.NeonGreen
                    )
                }
            }
        }

        // Botões do rato: três alvos grandes (padding vertical 15dp) com borda #1E3E30 — ESQUERDO, MEIO (mais estreito), DIREITO
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Box(
                modifier = Modifier
                    .weight(1.2f)
                    .clip(RoundedCornerShape(12.dp))
                    .border(1.dp, HyprColors.BorderTouchpad, RoundedCornerShape(12.dp))
                    .clickable { onMouseClick(1) }
                    .padding(vertical = 15.dp),
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = "ESQUERDO",
                    fontFamily = JetBrainsMono,
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold,
                    color = HyprColors.TextBody
                )
            }

            Box(
                modifier = Modifier
                    .weight(0.7f)
                    .clip(RoundedCornerShape(12.dp))
                    .border(1.dp, HyprColors.BorderTouchpad, RoundedCornerShape(12.dp))
                    .clickable { onMouseClick(2) }
                    .padding(vertical = 15.dp),
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = "MEIO",
                    fontFamily = JetBrainsMono,
                    fontSize = 10.sp,
                    fontWeight = FontWeight.Bold,
                    color = HyprColors.TextSecondary
                )
            }

            Box(
                modifier = Modifier
                    .weight(1.2f)
                    .clip(RoundedCornerShape(12.dp))
                    .border(1.dp, HyprColors.BorderTouchpad, RoundedCornerShape(12.dp))
                    .clickable { onMouseClick(3) }
                    .padding(vertical = 15.dp),
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = "DIREITO",
                    fontFamily = JetBrainsMono,
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold,
                    color = HyprColors.TextBody
                )
            }
        }

        // Entrada de texto: campo com placeholder "escrever no PC…" e um botão quadrado de 52dp em verde cheio com "↵"
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            OutlinedTextField(
                value = textInput,
                onValueChange = { textInput = it },
                modifier = Modifier.weight(1f),
                placeholder = {
                    Text(
                        "escrever no PC…",
                        fontFamily = JetBrainsMono,
                        fontSize = 12.sp,
                        color = HyprColors.TextCaption
                    )
                },
                singleLine = true,
                shape = RoundedCornerShape(12.dp),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedContainerColor = HyprColors.SurfaceCard,
                    unfocusedContainerColor = HyprColors.SurfaceCard,
                    focusedBorderColor = HyprColors.BorderHighlight,
                    unfocusedBorderColor = HyprColors.BorderNormal,
                    focusedTextColor = HyprColors.TextTitle,
                    unfocusedTextColor = HyprColors.TextTitle
                )
            )

            Box(
                modifier = Modifier
                    .size(52.dp)
                    .clip(RoundedCornerShape(12.dp))
                    .background(HyprColors.NeonGreen)
                    .clickable {
                        if (textInput.isNotEmpty()) {
                            onSendText(textInput)
                            textInput = ""
                        } else {
                            onSendKey("ENTER")
                        }
                    },
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = "↵",
                    fontSize = 18.sp,
                    color = Color.Black,
                    fontWeight = FontWeight.Bold
                )
            }
        }
    }
}
