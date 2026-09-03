package com.example.ui

import androidx.compose.foundation.*
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.example.ui.theme.*

data class WorkspaceItem(
    val id: Int,
    val name: String,
    val windowsCount: Int,
    val isActive: Boolean
)

data class WindowClientItem(
    val address: String,
    val title: String,
    val clientClass: String,
    val workspaceId: Int,
    val isFocused: Boolean
)

@Composable
fun MissionControlScreen(
    workspaces: List<WorkspaceItem>,
    windows: List<WindowClientItem>,
    onSelectWorkspace: (Int) -> Unit,
    onLaunchApp: (String) -> Unit,
    onCloseWindow: (String) -> Unit,
    onFocusWindow: (String) -> Unit,
    onRefresh: () -> Unit,
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
        // Cabeçalho: título "MISSION CONTROL" (20sp Archivo) com o subtítulo e botão circular de 36dp com "⟳"
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Column {
                Text(
                    text = "MISSION CONTROL",
                    fontFamily = ArchivoBlack,
                    fontWeight = FontWeight.Bold,
                    fontSize = 20.sp,
                    color = HyprColors.TextTitle
                )
                Text(
                    text = "Gere os workspaces e janelas ativas",
                    fontFamily = JetBrainsMono,
                    fontSize = 11.sp,
                    color = HyprColors.TextCaption,
                    modifier = Modifier.padding(top = 2.dp)
                )
            }

            Box(
                modifier = Modifier
                    .size(36.dp)
                    .clip(CircleShape)
                    .border(1.dp, HyprColors.BorderNormal, CircleShape)
                    .clickable { onRefresh() },
                contentAlignment = Alignment.Center
            ) {
                Text("⟳", fontSize = 16.sp, color = HyprColors.TextBody, fontWeight = FontWeight.Bold)
            }
        }

        // Secção "WORKSPACES"
        Text(
            text = "WORKSPACES",
            fontFamily = JetBrainsMono,
            fontSize = 9.sp,
            letterSpacing = 0.16.sp,
            color = HyprColors.TextCaption,
            fontWeight = FontWeight.Bold
        )

        val sampleWorkspaces = if (workspaces.isEmpty()) {
            listOf(
                WorkspaceItem(1, "WS 1", 2, true),
                WorkspaceItem(3, "WS 3", 1, false),
                WorkspaceItem(4, "WS 4", 2, false)
            )
        } else workspaces

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            sampleWorkspaces.forEach { ws ->
                if (ws.isActive) {
                    // Só o workspace ativo é preenchido (fundo #3DFF9E, largura fator 1.25)
                    Box(
                        modifier = Modifier
                            .weight(1.25f)
                            .clip(RoundedCornerShape(12.dp))
                            .background(HyprColors.NeonGreen)
                            .clickable { onSelectWorkspace(ws.id) }
                            .padding(11.dp, 12.dp)
                    ) {
                        Column {
                            Text(
                                text = "WS ${ws.id}",
                                fontFamily = ArchivoBlack,
                                fontWeight = FontWeight.Bold,
                                fontSize = 17.sp,
                                color = HyprColors.OnNeonGreen
                            )
                            Text(
                                text = "${ws.windowsCount} janelas · ativo",
                                fontFamily = JetBrainsMono,
                                fontSize = 9.sp,
                                color = Color(0xFF04301C),
                                modifier = Modifier.padding(top = 2.dp)
                            )
                        }
                    }
                } else {
                    // Outros: contorno #262626
                    Box(
                        modifier = Modifier
                            .weight(1.0f)
                            .clip(RoundedCornerShape(12.dp))
                            .border(1.dp, HyprColors.BorderHighlight, RoundedCornerShape(12.dp))
                            .clickable { onSelectWorkspace(ws.id) }
                            .padding(11.dp, 12.dp)
                    ) {
                        Column {
                            Text(
                                text = "WS ${ws.id}",
                                fontFamily = ArchivoBlack,
                                fontWeight = FontWeight.Bold,
                                fontSize = 17.sp,
                                color = HyprColors.TextSecondary
                            )
                            Text(
                                text = "${ws.windowsCount} janela${if (ws.windowsCount != 1) "s" else ""}",
                                fontFamily = JetBrainsMono,
                                fontSize = 9.sp,
                                color = HyprColors.TextCaption,
                                modifier = Modifier.padding(top = 2.dp)
                            )
                        }
                    }
                }
            }

            // Caixa estreita com "›"
            Box(
                modifier = Modifier
                    .weight(0.55f)
                    .height(60.dp)
                    .clip(RoundedCornerShape(12.dp))
                    .border(1.dp, HyprColors.BorderNormal, RoundedCornerShape(12.dp)),
                contentAlignment = Alignment.Center
            ) {
                Text("›", fontFamily = JetBrainsMono, fontSize = 12.sp, color = HyprColors.TextLog)
            }
        }

        // Fila de lançadores: quatro pílulas de largura igual com nomes de aplicação (firefox, code, ghostty, spotify), só borda
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            listOf("firefox", "code", "ghostty", "spotify").forEach { appName ->
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .clip(RoundedCornerShape(20.dp))
                        .border(1.dp, HyprColors.BorderHighlight, RoundedCornerShape(20.dp))
                        .clickable { onLaunchApp(appName) }
                        .padding(vertical = 9.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        text = appName,
                        fontFamily = JetBrainsMono,
                        fontSize = 11.sp,
                        color = HyprColors.TextBody
                    )
                }
            }
        }

        // Secção "JANELAS ATIVAS · 7" agrupada por workspace
        val sampleWindows = if (windows.isEmpty()) {
            listOf(
                WindowClientItem("0x1a", "GitHub - HyprLink Workstation", "opera", 1, true),
                WindowClientItem("0x1b", "Alacritty - cargo run daemon", "alacritty", 1, false),
                WindowClientItem("0x3a", "Spotify Free", "spotify", 3, false),
                WindowClientItem("0x4a", "Visual Studio Code", "code", 4, false),
                WindowClientItem("0x4b", "Hyprland Configuration - Thunar", "thunar", 4, false)
            )
        } else windows

        val grouped = sampleWindows.groupBy { it.workspaceId }

        Text(
            text = "JANELAS ATIVAS · ${sampleWindows.size}",
            fontFamily = JetBrainsMono,
            fontSize = 9.sp,
            letterSpacing = 0.16.sp,
            color = HyprColors.TextCaption,
            fontWeight = FontWeight.Bold
        )

        grouped.forEach { (wsId, clientList) ->
            Text(
                text = "WORKSPACE $wsId",
                fontFamily = JetBrainsMono,
                fontSize = 9.sp,
                letterSpacing = 0.12.sp,
                color = HyprColors.TextCaption,
                modifier = Modifier.padding(top = 4.dp)
            )

            clientList.forEach { client ->
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(12.dp))
                        .background(HyprColors.SurfaceCard)
                        .border(
                            1.dp,
                            if (client.isFocused) HyprColors.NeonGreen else HyprColors.BorderNormal,
                            RoundedCornerShape(12.dp)
                        )
                        .clickable { onFocusWindow(client.address) }
                        .padding(14.dp)
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            modifier = Modifier.weight(1f)
                        ) {
                            if (client.isFocused) {
                                Box(
                                    modifier = Modifier
                                        .width(2.dp)
                                        .height(30.dp)
                                        .background(HyprColors.NeonGreen)
                                )
                                Spacer(modifier = Modifier.width(10.dp))
                            }

                            Column {
                                Text(
                                    text = client.title,
                                    fontFamily = ArchivoBlack,
                                    fontWeight = FontWeight.Bold,
                                    fontSize = 13.sp,
                                    color = HyprColors.TextTitle,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis
                                )
                                Text(
                                    text = if (client.isFocused) "${client.clientClass} · em foco" else "${client.clientClass} · ${client.address}",
                                    fontFamily = JetBrainsMono,
                                    fontSize = 9.sp,
                                    color = if (client.isFocused) HyprColors.NeonGreen else HyprColors.TextCaption,
                                    modifier = Modifier.padding(top = 2.dp)
                                )
                            }
                        }

                        // Botão "×" vermelho à direita para fechar
                        IconButton(
                            onClick = { onCloseWindow(client.address) },
                            modifier = Modifier.size(28.dp)
                        ) {
                            Text("×", fontSize = 16.sp, color = HyprColors.Red, fontWeight = FontWeight.Bold)
                        }
                    }
                }
            }
        }

        Spacer(modifier = Modifier.height(16.dp))
    }
}
