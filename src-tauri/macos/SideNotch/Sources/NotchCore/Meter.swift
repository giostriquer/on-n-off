import Foundation

/// A palette entry kept as components, so a meter can interpolate along the ramp rather than
/// jumping between named colours. The view layer turns one into a `Color`; nothing here imports
/// SwiftUI, which is what lets `NotchCoreChecks` exercise the ramp.
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

// Each provider's accent for its ring and bars, the same values the app window uses: Codex and
// Antigravity resolve through `providerStyle.ts` to `tokens.css`'s dark theme, Cursor and Claude are
// that file's own literals. Claude is the brand terracotta `#d97757` on every surface.
//
// The inner Fable ring is a deeper shade of that same terracotta than the outer weekly ring, so the
// two read as one family with the inner arc the firmer of the pair. Codex's inner ring, a business
// workspace member's credit share, is a deeper shade of Codex's near-white in the same way.
//
// The Limits screen runs this ramp too, as `usageMeterColor` in `ui/src/lib/limitsFormat.ts`, over
// the same endpoints. Change the shape here and change it there and in `side_notch/model.rs`.
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

/// The provider's accent while there is room, hardening toward the trip red as the window fills,
/// red from 90 %; grey when unreadable.
///
/// This used to step to a warning amber at 70 %. That amber is a much lighter colour than the
/// accents it replaced — lightness 179 against Claude's 145 and Fable's 126 — and its hue sits near
/// 43°, away from red, where the accents sit at 15° and the trip red at 5°. So a meter that was
/// filling up went *paler and yellower* exactly as it ran out, which reads as cooling down.
/// Interpolating the accent toward the trip red instead keeps the ramp monotonic: every step sits
/// closer to red than the step before it. That amber still means "pending" elsewhere, on CI rollups
/// and badges, where nothing is filling up and yellow is the right signal.
///
/// The blend is eased rather than linear so that crossing 70 % announces itself: half the journey
/// to red is spent in the first quarter of the band, where a straight line would leave 75 % looking
/// much like 50 %.
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
