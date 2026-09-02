package com.example

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class ExampleRobolectricTest {

  @Test
  fun `read string from context`() {
    val context = ApplicationProvider.getApplicationContext<Context>()
    val appName = context.getString(R.string.app_name)
    assertEquals("HyprLink", appName)
  }

  @Test
  fun `test device identity generation`() {
    val context = ApplicationProvider.getApplicationContext<Context>()
    val identity = DeviceIdentity.getInstance(context)
    
    // Assert that the generated properties are not null or empty
    assert(identity.fingerprintHex.isNotEmpty())
    assert(identity.certificate.subjectDN.name.contains("Android-Phone-Client") || identity.certificate.subjectDN.name.contains("CN="))
    assert(identity.privateKey.algorithm == "EC")
    
    // Test that singleton instance works
    val identity2 = DeviceIdentity.getInstance(context)
    assertEquals(identity.fingerprintHex, identity2.fingerprintHex)
  }
}
