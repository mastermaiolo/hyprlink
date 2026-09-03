package com.example

import android.content.Context
import androidx.camera.core.*
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.google.mlkit.vision.barcode.BarcodeScannerOptions
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import co.nstant.`in`.cbor.CborBuilder
import co.nstant.`in`.cbor.CborEncoder
import co.nstant.`in`.cbor.CborDecoder
import co.nstant.`in`.cbor.model.*
import tech.kwik.core.QuicClientConnection
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.security.MessageDigest

// --- Data class representing the parsed QR code contents ---
data class ParsedPairingData(
    val fingerprint: String,
    val host: String,
    val port: Int,
    val pairingToken: String
)

// --- Helper to parse the QR Payload ---
// Formatos aceitos:
// 1. Canônico: "<FINGERPRINT_HEX>|<HOST>:<PORTA>|<TOKEN_HEX>"
// 2. Sem delimitadores de fingerprint (com ou sem dois pontos)
// 3. JSON: {"fingerprint": "...", "host": "...", "port": 7443, "token": "..."}
// 4. URI: hyprlink://pair?fp=...&host=...&port=7443&token=...
fun parsePairingQr(rawPayload: String): ParsedPairingData? {
    val payload = rawPayload.trim()

    // Formato 1: JSON
    if (payload.startsWith("{") && payload.endsWith("}")) {
        try {
            val obj = org.json.JSONObject(payload)
            val fp = obj.optString("fingerprint", obj.optString("fp", "")).trim()
            val host = obj.optString("host", obj.optString("ip", "")).trim()
            val port = obj.optInt("port", ConnectionUtils.HYPRLINK_SERVICE_PORT)
            val token = obj.optString("pairing_token", obj.optString("token", "")).trim()
            if (fp.isNotEmpty() && host.isNotEmpty()) {
                return ParsedPairingData(
                    fingerprint = fp,
                    host = host.removePrefix("[").removeSuffix("]"),
                    port = port,
                    pairingToken = token
                )
            }
        } catch (ignored: Exception) {}
    }

    // Formato 2: URI hyprlink://pair?...
    if (payload.startsWith("hyprlink://", ignoreCase = true)) {
        try {
            val uri = android.net.Uri.parse(payload)
            val fp = uri.getQueryParameter("fp") ?: uri.getQueryParameter("fingerprint") ?: ""
            val host = uri.host ?: uri.getQueryParameter("host") ?: ""
            val port = if (uri.port > 0) uri.port else (uri.getQueryParameter("port")?.toIntOrNull() ?: ConnectionUtils.HYPRLINK_SERVICE_PORT)
            val token = uri.getQueryParameter("token") ?: uri.getQueryParameter("pairing_token") ?: ""
            if (fp.isNotEmpty() && host.isNotEmpty()) {
                return ParsedPairingData(
                    fingerprint = fp,
                    host = host,
                    port = port,
                    pairingToken = token
                )
            }
        } catch (ignored: Exception) {}
    }

    // Formato 3: Canônico pipe-separated "<FINGERPRINT>|<HOST>:<PORT>|<TOKEN>"
    val parts = payload.split("|")
    if (parts.size >= 2) {
        val fingerprint = parts[0].trim()
        val hostPort = parts[1].trim()
        val pairingToken = if (parts.size >= 3) parts[2].trim() else ""
        
        val lastColonIndex = hostPort.lastIndexOf(':')
        val host: String
        val port: Int
        if (lastColonIndex != -1) {
            host = hostPort.substring(0, lastColonIndex).trim().removePrefix("[").removeSuffix("]")
            port = hostPort.substring(lastColonIndex + 1).trim().toIntOrNull() ?: ConnectionUtils.HYPRLINK_SERVICE_PORT
        } else {
            host = hostPort.trim().removePrefix("[").removeSuffix("]")
            port = ConnectionUtils.HYPRLINK_SERVICE_PORT
        }
        
        if (fingerprint.isNotEmpty() && host.isNotEmpty()) {
            return ParsedPairingData(
                fingerprint = fingerprint,
                host = host,
                port = port,
                pairingToken = pairingToken
            )
        }
    }

    return null
}

// --- Helper to format hex into standard SHA-256 visual style with colons ---
fun formatFingerprint(hex: String): String {
    val clean = hex.replace(":", "").uppercase().trim()
    if (clean.length != 64) return hex
    return clean.chunked(2).joinToString(":")
}

// --- Hex converter ---
fun hexToBytes(hex: String): ByteArray {
    val cleanHex = hex.replace(":", "").replace(" ", "").trim()
    val len = cleanHex.length
    val data = ByteArray(len / 2)
    for (i in 0 until len step 2) {
        data[i / 2] = ((Character.digit(cleanHex[i], 16) shl 4) + Character.digit(cleanHex[i + 1], 16)).toByte()
    }
    return data
}

