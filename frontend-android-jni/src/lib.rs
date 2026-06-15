use std::sync::Mutex;
use jni::JNIEnv;
use jni::objects::{JClass, JString, JByteArray, JIntArray};
use jni::sys::{jboolean, jfloatArray, jbyte, JNI_TRUE, JNI_FALSE};
use core_gb::GameBoy;

static EMULATOR: Mutex<Option<GameBoy>> = Mutex::new(None);

const PALETTE: [i32; 4] = [
    0xFFFFFFFFu32 as i32, // White
    0xFFAAAAAAu32 as i32, // Light gray
    0xFF555555u32 as i32, // Dark gray
    0xFF000000u32 as i32, // Black
];

#[no_mangle]
pub extern "system" fn Java_com_emulator_gb_EmulatorBridge_init<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    rom_bytes: JByteArray<'local>,
    save_dir: JString<'local>,
) -> jboolean {
    let save_dir_str: String = match env.get_string(&save_dir) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };

    if std::env::set_current_dir(&save_dir_str).is_err() {
        return JNI_FALSE;
    }

    let rom = match env.convert_byte_array(&rom_bytes) {
        Ok(bytes) => bytes,
        Err(_) => return JNI_FALSE,
    };

    match GameBoy::from_rom_bytes(rom) {
        Ok(gb) => {
            let mut lock = EMULATOR.lock().unwrap();
            *lock = Some(gb);
            JNI_TRUE
        }
        Err(_) => JNI_FALSE,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_emulator_gb_EmulatorBridge_runFrame<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    pixels_out: JIntArray<'local>,
) -> jboolean {
    let mut lock = EMULATOR.lock().unwrap();
    if let Some(ref mut gb) = *lock {
        if gb.run_frame().is_err() {
            return JNI_FALSE;
        }

        let framebuffer = gb.framebuffer();
        let mut rust_pixels = [0i32; 160 * 144];
        for (i, &val) in framebuffer.iter().enumerate() {
            rust_pixels[i] = PALETTE[(val & 3) as usize];
        }

        if env.set_int_array_region(&pixels_out, 0, &rust_pixels).is_err() {
            return JNI_FALSE;
        }

        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[no_mangle]
pub extern "system" fn Java_com_emulator_gb_EmulatorBridge_setInputs<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    buttons: jbyte,
) {
    let mut lock = EMULATOR.lock().unwrap();
    if let Some(ref mut gb) = *lock {
        gb.set_button_state(buttons as u8);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_emulator_gb_EmulatorBridge_getAudioSamples<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jfloatArray {
    let mut lock = EMULATOR.lock().unwrap();
    let samples = if let Some(ref mut gb) = *lock {
        gb.take_audio_samples()
    } else {
        Vec::new()
    };

    let jarray = match env.new_float_array(samples.len() as jni::sys::jsize) {
        Ok(arr) => arr,
        Err(_) => return std::ptr::null_mut(),
    };

    if !samples.is_empty() {
        if env.set_float_array_region(&jarray, 0, &samples).is_err() {
            return std::ptr::null_mut();
        }
    }

    jarray.into_raw()
}

#[no_mangle]
pub extern "system" fn Java_com_emulator_gb_EmulatorBridge_saveState<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
) -> jboolean {
    let path_str: String = match env.get_string(&path) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };

    let lock = EMULATOR.lock().unwrap();
    if let Some(ref gb) = *lock {
        match gb.save_state_to_memory() {
            Ok(bytes) => {
                // Drop the lock on EMULATOR immediately so the emulation loop
                // is not blocked during disk write.
                drop(lock);

                // Write the state to file in a background thread.
                std::thread::spawn(move || {
                    let _ = std::fs::write(path_str, bytes);
                });
                JNI_TRUE
            }
            Err(_) => JNI_FALSE,
        }
    } else {
        JNI_FALSE
    }
}

#[no_mangle]
pub extern "system" fn Java_com_emulator_gb_EmulatorBridge_loadState<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
) -> jboolean {
    let path_str: String = match env.get_string(&path) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };

    match GameBoy::load_state(path_str) {
        Ok(gb) => {
            let mut lock = EMULATOR.lock().unwrap();
            *lock = Some(gb);
            JNI_TRUE
        }
        Err(_) => JNI_FALSE,
    }
}
