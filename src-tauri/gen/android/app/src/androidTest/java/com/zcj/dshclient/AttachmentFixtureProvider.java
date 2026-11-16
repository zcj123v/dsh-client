package com.zcj.dshclient;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import java.io.IOException;
import java.nio.charset.StandardCharsets;

/** Test APK only. Exposes one fixed synthetic cache file, never target-app or user files. */
public final class AttachmentFixtureProvider extends ContentProvider {
    public static final String PAYLOAD = "DSH_CLIENT_ATTACHMENT_FIXTURE\n";
    public static final Uri URI = Uri.parse("content://com.zcj.dshclient.test.attachment/fixture.txt");
    @Override public boolean onCreate() {
        try (java.io.FileOutputStream output = new java.io.FileOutputStream(new java.io.File(getContext().getCacheDir(), "fixture.txt"))) {
            output.write(PAYLOAD.getBytes(StandardCharsets.UTF_8));
            return true;
        } catch (IOException error) { throw new IllegalStateException("Cannot create synthetic fixture", error); }
    }
    @Override public String getType(Uri uri) { return "text/plain"; }
    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] selectionArgs, String sortOrder) {
        if (!URI.equals(uri)) throw new IllegalArgumentException("Unknown fixture");
        String[] columns = projection == null ? new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE} : projection;
        MatrixCursor cursor = new MatrixCursor(columns);
        Object[] values = new Object[columns.length];
        for (int i=0; i<columns.length; i++) {
            if (OpenableColumns.DISPLAY_NAME.equals(columns[i])) values[i] = "fixture.txt";
            else if (OpenableColumns.SIZE.equals(columns[i])) values[i] = PAYLOAD.getBytes(StandardCharsets.UTF_8).length;
        }
        cursor.addRow(values);
        return cursor;
    }
    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws java.io.FileNotFoundException {
        if (!URI.equals(uri) || !"r".equals(mode)) throw new java.io.FileNotFoundException("Unknown fixture");
        return ParcelFileDescriptor.open(new java.io.File(getContext().getCacheDir(), "fixture.txt"), ParcelFileDescriptor.MODE_READ_ONLY);
    }
    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri uri, String selection, String[] args) { throw new UnsupportedOperationException(); }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) { throw new UnsupportedOperationException(); }
}