// --- ML Kit QR Code Analyzer for CameraX ---
@androidx.annotation.OptIn(ExperimentalGetImage::class)
class QrCodeAnalyzer(
    private val onQrCodeScanned: (String) -> Unit
) : ImageAnalysis.Analyzer {
    
    private val scanner = BarcodeScanning.getClient(
        BarcodeScannerOptions.Builder()
            .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
            .build()
    )

    override fun analyze(imageProxy: ImageProxy) {
        val mediaImage = imageProxy.image
        if (mediaImage != null) {
            val image = InputImage.fromMediaImage(mediaImage, imageProxy.imageInfo.rotationDegrees)
            scanner.process(image)
                .addOnSuccessListener { barcodes ->
                    for (barcode in barcodes) {
                        val rawValue = barcode.rawValue
                        if (rawValue != null) {
                            onQrCodeScanned(rawValue)
                            break
                        }
                    }
                }
                .addOnFailureListener {
                    // Fail silently
                }
                .addOnCompleteListener {
                    imageProxy.close()
                }
        } else {
            imageProxy.close()
        }
    }
}

// --- Jetpack Compose CameraX Preview widget ---
@Composable
fun CameraPreview(
    modifier: Modifier = Modifier,
    onQrCodeScanned: (String) -> Unit
) {
    val context = LocalContext.current
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    val cameraProviderFuture = remember { ProcessCameraProvider.getInstance(context) }

    AndroidView(
        factory = { ctx ->
            val previewView = PreviewView(ctx).apply {
                scaleType = PreviewView.ScaleType.FILL_CENTER
            }
            val executor = ContextCompat.getMainExecutor(ctx)
            cameraProviderFuture.addListener({
                val cameraProvider = cameraProviderFuture.get()
                val preview = Preview.Builder().build().also {
                    it.setSurfaceProvider(previewView.surfaceProvider)
                }

                val imageAnalysis = ImageAnalysis.Builder()
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .build()
                    .also {
                        it.setAnalyzer(executor, QrCodeAnalyzer { qrText ->
                            onQrCodeScanned(qrText)
                        })
                    }

                val cameraSelector = CameraSelector.DEFAULT_BACK_CAMERA

                try {
                    cameraProvider.unbindAll()
                    cameraProvider.bindToLifecycle(
                        lifecycleOwner,
                        cameraSelector,
                        preview,
                        imageAnalysis
                    )
                } catch (e: Exception) {
                    e.printStackTrace()
                }
            }, executor)
            previewView
        },
        modifier = modifier
    )
}

// --- CBOR Packet Serializer for core.hello ---
fun buildHelloCbor(deviceName: String, pairingTokenHex: String?): ByteArray {
    val baos = ByteArrayOutputStream()
    val builder = CborBuilder()
    val mapBuilder = builder.addMap()
    
    // { id: u64, type: "core.hello", body: <HelloBody>, has_payload: false }
    mapBuilder.put(UnicodeString("id"), UnsignedInteger(1L))
    mapBuilder.put(UnicodeString("type"), UnicodeString("core.hello"))
    
    val bodyMapBuilder = mapBuilder.putMap(UnicodeString("body"))
    bodyMapBuilder.put(UnicodeString("device_name"), UnicodeString(deviceName))
    
    val capabilitiesBuilder = bodyMapBuilder.putArray(UnicodeString("capabilities"))
    capabilitiesBuilder.add(UnicodeString("core"))
    capabilitiesBuilder.add(UnicodeString("clipboard"))
    capabilitiesBuilder.add(UnicodeString("notification"))
    capabilitiesBuilder.add(UnicodeString("media"))
    capabilitiesBuilder.add(UnicodeString("battery"))
    capabilitiesBuilder.add(UnicodeString("share"))
    capabilitiesBuilder.end()
    
    if (!pairingTokenHex.isNullOrBlank()) {
        bodyMapBuilder.put(UnicodeString("pairing_token"), ByteString(hexToBytes(pairingTokenHex)))
    }
    bodyMapBuilder.end()
    
    mapBuilder.put(UnicodeString("has_payload"), SimpleValue.FALSE)
    mapBuilder.end()
    
    CborEncoder(baos).encode(builder.build())
    return baos.toByteArray()
}

// --- CBOR Packet Parser/Verifier for core.hello response ---
fun isResponseCoreHello(cborBytes: ByteArray): Boolean {
    return parseCoreHelloDeviceName(cborBytes) != null
}

// --- Parse core.hello Response and extract workstation device_name ---
fun parseCoreHelloDeviceName(cborBytes: ByteArray): String? {
    try {
        val bais = ByteArrayInputStream(cborBytes)
        val decoded = CborDecoder(bais).decode()
        if (decoded.isEmpty()) return null
        val mainItem = decoded[0]
        if (mainItem is co.nstant.`in`.cbor.model.Map) {
            val typeItem = mainItem.get(UnicodeString("type"))
            if (typeItem is UnicodeString && typeItem.string == "core.hello") {
                val bodyItem = mainItem.get(UnicodeString("body"))
                if (bodyItem is co.nstant.`in`.cbor.model.Map) {
                    val deviceNameItem = bodyItem.get(UnicodeString("device_name"))
                    if (deviceNameItem is UnicodeString) {
                        return deviceNameItem.string
                    }
                }
            }
        }
    } catch (e: Exception) {
        e.printStackTrace()
    }
    return null
}

