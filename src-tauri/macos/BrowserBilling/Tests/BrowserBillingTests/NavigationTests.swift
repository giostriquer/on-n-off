import Foundation
import BillingCore
@main struct NavigationChecks {
    static func main() {
        precondition(NavigationPolicy.allows(url: URL(string: "about:blank"), isMainFrame: false))
        precondition(NavigationPolicy.allows(url: URL(string: "https://challenges.cloudflare.com/"), isMainFrame: false))
        precondition(NavigationPolicy.allows(url: URL(string: "https://chatgpt.com/"), isMainFrame: true))
        precondition(!NavigationPolicy.allows(url: URL(string: "https://accounts.google.com/"), isMainFrame: true))
        precondition(!NavigationPolicy.allows(url: URL(string: "https://chatgpt.com.example.org/"), isMainFrame: true))
        precondition(!NavigationPolicy.allows(url: URL(string: "http://chatgpt.com/"), isMainFrame: true))
        print("6 navigation checks passed")
    }
}
