import Foundation
public enum NavigationPolicy {
    public static func allows(url: URL?, isMainFrame: Bool) -> Bool {
        !isMainFrame || (url?.scheme == "https" && url?.host == "chatgpt.com")
    }
}
