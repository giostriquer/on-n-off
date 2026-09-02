import NotchCore
import SwiftUI

// Provider marks as native vector paths, converted offline from their SVG sources: Claude and
// Codex from Simple Icons (24 × 24), Cursor's official 2D cube (466.73 × 532.09), and the
// Antigravity arch silhouette (viewBox 13 14.5 85 85). Every mark is built once and fitted into
// the cell the same way.
struct ProviderMark: Shape {
  let provider: ProviderId

  func path(in rect: CGRect) -> Path {
    switch provider {
    case .claude: return fitted(Self.claude, box: Self.square, in: rect)
    case .codex: return fitted(Self.codex, box: Self.square, in: rect)
    case .cursor:
      return fitted(Self.cursor, box: CGRect(x: 0, y: 0, width: 466.73, height: 532.09), in: rect)
    case .antigravity:
      return fitted(Self.antigravity, box: CGRect(x: 13, y: 14.5, width: 85, height: 85), in: rect)
    }
  }

  /// `path` scaled uniformly into `rect` and centred, like an SVG with `xMidYMid meet`.
  private func fitted(_ path: Path, box: CGRect, in rect: CGRect) -> Path {
    let scale = min(rect.width / box.width, rect.height / box.height)
    let offset = CGPoint(
      x: rect.minX + (rect.width - box.width * scale) / 2 - box.minX * scale,
      y: rect.minY + (rect.height - box.height * scale) / 2 - box.minY * scale)
    return path.applying(
      CGAffineTransform(translationX: offset.x, y: offset.y).scaledBy(x: scale, y: scale))
  }
  private static let square = CGRect(x: 0, y: 0, width: 24, height: 24)
  private static let claude: Path = {
    var path = Path()
    path.move(to: CGPoint(x: 4.714400, y: 15.955500))
    path.addLine(to: CGPoint(x: 9.431800, y: 13.308400))
    path.addLine(to: CGPoint(x: 9.510800, y: 13.077700))
    path.addLine(to: CGPoint(x: 9.431800, y: 12.950200))
    path.addLine(to: CGPoint(x: 9.201100, y: 12.950200))
    path.addLine(to: CGPoint(x: 8.411800, y: 12.901600))
    path.addLine(to: CGPoint(x: 5.716200, y: 12.828700))
    path.addLine(to: CGPoint(x: 3.378700, y: 12.731600))
    path.addLine(to: CGPoint(x: 1.114100, y: 12.610200))
    path.addLine(to: CGPoint(x: 0.543400, y: 12.488700))
    path.addLine(to: CGPoint(x: 0.009100, y: 11.784500))
    path.addLine(to: CGPoint(x: 0.063700, y: 11.432300))
    path.addLine(to: CGPoint(x: 0.543400, y: 11.110500))
    path.addLine(to: CGPoint(x: 1.229400, y: 11.171300))
    path.addLine(to: CGPoint(x: 2.747300, y: 11.274500))
    path.addLine(to: CGPoint(x: 5.024000, y: 11.432300))
    path.addLine(to: CGPoint(x: 6.675400, y: 11.529500))
    path.addLine(to: CGPoint(x: 9.122200, y: 11.784500))
    path.addLine(to: CGPoint(x: 9.510800, y: 11.784500))
    path.addLine(to: CGPoint(x: 9.565400, y: 11.626600))
    path.addLine(to: CGPoint(x: 9.431800, y: 11.529500))
    path.addLine(to: CGPoint(x: 9.328600, y: 11.432300))
    path.addLine(to: CGPoint(x: 6.973000, y: 9.835600))
    path.addLine(to: CGPoint(x: 4.423000, y: 8.147700))
    path.addLine(to: CGPoint(x: 3.087400, y: 7.176300))
    path.addLine(to: CGPoint(x: 2.364900, y: 6.684500))
    path.addLine(to: CGPoint(x: 2.000600, y: 6.223100))
    path.addLine(to: CGPoint(x: 1.842800, y: 5.215300))
    path.addLine(to: CGPoint(x: 2.498500, y: 4.492800))
    path.addLine(to: CGPoint(x: 3.378800, y: 4.553500))
    path.addLine(to: CGPoint(x: 3.603400, y: 4.614200))
    path.addLine(to: CGPoint(x: 4.495900, y: 5.300200))
    path.addLine(to: CGPoint(x: 6.402300, y: 6.775600))
    path.addLine(to: CGPoint(x: 8.891600, y: 8.609200))
    path.addLine(to: CGPoint(x: 9.255900, y: 8.912700))
    path.addLine(to: CGPoint(x: 9.401600, y: 8.809500))
    path.addLine(to: CGPoint(x: 9.419800, y: 8.736700))
    path.addLine(to: CGPoint(x: 9.255800, y: 8.463400))
    path.addLine(to: CGPoint(x: 7.901900, y: 6.016700))
    path.addLine(to: CGPoint(x: 6.456900, y: 3.527400))
    path.addLine(to: CGPoint(x: 5.813400, y: 2.495400))
    path.addLine(to: CGPoint(x: 5.643400, y: 1.876000))
    path.addCurve(
      to: CGPoint(x: 5.540200, y: 1.147500), control1: CGPoint(x: 5.582700, y: 1.621000),
      control2: CGPoint(x: 5.540200, y: 1.408600))
    path.addLine(to: CGPoint(x: 6.287000, y: 0.133500))
    path.addLine(to: CGPoint(x: 6.699700, y: 0.000000))
    path.addLine(to: CGPoint(x: 7.695400, y: 0.133600))
    path.addLine(to: CGPoint(x: 8.114400, y: 0.497800))
    path.addLine(to: CGPoint(x: 8.733600, y: 1.912500))
    path.addLine(to: CGPoint(x: 9.735400, y: 4.140700))
    path.addLine(to: CGPoint(x: 11.289700, y: 7.170300))
    path.addLine(to: CGPoint(x: 11.745000, y: 8.068800))
    path.addLine(to: CGPoint(x: 11.987900, y: 8.900600))
    path.addLine(to: CGPoint(x: 12.078900, y: 9.155600))
    path.addLine(to: CGPoint(x: 12.236800, y: 9.155600))
    path.addLine(to: CGPoint(x: 12.236800, y: 9.009900))
    path.addLine(to: CGPoint(x: 12.364300, y: 7.303900))
    path.addLine(to: CGPoint(x: 12.601100, y: 5.209200))
    path.addLine(to: CGPoint(x: 12.831800, y: 2.513500))
    path.addLine(to: CGPoint(x: 12.910700, y: 1.754600))
    path.addLine(to: CGPoint(x: 13.287100, y: 0.843900))
    path.addLine(to: CGPoint(x: 14.033900, y: 0.352100))
    path.addLine(to: CGPoint(x: 14.616700, y: 0.631400))
    path.addLine(to: CGPoint(x: 15.096400, y: 1.317400))
    path.addLine(to: CGPoint(x: 15.029600, y: 1.760700))
    path.addLine(to: CGPoint(x: 14.744300, y: 3.612400))
    path.addLine(to: CGPoint(x: 14.185700, y: 6.514500))
    path.addLine(to: CGPoint(x: 13.821400, y: 8.457400))
    path.addLine(to: CGPoint(x: 14.033900, y: 8.457400))
    path.addLine(to: CGPoint(x: 14.276800, y: 8.214500))
    path.addLine(to: CGPoint(x: 15.260300, y: 6.909200))
    path.addLine(to: CGPoint(x: 16.911700, y: 4.844900))
    path.addLine(to: CGPoint(x: 17.640300, y: 4.025300))
    path.addLine(to: CGPoint(x: 18.490300, y: 3.120700))
    path.addLine(to: CGPoint(x: 19.036700, y: 2.689600))
    path.addLine(to: CGPoint(x: 20.068800, y: 2.689600))
    path.addLine(to: CGPoint(x: 20.827800, y: 3.818900))
    path.addLine(to: CGPoint(x: 20.487800, y: 4.984600))
    path.addLine(to: CGPoint(x: 19.425300, y: 6.332400))
    path.addLine(to: CGPoint(x: 18.544900, y: 7.473800))
    path.addLine(to: CGPoint(x: 17.282100, y: 9.173800))
    path.addLine(to: CGPoint(x: 16.492800, y: 10.533800))
    path.addLine(to: CGPoint(x: 16.565700, y: 10.643100))
    path.addLine(to: CGPoint(x: 16.753900, y: 10.624800))
    path.addLine(to: CGPoint(x: 19.607400, y: 10.017800))
    path.addLine(to: CGPoint(x: 21.149500, y: 9.738400))
    path.addLine(to: CGPoint(x: 22.989100, y: 9.422700))
    path.addLine(to: CGPoint(x: 23.820900, y: 9.811300))
    path.addLine(to: CGPoint(x: 23.911900, y: 10.205900))
    path.addLine(to: CGPoint(x: 23.584100, y: 11.013400))
    path.addLine(to: CGPoint(x: 21.617100, y: 11.499100))
    path.addLine(to: CGPoint(x: 19.309900, y: 11.960500))
    path.addLine(to: CGPoint(x: 15.873500, y: 12.774100))
    path.addLine(to: CGPoint(x: 15.831000, y: 12.804500))
    path.addLine(to: CGPoint(x: 15.879600, y: 12.865200))
    path.addLine(to: CGPoint(x: 17.427800, y: 13.010900))
    path.addLine(to: CGPoint(x: 18.089600, y: 13.047300))
    path.addLine(to: CGPoint(x: 19.710600, y: 13.047300))
    path.addLine(to: CGPoint(x: 22.728100, y: 13.272000))
    path.addLine(to: CGPoint(x: 23.517300, y: 13.794000))
    path.addLine(to: CGPoint(x: 23.990900, y: 14.431600))
    path.addLine(to: CGPoint(x: 23.911900, y: 14.917300))
    path.addLine(to: CGPoint(x: 22.697700, y: 15.536600))
    path.addLine(to: CGPoint(x: 21.058400, y: 15.148000))
    path.addLine(to: CGPoint(x: 17.233400, y: 14.237300))
    path.addLine(to: CGPoint(x: 15.922100, y: 13.909400))
    path.addLine(to: CGPoint(x: 15.739900, y: 13.909400))
    path.addLine(to: CGPoint(x: 15.739900, y: 14.018700))
    path.addLine(to: CGPoint(x: 16.832800, y: 15.087300))
    path.addLine(to: CGPoint(x: 18.836300, y: 16.896500))
    path.addLine(to: CGPoint(x: 21.343800, y: 19.227900))
    path.addLine(to: CGPoint(x: 21.471300, y: 19.804700))
    path.addLine(to: CGPoint(x: 21.149500, y: 20.260100))
    path.addLine(to: CGPoint(x: 20.809500, y: 20.211500))
    path.addLine(to: CGPoint(x: 18.605600, y: 18.554000))
    path.addLine(to: CGPoint(x: 17.755600, y: 17.807200))
    path.addLine(to: CGPoint(x: 15.831000, y: 16.186200))
    path.addLine(to: CGPoint(x: 15.703500, y: 16.186200))
    path.addLine(to: CGPoint(x: 15.703500, y: 16.356200))
    path.addLine(to: CGPoint(x: 16.146700, y: 17.005800))
    path.addLine(to: CGPoint(x: 18.490300, y: 20.527200))
    path.addLine(to: CGPoint(x: 18.611700, y: 21.607900))
    path.addLine(to: CGPoint(x: 18.441700, y: 21.960000))
    path.addLine(to: CGPoint(x: 17.834600, y: 22.172500))
    path.addLine(to: CGPoint(x: 17.166700, y: 22.051100))
    path.addLine(to: CGPoint(x: 15.794600, y: 20.126500))
    path.addLine(to: CGPoint(x: 14.380000, y: 17.959000))
    path.addLine(to: CGPoint(x: 13.238600, y: 16.016200))
    path.addLine(to: CGPoint(x: 13.098900, y: 16.095200))
    path.addLine(to: CGPoint(x: 12.424900, y: 23.350400))
    path.addLine(to: CGPoint(x: 12.109300, y: 23.720700))
    path.addLine(to: CGPoint(x: 11.380700, y: 24.000000))
    path.addLine(to: CGPoint(x: 10.773600, y: 23.538600))
    path.addLine(to: CGPoint(x: 10.451800, y: 22.791800))
    path.addLine(to: CGPoint(x: 10.773600, y: 21.316500))
    path.addLine(to: CGPoint(x: 11.162200, y: 19.391900))
    path.addLine(to: CGPoint(x: 11.477900, y: 17.861900))
    path.addLine(to: CGPoint(x: 11.763200, y: 15.961500))
    path.addLine(to: CGPoint(x: 11.933200, y: 15.330100))
    path.addLine(to: CGPoint(x: 11.921100, y: 15.287600))
    path.addLine(to: CGPoint(x: 11.781400, y: 15.305800))
    path.addLine(to: CGPoint(x: 10.348600, y: 17.273000))
    path.addLine(to: CGPoint(x: 8.169000, y: 20.217600))
    path.addLine(to: CGPoint(x: 6.444700, y: 22.063200))
    path.addLine(to: CGPoint(x: 6.031900, y: 22.227200))
    path.addLine(to: CGPoint(x: 5.315500, y: 21.856800))
    path.addLine(to: CGPoint(x: 5.382200, y: 21.195000))
    path.addLine(to: CGPoint(x: 5.783000, y: 20.606100))
    path.addLine(to: CGPoint(x: 8.169000, y: 17.570400))
    path.addLine(to: CGPoint(x: 9.607900, y: 15.688400))
    path.addLine(to: CGPoint(x: 10.536900, y: 14.601600))
    path.addLine(to: CGPoint(x: 10.530700, y: 14.443700))
    path.addLine(to: CGPoint(x: 10.476100, y: 14.443700))
    path.addLine(to: CGPoint(x: 4.137600, y: 18.560100))
    path.addLine(to: CGPoint(x: 3.008300, y: 18.705800))
    path.addLine(to: CGPoint(x: 2.522600, y: 18.250400))
    path.addLine(to: CGPoint(x: 2.583400, y: 17.503700))
    path.addLine(to: CGPoint(x: 2.814100, y: 17.260800))
    path.addLine(to: CGPoint(x: 4.720500, y: 15.949400))
    path.addLine(to: CGPoint(x: 4.714400, y: 15.955500))
    path.closeSubpath()
    return path
  }()

