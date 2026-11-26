//! JNI bridge for a real Android `EditText`.
//!
//! `NativeActivity` has no `GameTextInput`, and winit deliberately ignores
//! `TextEvent`s, so there is no way to get full IME support through the event
//! loop alone. Instead the app hosts a transparent `EditText` over the surface.
//! Android gives that view real IME behaviour — composition, suggestions,
//! selection handles, select all and copy/paste — and this module mirrors its
//! state into the engine.
//!
//! Java calls the static native methods below on the UI thread. They push a
//! small event into the winit event loop, where `Runner` applies it to `App`.
//! Going the other way, `sync()` tells Java which text and selection the
//! engine currently has, and whether the keyboard should be shown.

use jni::objects::{JString, JValue};
use jni::sys::{jclass, jint, jstring};
use jni::{JNIEnv, JavaVM};
use std::sync::Mutex;
use winit::event_loop::EventLoopProxy;
use winit::platform::android::activity::AndroidApp as WinitAndroidApp;

/// An IME change to apply on the event-loop thread.
#[derive(Clone, Debug)]
pub enum ImeEvent {
    /// The editor's complete state. Indices are character indices, not UTF-16.
    Text {
        text: String,
        start: usize,
        end: usize,
        composing_start: usize,
        composing_end: usize,
    },
    /// The IME's Done/Search/Go action.
    Action(jint),
}

static PROXY: Mutex<Option<EventLoopProxy<ImeEvent>>> = Mutex::new(None);

/// Installs the event-loop proxy once, at startup.
pub fn install(proxy: EventLoopProxy<ImeEvent>) {
    *PROXY.lock().expect("IME proxy lock") = Some(proxy);
}

fn send(event: ImeEvent) {
    if let Some(proxy) = PROXY.lock().expect("IME proxy lock").as_ref() {
        let _ = proxy.send_event(event);
    }
}

/// Called by `MreActivity.onTextChanged` with the editor's full state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_mre_MreActivity_onTextChanged(
    env: *mut jni::sys::JNIEnv,
    _class: jclass,
    text: jstring,
    sel_start: jint,
    sel_end: jint,
    composing_start: jint,
    composing_end: jint,
) {
    let env = unsafe { JNIEnv::from_raw(env) }.expect("valid JNIEnv");
    let java_text = unsafe { JString::from_raw(text) };
    let text = env
        .get_string(&java_text)
        .map(|value| value.into())
        .unwrap_or_default();

    send(ImeEvent::Text {
        text,
        start: sel_start.max(0) as usize,
        end: sel_end.max(0) as usize,
        composing_start: composing_start.max(0) as usize,
        composing_end: composing_end.max(0) as usize,
    });
}

/// Called by `MreActivity.onEditorAction` for Done/Search/Go.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_mre_MreActivity_onEditorAction(
    _env: *mut jni::sys::JNIEnv,
    _class: jclass,
    action: jint,
) {
    send(ImeEvent::Action(action));
}

/// Pushes the engine's editor state into the Java view.
///
/// `start` and `end` are character indices; Java converts them to UTF-16
/// offsets, which is what `EditText.setSelection` expects.
pub fn sync(android: &WinitAndroidApp, text: &str, start: usize, end: usize, show_keyboard: bool) {
    let vm = unsafe { JavaVM::from_raw(android.vm_as_ptr().cast()) }.expect("valid JavaVM");
    let result = vm.attach_current_thread(|env| {
        let value = env.new_string(text)?;
        env.call_static_method(
            "dev/mre/MreActivity",
            "sync",
            "(Ljava/lang/String;IIZ)V",
            &[
                JValue::from(&value),
                JValue::Int(start.min(i32::MAX as usize) as jint),
                JValue::Int(end.min(i32::MAX as usize) as jint),
                JValue::Bool(u8::from(show_keyboard)),
            ],
        )?;
        Ok(())
    });
    if let Err(error) = result {
        eprintln!("mre-android: IME sync failed: {error:?}");
    }
}
