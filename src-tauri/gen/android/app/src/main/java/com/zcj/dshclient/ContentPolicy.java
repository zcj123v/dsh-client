package com.zcj.dshclient;

import java.net.URI;
import java.util.Locale;
import java.util.regex.Pattern;

/** No Android dependency: the native navigation boundary is tested on a JVM. */
public final class ContentPolicy {
    private ContentPolicy() {}
    private static final Pattern ID = Pattern.compile("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}");
    private static final Pattern TOKEN_QUERY = Pattern.compile("token=[A-Za-z0-9_.~-]{1,4096}");
    public static boolean validId(String id) { return id != null && ID.matcher(id).matches(); }
    private static URI secure(String value) {
        try {
            URI uri = new URI(value);
            if (!"https".equals(uri.getScheme()) || uri.getHost() == null || uri.getRawUserInfo() != null) return null;
            return uri;
        } catch (Exception ignored) { return null; }
    }
    public static boolean validInitialUrl(String value) {
        URI uri = secure(value);
        return uri != null && "/".equals(uri.getRawPath()) && uri.getRawFragment() == null
            && (uri.getRawQuery() == null || TOKEN_QUERY.matcher(uri.getRawQuery()).matches());
    }
    private static int port(URI uri) { return uri.getPort() == -1 ? 443 : uri.getPort(); }
    public static boolean sameOrigin(String base, String target) {
        URI left = secure(base), right = secure(target);
        return left != null && right != null && left.getHost().equalsIgnoreCase(right.getHost()) && port(left) == port(right);
    }
    public static boolean externalAllowed(String value) {
        URI uri = secure(value);
        if (uri == null || uri.getRawFragment() != null) return false;
        String query = uri.getRawQuery();
        if (query == null) return true;
        String decoded;
        try { decoded = java.net.URLDecoder.decode(query, "UTF-8").toLowerCase(Locale.ROOT); }
        catch (Exception ignored) { return false; }
        return !Pattern.compile("(^|&)(token|password|secret|authorization|api_key|apikey)=").matcher(decoded).find();
    }
}
