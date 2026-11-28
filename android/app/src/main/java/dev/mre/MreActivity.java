package dev.mre;

import android.app.Activity;
import android.app.NativeActivity;
import android.os.Bundle;
import android.text.Editable;
import android.text.InputType;
import android.text.Spanned;
import android.text.TextWatcher;
import android.util.Log;
import android.util.TypedValue;
import android.view.KeyEvent;
import android.view.ViewGroup;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputConnectionWrapper;
import android.view.inputmethod.InputMethodManager;
import android.view.inputmethod.TextAttribute;
import android.widget.EditText;
import android.widget.FrameLayout;

/**
 * A NativeActivity with a real, invisible EditText over the surface.
 *
 * NativeActivity has no GameTextInput, so IME events never reach the Rust
 * event loop. A normal Android view does: it gets composition, suggestions,
 * selection handles, select all and copy/paste for free. We forward its full
 * state to Rust through JNI and mirror Rust's state back into the view.
 */
public class MreActivity extends NativeActivity {
    // NativeActivity dlopen()s libmain.so itself; that does not register the
    // library with the app class loader, so Java cannot resolve our JNI
    // methods. Load it explicitly as well (dlopen returns the same handle).
    static {
        System.loadLibrary("main");
    }

    public static native void onTextChanged(
            String text, int selStart, int selEnd, int composeStart, int composeEnd);

    public static native void onEditorAction(int action);

    private static MreActivity instance;
    private static MreEditText editor;
    private static boolean syncing;
    /** True while an InputConnection call is reporting its own state. */
    private static boolean forwarding;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        instance = this;

        editor = new MreEditText(this);
        editor.setBackgroundColor(0x00000000);
        editor.setTextColor(0x00000000);
        editor.setCursorVisible(false);
        editor.setAlpha(0f);
        editor.setSingleLine(true);
        editor.setInputType(InputType.TYPE_CLASS_TEXT);
        editor.setImeOptions(EditorInfo.IME_ACTION_DONE);
        // Focus is enabled only by sync(true), i.e. only when an MRE text
        // field is focused. Otherwise the platform may focus this view on its
        // own and open the keyboard at launch.
        editor.setFocusable(false);
        editor.setFocusableInTouchMode(false);

        int dp = Math.max(1, (int) TypedValue.applyDimension(
                TypedValue.COMPLEX_UNIT_DIP, 1, getResources().getDisplayMetrics()));
        FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(dp, dp);
        params.leftMargin = -10 * dp;
        params.topMargin = -10 * dp;
        ((ViewGroup) getWindow().getDecorView()).addView(editor, params);

        editor.addTextChangedListener(new TextWatcher() {
            @Override public void beforeTextChanged(CharSequence s, int start, int count, int after) { }

            @Override public void onTextChanged(CharSequence s, int start, int before, int count) {
                // All mutations are reported by MreInputConnection after the
                // edit, which also observes deletions of an empty field and
                // raw KEYCODE_DEL events. TextWatcher is kept only for
                // diagnostics: echoing it here would duplicate those reports.
                if (!syncing && !forwarding) {
                    Log.d("MreIme", "text watcher=" + s + " " + start + "/" + before + "/" + count);
                }
            }

            @Override public void afterTextChanged(Editable s) { }
        });

        editor.setOnEditorActionListener((view, actionId, event) -> {
            MreActivity.onEditorAction(actionId);
            return true;
        });

