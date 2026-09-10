// swift-tools-version: 6.2
import PackageDescription
let package = Package(
    name: "BrowserBilling",
    platforms: [.macOS(.v13)],
    products: [.executable(name: "on-n-off-billing", targets: ["BrowserBilling"])],
    dependencies: [.package(url: "https://github.com/steipete/SweetCookieKit", exact: "0.5.2")],
    targets: [.target(name: "BillingCore"), .executableTarget(
        name: "BrowserBilling",
        dependencies: ["BillingCore", .product(name: "SweetCookieKit", package: "SweetCookieKit")],
        resources: [.copy("billing.js")]
    ), .executableTarget(name: "BrowserBillingChecks", dependencies: ["BillingCore"], path: "Tests/BrowserBillingTests")]
)
