import XCTest

final class EditorUITests: XCTestCase {
    func testZedTabsAndProjectPanel() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        attachScreenshot("Zed workspace with tabs and project panel")

        // GPUI's native views are not exposed as XCTest accessibility elements yet.
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.10, dy: 0.012)).tap()
        attachScreenshot("After opening main.rs tab")

        app.coordinate(withNormalizedOffset: CGVector(dx: 0.86, dy: 0.049)).tap()
        attachScreenshot("After opening notes.md from project panel")
    }

    func testWorkspaceFileSave() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))

        app.coordinate(withNormalizedOffset: CGVector(dx: 0.10, dy: 0.012)).tap()
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.20, dy: 0.031)).tap()
        app.typeText("iSAVED")
        let keyboardPrompt = app.buttons["Not Now"]
        if keyboardPrompt.waitForExistence(timeout: 1) {
            keyboardPrompt.tap()
        }
        app.typeKey("s", modifierFlags: .command)
        if keyboardPrompt.waitForExistence(timeout: 1) {
            keyboardPrompt.tap()
            app.typeKey("s", modifierFlags: .command)
        }
        attachScreenshot("After editing and saving main.rs")
    }

    func testProjectPanelLongPressMenu() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))

        app.coordinate(withNormalizedOffset: CGVector(dx: 0.86, dy: 0.049))
            .press(forDuration: 0.7)
        attachScreenshot("Zed Project Panel context menu")
    }

    func testProjectPanelNewFile() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))

        app.coordinate(withNormalizedOffset: CGVector(dx: 0.86, dy: 0.049))
            .press(forDuration: 0.7)
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.69, dy: 0.060)).tap()
        attachScreenshot("Zed New File editor")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.86, dy: 0.029)).tap()
        app.typeText("a-cycle.txt\n")
        attachScreenshot("After creating file in Project Panel")
        app.typeText("iNew file text")
        let keyboardPrompt = app.buttons["Not Now"]
        if keyboardPrompt.waitForExistence(timeout: 1) {
            keyboardPrompt.tap()
        }
        app.typeKey("s", modifierFlags: .command)
        if keyboardPrompt.waitForExistence(timeout: 1) {
            keyboardPrompt.tap()
            app.typeKey("s", modifierFlags: .command)
        }
        attachScreenshot("After saving new file")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.86, dy: 0.029))
            .press(forDuration: 0.7)
        attachScreenshot("Delete context menu")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.69, dy: 0.326)).tap()
        let confirmation = app.alerts.firstMatch
        XCTAssertTrue(confirmation.waitForExistence(timeout: 3))
        attachScreenshot("Delete confirmation")
        confirmation.buttons["Delete"].tap()
        attachScreenshot("After deleting file")
    }

    private func attachScreenshot(_ name: String) {
        XCTContext.runActivity(named: name) { activity in
            let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            attachment.lifetime = .keepAlways
            activity.add(attachment)
        }
    }
}
