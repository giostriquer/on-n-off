import Foundation
import BillingCore

func checkResources() throws {
    let files = FileManager.default
    let root = files.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? files.removeItem(at: root) }
    for installed in [false, true] {
        for structured in [false, true] {
            let fixture = root.appendingPathComponent("\(installed)-\(structured)")
            let executable = fixture.appendingPathComponent(installed ? "Fixture.app/Contents/Helpers/on-n-off-billing" : "debug/on-n-off-billing")
            let resources = fixture.appendingPathComponent(installed ? "Fixture.app/Contents/Resources" : "debug")
            let bundle = resources.appendingPathComponent("BrowserBilling_BrowserBilling.bundle")
            let script = bundle.appendingPathComponent(structured ? "Contents/Resources/billing.js" : "billing.js")
            try files.createDirectory(at: script.deletingLastPathComponent(), withIntermediateDirectories: true)
            try "// fixture".write(to: script, atomically: true, encoding: .utf8)
            if structured {
                let plist: [String: String] = ["CFBundleIdentifier": "app.on-n-off.billing-fixture", "CFBundlePackageType": "BNDL"]
                let data = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
                try data.write(to: bundle.appendingPathComponent("Contents/Info.plist"))
            }
            let resolved = BillingResources.scriptURL(executable: executable)
            precondition(resolved?.standardizedFileURL == script.standardizedFileURL, "billing script must resolve in installed and dev layouts")
            let contents = try String(contentsOf: resolved!, encoding: .utf8)
            precondition(contents == "// fixture")
        }
    }
    precondition(BillingResources.scriptURL(executable: root.appendingPathComponent("missing/helper")) == nil)
    print("5 billing resource checks passed")
}
