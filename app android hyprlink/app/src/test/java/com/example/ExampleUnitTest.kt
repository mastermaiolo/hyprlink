package com.example

import co.nstant.`in`.cbor.CborBuilder
import co.nstant.`in`.cbor.CborDecoder
import co.nstant.`in`.cbor.CborEncoder
import co.nstant.`in`.cbor.model.*
import org.junit.Assert.*
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream

class ExampleUnitTest {
  @Test
  fun addition_isCorrect() {
    assertEquals(4, 2 + 2)
  }

  @Test
  fun testNotificationPostWithoutActions() {
    val packet = Packet(
        id = 2522,
        type = "notification.post",
        body = mapOf(
            "key" to "0|com.android.shell|2020|HyprLink|2000",
            "app" to "Shell",
            "title" to "Teste",
            "text" to "3",
            "actions" to emptyList<Any>()
        ),
        hasPayload = false
    )
    val bytes = ConnectionRepository.encodePacket(packet)
    val hex = bytes.joinToString("") { "%02x".format(it) }
    println("FIXED_ENCODED_HEX: $hex")
    
    // The sequence for actions followed by body close must be 9f ff ff
    // 9f = start indefinite array (actions)
    // ff = break indefinite array (actions)
    // ff = break indefinite map (body)
    assertTrue("Should have actions closed and body closed (9fffff)", hex.contains("9fffff"))
    
    val decoded = ConnectionRepository.decodePacket(bytes)
    assertNotNull(decoded)
    assertEquals(packet.id, decoded?.id)
    assertEquals(packet.type, decoded?.type)
    val decodedBody = decoded?.body
    assertNotNull(decodedBody)
    assertEquals("0|com.android.shell|2020|HyprLink|2000", decodedBody?.get("key"))
    assertEquals("Shell", decodedBody?.get("app"))
    assertEquals("Teste", decodedBody?.get("title"))
    assertEquals("3", decodedBody?.get("text"))
    val actions = decodedBody?.get("actions") as? List<*>
    assertNotNull(actions)
    assertTrue("Actions list should be empty", actions!!.isEmpty())
  }

  @Test
  fun testNotificationPostWithActions() {
    val packet = Packet(
        id = 2523,
        type = "notification.post",
        body = mapOf(
            "key" to "0|com.whatsapp|100|HyprLink|1000",
            "app" to "WhatsApp",
            "title" to "Alice",
            "text" to "Olá tudo bem?",
            "actions" to listOf(
                mapOf("idx" to 0, "label" to "Responder", "is_reply" to true),
                mapOf("idx" to 1, "label" to "Marcar como lida", "is_reply" to false)
            )
        ),
        hasPayload = false
    )
    val bytes = ConnectionRepository.encodePacket(packet)
    val decoded = ConnectionRepository.decodePacket(bytes)
    assertNotNull(decoded)
    assertEquals(packet.id, decoded?.id)
    assertEquals(packet.type, decoded?.type)
    val decodedBody = decoded?.body
    assertNotNull(decodedBody)
    val actions = decodedBody?.get("actions") as? List<*>
    assertNotNull(actions)
    assertEquals(2, actions!!.size)
    val firstAction = actions[0] as? Map<*, *>
    assertEquals(0L, firstAction?.get("idx"))
    assertEquals("Responder", firstAction?.get("label"))
    assertEquals(true, firstAction?.get("is_reply"))
  }

  @Test
  fun testAudioStreamPlayerLifecycle() {
    assertFalse(AudioStreamPlayer.isPlaying.value)
    AudioStreamPlayer.stop()
    assertFalse(AudioStreamPlayer.isPlaying.value)
  }
}

