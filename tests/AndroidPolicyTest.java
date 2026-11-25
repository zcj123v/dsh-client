import com.zcj.dshclient.ContentPolicy;

public final class AndroidPolicyTest {
    private static void check(boolean passed, String detail) {
        if (!passed) throw new AssertionError(detail);
    }
    public static void main(String[] args) {
        check(ContentPolicy.validId("170ce412-3cbb-4467-a35a-b49fce65e8ef"), "valid id");
        check(!ContentPolicy.validId("../default"), "directory traversal");
        check(ContentPolicy.validInitialUrl("https://dsh.test/?token=fake-value"), "ephemeral login");
        for (String value : new String[]{"http://127.0.0.1/", "https://user:pass@dsh.test/", "file:///tmp/a", "https://dsh.test/path", "https://dsh.test/?password=value", "https://dsh.test/#token"}) {
            check(!ContentPolicy.validInitialUrl(value), "unsafe initial URL accepted");
        }
        check(ContentPolicy.sameOrigin("https://dsh.test/", "https://dsh.test:443/session/a"), "default port");
        check(!ContentPolicy.sameOrigin("https://dsh.test/", "https://evil.test/"), "different host");
        check(!ContentPolicy.sameOrigin("https://dsh.test/", "http://dsh.test/"), "downgrade");
        check(!ContentPolicy.sameOrigin("https://dsh.test/", "https://dsh.test:444/"), "different port");
        check(!ContentPolicy.externalAllowed("https://evil.test/?TOKEN=private"), "credential query");
        check(!ContentPolicy.externalAllowed("intent://evil"), "intent scheme");
        check(!ContentPolicy.externalAllowed("https://evil.test/#token=private"), "credential fragment");
        check(ContentPolicy.externalAllowed("https://docs.example.test/help"), "ordinary external docs");
        check(ContentPolicy.reachableResponse("https://dsh.test/", 401, null), "login gate remains reachable");
        check(ContentPolicy.reachableResponse("https://dsh.test/", 302, "/login"), "same-origin login redirect");
        check(!ContentPolicy.reachableResponse("https://dsh.test/", 302, "//other.test/login"), "cross-origin redirect must fall back");
        check(!ContentPolicy.reachableResponse("https://dsh.test/", 302, null), "redirect without location must fall back");
        System.out.println("Android native URL policy: 20 checks passed");
    }
}
