import XCTest

final class GpuiExampleUITests: XCTestCase {
    func testFormTextEntry() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))

        // GPUI's sample draws its own controls, so use normalized screen positions.
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.535)).tap()
        let form = XCUIScreen.main.screenshot()
        XCTContext.runActivity(named: "Form after touch navigation") { activity in
            let attachment = XCTAttachment(screenshot: form)
            attachment.lifetime = .keepAlways
            activity.add(attachment)
        }
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.35, dy: 0.17)).tap()
        app.typeText(" GPUI")
        XCTContext.runActivity(named: "Form after text entry") { activity in
            let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            attachment.lifetime = .keepAlways
            activity.add(attachment)
        }
        XCUIDevice.shared.orientation = .landscapeLeft
        XCTContext.runActivity(named: "Form after rotation") { activity in
            let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            attachment.lifetime = .keepAlways
            activity.add(attachment)
        }
        XCUIDevice.shared.orientation = .portrait
    }
}