// --- Coroutine task to execute the full secure QUIC-CBOR Pairing Handshake ---
suspend fun executePairingHandshake(
    context: Context,
    localDeviceName: String,
    data: ParsedPairingData,
    onLog: (String) -> Unit
): String? = withContext(Dispatchers.IO) {
    try {
        onLog("[INFO] Starting handshake pairing process...")
        onLog("[INFO] Host: ${data.host}:${data.port}")
        onLog("[INFO] Pinning TLS to certificate fingerprint: ${formatFingerprint(data.fingerprint)}")
        
        // Load secure local mTLS identity
        val identity = DeviceIdentity.getInstance(context)
        onLog("[INFO] Presenting client certificate with fingerprint: ${formatFingerprint(identity.fingerprintHex)}")

        val quicConn = ConnectionUtils.connectPinned(
            host = data.host,
            port = data.port,
            applicationProtocol = "hyprlink/1",
            expectedFingerprint = data.fingerprint,
            identity = identity
        )
            
        onLog("[INFO] Connecting to workstation...")
        // ConnectionUtils.connectPinned already called connect() and verified fingerprint pinning!
        
        if (!quicConn.isConnected) {
            onLog("[ERROR] QUIC connection failed to establish.")
            return@withContext null
        }
        
        onLog("[SUCCESS] Secure QUIC connection established (TLS pinned).")
        
        onLog("[INFO] Creating bidirectional stream (ID=0)...")
        val stream = quicConn.createStream(true)
        onLog("[SUCCESS] Stream created successfully. ID: ${stream.streamId}")
        
        onLog("[INFO] Serializing core.hello CBOR body...")
        val cborHello = buildHelloCbor(localDeviceName, data.pairingToken)
        val size = cborHello.size
        
        val frameHeader = byteArrayOf(
            ((size shr 24) and 0xFF).toByte(),
            ((size shr 16) and 0xFF).toByte(),
            ((size shr 8) and 0xFF).toByte(),
            (size and 0xFF).toByte()
        )
        
        onLog("[INFO] Sending core.hello packet (${size} body bytes) with 4-byte big-endian framing...")
        val out = stream.outputStream
        out.write(frameHeader)
        out.write(cborHello)
        out.flush()
        onLog("[DATA] Packet successfully written.")
        
        onLog("[INFO] Waiting for workstation response...")
        val inp = stream.inputStream
        val header = try {
            ConnectionUtils.readWithDeadline(stream, 10_000) {
                val h = ByteArray(4)
                var headerReadBytes = 0
                while (headerReadBytes < 4) {
                    val r = inp.read(h, headerReadBytes, 4 - headerReadBytes)
                    if (r == -1) {
                        throw java.io.IOException("Connection closed by server while reading header.")
                    }
                    headerReadBytes += r
                }
                h
            }
        } catch (e: Exception) {
            onLog("[ERROR] Handshake failed: ${e.message}")
            quicConn.close()
            return@withContext null
        }
        
        val responseSize = ((header[0].toInt() and 0xFF) shl 24) or
                           ((header[1].toInt() and 0xFF) shl 16) or
                           ((header[2].toInt() and 0xFF) shl 8) or
                           (header[3].toInt() and 0xFF)
                           
         if (responseSize <= 0 || responseSize > 5 * 1024 * 1024) {
            onLog("[ERROR] Handshake failed: Invalid response packet size ($responseSize).")
            quicConn.close()
            return@withContext null
        }
        
        onLog("[INFO] Reading response CBOR body of size $responseSize bytes...")
        val responseBody = try {
            ConnectionUtils.readWithDeadline(stream, 10_000) {
                val rb = ByteArray(responseSize)
                var bodyReadBytes = 0
                while (bodyReadBytes < responseSize) {
                    val r = inp.read(rb, bodyReadBytes, responseSize - bodyReadBytes)
                    if (r == -1) {
                        throw java.io.IOException("Connection closed by server while reading body.")
                    }
                    bodyReadBytes += r
                }
                rb
            }
        } catch (e: Exception) {
            onLog("[ERROR] Handshake failed: ${e.message}")
            quicConn.close()
            return@withContext null
        }
        
        onLog("[DATA] Response body successfully read. Parsing CBOR...")
        val workstationName = parseCoreHelloDeviceName(responseBody)
        
        quicConn.close()
        
        if (workstationName != null) {
            onLog("[SUCCESS] Workstation '$workstationName' returned valid core.hello! Pairing complete.")
            return@withContext workstationName
        } else {
            onLog("[ERROR] Workstation returned unexpected or invalid CBOR payload.")
            return@withContext null
        }
    } catch (e: Exception) {
        onLog("[ERROR] Pairing handshake exception: ${e.javaClass.simpleName}")
        onLog("[ERROR] Reason: ${e.localizedMessage ?: "Unknown error"}")
        return@withContext null
    }
}
