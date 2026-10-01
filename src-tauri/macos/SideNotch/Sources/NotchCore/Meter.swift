import Foundation

public struct Ink: Equatable, Sendable {
  public let r: Double
  public let g: Double
  public let b: Double

  init(r: Double, g: Double, b: Double) {
    self.r = r
    self.g = g
    self.b = b
  }

  public func mixed(toward other: Ink, _ amount: Double) -> Ink {
    let t = min(max(amount, 0), 1)
    return Ink(
      r: r + (other.r - r) * t, g: g + (other.g - g) * t, b: b + (other.b - b) * t)
  }
}

public let claudeInk = Ink(r: 217, g: 119, b: 87)
public let codexInk = Ink(r: 238, g: 240, b: 242)
public let cursorInk = Ink(r: 122, g: 162, b: 255)
public let antigravityInk = Ink(r: 140, g: 147, b: 157)
public let fableInk = Ink(r: 204, g: 98, b: 64)
public let creditsInk = Ink(r: 168, g: 176, b: 186)
public let tripInk = Ink(r: 226, g: 89, b: 76)
public let unreadableInk = Ink(r: 77, g: 77, b: 77)

public func providerInk(_ id: ProviderId) -> Ink {
  switch id {
  case .claude: return claudeInk
  case .codex: return codexInk
  case .cursor: return cursorInk
  case .antigravity: return antigravityInk
  }
}

public func meterInk(_ quota: Quota?, base: Ink, at now: Date) -> Ink {
  guard let percent = quota?.percent(at: now) else { return unreadableInk }
  if percent >= 90 { return tripInk }
  if percent <= 70 { return base }
  let travelled = (percent - 70) / 20
  return base.mixed(toward: tripInk, travelled.squareRoot())
}

public func meterInk(_ quota: Quota?, provider: ProviderId, at now: Date) -> Ink {
  meterInk(quota, base: providerInk(provider), at: now)
}
