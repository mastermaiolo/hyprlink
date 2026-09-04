package com.example

import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import android.media.MediaRecorder
import android.util.Range
import android.util.Size
import androidx.camera.camera2.interop.Camera2Interop
import androidx.camera.camera2.interop.ExperimentalCamera2Interop
import androidx.camera.core.CameraInfo
import androidx.camera.core.CameraSelector
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.content.ContextCompat
import androidx.lifecycle.ProcessLifecycleOwner
import tech.kwik.core.QuicClientConnection
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.concurrent.Executor

@OptIn(ExperimentalCamera2Interop::class)
object WebcamStreamer {
    private var mediaCodec: MediaCodec? = null
    private var cameraProvider: ProcessCameraProvider? = null
    private var streamJob: Job? = null
    private var micJob: Job? = null
    private var activeConn: QuicClientConnection? = null
    private var appContext: Context? = null

    private var encoderPreview: Preview? = null
    private var screenPreview: Preview? = null
    private var camera: androidx.camera.core.Camera? = null

    // Lentes: todas as câmaras que o dispositivo expõe via CameraX
    // (frontal, traseira principal e, em muitos aparelhos, ultra-wide/
    // tele como entradas próprias). O botão LENTE cicla por todas.
    private var cameraInfos: List<CameraInfo> = emptyList()
    private var lensIndex = 0

    private var mainExecutor: Executor? = null

    private val _isStreaming = MutableStateFlow(false)
    val isStreaming = _isStreaming.asStateFlow()

    private val _isMicOn = MutableStateFlow(false)
    val isMicOn = _isMicOn.asStateFlow()

    private val _isTorchOn = MutableStateFlow(false)
    val isTorchOn = _isTorchOn.asStateFlow()

    private val _zoomRatio = MutableStateFlow(1.0f)
    val zoomRatio = _zoomRatio.asStateFlow()

    private val _lensLabel = MutableStateFlow("LENTE")
    val lensLabel = _lensLabel.asStateFlow()

    private val _rotationDeg = MutableStateFlow(0)
    val rotationDeg = _rotationDeg.asStateFlow()

    private val _mirrored = MutableStateFlow(false)
    val mirrored = _mirrored.asStateFlow()

    // "auto" segue o pedido do PC; "h264"/"h265" forçam (aplicado no
    // próximo início da câmara). O codec REAL usado é declarado ao PC
    // num byte no início do stream, por isso nunca há dessincronização.
    private val _codecPreference = MutableStateFlow("auto")
    val codecPreference = _codecPreference.asStateFlow()

    private val _activeCodec = MutableStateFlow("")
    val activeCodec = _activeCodec.asStateFlow()

    fun getSupportedResolutions(context: Context): List<Map<String, Any>> {
        val cameraManager = context.getSystemService(Context.CAMERA_SERVICE) as? CameraManager
            ?: return emptyList()
        val cameraId = try {
            cameraManager.cameraIdList.getOrNull(lensIndex) ?: cameraManager.cameraIdList.firstOrNull()
        } catch (e: Exception) {
            null
        } ?: return emptyList()

        val characteristics = try {
            cameraManager.getCameraCharacteristics(cameraId)
        } catch (e: Exception) {
            return emptyList()
        }

        val map = characteristics.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            ?: return emptyList()
        val outputSizes = map.getOutputSizes(android.graphics.ImageFormat.YUV_420_888)
            ?: map.getOutputSizes(android.view.SurfaceHolder::class.java)
            ?: emptyArray()

        return outputSizes
            .sortedByDescending { it.width * it.height }
            .distinctBy { "${it.width}x${it.height}" }
            .map { size ->
                mapOf(
                    "name" to "${size.width}x${size.height}",
                    "width" to size.width,
                    "height" to size.height
                )
            }
    }

    // Manda webcam.error ao PC (além do log local) — sem isto, qualquer
    // falha aqui parecia sempre o mesmo timeout genérico de 10s do lado
    // do PC, sem pista nenhuma da causa real.
    private fun reportError(message: String) {
        ConnectionRepository.appendLog("[WEBCAM] $message")
        CoroutineScope(Dispatchers.IO).launch {
            ConnectionRepository.sendOneWayPacket("webcam.error", mapOf("message" to message))
        }
    }