  private static let codex: Path = {
    var path = Path()
    path.move(to: CGPoint(x: 22.281900, y: 9.821100))
    path.addCurve(
      to: CGPoint(x: 21.766200, y: 4.910300), control1: CGPoint(x: 22.824776, y: 8.186235),
      control2: CGPoint(x: 22.636854, y: 6.396725))
    path.addCurve(
      to: CGPoint(x: 15.256400, y: 2.010300), control1: CGPoint(x: 20.457089, y: 2.631633),
      control2: CGPoint(x: 17.825979, y: 1.459521))
    path.addCurve(
      to: CGPoint(x: 9.491981, y: 0.131078), control1: CGPoint(x: 13.808329, y: 0.399528),
      control2: CGPoint(x: 11.611165, y: -0.316756))
    path.addCurve(
      to: CGPoint(x: 4.980700, y: 4.181800), control1: CGPoint(x: 7.372797, y: 0.578912),
      control2: CGPoint(x: 5.653279, y: 2.122884))
    path.addCurve(
      to: CGPoint(x: 0.983000, y: 7.081800), control1: CGPoint(x: 3.292803, y: 4.527919),
      control2: CGPoint(x: 1.835975, y: 5.584727))
    path.addCurve(
      to: CGPoint(x: 1.725700, y: 14.178400), control1: CGPoint(x: -0.340434, y: 9.356841),
      control2: CGPoint(x: -0.040091, y: 12.226664))
    path.addCurve(
      to: CGPoint(x: 2.236700, y: 19.089100), control1: CGPoint(x: 1.180815, y: 15.812499),
      control2: CGPoint(x: 1.367049, y: 17.602196))
    path.addCurve(
      to: CGPoint(x: 8.751300, y: 21.989200), control1: CGPoint(x: 3.547453, y: 21.368581),
      control2: CGPoint(x: 6.180305, y: 22.540646))
    path.addCurve(
      to: CGPoint(x: 13.259900, y: 24.000000), control1: CGPoint(x: 9.894838, y: 23.276963),
      control2: CGPoint(x: 11.537716, y: 24.009673))
    path.addCurve(
      to: CGPoint(x: 19.031700, y: 19.794200), control1: CGPoint(x: 15.893738, y: 24.002424),
      control2: CGPoint(x: 18.227114, y: 22.302138))
    path.addCurve(
      to: CGPoint(x: 23.029400, y: 16.894100), control1: CGPoint(x: 20.719362, y: 19.447484),
      control2: CGPoint(x: 22.175980, y: 18.390793))
    path.addCurve(
      to: CGPoint(x: 22.281900, y: 9.821200), control1: CGPoint(x: 24.336803, y: 14.623065),
      control2: CGPoint(x: 24.035146, y: 11.768770))
    path.addLine(to: CGPoint(x: 22.281900, y: 9.821100))
    path.closeSubpath()
    path.move(to: CGPoint(x: 13.259900, y: 22.429200))
    path.addCurve(
      to: CGPoint(x: 10.383500, y: 21.388400), control1: CGPoint(x: 12.208618, y: 22.430864),
      control2: CGPoint(x: 11.190301, y: 22.062395))
    path.addLine(to: CGPoint(x: 10.525400, y: 21.308000))
    path.addLine(to: CGPoint(x: 15.303700, y: 18.549800))
    path.addCurve(
      to: CGPoint(x: 15.696400, y: 17.868500), control1: CGPoint(x: 15.545646, y: 18.407902),
      control2: CGPoint(x: 15.694886, y: 18.148983))
    path.addLine(to: CGPoint(x: 15.696400, y: 11.131600))
    path.addLine(to: CGPoint(x: 17.716400, y: 12.300200))
    path.addCurve(
      to: CGPoint(x: 17.754400, y: 12.352200), control1: CGPoint(x: 17.736652, y: 12.310461),
      control2: CGPoint(x: 17.750776, y: 12.329788))
    path.addLine(to: CGPoint(x: 17.754400, y: 17.934800))
    path.addCurve(
      to: CGPoint(x: 13.259900, y: 22.429200), control1: CGPoint(x: 17.749119, y: 20.414837),
      control2: CGPoint(x: 15.739937, y: 22.423975))
    path.addLine(to: CGPoint(x: 13.259900, y: 22.429200))
    path.closeSubpath()
    path.move(to: CGPoint(x: 3.599200, y: 18.303800))
    path.addCurve(
      to: CGPoint(x: 3.064600, y: 15.290100), control1: CGPoint(x: 3.071972, y: 17.393420),
      control2: CGPoint(x: 2.882672, y: 16.326277))
    path.addLine(to: CGPoint(x: 3.206600, y: 15.375300))
    path.addLine(to: CGPoint(x: 7.989600, y: 18.133500))
    path.addCurve(
      to: CGPoint(x: 8.770200, y: 18.133500), control1: CGPoint(x: 8.230587, y: 18.274909),
      control2: CGPoint(x: 8.529213, y: 18.274909))
    path.addLine(to: CGPoint(x: 14.613000, y: 14.765000))
    path.addLine(to: CGPoint(x: 14.613000, y: 17.097400))
    path.addCurve(
      to: CGPoint(x: 14.579800, y: 17.158900), control1: CGPoint(x: 14.611888, y: 17.121889),
      control2: CGPoint(x: 14.599664, y: 17.144534))
    path.addLine(to: CGPoint(x: 9.740000, y: 19.950200))
    path.addCurve(
      to: CGPoint(x: 3.599200, y: 18.303800), control1: CGPoint(x: 7.589341, y: 21.189138),
      control2: CGPoint(x: 4.841618, y: 20.452450))
    path.addLine(to: CGPoint(x: 3.599200, y: 18.303800))
    path.closeSubpath()
    path.move(to: CGPoint(x: 2.340800, y: 7.895600))
    path.addCurve(
      to: CGPoint(x: 4.706300, y: 5.922800), control1: CGPoint(x: 2.871683, y: 6.979369),
      control2: CGPoint(x: 3.709632, y: 6.280529))
    path.addLine(to: CGPoint(x: 4.706300, y: 11.600000))
    path.addCurve(
      to: CGPoint(x: 5.094200, y: 12.276500), control1: CGPoint(x: 4.702637, y: 11.879344),
      control2: CGPoint(x: 4.851265, y: 12.138553))
    path.addLine(to: CGPoint(x: 10.908600, y: 15.630800))
    path.addLine(to: CGPoint(x: 8.888500, y: 16.799300))
    path.addCurve(
      to: CGPoint(x: 8.817500, y: 16.799300), control1: CGPoint(x: 8.866301, y: 16.811087),
      control2: CGPoint(x: 8.839699, y: 16.811087))
    path.addLine(to: CGPoint(x: 3.987200, y: 14.012800))
    path.addCurve(
      to: CGPoint(x: 2.340800, y: 7.872000), control1: CGPoint(x: 1.840816, y: 12.768645),
      control2: CGPoint(x: 1.104693, y: 10.023029))
    path.addLine(to: CGPoint(x: 2.340800, y: 7.895600))
    path.closeSubpath()
    path.move(to: CGPoint(x: 18.937100, y: 11.751400))
    path.addLine(to: CGPoint(x: 13.103800, y: 8.364000))
    path.addLine(to: CGPoint(x: 15.119200, y: 7.200000))
    path.addCurve(
      to: CGPoint(x: 15.190200, y: 7.200000), control1: CGPoint(x: 15.141399, y: 7.188213),
      control2: CGPoint(x: 15.168001, y: 7.188213))
    path.addLine(to: CGPoint(x: 20.020500, y: 9.991300))
    path.addCurve(
      to: CGPoint(x: 22.253106, y: 14.258003), control1: CGPoint(x: 21.528124, y: 10.861219),
      control2: CGPoint(x: 22.397899, y: 12.523435))
    path.addCurve(
      to: CGPoint(x: 19.344000, y: 18.095500), control1: CGPoint(x: 22.108312, y: 15.992570),
      control2: CGPoint(x: 20.974987, y: 17.487577))
    path.addLine(to: CGPoint(x: 19.344000, y: 12.418300))
    path.addCurve(
      to: CGPoint(x: 18.937000, y: 11.751300), control1: CGPoint(x: 19.335479, y: 12.139742),
      control2: CGPoint(x: 19.180819, y: 11.886281))
    path.addLine(to: CGPoint(x: 18.937100, y: 11.751400))
    path.closeSubpath()
    path.move(to: CGPoint(x: 20.947800, y: 8.728300))
    path.addLine(to: CGPoint(x: 20.805800, y: 8.643100))
    path.addLine(to: CGPoint(x: 16.032300, y: 5.861300))
    path.addCurve(
      to: CGPoint(x: 15.246900, y: 5.861300), control1: CGPoint(x: 15.789833, y: 5.719012),
      control2: CGPoint(x: 15.489367, y: 5.719012))
    path.addLine(to: CGPoint(x: 9.409000, y: 9.229700))
    path.addLine(to: CGPoint(x: 9.409000, y: 6.897400))
    path.addCurve(
      to: CGPoint(x: 9.437400, y: 6.835900), control1: CGPoint(x: 9.406467, y: 6.873242),
      control2: CGPoint(x: 9.417367, y: 6.849637))
    path.addLine(to: CGPoint(x: 14.267700, y: 4.049300))
    path.addCurve(
      to: CGPoint(x: 19.087737, y: 4.257782), control1: CGPoint(x: 15.778978, y: 3.178674),
      control2: CGPoint(x: 17.657277, y: 3.259917))
    path.addCurve(
      to: CGPoint(x: 20.947900, y: 8.709300), control1: CGPoint(x: 20.518196, y: 5.255648),
      control2: CGPoint(x: 21.243075, y: 6.990341))
    path.addLine(to: CGPoint(x: 20.947800, y: 8.728300))
    path.closeSubpath()
    path.move(to: CGPoint(x: 8.306500, y: 12.863000))
    path.addLine(to: CGPoint(x: 6.286500, y: 11.699200))
    path.addCurve(
      to: CGPoint(x: 6.248500, y: 11.642500), control1: CGPoint(x: 6.266041, y: 11.686882),
      control2: CGPoint(x: 6.252117, y: 11.666106))
    path.addLine(to: CGPoint(x: 6.248500, y: 6.074200))
    path.addCurve(
      to: CGPoint(x: 8.839741, y: 2.005438), control1: CGPoint(x: 6.250770, y: 4.330388),
      control2: CGPoint(x: 7.260488, y: 2.744930))
    path.addCurve(
      to: CGPoint(x: 13.624200, y: 2.620500), control1: CGPoint(x: 10.418993, y: 1.265947),
      control2: CGPoint(x: 12.283335, y: 1.505616))
    path.addLine(to: CGPoint(x: 13.482200, y: 2.701000))
    path.addLine(to: CGPoint(x: 8.704000, y: 5.459000))
    path.addCurve(
      to: CGPoint(x: 8.311300, y: 6.140300), control1: CGPoint(x: 8.462054, y: 5.600898),
      control2: CGPoint(x: 8.312814, y: 5.859817))
    path.addLine(to: CGPoint(x: 8.306500, y: 12.863000))
    path.closeSubpath()
    path.move(to: CGPoint(x: 9.404100, y: 10.497600))
    path.addLine(to: CGPoint(x: 12.006100, y: 8.997800))
    path.addLine(to: CGPoint(x: 14.613000, y: 10.497600))
    path.addLine(to: CGPoint(x: 14.613000, y: 13.497000))
    path.addLine(to: CGPoint(x: 12.015600, y: 14.996700))
    path.addLine(to: CGPoint(x: 9.408900, y: 13.497000))
    path.addLine(to: CGPoint(x: 9.404100, y: 10.497600))
    path.closeSubpath()
  
    return path
  }()
}

