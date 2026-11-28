//! JNI implementation of `mre.platform_call`.
//!
//! Java exposes one static method, `dev.mre.MrePluginRegistry.call`, so adding
//! a plugin never changes the engine. Calls are logged by the registry and
//! return a string; failures become `""` instead of crashing a frame.

use jni::JavaVM;
use jni::objects::{JObject, JString, JValue};
use jni::sys::jobject;
use winit::platform::android::activity::AndroidApp as WinitAndroidApp;

/// Builds the callback installed into `mre-lua`.
pub fn bridge(app: &WinitAndroidApp) -> impl Fn(&str, &str, &str) -> String + 'static {
    let app = app.clone();
    move |name, method, args| call(&app, name, method, args)
}

fn call(app: &WinitAndroidApp, name: &str, method: &str, args: &str) -> String {
    let Ok(vm) = (unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }) else {
        return String::new();
    };
    let Ok(mut env) = vm.attach_current_thread() else {
        return String::new();
    };
    // The system class loader cannot see app classes. Resolve the registry
    // through the live Activity's class, exactly like the IME bridge does.
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jobject) };
    let Ok(class) = env.get_object_class(&activity) else {
        return String::new();
    };
    let (Ok(java_name), Ok(java_method), Ok(java_args)) = (
        env.new_string(name),
        env.new_string(method),
        env.new_string(args),
    ) else {
        return String::new();
    };
    let Ok(result) = env.call_static_method(
        class,
        "call",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        &[
            JValue::from(&java_name),
            JValue::from(&java_method),
            JValue::from(&java_args),
        ],
    ) else {
        return String::new();
    };
    let JValue::Object(object) = result else {
        return String::new();
    };
    env.get_string(&JString::from(object))
        .map(|value| value.into())
        .unwrap_or_default()
}
