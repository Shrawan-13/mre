package dev.mre;

import android.content.Context;
import android.util.Log;
import java.util.LinkedHashMap;
import java.util.Map;

/** Registry called from Rust through JNI and populated by generated Plugins.java. */
public final class MrePluginRegistry {
    private static final String TAG = "MrePlugin";
    private static final Map<String, MrePlugin> PLUGINS = new LinkedHashMap<>();

    private MrePluginRegistry() { }

    /** Called once from MreActivity.onCreate, before the Lua VM starts. */
    public static void register(Context context) {
        synchronized (PLUGINS) {
            for (MrePlugin plugin : PLUGINS.values()) {
                try { plugin.onDestroy(); } catch (Exception ignored) { }
            }
            PLUGINS.clear();
            for (MrePlugin plugin : Plugins.create(context)) {
                if (plugin == null || plugin.name() == null) continue;
                try {
                    plugin.onCreate(context);
                    PLUGINS.put(plugin.name(), plugin);
                    Log.i(TAG, "registered " + plugin.name());
                } catch (Exception error) {
                    Log.e(TAG, "failed to register " + plugin.name(), error);
                }
            }
        }
    }

    /** Called from Rust (JNI) for `mre.platform_call(name, method, args)`. */
    public static String call(String name, String method, String args) {
        // Built-in app lifecycle, handled without a plugin so `mre.exit()`
        // works in every generated project.
        if ("app".equals(name) && "finish".equals(method)) {
            MreActivity activity = MreActivity.current();
            if (activity != null) activity.finish();
            return "ok";
        }
        synchronized (PLUGINS) {
            MrePlugin plugin = PLUGINS.get(name);
            if (plugin == null) {
                Log.w(TAG, "no plugin named " + name);
                return "";
            }
            try {
                String result = plugin.call(method, args == null ? "" : args);
                return result == null ? "" : result;
            } catch (Exception error) {
                Log.e(TAG, name + "." + method + " failed", error);
                return "";
            }
        }
    }
}
