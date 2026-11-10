package com.monkhai.ized.platform;

import android.content.Context;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.AtomicFile;
import java.io.File;
import java.io.FileNotFoundException;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.file.Files;
import java.security.KeyStore;
import java.util.Arrays;
import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

/** Device-local encryption for the exportable Rust SSH identity, not SSH signing. */
public final class TerminalKeys {
    private static final String ALIAS = "com.monkhai.ized.ssh.wrap.v1";
    private static final int VERSION = 1;
    private static final int IV_LENGTH = 12;

    private TerminalKeys() {}

    private static AtomicFile file(Context context) {
        return new AtomicFile(new File(context.getNoBackupFilesDir(), "device-ed25519.enc"));
    }

    private static KeyStore keyStore() throws Exception {
        KeyStore store = KeyStore.getInstance("AndroidKeyStore");
        store.load(null);
        return store;
    }

    // Treat only confirmed absence as a new installation. An unreadable or
    // interrupted write must not cause an existing identity to be replaced.
    private static boolean absent(AtomicFile file) {
        File base = file.getBaseFile();
        return Files.notExists(base.toPath())
                && Files.notExists(new File(base.getPath() + ".bak").toPath())
                && Files.notExists(new File(base.getPath() + ".new").toPath());
    }

    public static synchronized byte[] load(Context context) throws Exception {
        AtomicFile file = file(context);
        byte[] encoded;
        try {
            encoded = file.readFully();
        } catch (FileNotFoundException error) {
            if (absent(file)) return null;
            throw error;
        }
        if (encoded.length < 1 + IV_LENGTH + 16 || encoded[0] != VERSION) {
            throw new IOException("Invalid SSH identity file");
        }
        SecretKey wrapping = (SecretKey) keyStore().getKey(ALIAS, null);
        if (wrapping == null) throw new IOException("SSH wrapping key is missing");
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.DECRYPT_MODE, wrapping,
                new GCMParameterSpec(128, encoded, 1, IV_LENGTH));
        cipher.updateAAD(new byte[] {VERSION});
        return cipher.doFinal(encoded, 1 + IV_LENGTH, encoded.length - 1 - IV_LENGTH);
    }

    public static synchronized void store(Context context, byte[] plaintext) throws Exception {
        try {
            AtomicFile file = file(context);
            if (!absent(file)) throw new IOException("SSH identity already exists or is unreadable");
            KeyStore store = keyStore();
            SecretKey wrapping = (SecretKey) store.getKey(ALIAS, null);
            if (wrapping == null) {
                KeyGenerator generator = KeyGenerator.getInstance("AES", "AndroidKeyStore");
                generator.init(new KeyGenParameterSpec.Builder(ALIAS,
                        KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                        .setKeySize(256)
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .build());
                wrapping = generator.generateKey();
            }
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.ENCRYPT_MODE, wrapping);
            cipher.updateAAD(new byte[] {VERSION});
            byte[] iv = cipher.getIV();
            if (iv.length != IV_LENGTH) throw new IOException("Unexpected SSH encryption IV length");
            byte[] ciphertext = cipher.doFinal(plaintext);
            byte[] encoded = ByteBuffer.allocate(1 + iv.length + ciphertext.length)
                    .put((byte) VERSION).put(iv).put(ciphertext).array();
            FileOutputStream output = file.startWrite();
            try {
                output.write(encoded);
                output.getFD().sync();
                file.finishWrite(output);
            } catch (Exception error) {
                file.failWrite(output);
                throw error;
            }
            // AtomicFile reports some finish/rename failures only through logs.
            if (!Arrays.equals(encoded, file.readFully())) {
                throw new IOException("Could not persist SSH identity");
            }
        } finally {
            Arrays.fill(plaintext, (byte) 0);
        }
    }
}
