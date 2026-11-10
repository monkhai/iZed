package com.monkhai.ized.platform;

import android.content.Context;
import android.util.AtomicFile;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;

/** One encrypted GPUI credential record per service, preserving its username. */
public final class PlatformCredentials {
    private static String identifier(String service) throws Exception {
        byte[] digest = MessageDigest.getInstance("SHA-256").digest(service.getBytes(StandardCharsets.UTF_8));
        StringBuilder name = new StringBuilder();
        for (byte value : digest) name.append(String.format("%02x", value & 255));
        return name.toString();
    }
    private static AtomicFile file(Context context, String id) {
        return new AtomicFile(new File(context.getNoBackupFilesDir(), "credential-" + id + ".enc"));
    }
    public static synchronized byte[] load(Context context, String service) throws Exception {
        String id = identifier(service);
        return TerminalKeys.loadRecord(file(context, id), "com.monkhai.ized.credential." + id);
    }
    public static synchronized void store(Context context, String service, byte[] record) throws Exception {
        String id = identifier(service);
        TerminalKeys.storeRecord(file(context, id), "com.monkhai.ized.credential." + id, record, true);
    }
    public static synchronized void delete(Context context, String service) throws Exception {
        AtomicFile record = file(context, identifier(service));
        record.delete();
        File base = record.getBaseFile();
        if (!java.nio.file.Files.notExists(base.toPath())
                || !java.nio.file.Files.notExists(new File(base.getPath() + ".bak").toPath())
                || !java.nio.file.Files.notExists(new File(base.getPath() + ".new").toPath())) {
            throw new java.io.IOException("Could not delete credential");
        }
    }
}
