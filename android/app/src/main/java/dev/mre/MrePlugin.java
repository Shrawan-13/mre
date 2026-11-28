package dev.mre;

import android.content.Context;

/**
 * A native extension point.
 *
 * Plugins are deliberately small: one stable name and one string-in,
 * string-out call. That keeps the Rust/Lua boundary deterministic and makes
 * every call easy to log. Structured data can be layered on later without
 * changing this interface.
 */
public interface MrePlugin {
    /** Stable Lua-visible name, e.g. "battery". */
    String name();

    /**
     * Handles a method call from Lua.
     *
     * @param method plugin-defined method name
     * @param args   opaque string argument ("" when unused)
     * @return opaque string result ("" when unused)
     */
    String call(String method, String args);

    /** Optional context accessor; default implementations ignore it. */
    default void onCreate(Context context) { }

    /** Called when the app is destroyed. */
    default void onDestroy() { }
}
