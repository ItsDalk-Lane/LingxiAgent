
import Foundation
import UserNotifications
let sem = DispatchSemaphore(value: 0)
var result: [[String: Any]] = []
UNUserNotificationCenter.current().getDeliveredNotifications { notifications in
  result = notifications.map { n in
    ["title": n.request.content.title, "body": n.request.content.body,
     "identifier": n.request.identifier]
  }
  sem.signal()
}
_ = sem.wait(timeout: .now() + 5)
let data = try! JSONSerialization.data(withJSONObject: result, options: [])
print(String(data: data, encoding: .utf8)!)
