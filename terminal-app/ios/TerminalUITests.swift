import XCTest

final class TerminalUITests: XCTestCase {
    func capture(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testMachinePicker() throws {
        continueAfterFailure = false
        let app = XCUIApplication(bundleIdentifier: "com.monkhai.ized-terminal")
        app.launch()
        let table = app.tables["machine-list"]
        XCTAssertTrue(table.waitForExistence(timeout: 12), "Launch should open the machine picker directly")
        let machines = table.cells.matching(NSPredicate(format: "identifier BEGINSWITH %@", "machine-"))
        guard machines.count > 0 else { throw XCTSkip("Import a Tailscale roster first") }
        sleep(3) // Allow bounded SSH reachability checks to finish.
        capture("01-tailnet-machines")
        let search = app.textFields["machine-search"]
        search.tap()
        let absent = "this-machine-does-not-exist"
        search.typeText(absent)
        XCTAssertEqual(machines.count, 0)
        search.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: absent.count))
        search.typeText("\n")
        XCTAssertGreaterThan(machines.count, 0)
        machines.firstMatch.tap()
        XCTAssertTrue(app.textFields["ssh-username"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["open-terminal"].exists)
        XCTAssertFalse(app.textFields["connection-command"].exists, "Advanced settings start collapsed")
        XCTAssertFalse(app.alerts.firstMatch.exists, "Connection setup must not use an alert")
        capture("02-machine-username")
        app.navigationBars.buttons["Machines"].tap()
        app.buttons["add-manually"].tap()
        XCTAssertTrue(app.textFields["connection-address"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.alerts.firstMatch.exists)
        capture("03-manual-details")
        app.navigationBars.buttons["Machines"].tap()
    }

    // Use the same picker as a normal launch, keeping only test settings isolated.
    @discardableResult
    func connectFixture(_ app: XCUIApplication, address: String) throws -> String {
        app.launchEnvironment["IZED_PICKER_TEST"] = "1"
        app.launch()
        XCTAssertTrue(app.tables["machine-list"].waitForExistence(timeout: 12))
        // Cached rows can exist before the live Tailscale proxy is ready.
        app.buttons["Device settings"].tap()
        XCTAssertTrue(app.buttons["save-tailscale-device-name"].waitForExistence(timeout: 30), "Tailscale must be ready")
        capture("00-device-settings")
        app.navigationBars.buttons["Machines"].tap()
        capture("00-machine-picker")
        let target = try XCTUnwrap(URLComponents(string: "ssh://\(address)"))
        let host = try XCTUnwrap(target.host)
        let user = try XCTUnwrap(target.user)
        XCTAssertTrue(target.port == nil || target.port == 22, "Picker requires a fixture on tailnet port 22")
        let savedAddress = "\(user)@\(host.contains(":") ? "[\(host)]" : host)"
        let recent = app.tables["machine-list"].cells["saved-\(savedAddress)"]
        if recent.exists {
            recent.tap()
            return savedAddress
        }
        let machine = app.tables["machine-list"].cells["machine-\(host)"]
        XCTAssertTrue(machine.waitForExistence(timeout: 20), "Fixture must be discovered in Tailscale")
        machine.tap()
        let username = app.textFields["ssh-username"]
        XCTAssertTrue(username.waitForExistence(timeout: 5))
        username.tap(); username.typeText(user)
        app.buttons["open-terminal"].tap()
        return savedAddress
    }

    func testHostVerificationAfterPickerDismissal() throws {
        continueAfterFailure = false
        let address = ProcessInfo.processInfo.environment["IZED_TEST_HOST"] ?? ""
        guard !address.isEmpty && !address.contains("$(") else { throw XCTSkip("Set IZED_TEST_HOST to the SSH fixture") }
        let app = XCUIApplication(bundleIdentifier: "com.monkhai.ized-terminal")
        let savedAddress = try connectFixture(app, address: address)
        let trust = app.alerts.matching(NSPredicate(format: "label IN %@", ["Verify host identity", "Host identity changed"])).firstMatch
        XCTAssertTrue(trust.waitForExistence(timeout: 15), "Fingerprint prompt must survive the picker closing")
        XCTAssertTrue(trust.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "SHA256:")).firstMatch.exists)
        capture("01-verification-after-picker-connection")
        trust.buttons["Cancel"].tap()
        XCTAssertTrue(trust.waitForNonExistence(timeout: 5))
        // Cancelling must release the busy state; choosing the recent connection
        // immediately afterward must present its fingerprint again.
        let frame = app.frame
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.93, dy: 85 / frame.height)).tap()
        let recent = app.tables["machine-list"].cells["saved-\(savedAddress)"]
        XCTAssertTrue(recent.waitForExistence(timeout: 5))
        recent.tap()
        XCTAssertTrue(trust.waitForExistence(timeout: 15), "Recent connections must also wait for picker dismissal")
        capture("02-verification-after-recent-connection")
        trust.buttons["Cancel"].tap()
        XCTAssertTrue(trust.waitForNonExistence(timeout: 5))
    }

    func testPhoneSSHAndKeyboard() throws {
        continueAfterFailure = false
        let address = ProcessInfo.processInfo.environment["IZED_TEST_HOST"] ?? ""
        guard !address.isEmpty && !address.contains("$(") else { throw XCTSkip("Set IZED_TEST_HOST to the isolated SSH fixture") }
        let app = XCUIApplication(bundleIdentifier: "com.monkhai.ized-terminal")
        try connectFixture(app, address: address)
        let trust = app.alerts.matching(NSPredicate(format: "label IN %@", ["Verify host identity", "Host identity changed"])).firstMatch
        XCTAssertTrue(trust.waitForExistence(timeout: 20), "Must reach the SSH host and ask for trust")
        let expected = ProcessInfo.processInfo.environment["IZED_TEST_FINGERPRINT"] ?? ""
        XCTAssertTrue(expected.hasPrefix("SHA256:"), "Supply the independently verified fixture fingerprint")
        XCTAssertTrue(trust.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", expected)).firstMatch.exists, "Fixture fingerprint must match before accepting")
        capture("01-host-verification")
        let approve = trust.buttons.matching(NSPredicate(format: "label IN %@", ["Trust and connect", "Trust new identity"])).firstMatch
        approve.tap()
        if trust.exists { approve.tap() }
        XCTAssertTrue(trust.waitForNonExistence(timeout: 5))
        let terminal = app.descendants(matching: .any).matching(identifier: "ssh-terminal-screen").firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 15))
        expectation(for: NSPredicate(format: "value CONTAINS %@", "SSH ready"), evaluatedWith: terminal)
        waitForExpectations(timeout: 10)
        XCTAssertTrue((terminal.value as? String ?? "").contains("UTF-8: ✓"), "Split UTF-8 and ANSI packets must survive")
        let keyboard = app.keyboards.firstMatch
        // Dismiss the simulator keyboard’s non-binding first-use typing tip.
        let typingTip = app.buttons["Continue"]
        if typingTip.exists { typingTip.tap() }
        XCTAssertTrue(keyboard.keys["q"].waitForExistence(timeout: 5), "The keyboard must expose actual letter keys")
        let keyboardVisible = NSPredicate { _, _ in
            keyboard.exists && keyboard.frame.intersection(app.frame).height >= 100
        }
        XCTAssertTrue(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: keyboardVisible, object: nil)], timeout: 10) == .completed,
                      "Show the simulator software keyboard before running the keyboard-layout test")
        XCTAssertLessThanOrEqual(terminal.frame.maxY, keyboard.frame.minY - 40)
        capture("02-terminal-keyboard")
        app.typeText("phone_ssh_smoke\n")
        expectation(for: NSPredicate(format: "value CONTAINS %@", "PHONE_SSH_OK"), evaluatedWith: terminal)
        waitForExpectations(timeout: 10)
        capture("03-ssh-command")
        let frame = app.frame
        // GPUI exposes the grid, not its accessory buttons, to native accessibility.
        // Grid bottom inset 8 + strip border 1 + padding 5 + half button height 22.
        // Eight equal buttons: horizontal padding 6 and seven gaps of 2.
        let contentLeft = terminal.frame.minX - 10
        let contentWidth = terminal.frame.width + 20
        let buttonWidth = (contentWidth - 12 - 14) / 8
        let ctrl = CGPoint(x: contentLeft + 6 + 2 * (buttonWidth + 2) + buttonWidth / 2,
                           y: terminal.frame.maxY + 8 + 1 + 5 + 22)
        XCTAssertTrue(frame.contains(ctrl), "Ctrl must be inside the visible app")
        XCTAssertLessThan(ctrl.y + 22, keyboard.frame.minY, "Ctrl must be above the keyboard")
        app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: ctrl.x - frame.minX, dy: ctrl.y - frame.minY)).tap()
        app.typeText("c")
        expectation(for: NSPredicate(format: "value CONTAINS %@", "^C"), evaluatedWith: terminal)
        waitForExpectations(timeout: 5)
        capture("04-control-c")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 85 / frame.height)).tap()
        sleep(1)
        capture("05-session-switcher")
    }
}
