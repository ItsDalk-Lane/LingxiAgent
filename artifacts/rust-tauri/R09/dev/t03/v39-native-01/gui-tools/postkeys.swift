
import CoreGraphics
import Foundation
// usage: postkeys <combo>   combo 形如 "alt+space" / "cmd+alt+r" / "cmd+period"
let combo = CommandLine.arguments.count > 1 ? CommandLine.arguments[1].lowercased() : ""
func flag(_ name: String) -> CGEventFlags {
  switch name {
  case "cmd", "command", "commandorcontrol", "cmdorctrl": return .maskCommand
  case "ctrl", "control": return .maskControl
  case "alt", "option": return .maskAlternate
  case "shift": return .maskShift
  default: return []
  }
}
func keyCode(_ name: String) -> CGKeyCode? {
  let map: [String: CGKeyCode] = ["a":0,"s":1,"d":2,"f":3,"h":4,"g":5,"z":6,"x":7,"c":8,"v":9,
    "b":11,"q":12,"w":13,"e":14,"r":15,"y":16,"t":17,"1":18,"2":19,"3":20,"4":21,"6":22,"5":23,
    "=":24,"9":25,"7":26,"-":27,"8":28,"0":29,"]":30,"o":31,"u":32,"[":33,"i":34,"p":35,
    "return":36,"enter":36,"l":37,"j":38,"'":39,"k":40,";":41,"\\":42,",":43,"/":44,"n":45,"m":46,
    ".":47,"tab":48,"space":49,"`":50,"delete":51,"escape":53,"esc":53,
    "left":123,"right":124,"down":125,"up":126,"home":115,"end":119,"pageup":116,"pagedown":121]
  return map[name]
}
let parts = combo.split(separator: "+").map(String.init)
guard let keyName = parts.last, let code = keyCode(keyName) else { print("{\"ok\":false,\"error\":\"unknown key\"}"); exit(0) }
var flags: CGEventFlags = []
for part in parts.dropLast() { flags = flags.union(flag(part)) }
guard let down = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true),
      let up = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: false) else {
  print("{\"ok\":false,\"error\":\"event create failed\"}"); exit(0) }
down.flags = flags; up.flags = flags
down.post(tap: .cghidEventTap)
usleep(40_000)
up.post(tap: .cghidEventTap)
print("{\"ok\":true}")