extension ProviderMark {
  fileprivate static let cursor: Path = {
    var path = Path()
    path.move(to: CGPoint(x: 457.4300, y: 125.9400))
    path.addLine(to: CGPoint(x: 244.4200, y: 2.9600))
    path.addCurve(
      to: CGPoint(x: 222.3000, y: 2.9600), control1: CGPoint(x: 237.5800, y: -0.9900),
      control2: CGPoint(x: 229.1400, y: -0.9900))
    path.addLine(to: CGPoint(x: 9.3000, y: 125.9400))
    path.addCurve(
      to: CGPoint(x: 0.0000, y: 142.0500), control1: CGPoint(x: 3.5500, y: 129.2600),
      control2: CGPoint(x: 0.0000, y: 135.4000))
    path.addLine(to: CGPoint(x: 0.0000, y: 390.0400))
    path.addCurve(
      to: CGPoint(x: 9.3000, y: 406.1500), control1: CGPoint(x: 0.0000, y: 396.6900),
      control2: CGPoint(x: 3.5500, y: 402.8300))
    path.addLine(to: CGPoint(x: 222.3100, y: 529.1300))
    path.addCurve(
      to: CGPoint(x: 244.4300, y: 529.1300), control1: CGPoint(x: 229.1500, y: 533.0800),
      control2: CGPoint(x: 237.5900, y: 533.0800))
    path.addLine(to: CGPoint(x: 457.4400, y: 406.1500))
    path.addCurve(
      to: CGPoint(x: 466.7400, y: 390.0400), control1: CGPoint(x: 463.1900, y: 402.8300),
      control2: CGPoint(x: 466.7400, y: 396.6900))
    path.addLine(to: CGPoint(x: 466.7400, y: 142.0500))
    path.addCurve(
      to: CGPoint(x: 457.4400, y: 125.9400), control1: CGPoint(x: 466.7400, y: 135.4000),
      control2: CGPoint(x: 463.1900, y: 129.2600))
    path.addLine(to: CGPoint(x: 457.4300, y: 125.9400))
    path.closeSubpath()
    path.move(to: CGPoint(x: 444.0500, y: 151.9900))
    path.addLine(to: CGPoint(x: 238.4200, y: 508.1500))
    path.addCurve(
      to: CGPoint(x: 233.3600, y: 506.7900), control1: CGPoint(x: 237.0300, y: 510.5500),
      control2: CGPoint(x: 233.3600, y: 509.5700))
    path.addLine(to: CGPoint(x: 233.3600, y: 273.5800))
    path.addCurve(
      to: CGPoint(x: 226.8300, y: 262.2700), control1: CGPoint(x: 233.3600, y: 268.9200),
      control2: CGPoint(x: 230.8700, y: 264.6100))
    path.addLine(to: CGPoint(x: 24.8700, y: 145.6700))
    path.addCurve(
      to: CGPoint(x: 26.2300, y: 140.6100), control1: CGPoint(x: 22.4700, y: 144.2800),
      control2: CGPoint(x: 23.4500, y: 140.6100))
    path.addLine(to: CGPoint(x: 437.4900, y: 140.6100))
    path.addCurve(
      to: CGPoint(x: 444.0600, y: 152.0000), control1: CGPoint(x: 443.3300, y: 140.6100),
      control2: CGPoint(x: 446.9800, y: 146.9400))
    path.addLine(to: CGPoint(x: 444.0500, y: 152.0000))
    path.closeSubpath()
    return path
  }()

