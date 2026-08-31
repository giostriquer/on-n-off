import AppKit
import Darwin
import NotchCore

let application = NSApplication.shared
application.setActivationPolicy(.accessory)
if CommandLine.arguments.contains("--displays") {
  do {
    let data = try JSONEncoder().encode(Screens.read())
    FileHandle.standardOutput.write(data + Data([10]))
    exit(0)
  } catch { exit(1) }
}
let controller = PanelController()
let outputQueue = DispatchQueue(label: "app.on-n-off.notch.output")
let outputSlots = DispatchSemaphore(value: 32)
func stopAfterDisconnect() {
  // AppKit normally terminates cleanly. A stuck main queue must not leave an
  // orphan window after the parent exits or its protocol connection fails.
  DispatchQueue.main.async {
    controller.shutdown()
    application.terminate(nil)
  }
  DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 2) { _exit(0) }
}
controller.emit = { action in
  guard let data = try? JSONEncoder().encode(action),
    outputSlots.wait(timeout: .now()) == .success
  else {
    stopAfterDisconnect()
    return
  }
  outputQueue.async {
    defer { outputSlots.signal() }
    do { try FileHandle.standardOutput.write(contentsOf: data + Data([10])) } catch {
      stopAfterDisconnect()
    }
  }
}
// One message at a time reaches the main thread; EOF also covers an abrupt parent exit.
DispatchQueue.global(qos: .utility).async {
  var buffer = Data()
  let pending = DispatchSemaphore(value: 1)
  do {
    while true {
      let chunk = FileHandle.standardInput.availableData
      if chunk.isEmpty { break }
      buffer.append(chunk)
      while let end = buffer.firstIndex(of: 10) {
        let message = try HostMessage.decode(Data(buffer[..<end]))
        buffer.removeSubrange(...end)
        guard pending.wait(timeout: .now() + 2) == .success else {
          throw ProtocolError.invalidMessage
        }
        DispatchQueue.main.async {
          controller.accept(message)
          pending.signal()
        }
      }
      if buffer.count > 262_144 { throw ProtocolError.invalidMessage }
    }
  } catch {
    try? FileHandle.standardError.write(contentsOf: Data("Native notch connection failed.\n".utf8))
  }
  stopAfterDisconnect()
}
controller.emit(.ready)
#if DEBUG
  // Exercise parent-death cleanup without a responsive AppKit event loop.
  if CommandLine.arguments.contains("--check-unresponsive-main") {
    DispatchSemaphore(value: 0).wait()
  }
#endif
application.run()