    private fun hasHardwareEncoder(mime: String): Boolean {
        val list = MediaCodecList(MediaCodecList.REGULAR_CODECS)
        for (info in list.codecInfos) {
            if (!info.isEncoder) continue
            if (info.supportedTypes.none { it.equals(mime, ignoreCase = true) }) continue
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q) {
                if (info.isHardwareAccelerated) return true
            } else {
                if (!info.name.startsWith("OMX.google.")) return true
            }
        }
        return false
    }

    fun start(context: Context, connection: QuicClientConnection, packetId: Long, width: Int, height: Int, fps: Int, requestedCodec: String) {
        val executor = ContextCompat.getMainExecutor(context)
        mainExecutor = executor
        appContext = context.applicationContext
        activeConn = connection

        // TODO o setup do CameraX corre na main thread (setSurfaceProvider/
        // unbindAll/bindToLifecycle lançam "Not in application's main
        // thread" fora dela). Só o trabalho de rede fica fora da main.
        executor.execute {
            try {
                stopOnMain()

                if (ContextCompat.checkSelfPermission(context, android.Manifest.permission.CAMERA)
                    != PackageManager.PERMISSION_GRANTED
                ) {
                    reportError("Permissão de câmara não concedida — não é possível iniciar a transmissão")
                    return@execute
                }

                // Rotação/espelho recomeçam neutros a cada sessão (o
                // videoflip do PC também começa em "none").
                _rotationDeg.value = 0
                _mirrored.value = false

                // Codec efetivo: preferência local > pedido do PC, sempre
                // sujeito a haver encoder por hardware (senão H.264).
                val wanted = when (_codecPreference.value) {
                    "auto" -> requestedCodec.lowercase()
                    else -> _codecPreference.value
                }
                val useHevc = wanted == "h265" && hasHardwareEncoder(MediaFormat.MIMETYPE_VIDEO_HEVC)
                if (wanted == "h265" && !useHevc) {
                    ConnectionRepository.appendLog("[WEBCAM] Sem encoder HEVC por hardware — a usar H.264")
                }
                val mime = if (useHevc) MediaFormat.MIMETYPE_VIDEO_HEVC else MediaFormat.MIMETYPE_VIDEO_AVC
                val codecByte: Byte = if (useHevc) 0x02 else 0x01
                _activeCodec.value = if (useHevc) "H.265" else "H.264"

                // Verificar resolução e FPS suportados antes de iniciar
                val supported = getSupportedResolutions(context)
                val targetResolutionSupported = supported.any {
                    (it["width"] as? Int) == width && (it["height"] as? Int) == height
                }
                if (supported.isNotEmpty() && !targetResolutionSupported) {
                    ConnectionRepository.appendLog("[WEBCAM] Aviso: Resolução ${width}x${height} não está listada como suportada nativamente. O CameraX tentará fazer o melhor ajuste.")
                }

                val format = MediaFormat.createVideoFormat(mime, width, height).apply {
                    setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
                    setInteger(MediaFormat.KEY_BIT_RATE, bitrateFor(width, height, useHevc))
                    setInteger(MediaFormat.KEY_FRAME_RATE, fps)
                    setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
                }
                val codec = MediaCodec.createEncoderByType(mime)
                codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
                val inputSurface = codec.createInputSurface()
                codec.start()
                mediaCodec = codec

                val previewBuilder = Preview.Builder().setTargetResolution(Size(width, height))
                // Pede à câmara o fps real — o KEY_FRAME_RATE do MediaCodec
                // só afeta a codificação, não a cadência de captura.
                try {
                    Camera2Interop.Extender(previewBuilder).setCaptureRequestOption(
                        CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE,
                        Range(fps, fps)
                    )
                } catch (e: Exception) {
                    ConnectionRepository.appendLog("[WEBCAM] Não foi possível pedir ${fps}fps à câmara: ${e.message}")
                }
                val encPreview = previewBuilder.build()
                encPreview.setSurfaceProvider { request ->
                    request.provideSurface(inputSurface, executor) { }
                }
                encoderPreview = encPreview

                val cameraProviderFuture = ProcessCameraProvider.getInstance(context)
                cameraProviderFuture.addListener({
                    try {
                        val provider = cameraProviderFuture.get()
                        cameraProvider = provider
                        cameraInfos = provider.availableCameraInfos
                        if (lensIndex >= cameraInfos.size) lensIndex = 0
                        updateLensLabel()
                        bindUseCases()
                    } catch (e: Exception) {
                        reportError("Falha ao vincular a câmara: ${e.message}")
                        stopOnMain()
                    }
                }, executor)

                _isStreaming.value = true

                // Rede fora da main thread (NetworkOnMainThreadException).
                // O PC espera até 10s pelo stream.
                streamJob = CoroutineScope(Dispatchers.IO).launch {
                    var out: java.io.OutputStream? = null
                    try {
                        val uniStream = connection.createStream(false)
                        out = uniStream.outputStream
                        val idBytes = ByteArray(8)
                        for (i in 0..7) idBytes[i] = ((packetId shr (56 - i * 8)) and 0xFF).toByte()
                        out.write(idBytes)
                        // Declaração de codec: 0x01=H.264, 0x02=H.265 — o
                        // daemon monta o pipeline a partir DISTO, não do
                        // que pediu (cobre o fallback sem HEVC).
                        out.write(byteArrayOf(codecByte))

                        val bufferInfo = MediaCodec.BufferInfo()
                        while (isActive) {
                            val outIndex = codec.dequeueOutputBuffer(bufferInfo, 100_000)
                            if (outIndex >= 0) {
                                val encoded = codec.getOutputBuffer(outIndex) ?: continue
                                val bytes = ByteArray(bufferInfo.size)
                                encoded.position(bufferInfo.offset)
                                encoded.get(bytes)
                                out.write(bytes)
                                codec.releaseOutputBuffer(outIndex, false)
                            }
                        }
                    } catch (e: Exception) {
                        ConnectionRepository.appendLog("[WEBCAM] Stream ended or error: ${e.message}")
                    } finally {
                        try { out?.close() } catch (e: Exception) {}
                        _isStreaming.value = false
                    }
                }
            } catch (e: Exception) {
                reportError("Falha ao iniciar a câmara: ${e.message}")
                stopOnMain()
            }
        }
    }

    fun attachPreview(surfaceProvider: Preview.SurfaceProvider) {
        val preview = Preview.Builder().build()
        preview.setSurfaceProvider(surfaceProvider)
        screenPreview = preview
        bindUseCases()
    }

    fun detachPreview() {
        screenPreview = null
        bindUseCases()
    }

    /// Cicla por TODAS as câmaras expostas (frontal/traseira/ultra-wide/
    /// tele, conforme o aparelho) sem interromper encoder nem stream.
    fun nextLens() {
        if (cameraInfos.size <= 1) return
        lensIndex = (lensIndex + 1) % cameraInfos.size
        updateLensLabel()
        bindUseCases()
    }

    private fun updateLensLabel() {
        val info = cameraInfos.getOrNull(lensIndex) ?: return
        val facing = try {
            if (info.lensFacing == CameraSelector.LENS_FACING_FRONT) "FRONTAL" else "TRASEIRA"
        } catch (e: Exception) {
            "?"
        }
        _lensLabel.value = "LENTE ${lensIndex + 1}/${cameraInfos.size} · $facing"
    }

    /// Rotação em passos de 90° — aplicada no PC (videoflip), o encoder
    /// continua a mandar a imagem tal como a câmara a captura.
    fun rotate90() {
        _rotationDeg.value = (_rotationDeg.value + 90) % 360
        sendTransform()
    }

    fun toggleMirror() {
        _mirrored.value = !_mirrored.value
        sendTransform()
    }

    private fun sendTransform() {
        CoroutineScope(Dispatchers.IO).launch {
            ConnectionRepository.sendOneWayPacket(
                "webcam.transform",
                mapOf("rotation" to _rotationDeg.value, "mirror" to _mirrored.value)
            )
        }
    }

    fun cycleCodecPreference() {
        _codecPreference.value = when (_codecPreference.value) {
            "auto" -> "h264"
            "h264" -> "h265"
            else -> "auto"
        }
    }

    /// Microfone do telemóvel → PC (PCM 16-bit 48kHz mono, cru — na LAN
    /// não vale a pena codec). Segue o padrão share.file: anuncia com o
    /// próprio id e abre o uni stream com esse id.
    @SuppressLint("MissingPermission")
    fun startMic(context: Context) {
        if (micJob != null) return
        if (ContextCompat.checkSelfPermission(context, android.Manifest.permission.RECORD_AUDIO)
            != PackageManager.PERMISSION_GRANTED
        ) {
            ConnectionRepository.appendLog("[WEBCAM] Permissão de microfone não concedida")
            return
        }
        val conn = activeConn ?: return

        micJob = CoroutineScope(Dispatchers.IO).launch {
            var out: java.io.OutputStream? = null
            var recorder: AudioRecord? = null
            try {
                val packetId = ConnectionRepository.sendAnnouncedPacket(
                    "webcam.mic_start",
                    mapOf("rate" to 48000, "channels" to 1)
                ) ?: return@launch

                val uniStream = conn.createStream(false)
                out = uniStream.outputStream
                val idBytes = ByteArray(8)
                for (i in 0..7) idBytes[i] = ((packetId shr (56 - i * 8)) and 0xFF).toByte()
                out.write(idBytes)

                val minBuf = AudioRecord.getMinBufferSize(
                    48000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT
                )
                recorder = AudioRecord(
                    MediaRecorder.AudioSource.VOICE_COMMUNICATION,
                    48000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT,
                    maxOf(minBuf, 9600)
                )
                recorder.startRecording()
                _isMicOn.value = true

                val buf = ByteArray(4096)
                while (isActive) {
                    val n = recorder.read(buf, 0, buf.size)
                    if (n > 0) out.write(buf, 0, n)
                }
            } catch (e: Exception) {
                ConnectionRepository.appendLog("[WEBCAM] Microfone terminou: ${e.message}")
            } finally {
                runCatching { recorder?.stop() }
                recorder?.release()
                try { out?.close() } catch (e: Exception) {}
                _isMicOn.value = false
            }
        }
    }

    fun stopMic() {
        micJob?.cancel()
        micJob = null
        _isMicOn.value = false
        CoroutineScope(Dispatchers.IO).launch {
            ConnectionRepository.sendOneWayPacket("webcam.mic_stop", emptyMap())
        }
    }

    // Só na main thread (attachPreview/detachPreview/nextLens vêm do
    // Compose; o listener do provider usa o mainExecutor).
    private fun bindUseCases() {
        val provider = cameraProvider ?: return
        val useCases = listOfNotNull(encoderPreview, screenPreview).toTypedArray()
        if (useCases.isEmpty()) return
        val selector = cameraInfos.getOrNull(lensIndex)?.cameraSelector ?: CameraSelector.DEFAULT_BACK_CAMERA
        try {
            provider.unbindAll()
            camera = provider.bindToLifecycle(ProcessLifecycleOwner.get(), selector, *useCases)
        } catch (e: Exception) {
            reportError("Falha ao vincular use cases da câmara: ${e.message}")
        }
    }

    fun focusAt(point: androidx.camera.core.MeteringPoint) {
        val action = androidx.camera.core.FocusMeteringAction.Builder(
            point,
            androidx.camera.core.FocusMeteringAction.FLAG_AF or androidx.camera.core.FocusMeteringAction.FLAG_AE
        )
            .setAutoCancelDuration(3, java.util.concurrent.TimeUnit.SECONDS)
            .build()
        camera?.cameraControl?.startFocusAndMetering(action)
    }

    fun focusAtNormalized(x: Float, y: Float) {
        val factory = androidx.camera.core.SurfaceOrientedMeteringPointFactory(1.0f, 1.0f)
        val point = factory.createPoint(x.coerceIn(0f, 1f), y.coerceIn(0f, 1f))
        focusAt(point)
    }

    fun setZoomRatio(ratio: Float) {
        val clamped = ratio.coerceAtLeast(1.0f)
        _zoomRatio.value = clamped
        camera?.cameraControl?.setZoomRatio(clamped)
    }

    fun toggleTorch(on: Boolean) {
        _isTorchOn.value = on
        camera?.cameraControl?.enableTorch(on)
    }

    // Pode ser chamado de qualquer thread — despacha o teardown do
    // CameraX para a main (unbindAll fora dela lança "Not in
    // application's main thread").
    fun stop() {
        val exec = mainExecutor
        if (exec != null) {
            exec.execute { stopOnMain() }
        } else {
            stopOnMain()
        }
    }

    private fun stopOnMain() {
        micJob?.cancel()
        micJob = null
        _isMicOn.value = false
        _isTorchOn.value = false
        _zoomRatio.value = 1.0f
        streamJob?.cancel()
        streamJob = null
        camera = null
        cameraProvider?.unbindAll()
        cameraProvider = null
        encoderPreview = null
        screenPreview = null
        mediaCodec?.let { runCatching { it.stop() }; runCatching { it.release() } }
        mediaCodec = null
        _isStreaming.value = false
    }

    private fun bitrateFor(width: Int, height: Int, hevc: Boolean): Int {
        val h264 = when {
            height <= 480 -> 1_200_000
            height <= 720 -> 3_000_000
            else -> 6_000_000
        }
        // HEVC atinge qualidade equivalente com ~60% do bitrate.
        return if (hevc) (h264 * 6) / 10 else h264
    }
}
