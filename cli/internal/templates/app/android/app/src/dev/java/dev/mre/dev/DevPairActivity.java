package dev.mre.dev;

import android.app.Activity;
import android.content.Intent;
import android.graphics.Color;
import android.net.Uri;
import android.os.Bundle;
import android.util.Log;
import android.view.Gravity;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.widget.Toast;
import androidx.activity.ComponentActivity;
import androidx.activity.result.ActivityResultLauncher;
import androidx.activity.result.contract.ActivityResultContracts;
import com.journeyapps.barcodescanner.ScanContract;
import com.journeyapps.barcodescanner.ScanOptions;
import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;

/**
 * Dev-flavour only. Scans the QR printed by `mre dev` (or accepts an
 * `mre://dev?...` deep link), stores the pairing on app-private storage, then
 * launches the native renderer. Release builds do not include this class.
 */
public class DevPairActivity extends ComponentActivity {
    private static final String TAG = "MreDev";
    private static final String CONFIG = "dev-config.txt";
    private boolean launching;

    private final ActivityResultLauncher<ScanOptions> scan =
            registerForActivityResult(new ScanContract(), result -> {
                if (result.getContents() != null) handleUri(result.getContents());
            });

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(makeUi());
        handleIntent(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleIntent(intent);
    }

    private LinearLayout makeUi() {
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setGravity(Gravity.CENTER);
        root.setBackgroundColor(Color.rgb(11, 17, 32));
        int pad = (int) (28 * getResources().getDisplayMetrics().density);

        TextView title = new TextView(this);
        title.setText("MRE dev client");
        title.setTextColor(Color.rgb(248, 250, 252));
        title.setTextSize(24);
        title.setGravity(Gravity.CENTER);

        TextView hint = new TextView(this);
        hint.setText("Scan the QR printed by `mre dev`, or open a mre://dev link.");
        hint.setTextColor(Color.rgb(148, 163, 184));
        hint.setTextSize(14);
        hint.setGravity(Gravity.CENTER);
        hint.setPadding(0, 12, 0, 28);

        Button button = new Button(this);
        button.setText("Scan QR");
        button.setOnClickListener(v -> {
            ScanOptions options = new ScanOptions();
            options.setDesiredBarcodeFormats(ScanOptions.QR_CODE);
            options.setPrompt("Point at the QR from `mre dev`");
            options.setBeepEnabled(false);
            scan.launch(options);
        });

        root.addView(title, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        root.addView(hint, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        root.addView(button, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        root.setPadding(pad, pad, pad, pad);
        return root;
    }

    private void handleIntent(Intent intent) {
        Uri data = intent == null ? null : intent.getData();
        if (data == null) return;
        handleUri(data.toString());
    }

    private void handleUri(String raw) {
        if (launching) return;
        Uri uri = Uri.parse(raw);
        if (!"mre".equals(uri.getScheme()) || !"dev".equals(uri.getHost())) {
            Toast.makeText(this, "Not an MRE dev QR", Toast.LENGTH_LONG).show();
            return;
        }
        String host = uri.getQueryParameter("host");
        String port = uri.getQueryParameter("port");
        String token = uri.getQueryParameter("token");
        String project = uri.getQueryParameter("project");
        if (host == null || host.isEmpty() || port == null || token == null || token.isEmpty()) {
            Toast.makeText(this, "Incomplete pairing QR", Toast.LENGTH_LONG).show();
            return;
        }
        int portNumber;
        try {
            portNumber = Integer.parseInt(port);
        } catch (NumberFormatException error) {
            Toast.makeText(this, "Bad port in pairing QR", Toast.LENGTH_LONG).show();
            return;
        }
        launching = true;
        String config = "protocol mre-dev/1\n"
                + "host " + host + "\n"
                + "port " + portNumber + "\n"
                + "token " + token + "\n"
                + "project " + (project == null ? "" : project) + "\n";
        File file = new File(getFilesDir(), CONFIG);
        try (FileOutputStream out = new FileOutputStream(file, false)) {
            out.write(config.getBytes(StandardCharsets.UTF_8));
        } catch (Exception error) {
            Log.e(TAG, "could not write pairing config", error);
            Toast.makeText(this, "Could not save pairing", Toast.LENGTH_LONG).show();
            launching = false;
            return;
        }
        Log.i(TAG, "paired with " + host + ":" + portNumber);
        Intent next = new Intent(this, dev.mre.MreActivity.class);
        next.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_NEW_TASK);
        startActivity(next);
        finish();
    }
}
