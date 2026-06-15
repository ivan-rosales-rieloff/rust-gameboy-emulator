package com.emulator.gb

import android.content.Context
import android.graphics.Bitmap
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.net.Uri
import android.os.Bundle
import android.content.res.Configuration
import androidx.compose.ui.platform.LocalConfiguration
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.io.File

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            EmulatorScreen()
        }
    }
}

// Button bits matching emulator
const val BTN_A: Byte = 0x01
const val BTN_B: Byte = 0x02
const val BTN_SELECT: Byte = 0x04
const val BTN_START: Byte = 0x08
const val BTN_RIGHT: Byte = 0x10
const val BTN_LEFT: Byte = 0x20
const val BTN_UP: Byte = 0x40
const val BTN_DOWN: Byte = -0x80

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EmulatorScreen() {
    val context = LocalContext.current
    val configuration = LocalConfiguration.current
    val isLandscape = configuration.orientation == Configuration.ORIENTATION_LANDSCAPE

    var isRomLoaded by remember { mutableStateOf(false) }
    var isRunning by remember { mutableStateOf(false) }
    var romTitle by remember { mutableStateOf("No ROM Loaded") }
    var statusMessage by remember { mutableStateOf("") }

    val pixelBuffer = remember { IntArray(160 * 144) }
    val bitmap = remember { Bitmap.createBitmap(160, 144, Bitmap.Config.ARGB_8888) }
    var frameTick by remember { mutableStateOf(0) }

    var buttonState by remember { mutableStateOf<Byte>(0) }

    // ROM file picker
    val romPickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument()
    ) { uri: Uri? ->
        if (uri != null) {
            try {
                context.contentResolver.openInputStream(uri)?.use { stream ->
                    val bytes = stream.readBytes()
                    if (EmulatorBridge.init(bytes, context.filesDir.absolutePath)) {
                        isRomLoaded = true
                        isRunning = true
                        romTitle = getFileName(context, uri) ?: "Game Boy Game"
                        statusMessage = "Loaded successfully!"
                    } else {
                        statusMessage = "Failed to initialize ROM"
                    }
                }
            } catch (e: Exception) {
                statusMessage = "Error: ${e.message}"
            }
        }
    }

    // Input state updates helper
    fun updateInput(bit: Byte, isPressed: Boolean) {
        buttonState = if (isPressed) {
            (buttonState.toInt() or bit.toInt()).toByte()
        } else {
            (buttonState.toInt() and bit.toInt().inv()).toByte()
        }
        if (isRomLoaded) {
            EmulatorBridge.setInputs(buttonState)
        }
    }

    // Emulator frame loop
    LaunchedEffect(isRomLoaded, isRunning) {
        if (isRomLoaded && isRunning) {
            // Setup AudioTrack for stereo float playback at 44100Hz
            val minBufferSize = AudioTrack.getMinBufferSize(
                44100,
                AudioFormat.CHANNEL_OUT_STEREO,
                AudioFormat.ENCODING_PCM_FLOAT
            )
            // Increase buffer size to at least 16384 bytes to prevent stutter
            val audioTrack = AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_GAME)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build()
                )
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                        .setSampleRate(44100)
                        .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
                        .build()
                )
                .setBufferSizeInBytes(minBufferSize.coerceAtLeast(32768))
                .setTransferMode(AudioTrack.MODE_STREAM)
                .build()

            var threadRunning = true

            // Run in a dedicated high-priority thread to prevent audio starvation and OS preemption
            val emulatorThread = Thread {
                android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_AUDIO)
                try {
                    audioTrack.play()
                    while (threadRunning) {
                        val frameSuccess = EmulatorBridge.runFrame(pixelBuffer)
                        if (frameSuccess) {
                            bitmap.setPixels(pixelBuffer, 0, 160, 0, 0, 160, 144)
                            frameTick++

                            // Process Audio Samples
                            val samples = EmulatorBridge.getAudioSamples()
                            if (samples != null && samples.isNotEmpty()) {
                                audioTrack.write(samples, 0, samples.size, AudioTrack.WRITE_BLOCKING)
                            }
                        } else {
                            Thread.sleep(10)
                        }
                    }
                } catch (e: Exception) {
                    Log.e("Emulator", "Error in emulation thread: ${e.message}", e)
                } finally {
                    try {
                        audioTrack.stop()
                    } catch (e: Exception) {}
                    audioTrack.release()
                }
            }
            emulatorThread.name = "EmulatorCoreThread"
            emulatorThread.start()

            try {
                awaitCancellation()
            } finally {
                threadRunning = false
                emulatorThread.join(1000)
            }
        }
    }

    // Outer layout
    if (isLandscape) {
        // Landscape Layout
        Row(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.verticalGradient(
                        colors = listOf(Color(0xFF1E2022), Color(0xFF121214))
                    )
                )
                .padding(12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            // Left Controls Area (D-Pad)
            Box(
                modifier = Modifier
                    .weight(1.2f)
                    .fillMaxHeight(),
                contentAlignment = Alignment.Center
            ) {
                Box(
                    modifier = Modifier
                        .size(130.dp)
                        .background(Color(0xFF393E46), shape = CircleShape)
                        .padding(6.dp),
                    contentAlignment = Alignment.Center
                ) {
                    // Up Button
                    DPadButton(
                        label = "▲",
                        modifier = Modifier
                            .align(Alignment.TopCenter)
                            .size(36.dp),
                        onPress = { isPressed -> updateInput(BTN_UP, isPressed) }
                    )
                    // Down Button
                    DPadButton(
                        label = "▼",
                        modifier = Modifier
                            .align(Alignment.BottomCenter)
                            .size(36.dp),
                        onPress = { isPressed -> updateInput(BTN_DOWN, isPressed) }
                    )
                    // Left Button
                    DPadButton(
                        label = "◀",
                        modifier = Modifier
                            .align(Alignment.CenterStart)
                            .size(36.dp),
                        onPress = { isPressed -> updateInput(BTN_LEFT, isPressed) }
                    )
                    // Right Button
                    DPadButton(
                        label = "▶",
                        modifier = Modifier
                            .align(Alignment.CenterEnd)
                            .size(36.dp),
                        onPress = { isPressed -> updateInput(BTN_RIGHT, isPressed) }
                    )
                }
            }

            // Center Screen Area
            Column(
                modifier = Modifier
                    .weight(2f)
                    .fillMaxHeight(),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center
            ) {
                // Info Header (ROM title & Status)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 8.dp, vertical = 2.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = if (romTitle.length > 20) romTitle.substring(0, 18) + "..." else romTitle,
                        color = Color.White,
                        fontSize = 13.sp,
                        fontWeight = FontWeight.Bold
                    )
                    if (statusMessage.isNotEmpty()) {
                        Text(
                            text = statusMessage,
                            color = Color.Gray,
                            fontSize = 11.sp
                        )
                    }
                }

                // Game Screen Box
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .aspectRatio(160f / 144f)
                        .clip(RoundedCornerShape(8.dp))
                        .shadow(elevation = 8.dp, shape = RoundedCornerShape(8.dp))
                        .background(Color.Black),
                    contentAlignment = Alignment.Center
                ) {
                    if (isRomLoaded) {
                        Canvas(
                            modifier = Modifier.fillMaxSize()
                        ) {
                            @Suppress("UNUSED_EXPRESSION")
                            frameTick // read state in DrawScope to invalidate only the draw phase
                            drawIntoCanvas { canvas ->
                                val dstRect = android.graphics.Rect(0, 0, size.width.toInt(), size.height.toInt())
                                val paint = android.graphics.Paint().apply {
                                    isAntiAlias = false
                                    isFilterBitmap = false
                                }
                                canvas.nativeCanvas.drawBitmap(bitmap, null, dstRect, paint)
                            }
                        }
                    } else {
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            CircularProgressIndicator(color = Color(0xFF00ADB5))
                            Spacer(modifier = Modifier.height(8.dp))
                            Button(
                                onClick = { romPickerLauncher.launch(arrayOf("*/*")) },
                                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF00ADB5)),
                                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 4.dp),
                                modifier = Modifier.height(28.dp)
                            ) {
                                Text("LOAD ROM", fontSize = 11.sp, color = Color.White)
                            }
                        }
                    }
                }

                Spacer(modifier = Modifier.height(6.dp))

                // Bottom control panel (Select, Start, Quick Save/Load)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 4.dp),
                    horizontalArrangement = Arrangement.SpaceEvenly,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    MenuButton(
                        label = "SELECT",
                        onPress = { isPressed -> updateInput(BTN_SELECT, isPressed) }
                    )

                    if (isRomLoaded) {
                        Button(
                            onClick = {
                                val stateFile = File(context.filesDir, "quick_save.state").absolutePath
                                if (EmulatorBridge.saveState(stateFile)) {
                                    statusMessage = "Saved!"
                                } else {
                                    statusMessage = "Save failed"
                                }
                            },
                            colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF393E46)),
                            contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                            modifier = Modifier.height(26.dp),
                            shape = RoundedCornerShape(4.dp)
                        ) {
                            Text("SAVE", fontSize = 10.sp, color = Color.White)
                        }

                        Button(
                            onClick = {
                                val stateFile = File(context.filesDir, "quick_save.state").absolutePath
                                if (File(stateFile).exists()) {
                                    if (EmulatorBridge.loadState(stateFile)) {
                                        statusMessage = "Loaded!"
                                    } else {
                                        statusMessage = "Load failed"
                                    }
                                } else {
                                    statusMessage = "No save"
                                }
                            },
                            colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF393E46)),
                            contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                            modifier = Modifier.height(26.dp),
                            shape = RoundedCornerShape(4.dp)
                        ) {
                            Text("LOAD", fontSize = 10.sp, color = Color.White)
                        }
                    } else {
                        Button(
                            onClick = { romPickerLauncher.launch(arrayOf("*/*")) },
                            colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF00ADB5)),
                            contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                            modifier = Modifier.height(26.dp),
                            shape = RoundedCornerShape(4.dp)
                        ) {
                            Text("LOAD ROM", fontSize = 10.sp, color = Color.White)
                        }
                    }

                    MenuButton(
                        label = "START",
                        onPress = { isPressed -> updateInput(BTN_START, isPressed) }
                    )
                }
            }

            // Right Controls Area (A/B Buttons)
            Box(
                modifier = Modifier
                    .weight(1.2f)
                    .fillMaxHeight(),
                contentAlignment = Alignment.Center
            ) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    // B Button
                    ActionButton(
                        label = "B",
                        modifier = Modifier.size(52.dp),
                        color = Color(0xFFEEEEEE),
                        onPress = { isPressed -> updateInput(BTN_B, isPressed) }
                    )

                    // A Button
                    ActionButton(
                        label = "A",
                        modifier = Modifier.size(52.dp),
                        color = Color(0xFF00ADB5),
                        onPress = { isPressed -> updateInput(BTN_A, isPressed) }
                    )
                }
            }
        }
    } else {
        // Portrait Layout (Original Layout)
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.verticalGradient(
                        colors = listOf(Color(0xFF1E2022), Color(0xFF121214))
                    )
                ),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            // App header / Title bar
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Column {
                    Text(
                        text = romTitle,
                        color = Color.White,
                        fontSize = 18.sp,
                        fontWeight = FontWeight.Bold
                    )
                    if (statusMessage.isNotEmpty()) {
                        Text(
                            text = statusMessage,
                            color = Color.Gray,
                            fontSize = 12.sp
                        )
                    }
                }

                Button(
                    onClick = { romPickerLauncher.launch(arrayOf("*/*")) },
                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF00ADB5))
                ) {
                    Text(text = "Load ROM", color = Color.White)
                }
            }

            // Emulator Screen Console Area
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(horizontal = 24.dp, vertical = 8.dp),
                contentAlignment = Alignment.Center
            ) {
                Box(
                    modifier = Modifier
                        .aspectRatio(160f / 144f)
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(12.dp))
                        .shadow(elevation = 16.dp, shape = RoundedCornerShape(12.dp))
                        .background(Color.Black)
                        .pointerInput(Unit) {},
                    contentAlignment = Alignment.Center
                ) {
                    if (isRomLoaded) {
                        Canvas(
                            modifier = Modifier.fillMaxSize()
                        ) {
                            @Suppress("UNUSED_EXPRESSION")
                            frameTick // read state in DrawScope to invalidate only the draw phase
                            drawIntoCanvas { canvas ->
                                val dstRect = android.graphics.Rect(0, 0, size.width.toInt(), size.height.toInt())
                                val paint = android.graphics.Paint().apply {
                                    isAntiAlias = false
                                    isFilterBitmap = false // Keep pixel art crisp
                                }
                                canvas.nativeCanvas.drawBitmap(bitmap, null, dstRect, paint)
                            }
                        }
                    } else {
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            CircularProgressIndicator(color = Color(0xFF00ADB5))
                            Spacer(modifier = Modifier.height(16.dp))
                            Text(
                                text = "Please load a Game Boy ROM to begin",
                                color = Color.LightGray,
                                fontSize = 14.sp,
                                textAlign = TextAlign.Center
                            )
                        }
                    }
                }
            }

            // Emulator State Save / Load Action Row
            if (isRomLoaded) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 32.dp, vertical = 8.dp),
                    horizontalArrangement = Arrangement.SpaceEvenly
                ) {
                    Button(
                        onClick = {
                            val stateFile = File(context.filesDir, "quick_save.state").absolutePath
                            if (EmulatorBridge.saveState(stateFile)) {
                                statusMessage = "Quick state saved!"
                            } else {
                                statusMessage = "Save state failed"
                            }
                        },
                        colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF393E46)),
                        shape = RoundedCornerShape(8.dp)
                    ) {
                        Text("Save State", color = Color.White)
                    }

                    Button(
                        onClick = {
                            val stateFile = File(context.filesDir, "quick_save.state").absolutePath
                            if (File(stateFile).exists()) {
                                if (EmulatorBridge.loadState(stateFile)) {
                                    statusMessage = "Quick state loaded!"
                                } else {
                                    statusMessage = "Load state failed"
                                }
                            } else {
                                statusMessage = "No save state found"
                            }
                        },
                        colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF393E46)),
                        shape = RoundedCornerShape(8.dp)
                    ) {
                        Text("Load State", color = Color.White)
                    }
                }
            }

            // Controls Console Area
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(Color(0xFF222831))
                    .padding(vertical = 24.dp)
            ) {
                // Upper Control panel: D-Pad (Left) and A/B Buttons (Right)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 32.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    // D-Pad
                    Box(
                        modifier = Modifier
                            .size(140.dp)
                            .background(Color(0xFF393E46), shape = CircleShape)
                            .padding(8.dp),
                        contentAlignment = Alignment.Center
                    ) {
                        // Up Button
                        DPadButton(
                            label = "▲",
                            modifier = Modifier
                                .align(Alignment.TopCenter)
                                .size(40.dp),
                            onPress = { isPressed -> updateInput(BTN_UP, isPressed) }
                        )
                        // Down Button
                        DPadButton(
                            label = "▼",
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .size(40.dp),
                            onPress = { isPressed -> updateInput(BTN_DOWN, isPressed) }
                        )
                        // Left Button
                        DPadButton(
                            label = "◀",
                            modifier = Modifier
                                .align(Alignment.CenterStart)
                                .size(40.dp),
                            onPress = { isPressed -> updateInput(BTN_LEFT, isPressed) }
                        )
                        // Right Button
                        DPadButton(
                            label = "▶",
                            modifier = Modifier
                                .align(Alignment.CenterEnd)
                                .size(40.dp),
                            onPress = { isPressed -> updateInput(BTN_RIGHT, isPressed) }
                        )
                    }

                    // Action Buttons: A and B
                    Row(
                        modifier = Modifier.padding(top = 16.dp),
                        horizontalArrangement = Arrangement.spacedBy(16.dp)
                    ) {
                        // B Button
                        ActionButton(
                            label = "B",
                            modifier = Modifier.size(60.dp),
                            color = Color(0xFFEEEEEE),
                            onPress = { isPressed -> updateInput(BTN_B, isPressed) }
                        )

                        // A Button
                        ActionButton(
                            label = "A",
                            modifier = Modifier.size(60.dp),
                            color = Color(0xFF00ADB5),
                            onPress = { isPressed -> updateInput(BTN_A, isPressed) }
                        )
                    }
                }

                Spacer(modifier = Modifier.height(24.dp))

                // Select & Start Row
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 48.dp),
                    horizontalArrangement = Arrangement.SpaceEvenly
                ) {
                    // Select
                    MenuButton(
                        label = "SELECT",
                        onPress = { isPressed -> updateInput(BTN_SELECT, isPressed) }
                    )

                    // Start
                    MenuButton(
                        label = "START",
                        onPress = { isPressed -> updateInput(BTN_START, isPressed) }
                    )
                }
            }
        }
    }
}

