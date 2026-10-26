import Foundation
import UIKit
import AuthenticationServices
import TailscaleKit

actor EmbeddedTailscale {
    static let shared = EmbeddedTailscale()

    private var node: TailscaleNode?
    private var startupTask: Task<Void, Never>?
    private var loginMonitorTask: Task<Void, Never>?
    private var loginStarting = false
    private var activationObserver: NSObjectProtocol?
    private var hostnameApplied = false

    func start() async {
        do {
            let node = try await getOrCreateNode()
            if startupTask == nil {
                startupTask = Task {
                    do {
                        try await node.up()
                        await self.refresh()
                    } catch {
                        if !Task.isCancelled {
                            self.publishError("Could not start Tailscale: \(error.localizedDescription)")
                        }
                    }
                }
            }
            observeActivation()
            await refresh()
        } catch {
            publishError("Could not start Tailscale: \(error.localizedDescription)")
        }
    }

    func signIn(deviceName: String) async {
        guard !loginStarting, loginMonitorTask == nil else { return }
        loginStarting = true
        defer { loginStarting = false }
        await saveDeviceName(deviceName)
        await start()
        guard let node else { return }
        do {
            let localAPI = LocalAPIClient(localNode: node, logger: nil)
            try await localAPI.startLoginInteractive()

            // Login is the only time we poll. The polling ends as soon as the
            // browser URL appears; machine updates use explicit refreshes.
            for _ in 0..<12 {
                let status = try await node.statusJSON()
                if let object = try JSONSerialization.jsonObject(with: status) as? [String: Any],
                   let authURL = object["AuthURL"] as? String,
                   let url = URL(string: authURL), !authURL.isEmpty {
                    publishError("Waiting for Tailscale sign-in…")
                    let started = await MainActor.run {
                        TailscaleWebAuthentication.shared.start(url: url) { [weak self] finished, error in
                            Task { await self?.loginSessionFinished(finished: finished, error: error) }
                        }
                    }
                    guard started else {
                        publishError("Could not open Tailscale sign-in. Try again.")
                        return
                    }
                    loginMonitorTask = Task { [weak self] in await self?.monitorLogin() }
                    return
                }
                try await Task.sleep(nanoseconds: 500_000_000)
            }
            publishError("Tailscale sign-in is still starting. Tap refresh in a moment.")
        } catch {
            publishError("Tailscale sign-in failed: \(error.localizedDescription)")
        }
    }

    func updateDeviceName(_ displayName: String) async {
        await saveDeviceName(displayName)
        hostnameApplied = false
        await refresh()
    }

    func signOut() async {
        loginMonitorTask?.cancel()
        loginMonitorTask = nil
        guard let node else { return }
        do {
            let localAPI = LocalAPIClient(localNode: node, logger: nil)
            try await localAPI.resetAuth()
            hostnameApplied = false
            await refresh()
        } catch {
            publishError("Could not sign out of Tailscale: \(error.localizedDescription)")
        }
    }

    private func loginSessionFinished(finished: Bool, error: Error?) async {
        loginMonitorTask?.cancel()
        loginMonitorTask = nil
        if finished {
            await refresh()
        } else if let authError = error as? ASWebAuthenticationSessionError,
                  authError.code == .canceledLogin {
            await refresh()
        } else if let error {
            publishError("Tailscale sign-in failed: \(error.localizedDescription)")
        } else {
            await refresh()
        }
    }

    private func monitorLogin() async {
        defer { loginMonitorTask = nil }
        while !Task.isCancelled {
            guard let node else { return }
            if let status = try? await node.statusJSON(),
               let object = try? JSONSerialization.jsonObject(with: status) as? [String: Any],
               (object["BackendState"] as? String) == "Running",
               ((object["AuthURL"] as? String) ?? "").isEmpty {
                await MainActor.run { TailscaleWebAuthentication.shared.finishAfterLogin() }
                await refresh()
                return
            }
            do { try await Task.sleep(nanoseconds: 2_000_000_000) }
            catch { return }
        }
    }

    func refresh() async {
        guard let node else {
            await start()
            return
        }
        do {
            let status = try await node.statusJSON()
            guard let object = try JSONSerialization.jsonObject(with: status) as? [String: Any] else {
                publishError("Tailscale returned an unreadable device status.")
                return
            }
            guard (object["BackendState"] as? String) == "Running" else {
                let message = (object["BackendState"] as? String) == "NeedsLogin"
                    ? "Sign in to Tailscale to see your machines."
                    : "Connecting to Tailscale…"
                publishError(message)
                return
            }

            if !hostnameApplied {
                do {
                    let localAPI = LocalAPIClient(localNode: node, logger: nil)
                    _ = try await localAPI.editPrefs(mask: Ipn.MaskedPrefs().hostname(await persistentHostName()))
                    hostnameApplied = true
                } catch {
                    // Keep the tailnet connection usable. A later refresh retries the rename.
                }
            }

            let proxy = try await node.loopback()
            guard let host = proxy.ip, let port = proxy.port, (1...65535).contains(port) else {
                publishError("Tailscale’s local connection is not ready yet.")
                return
            }
            publish(status, proxyHost: host, proxyPort: UInt16(port), credential: proxy.proxyCredential)
        } catch {
            publishError("Could not refresh Tailscale machines: \(error.localizedDescription)")
        }
    }

    private func getOrCreateNode() async throws -> TailscaleNode {
        if let node { return node }
        let base = try FileManager.default.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        let directory = base.appendingPathComponent("Tailscale", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var resourceValues = URLResourceValues()
        resourceValues.isExcludedFromBackup = true
        var protectedDirectory = directory
        try protectedDirectory.setResourceValues(resourceValues)
        let configuration = Configuration(
            hostName: await persistentHostName(),
            path: directory.path,
            authKey: nil,
            controlURL: kDefaultControlURL,
            ephemeral: false
        )
        let node = try TailscaleNode(config: configuration, logger: nil)
        self.node = node
        return node
    }

    private func persistentHostName() async -> String {
        let key = "ized-terminal.tailscale-hostname"
        if let value = UserDefaults.standard.string(forKey: key), !isLegacyGeneratedHostName(value) { return value }
        let value = hostName(from: await preferredDeviceName())
        UserDefaults.standard.set(value, forKey: key)
        return value
    }

    private func saveDeviceName(_ name: String) async {
        let cleaned = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let displayName = cleaned.isEmpty ? await preferredDeviceName() : cleaned
        UserDefaults.standard.set(displayName, forKey: "ized-terminal.tailscale-device-name")
        UserDefaults.standard.set(hostName(from: displayName), forKey: "ized-terminal.tailscale-hostname")
        hostnameApplied = false
    }

    private func preferredDeviceName() async -> String {
        if let name = UserDefaults.standard.string(forKey: "ized-terminal.tailscale-device-name") {
            return name.caseInsensitiveCompare("iPhone") == .orderedSame ? "iZed iPhone" : name
        }
        let deviceName = await MainActor.run { UIDevice.current.name }
        return deviceName.caseInsensitiveCompare("iPhone") == .orderedSame ? "iZed iPhone" : deviceName
    }

    private func hostName(from displayName: String) -> String {
        let withoutPossessive = displayName
            .replacingOccurrences(of: "’s", with: "", options: .caseInsensitive)
            .replacingOccurrences(of: "'s", with: "", options: .caseInsensitive)
        let folded = withoutPossessive.folding(options: [.diacriticInsensitive, .caseInsensitive], locale: Locale(identifier: "en_US_POSIX"))
        let slug = folded
            .components(separatedBy: CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyz0123456789").inverted)
            .filter { !$0.isEmpty }
            .joined(separator: "-")
        let label = slug.isEmpty ? "iphone" : slug
        let hostname = label.hasPrefix("ized-") ? label : "ized-\(label)"
        return String(hostname.prefix(63)).trimmingCharacters(in: CharacterSet(charactersIn: "-"))
    }

    private func isLegacyGeneratedHostName(_ value: String) -> Bool {
        value.range(of: "^ized-terminal-[0-9a-f]{8}$", options: .regularExpression) != nil
    }

    private func observeActivation() {
        guard activationObserver == nil else { return }
        activationObserver = NotificationCenter.default.addObserver(
            forName: UIApplication.didBecomeActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in Task { await self?.refresh() } }
    }

    private func publish(_ data: Data, proxyHost: String, proxyPort: UInt16, credential: String) {
        guard let status = String(data: data, encoding: .utf8) else {
            publishError("Tailscale returned invalid device data.")
            return
        }
        status.withCString { statusPointer in
            proxyHost.withCString { hostPointer in
                credential.withCString { credentialPointer in
                    terminal_update_tailnet(statusPointer, hostPointer, proxyPort, credentialPointer, nil)
                }
            }
        }
    }

    private func publishError(_ message: String) {
        message.withCString { errorPointer in
            terminal_update_tailnet(nil, nil, 0, nil, errorPointer)
        }
    }
}

@MainActor
private final class TailscaleWebAuthentication: NSObject, ASWebAuthenticationPresentationContextProviding {
    static let shared = TailscaleWebAuthentication()

    private var session: ASWebAuthenticationSession?
    private var completion: ((Bool, Error?) -> Void)?
    private var finishingAfterLogin = false

    func start(url: URL, completion: @escaping (Bool, Error?) -> Void) -> Bool {
        guard session == nil else { return false }
        self.completion = completion
        finishingAfterLogin = false
        let session = ASWebAuthenticationSession(
            url: url,
            callback: .customScheme("com.monkhai.ized-terminal.auth")
        ) { [weak self] callbackURL, error in
            Task { @MainActor in
                guard let self else { return }
                let finished = self.finishingAfterLogin || callbackURL != nil
                let completion = self.completion
                self.session = nil
                self.completion = nil
                self.finishingAfterLogin = false
                completion?(finished, error)
            }
        }
        session.presentationContextProvider = self
        session.prefersEphemeralWebBrowserSession = false
        self.session = session
        if session.start() { return true }
        self.session = nil
        self.completion = nil
        return false
    }

    func finishAfterLogin() {
        guard let session else { return }
        finishingAfterLogin = true
        session.cancel()
    }

    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
            .first(where: \.isKeyWindow) ?? ASPresentationAnchor()
    }
}

@_cdecl("terminal_start_tailscale")
func terminalStartTailscale() {
    Task { await EmbeddedTailscale.shared.start() }
}

@_cdecl("terminal_refresh_tailscale")
func terminalRefreshTailscale() {
    Task { await EmbeddedTailscale.shared.refresh() }
}