  fileprivate static let antigravity: Path = {
    var path = Path()
    path.move(to: CGPoint(x: 89.6992, y: 93.6950))
    path.addCurve(
      to: CGPoint(x: 94.9492, y: 88.4450), control1: CGPoint(x: 94.3659, y: 97.1950),
      control2: CGPoint(x: 101.3660, y: 94.8617))
    path.addCurve(
      to: CGPoint(x: 55.8659, y: 18.4450), control1: CGPoint(x: 75.6992, y: 69.7783),
      control2: CGPoint(x: 79.7825, y: 18.4450))
    path.addCurve(
      to: CGPoint(x: 16.7825, y: 88.4450), control1: CGPoint(x: 31.9492, y: 18.4450),
      control2: CGPoint(x: 36.0325, y: 69.7783))
    path.addCurve(
      to: CGPoint(x: 22.0325, y: 93.6950), control1: CGPoint(x: 9.7825, y: 95.4450),
      control2: CGPoint(x: 17.3658, y: 97.1950))
    path.addCurve(
      to: CGPoint(x: 55.8659, y: 59.8617), control1: CGPoint(x: 40.1159, y: 81.4450),
      control2: CGPoint(x: 38.9492, y: 59.8617))
    path.addCurve(
      to: CGPoint(x: 89.6992, y: 93.6950), control1: CGPoint(x: 72.7825, y: 59.8617),
      control2: CGPoint(x: 71.6159, y: 81.4450))
    path.closeSubpath()
    return path
  }()
}
