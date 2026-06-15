package com.emulator.gb

object EmulatorBridge {
    init {
        System.loadLibrary("frontend_android_jni")
    }

    external fun init(romBytes: ByteArray): Boolean
    external fun runFrame(pixelsOut: IntArray): Boolean
    external fun setInputs(buttons: Byte)
    external fun getAudioSamples(): FloatArray?
    external fun saveState(path: String): Boolean
    external fun loadState(path: String): Boolean
}
