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

    func testHostVerificationAfterPickerDismissal() throws {
        continueAfterFailure = false
        let address = ProcessInfo.processInfo.environment["IZED_TEST_HOST"] ?? ""
        guard !address.isEmpty && !address.contains("$(") else { throw XCTSkip("Set IZED_TEST_HOST to the SSH fixture") }
        let app = XCUIApplication(bundleIdentifier: "com.monkhai.ized-terminal")
        app.launchEnvironment["IZED_PICKER_TEST"] = "1"
        app.launch()
        XCTAssertTrue(app.tables["machine-list"].waitForExistence(timeout: 12))
        app.buttons["add-manually"].tap()
        let name = app.textFields["connection-name"]
        XCTAssertTrue(name.waitForExistence(timeout: 5))
        name.tap(); name.typeText("Verification regression")
        let target = app.textFields["connection-address"]
        target.tap(); target.typeText(address)
        app.buttons["open-terminal"].tap()
        let trust = app.alerts["Verify host identity"]
        XCTAssertTrue(trust.waitForExistence(timeout: 15), "Fingerprint prompt must survive the picker closing")
        XCTAssertTrue(trust.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "SHA256:")).firstMatch.exists)
        capture("01-verification-after-new-connection")
        trust.buttons["Cancel"].tap()
        XCTAssertTrue(trust.waitForNonExistence(timeout: 5))
        // Cancelling must release the busy state; choosing the recent connection
        // immediately afterward must present its fingerprint again.
        let frame = app.frame
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.93, dy: 85 / frame.height)).tap()
        let recent = app.tables["machine-list"].cells["saved-\(address)"]
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
        app.launchEnvironment["IZED_SMOKE_TEST"] = "1"
        app.launchEnvironment["IZED_TEST_AUTO_HOST"] = address
        app.launch()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 15))
        let trust = app.alerts["Verify host identity"]
        XCTAssertTrue(trust.waitForExistence(timeout: 20), "Must reach the SSH host and ask for trust")
        capture("01-host-verification")
        trust.buttons["Trust and connect"].tap()
        if trust.exists { trust.buttons["Trust and connect"].tap() }
        XCTAssertTrue(trust.waitForNonExistence(timeout: 5))
        let terminal = app.descendants(matching: .any).matching(identifier: "ssh-terminal-screen").firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 15))
        expectation(for: NSPredicate(format: "value CONTAINS %@", "SSH ready"), evaluatedWith: terminal)
        waitForExpectations(timeout: 10)
        XCTAssertTrue((terminal.value as? String ?? "").contains("UTF-8: ✓"), "Split UTF-8 and ANSI packets must survive")
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        XCTAssertLessThanOrEqual(terminal.frame.maxY, app.keyboards.firstMatch.frame.minY - 40)
        capture("02-terminal-keyboard")
        app.typeText("phone_ssh_smoke\n")
        expectation(for: NSPredicate(format: "value CONTAINS %@", "PHONE_SSH_OK"), evaluatedWith: terminal)
        waitForExpectations(timeout: 10)
        capture("03-ssh-command")
        let frame = app.frame
        let keyboardTop = app.keyboards.firstMatch.frame.minY
        app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: frame.width * 2.5 / 7, dy: keyboardTop - 27)).tap()
        app.typeText("c")
        expectation(for: NSPredicate(format: "value CONTAINS %@", "^C"), evaluatedWith: terminal)
        waitForExpectations(timeout: 5)
        capture("04-control-c")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 85 / frame.height)).tap()
        sleep(1)
        capture("05-session-switcher")
    }
}
