import Foundation

public enum BillingResources {
    public static func scriptURL(executable: URL) -> URL? {
        let folder = executable.standardizedFileURL.deletingLastPathComponent()
        // Installed app resources live outside Helpers; cargo stages them beside the dev binary.
        let resources = folder.lastPathComponent == "Helpers" && folder.deletingLastPathComponent().lastPathComponent == "Contents"
            ? folder.deletingLastPathComponent().appendingPathComponent("Resources")
            : folder
        return Bundle(url: resources.appendingPathComponent("BrowserBilling_BrowserBilling.bundle"))?
            .url(forResource: "billing", withExtension: "js")
    }
}
