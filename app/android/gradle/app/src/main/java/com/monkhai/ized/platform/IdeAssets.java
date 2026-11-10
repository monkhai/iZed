package com.monkhai.ized.platform;

import android.content.Context;
import android.util.AtomicFile;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;

/** Materialize the matching bundled remote-server archives only when connecting. */
public final class IdeAssets {
    public static synchronized String remoteServers(Context context) throws Exception {
        long version = context.getPackageManager().getPackageInfo(context.getPackageName(), 0).lastUpdateTime;
        File directory = new File(context.getNoBackupFilesDir(), "remote-servers/" + version);
        if (!directory.isDirectory() && !directory.mkdirs()) {
            throw new java.io.IOException("Could not create remote server directory");
        }
        String[] archives = context.getAssets().list("RemoteServers");
        if (archives == null || archives.length == 0) {
            throw new java.io.IOException("This build has no bundled remote servers");
        }
        for (String assetName : archives) {
            if (!assetName.equals("macos-arm64.gz.bin") && !assetName.equals("macos-x86_64.gz.bin")) {
                throw new java.io.IOException("Unexpected remote server archive");
            }
            String name = assetName.substring(0, assetName.length() - ".bin".length());
            AtomicFile target = new AtomicFile(new File(directory, name));
            if (target.getBaseFile().isFile()) continue;
            try (InputStream input = context.getAssets().open("RemoteServers/" + assetName)) {
                FileOutputStream output = target.startWrite();
                try {
                    byte[] buffer = new byte[65536];
                    int length;
                    while ((length = input.read(buffer)) != -1) output.write(buffer, 0, length);
                    output.getFD().sync();
                    target.finishWrite(output);
                } catch (Exception error) {
                    target.failWrite(output);
                    throw error;
                }
            }
            if (!target.getBaseFile().isFile()) throw new java.io.IOException("Could not persist remote server archive");
        }
        return directory.getAbsolutePath();
    }
}