@Composable
fun DPadButton(
    label: String,
    modifier: Modifier = Modifier,
    onPress: (Boolean) -> Unit
) {
    Box(
        modifier = modifier
            .background(Color(0xFF222831), shape = RoundedCornerShape(4.dp))
            .pointerInput(Unit) {
                awaitPointerEventScope {
                    while (true) {
                        awaitFirstDown(requireUnconsumed = false)
                        onPress(true)
                        waitForUpOrCancellation()
                        onPress(false)
                    }
                }
            },
        contentAlignment = Alignment.Center
    ) {
        Text(text = label, color = Color.White, fontWeight = FontWeight.Bold, fontSize = 16.sp)
    }
}

@Composable
fun ActionButton(
    label: String,
    modifier: Modifier = Modifier,
    color: Color,
    onPress: (Boolean) -> Unit
) {
    Box(
        modifier = modifier
            .background(color, shape = CircleShape)
            .pointerInput(Unit) {
                awaitPointerEventScope {
                    while (true) {
                        awaitFirstDown(requireUnconsumed = false)
                        onPress(true)
                        waitForUpOrCancellation()
                        onPress(false)
                    }
                }
            },
        contentAlignment = Alignment.Center
    ) {
        Text(
            text = label,
            color = Color.Black,
            fontWeight = FontWeight.Bold,
            fontSize = 20.sp
        )
    }
}

@Composable
fun MenuButton(
    label: String,
    onPress: (Boolean) -> Unit
) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Box(
            modifier = Modifier
                .size(width = 64.dp, height = 18.dp)
                .background(Color(0xFF393E46), shape = RoundedCornerShape(12.dp))
                .pointerInput(Unit) {
                    awaitPointerEventScope {
                        while (true) {
                            awaitFirstDown(requireUnconsumed = false)
                            onPress(true)
                            waitForUpOrCancellation()
                            onPress(false)
                        }
                    }
                }
        )
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = label,
            color = Color.Gray,
            fontSize = 10.sp,
            fontWeight = FontWeight.Bold
        )
    }
}

private fun getFileName(context: Context, uri: Uri): String? {
    var result: String? = null
    if (uri.scheme == "content") {
        val cursor = context.contentResolver.query(uri, null, null, null, null)
        try {
            if (cursor != null && cursor.moveToFirst()) {
                val index = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                if (index != -1) {
                    result = cursor.getString(index)
                }
            }
        } finally {
            cursor?.close()
        }
    }
    if (result == null) {
        result = uri.path
        val cut = result?.lastIndexOf('/')
        if (cut != null && cut != -1) {
            result = result?.substring(cut + 1)
        }
    }
    return result
}