        // Focus is driven by Rust: only a focused MRE text field shows the
        // keyboard. Do not request focus here, or the keyboard opens at launch.
    }

    /** The composing region, in character indices, or {-1, -1}. */
    private static int[] composingSpan(CharSequence text) {
        if (text instanceof Spanned) {
            Spanned spanned = (Spanned) text;
            for (Object span : spanned.getSpans(0, spanned.length(), Object.class)) {
                if ((spanned.getSpanFlags(span) & Spanned.SPAN_COMPOSING) != 0) {
                    String value = text.toString();
                    return new int[] {
                            charsBefore(value, spanned.getSpanStart(span)),
                            charsBefore(value, spanned.getSpanEnd(span))
                    };
                }
            }
        }
        return new int[] {-1, -1};
    }

    /** UTF-16 offset to Unicode scalar count, matching Rust's `chars()`. */
    private static int charsBefore(String text, int utf16) {
        int end = Math.max(0, Math.min(utf16, text.length()));
        return text.codePointCount(0, end);
    }

    /** Unicode scalar count to UTF-16 offset, for `EditText.setSelection`. */
    private static int utf16At(String text, int chars) {
        int count = Math.max(0, Math.min(chars, text.codePointCount(0, text.length())));
        return text.offsetByCodePoints(0, count);
    }

    /**
     * Sends the editor's complete state to Rust. Called after every IME edit,
     * including deletions that may leave the field empty (where TextWatcher
     * alone is not always enough) and raw key events from the soft keyboard.
     */
    private static void reportEditorState() {
        final EditText field = editor;
        if (field == null) return;
        String text = field.getText().toString();
        int[] composing = composingSpan(field.getText());
        Log.d("MreIme", "report text=" + text + " sel=" + field.getSelectionStart()
                + ".." + field.getSelectionEnd() + " composing=" + composing[0]
                + ".." + composing[1]);
        MreActivity.onTextChanged(
                text,
                charsBefore(text, field.getSelectionStart()),
                charsBefore(text, field.getSelectionEnd()),
                composing[0],
                composing[1]);
    }

    /**
     * An EditText that exposes its InputConnection so deletions can be
     * observed explicitly.
     *
     * Android IMEs delete through `deleteSurroundingText` (or its code-point
     * variant) and sometimes through raw `sendKeyEvent(KEYCODE_DEL)` calls.
     * A TextWatcher sees the resulting text change, but not deletions of an
     * empty field and not every keyboard's internal composition handling.
     * Wrapping the connection makes all three paths report to Rust exactly
     * once, while `EditText` keeps doing the real editing and keeps its
     * composing spans intact.
     */
    static class MreEditText extends EditText {
        MreEditText(MreActivity activity) {
            super(activity);
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
            InputConnection target = super.onCreateInputConnection(outAttrs);
            return target == null ? null : new MreInputConnection(target);
        }
    }

    /** Forwards editor state after each mutating InputConnection operation. */
    static class MreInputConnection extends InputConnectionWrapper {
        MreInputConnection(InputConnection target) {
            super(target, true);
        }

        private boolean reported(boolean result) {
            if (result) reportEditorState();
            return result;
        }

        @Override
        public boolean deleteSurroundingText(int beforeLength, int afterLength) {
            forwarding = true;
            try {
                return reported(super.deleteSurroundingText(beforeLength, afterLength));
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean deleteSurroundingTextInCodePoints(int beforeLength, int afterLength) {
            forwarding = true;
            try {
                return reported(super.deleteSurroundingTextInCodePoints(beforeLength, afterLength));
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean commitText(CharSequence text, int newCursorPosition) {
            forwarding = true;
            try {
                return reported(super.commitText(text, newCursorPosition));
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean setComposingText(CharSequence text, int newCursorPosition) {
            forwarding = true;
            try {
                return reported(super.setComposingText(text, newCursorPosition));
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean finishComposingText() {
            forwarding = true;
            try {
                return reported(super.finishComposingText());
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean setSelection(int start, int end) {
            forwarding = true;
            try {
                return reported(super.setSelection(start, end));
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean setComposingRegion(int start, int end) {
            forwarding = true;
            try {
                return reported(super.setComposingRegion(start, end));
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean commitCompletion(android.view.inputmethod.CompletionInfo text) {
            forwarding = true;
            try {
                return reported(super.commitCompletion(text));
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean commitCorrection(android.view.inputmethod.CorrectionInfo info) {
            forwarding = true;
            try {
                return reported(super.commitCorrection(info));
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean replaceText(int start, int end, CharSequence text, int newCursorPosition,
                TextAttribute textAttribute) {
            forwarding = true;
            try {
                return reported(super.replaceText(
                        start, end, text, newCursorPosition, textAttribute));
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean performContextMenuAction(int id) {
            forwarding = true;
            try {
                return reported(super.performContextMenuAction(id));
            } finally {
                forwarding = false;
            }
        }

        @Override
        public boolean sendKeyEvent(KeyEvent event) {
            int code = event.getKeyCode();
            boolean delete = code == KeyEvent.KEYCODE_DEL || code == KeyEvent.KEYCODE_FORWARD_DEL;
            if (delete && event.getAction() == KeyEvent.ACTION_DOWN
                    && editor != null
                    && editor.getSelectionStart() == editor.getSelectionEnd()) {
                // The default implementation does not always deliver a
                // backspace to an invisible/offscreen editor. Delete through
                // the InputConnection, which is the documented editor path.
                return code == KeyEvent.KEYCODE_DEL
                        ? deleteSurroundingText(1, 0)
                        : deleteSurroundingText(0, 1);
            }
            return super.sendKeyEvent(event);
        }
    }

    /** Called from Rust. The engine and the view must agree on text/selection. */
    public static void sync(String text, int start, int end, boolean show) {
        final MreActivity activity = instance;
        final EditText field = editor;
        if (activity == null || field == null) return;

        activity.runOnUiThread(() -> {
            Log.d("MreIme", "sync text=" + text + " sel=" + start + ".." + end
                    + " show=" + show + " focus=" + field.hasFocus());
            syncing = true;
            try {
                if (!field.getText().toString().equals(text)) {
                    field.setText(text);
                }
                int a = utf16At(text, start);
                int b = utf16At(text, end);
                if (field.getSelectionStart() != a || field.getSelectionEnd() != b) {
                    field.setSelection(a, b);
                }
            } finally {
                syncing = false;
            }

            InputMethodManager imm = (InputMethodManager) activity
                    .getSystemService(Activity.INPUT_METHOD_SERVICE);
            if (show) {
                field.setFocusable(true);
                field.setFocusableInTouchMode(true);
                field.requestFocus();
                if (imm != null) imm.showSoftInput(field, 0);
            } else {
                field.clearFocus();
                field.setFocusable(false);
                field.setFocusableInTouchMode(false);
                if (imm != null) imm.hideSoftInputFromWindow(field.getWindowToken(), 0);
            }
        });
    }
}
