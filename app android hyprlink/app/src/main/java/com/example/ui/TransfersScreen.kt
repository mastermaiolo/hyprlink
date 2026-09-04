package com.example.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.example.ui.theme.*

data class ActiveTransferItem(
    val id: String,
    val name: String,
    val isIncoming: Boolean,
    val currentBytes: Long,
    val totalBytes: Long,
    val progress: Float
)

data class CompletedTransferItem(
    val id: String,
    val name: String,
    val isIncoming: Boolean,
    val totalBytes: Long,
    val dateLabel: String,
    val sha256Hash: String,
    val isVerified: Boolean = true,
    val uri: android.net.Uri? = null
)

data class ErrorTransferItem(
    val id: String,
    val name: String,
    val isIncoming: Boolean,
    val totalBytes: Long,
    val failedPercentage: Int,
    val technicalMessage: String,
    val friendlyExplanation: String
)

@Composable
fun TransfersScreen(
    onBack: () -> Unit,
    activeTransfers: List<ActiveTransferItem>,
    completedTransfers: List<CompletedTransferItem>,
    errorTransfers: List<ErrorTransferItem>,
    onSendFileClick: () -> Unit,
    onRetryTransfer: (String) -> Unit,
    onDeleteErrorTransfer: (String) -> Unit,
    onClearHistory: () -> Unit,
    onOpenTransfer: (CompletedTransferItem) -> Unit = {},
    modifier: Modifier = Modifier
) {
    Box(modifier = modifier.fillMaxSize().background(HyprColors.Background)) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 12.dp)
                .padding(bottom = 80.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            // Cabeçalho com seta, título "TRANSFERÊNCIAS", subtítulo e "Limpar" à direita
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween
            ) {
                Row(
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

                    Column {
                        Text(
                            text = "TRANSFERÊNCIAS",
                            fontFamily = ArchivoBlack,
                            fontWeight = FontWeight.Bold,
                            fontSize = 19.sp,
                            color = HyprColors.TextTitle
                        )
                        Text(
                            text = "Ficheiros partilhados na sessão",
                            fontFamily = JetBrainsMono,
                            fontSize = 10.sp,
                            color = HyprColors.TextCaption,
                            modifier = Modifier.padding(top = 2.dp)
                        )
                    }
                }

                Text(
                    text = "Limpar",
                    fontFamily = JetBrainsMono,
                    fontSize = 11.sp,
                    color = HyprColors.TextSecondary,
                    modifier = Modifier.clickable { onClearHistory() }
                )
            }

            // "SESSÃO ATUAL" (etiqueta em verde)
            Text(
                text = "SESSÃO ATUAL",
                fontFamily = JetBrainsMono,
                fontSize = 9.sp,
                letterSpacing = 0.16.sp,
                color = HyprColors.NeonGreen,
                fontWeight = FontWeight.Bold
            )

            // Transferência em curso (Exemplo padrão caso vazio)
            val activeList = if (activeTransfers.isEmpty()) {
                listOf(
                    ActiveTransferItem("tx_1", "hyprland-config.tar.gz", false, 181000, 231900, 0.78f)
                )
            } else activeTransfers

            activeList.forEach { active ->
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
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(11.dp)
                        ) {
                            Box(
                                modifier = Modifier
                                    .size(32.dp)
                                    .clip(RoundedCornerShape(10.dp))
                                    .background(HyprColors.GreenTintSurface)
                                    .border(1.dp, HyprColors.BorderTouchpad, RoundedCornerShape(10.dp)),
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = if (active.isIncoming) "↓" else "↑",
                                    fontSize = 13.sp,
                                    color = HyprColors.NeonGreen,
                                    fontWeight = FontWeight.Bold
                                )
                            }

                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    text = active.name,
                                    fontFamily = JetBrainsMono,
                                    fontSize = 12.sp,
                                    color = HyprColors.TextTitle,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis
                                )
                                Text(
                                    text = "${if (active.isIncoming) "recebido" else "enviado"} · ${active.totalBytes / 1024.0} KB",
                                    fontFamily = JetBrainsMono,
                                    fontSize = 9.sp,
                                    color = HyprColors.TextCaption,
                                    modifier = Modifier.padding(top = 2.dp)
                                )
                            }

                            Box(
                                modifier = Modifier
                                    .clip(RoundedCornerShape(7.dp))
                                    .border(1.dp, HyprColors.BorderTouchpad, RoundedCornerShape(7.dp))
                                    .padding(horizontal = 8.dp, vertical = 4.dp)
                            ) {
                                Text(
                                    text = "EM CURSO",
                                    fontFamily = JetBrainsMono,
                                    fontSize = 9.sp,
                                    color = HyprColors.NeonGreen,
                                    fontWeight = FontWeight.Bold
                                )
                            }
                        }

                        // Barra de progresso de 3dp
                        Spacer(modifier = Modifier.height(10.dp))
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(3.dp)
                                .clip(RoundedCornerShape(3.dp))
                                .background(HyprColors.BorderNormal)
                        ) {
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth(fraction = active.progress.coerceIn(0f, 1f))
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
                            Text(
                                text = "${(active.progress * 100).toInt()}%",
                                fontFamily = JetBrainsMono,
                                fontSize = 9.sp,
                                color = HyprColors.TextCaption
                            )
                            Text(
                                text = "${active.currentBytes / 1024} KB de ${active.totalBytes / 1024} KB",
                                fontFamily = JetBrainsMono,
                                fontSize = 9.sp,
                                color = HyprColors.TextCaption
                            )
                        }
                    }
                }
            }

            // Transferências com erro (o único que abre por defeito)
            val errorList = if (errorTransfers.isEmpty()) {
                listOf(
                    ErrorTransferItem(
                        id = "err_1",
                        name = "IMG_20260901_203409.jpg",
                        isIncoming = false,
                        totalBytes = 6186598,
                        failedPercentage = 62,
                        technicalMessage = "write failed because stream was aborted",
                        friendlyExplanation = "o PC fechou a ligação a meio · tenta de novo"
                    )
                )
            } else errorTransfers

            errorList.forEach { err ->
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(14.dp))
                        .background(HyprColors.SurfaceCard)
                        .border(1.dp, HyprColors.BorderRed, RoundedCornerShape(14.dp))
                        .padding(14.dp)
                ) {
                    Column {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(11.dp)
                        ) {
                            Box(
                                modifier = Modifier
                                    .size(32.dp)
                                    .clip(RoundedCornerShape(10.dp))
                                    .background(HyprColors.RedTintSurface)
                                    .border(1.dp, HyprColors.BorderRed, RoundedCornerShape(10.dp)),
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = if (err.isIncoming) "↓" else "↑",
                                    fontSize = 13.sp,
                                    color = HyprColors.Red,
                                    fontWeight = FontWeight.Bold
                                )
                            }

                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    text = err.name,
                                    fontFamily = JetBrainsMono,
                                    fontSize = 12.sp,
                                    color = HyprColors.TextTitle,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis
                                )
                                Text(
                                    text = "${String.format("%.1f", err.totalBytes / 1048576.0)} MB · falhou a ${err.failedPercentage}%",
                                    fontFamily = JetBrainsMono,
                                    fontSize = 9.sp,
                                    color = HyprColors.TextCaption,
                                    modifier = Modifier.padding(top = 2.dp)
                                )
                            }

                            Box(
                                modifier = Modifier
                                    .clip(RoundedCornerShape(7.dp))
                                    .background(HyprColors.RedTintSurface)
                                    .border(1.dp, HyprColors.BorderRed, RoundedCornerShape(7.dp))
                                    .padding(horizontal = 8.dp, vertical = 4.dp)
                            ) {
                                Text(
                                    text = "ERRO",
                                    fontFamily = JetBrainsMono,
                                    fontSize = 9.sp,
                                    color = HyprColors.Red,
                                    fontWeight = FontWeight.Bold
                                )
                            }
                        }

                        // Bloco #120609 de detalhes de erro
                        Spacer(modifier = Modifier.height(10.dp))
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(9.dp))
                                .background(HyprColors.RedTintBlock)
                                .border(1.dp, HyprColors.BorderRed, RoundedCornerShape(9.dp))
                                .padding(10.dp)
                        ) {
                            Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                                Text(
                                    text = err.technicalMessage,
                                    fontFamily = JetBrainsMono,
                                    fontSize = 9.sp,
                                    color = Color(0xFFE08A94)
                                )
                                Text(
                                    text = err.friendlyExplanation,
                                    fontFamily = JetBrainsMono,
                                    fontSize = 9.sp,
                                    color = Color(0xFF8A5A60)
                                )
                            }
                        }

                        // Botões de ação do erro
                        Spacer(modifier = Modifier.height(10.dp))
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            OutlinedButton(
                                onClick = { onRetryTransfer(err.id) },
                                modifier = Modifier.weight(1f).height(38.dp),
                                shape = RoundedCornerShape(9.dp),
                                border = BorderStroke(1.dp, HyprColors.Red),
                                colors = ButtonDefaults.outlinedButtonColors(contentColor = HyprColors.Red)
                            ) {
                                Text("TENTAR DE NOVO", fontFamily = JetBrainsMono, fontSize = 10.sp, fontWeight = FontWeight.Bold)
                            }

                            OutlinedButton(
                                onClick = { onDeleteErrorTransfer(err.id) },
                                modifier = Modifier.height(38.dp),
                                shape = RoundedCornerShape(9.dp),
                                border = BorderStroke(1.dp, HyprColors.BorderHighlight),
                                colors = ButtonDefaults.outlinedButtonColors(contentColor = HyprColors.TextSecondary)
                            ) {
                                Text("apagar", fontFamily = JetBrainsMono, fontSize = 10.sp)
                            }
                        }
                    }
                }
            }

            // "HISTÓRICO PERSISTIDO"
            Row(
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = "HISTÓRICO PERSISTIDO",
                    fontFamily = JetBrainsMono,
                    fontSize = 9.sp,
                    letterSpacing = 0.16.sp,
                    color = HyprColors.TextCaption,
                    fontWeight = FontWeight.Bold
                )
                Text(
                    text = "limpar histórico",
                    fontFamily = JetBrainsMono,
                    fontSize = 9.sp,
                    color = HyprColors.TextCaption,
                    modifier = Modifier.clickable { onClearHistory() }
                )
            }

            val completedList = if (completedTransfers.isEmpty()) {
                listOf(
                    CompletedTransferItem(
                        id = "comp_1",
                        name = "VID_20260831_023707~2.mp4",
                        isIncoming = false,
                        totalBytes = 11219763,
                        dateLabel = "ontem",
                        sha256Hash = "4fa29c8e192f1b0a887d77f3a9e145c3bb836109e452a42dc8c34ea81944da2b"
                    ),
                    CompletedTransferItem(
                        id = "comp_2",
                        name = "wallpapers_nord.zip",
                        isIncoming = true,
                        totalBytes = 4508876,
                        dateLabel = "há 2 dias",
                        sha256Hash = "8e192f1b0a887d77f3a9e145c3bb836109e452a42dc8c34ea81944da2b4fa29c"
                    )
                )
            } else completedTransfers

            completedList.forEach { item ->
                var showHash by remember { mutableStateOf(false) }

                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(12.dp))
                        .background(HyprColors.SurfaceCard)
                        .border(1.dp, HyprColors.BorderNormal, RoundedCornerShape(12.dp))
                        .clickable { onOpenTransfer(item) }
                        .padding(12.dp)
                ) {
                    Column {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
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
                                Text(
                                    text = if (item.isIncoming) "↓" else "↑",
                                    fontSize = 12.sp,
                                    color = HyprColors.TextSecondary
                                )
                            }

                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    text = item.name,
                                    fontFamily = JetBrainsMono,
                                    fontSize = 12.sp,
                                    color = Color(0xFFD4D4D4),
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis
                                )
                                Row(
                                    modifier = Modifier.padding(top = 2.dp),
                                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                                ) {
                                    Text(
                                        text = "${String.format("%.1f", item.totalBytes / 1048576.0)} MB · ${item.dateLabel}",
                                        fontFamily = JetBrainsMono,
                                        fontSize = 9.sp,
                                        color = HyprColors.TextCaption
                                    )
                                    Text(
                                        text = if (showHash) "esconder hash ▴" else "ver hash ▾",
                                        fontFamily = JetBrainsMono,
                                        fontSize = 9.sp,
                                        color = HyprColors.TextLog,
                                        modifier = Modifier.clickable { showHash = !showHash }
                                    )
                                }
                            }

                            Box(
                                modifier = Modifier
                                    .clip(RoundedCornerShape(7.dp))
                                    .border(1.dp, HyprColors.BorderTouchpad, RoundedCornerShape(7.dp))
                                    .padding(horizontal = 7.dp, vertical = 3.dp)
                            ) {
                                Text(
                                    text = "✓ VERIFICADO",
                                    fontFamily = JetBrainsMono,
                                    fontSize = 8.sp,
                                    color = HyprColors.NeonGreen,
                                    fontWeight = FontWeight.Bold
                                )
                            }
                        }

                        if (showHash) {
                            Spacer(modifier = Modifier.height(8.dp))
                            Text(
                                text = "SHA-256: ${item.sha256Hash}",
                                fontFamily = JetBrainsMono,
                                fontSize = 8.sp,
                                color = HyprColors.TextCaption,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis
                            )
                        }
                    }
                }
            }
        }

        // FAB: botão "＋ ENVIAR FICHEIRO" ancorado em baixo à direita, verde cheio com texto #001a0e
        Box(
            modifier = Modifier
                .align(Alignment.BottomEnd)
                .padding(20.dp)
        ) {
            Button(
                onClick = onSendFileClick,
                shape = RoundedCornerShape(14.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = HyprColors.NeonGreen,
                    contentColor = HyprColors.OnNeonGreen
                ),
                contentPadding = PaddingValues(horizontal = 22.dp, vertical = 14.dp),
                elevation = ButtonDefaults.buttonElevation(defaultElevation = 8.dp)
            ) {
                Text(
                    text = "＋ ENVIAR FICHEIRO",
                    fontFamily = JetBrainsMono,
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold
                )
            }
        }
    }
}
