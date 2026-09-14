// Session import and offscreen hosting adapted from CodexBar (MIT; see THIRD_PARTY_NOTICES).
import AppKit
import BillingCore
import Foundation
import Darwin
import SweetCookieKit
import WebKit

@main
struct BrowserBilling {
    @MainActor static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.prohibited)
        guard (3...4).contains(CommandLine.arguments.count), !CommandLine.arguments[1].isEmpty else { exit(2) }
        let expectedUser = CommandLine.arguments[2]
        let nonInteractive = CommandLine.arguments.count == 4
        if nonInteractive && CommandLine.arguments[3] != "--non-interactive" { exit(2) }
        let account = CommandLine.arguments[1]
        // Keep the crash-recovery lease through process exit. A recovery that won
        // the lock before us may have deleted the directory: refuse to read cookies.
        guard let leasePath = ProcessInfo.processInfo.environment["ON_N_OFF_IMPORT_LEASE"],
              let lease = FileHandle(forReadingAtPath: leasePath),
              flock(lease.fileDescriptor, LOCK_SH) == 0,
              FileManager.default.fileExists(atPath: leasePath) else { exit(2) }
        // The parent holds the write end for the whole import. EOF also covers a
        // crash; exiting releases the lease so the next startup can recover scratch.
        DispatchQueue.global().async {
            _ = try? FileHandle.standardInput.read(upToCount: 1)
            exit(4)
        }
        // A native deadline also covers synchronous cookie reads and Keychain prompts.
        DispatchQueue.global().asyncAfter(deadline: .now() + 55) { exit(3) }
        Task { @MainActor in
            let value = await read(account: account, expectedUser: expectedUser, nonInteractive: nonInteractive) ?? ["error": "unavailable"]
            if JSONSerialization.isValidJSONObject(value),
               let data = try? JSONSerialization.data(withJSONObject: value) {
                FileHandle.standardOutput.write(data)
                exit(0)
            }
            exit(1)
        }
        withExtendedLifetime(lease) { app.run() }
    }

    @MainActor static func read(account: String, expectedUser: String, nonInteractive: Bool) async -> [String: Any]? {
        let client: BrowserCookieClient
        if let home = ProcessInfo.processInfo.environment["ON_N_OFF_HOME"] {
            client = BrowserCookieClient(configuration: .init(homeDirectories: [URL(fileURLWithPath: home)]))
        } else { client = BrowserCookieClient() }
        let query = BrowserCookieQuery(domains: ["chatgpt.com", "openai.com"])
        var sawMismatch = false
        var incomplete = false
        // Same strategy as CodexBar: try separate profile candidates; never merge accounts.
        for browser: Browser in [.safari, .chrome, .edge, .brave, .firefox] {
            for store in client.stores(for: browser) {
                guard let records = try? await Task.detached(operation: { if nonInteractive {
                    return try BrowserCookieKeychainAccessGate.withUserInteractionDisallowed {
                        try client.records(matching: query, in: store)
                    }
                }
                return try client.records(matching: query, in: store) }).value else { incomplete = true; continue }
                let cookies = BrowserCookieClient.makeHTTPCookies(records, origin: query.origin).filter {
                    let domain = $0.domain.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: "."))
                    return domain == "chatgpt.com" || domain.hasSuffix(".chatgpt.com") ||
                        domain == "openai.com" || domain.hasSuffix(".openai.com")
                }
                guard cookies.contains(where: { $0.name.contains("session-token") }) else { continue }
                let reader = Reader()
                let result = await reader.read(cookies: cookies, account: account, expectedUser: expectedUser)
                withExtendedLifetime(reader) {}
                if let result {
                    if result["error"] as? String == "accountMismatch" { sawMismatch = true }
                    else if result["error"] == nil { return result }
                    else { incomplete = true }
                } else { incomplete = true }
            }
        }
        return sawMismatch && !incomplete ? ["error": "accountMismatch"] : nil
    }
}

@MainActor
final class Reader: NSObject, WKNavigationDelegate {
    private var window: NSWindow?
    private var loaded = false
    private var failed = false

    func read(cookies: [HTTPCookie], account: String, expectedUser: String) async -> [String: Any]? {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        guard let url = BillingResources.scriptURL(executable: URL(fileURLWithPath: CommandLine.arguments[0])),
              let script = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        config.userContentController.addUserScript(WKUserScript(source: script, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        for cookie in cookies { await config.websiteDataStore.httpCookieStore.setCookie(cookie) }
        let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 900, height: 700), configuration: config)
        web.navigationDelegate = self
        // CodexBar keeps WebKit technically visible to avoid suspended hydration, without a UI.
        let screen = NSScreen.main?.frame ?? NSRect(x: 0, y: 0, width: 900, height: 700)
        let host = NSWindow(contentRect: NSRect(x: screen.maxX - 1, y: screen.maxY - 1, width: 900, height: 700), styleMask: [.borderless], backing: .buffered, defer: false)
        host.isReleasedWhenClosed = false
        host.isOpaque = false
        host.backgroundColor = .clear
        host.alphaValue = 0.01
        host.hasShadow = false
        host.ignoresMouseEvents = true
        host.contentView = web
        window = host
        host.orderFrontRegardless()
        defer { web.stopLoading(); web.navigationDelegate = nil; host.orderOut(nil); host.close(); window = nil }
        web.load(URLRequest(url: URL(string: "https://chatgpt.com/")!))
        let deadline = Date().addingTimeInterval(15)
        while !loaded && !failed && Date() < deadline { try? await Task.sleep(for: .milliseconds(100)) }
        guard loaded, !failed else { return nil }
        let result = try? await web.callAsyncJavaScript("return await window.__onNOffReadBilling(account, expectedUser || undefined)", arguments: ["account": account, "expectedUser": expectedUser], in: nil, contentWorld: .page)
        guard let value = result as? [String: Any] else { return nil }
        if let error = value["error"] as? String, ["accountMismatch", "unavailable"].contains(error) { return ["error": error] }
        guard value["accountId"] as? String == account else { return nil }
        // Construct the protocol envelope, never serialize arbitrary page data.
        guard let date = value["active_until"], let renew = value["will_renew"],
              date is NSNull || date is String, renew is NSNull || renew is Bool else { return nil }
        return ["accountId": account, "active_until": date, "will_renew": renew]
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { loaded = true }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { failed = true }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { failed = true }
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
        // No interactive authentication. An expired imported session must fail visibly in the app.
        let allowed = NavigationPolicy.allows(url: action.request.url, isMainFrame: action.targetFrame?.isMainFrame != false)
        if !allowed { failed = true }
        return allowed ? .allow : .cancel
    }
}
