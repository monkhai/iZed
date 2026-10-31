import UIKit

private enum ZedStyle {
    static let background = UIColor(white: 10 / 255, alpha: 1)
    static let surface = UIColor(white: 17 / 255, alpha: 1)
    static let border = UIColor(white: 46 / 255, alpha: 1)
    static let text = UIColor(white: 237 / 255, alpha: 1)
    static let muted = UIColor(white: 160 / 255, alpha: 1)
    static let green = UIColor(red: 0, green: 172 / 255, blue: 58 / 255, alpha: 1)
    static func mono(_ size: CGFloat, medium: Bool = false) -> UIFont {
        UIFont(name: medium ? "JetBrainsMono-Medium" : "JetBrainsMono-Regular", size: size)
            ?? .monospacedSystemFont(ofSize: size, weight: medium ? .medium : .regular)
    }
}

private func defaultTailscaleDeviceName() -> String {
    let name = UIDevice.current.name
    return name.caseInsensitiveCompare("iPhone") == .orderedSame ? "iZed iPhone" : name
}

private struct Machine: Decodable {
    let name: String
    let hostname: String
    let address: String
    let os: String
    let online: Bool
}
private struct SavedHost: Decodable {
    let name: String
    let address: String
    let command: String
    func matches(_ machine: Machine) -> Bool {
        guard address.contains("@") else { return false }
        let endpoint = String(address.split(separator: "@", maxSplits: 1).last ?? "")
        let host: String
        if endpoint.hasPrefix("[") { host = String(endpoint.dropFirst().split(separator: "]", maxSplits: 1).first ?? "") }
        else { host = String(endpoint.split(separator: ":", maxSplits: 1).first ?? "") }
        let names = [machine.address, machine.hostname, String(machine.hostname.split(separator: ".").first ?? "")]
        return names.filter { !$0.isEmpty }.contains { $0.lowercased() == host.lowercased() }
    }
}
private struct Roster: Decodable {
    let machines: [Machine]
    let updated_at: UInt64
}
private struct PickerPayload: Decodable {
    let roster: Roster
    let saved: [SavedHost]
    let public_key: String
    let message: String
    let refreshing: Bool
    let has_session: Bool
    let tailnet_ready: Bool
}

@_cdecl("terminal_machines")
func terminalMachines(_ json: UnsafePointer<CChar>, _ present: Bool) {
    let data = Data(String(cString: json).utf8)
    guard let payload = try? JSONDecoder().decode(PickerPayload.self, from: data) else { return }
    DispatchQueue.main.async {
        if let picker = MachinePicker.current {
            picker.update(payload)
        } else if present {
            MachinePicker.present(payload)
        }
    }
}

private final class MachinePicker: UITableViewController, UITextFieldDelegate {
    static weak var current: MachinePicker?
    private static var activationObserver: NSObjectProtocol?
    private var payload: PickerPayload
    private let search = UITextField()
    private let deviceNameField = UITextField()
    private let note = UILabel()
    private var query = ""
    private weak var deviceSettings: MachineDeviceSettings?

    init(_ payload: PickerPayload) { self.payload = payload; super.init(style: .plain) }
    required init?(coder: NSCoder) { fatalError() }

    static func present(_ payload: PickerPayload) {
        if UIApplication.shared.applicationState != .active {
            guard activationObserver == nil else { return }
            activationObserver = NotificationCenter.default.addObserver(forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main) { _ in
                if let observer = Self.activationObserver { NotificationCenter.default.removeObserver(observer) }
                Self.activationObserver = nil
                if Self.current == nil { Self.present(payload) }
            }
            return
        }
        guard let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first(where: { $0.activationState == .foregroundActive }),
              let root = scene.windows.first(where: { $0.isKeyWindow })?.rootViewController else { return }
        var presenter = root
        while let next = presenter.presentedViewController { presenter = next }
        let picker = MachinePicker(payload)
        let navigation = UINavigationController(rootViewController: picker)
        navigation.overrideUserInterfaceStyle = .dark
        navigation.modalPresentationStyle = payload.has_session ? .pageSheet : .fullScreen
        navigation.sheetPresentationController?.prefersGrabberVisible = true
        current = picker
        presenter.present(navigation, animated: true)
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        title = "Machines"
        view.backgroundColor = ZedStyle.background
        tableView.backgroundColor = ZedStyle.background
        tableView.separatorColor = ZedStyle.border
        tableView.separatorInset = UIEdgeInsets(top: 0, left: 66, bottom: 0, right: 18)
        tableView.rowHeight = 76
        tableView.sectionHeaderTopPadding = 0
        tableView.estimatedSectionHeaderHeight = 0
        tableView.estimatedSectionFooterHeight = 0
        tableView.sectionFooterHeight = .leastNormalMagnitude
        tableView.keyboardDismissMode = .interactive
        tableView.accessibilityIdentifier = "machine-list"
        let appearance = UINavigationBarAppearance()
        appearance.configureWithOpaqueBackground()
        appearance.backgroundColor = ZedStyle.background
        appearance.shadowColor = ZedStyle.border
        appearance.titleTextAttributes = [.font: ZedStyle.mono(17, medium: true), .foregroundColor: ZedStyle.text]
        navigationController?.navigationBar.standardAppearance = appearance
        navigationController?.navigationBar.scrollEdgeAppearance = appearance
        navigationController?.navigationBar.tintColor = ZedStyle.text
        let refreshItem = UIBarButtonItem(image: UIImage(systemName: "arrow.clockwise"), style: .plain, target: self, action: #selector(refresh))
        refreshItem.accessibilityLabel = "Refresh machines"
        let settingsItem = UIBarButtonItem(image: UIImage(systemName: "gearshape"), style: .plain, target: self, action: #selector(showDeviceSettings))
        settingsItem.accessibilityLabel = "Device settings"
        navigationItem.rightBarButtonItems = [refreshItem, settingsItem]
        if payload.has_session { navigationItem.leftBarButtonItem = UIBarButtonItem(barButtonSystemItem: .close, target: self, action: #selector(close)) }

        let header = UIView(frame: CGRect(x: 0, y: 0, width: 430, height: 118))
        search.placeholder = "Find a machine"
        search.font = ZedStyle.mono(14)
        search.textColor = ZedStyle.text
        search.tintColor = ZedStyle.text
        search.backgroundColor = ZedStyle.surface
        search.layer.cornerRadius = 6
        search.layer.borderWidth = 1
        search.layer.borderColor = ZedStyle.border.cgColor
        search.autocorrectionType = .no
        search.autocapitalizationType = .none
        deviceNameField.text = UserDefaults.standard.string(forKey: "ized-terminal.tailscale-device-name") ?? defaultTailscaleDeviceName()
        deviceNameField.placeholder = "Name this iPhone in Tailscale"
        deviceNameField.font = ZedStyle.mono(12)
        deviceNameField.textColor = ZedStyle.text
        deviceNameField.tintColor = ZedStyle.text
        deviceNameField.backgroundColor = ZedStyle.surface
        deviceNameField.layer.cornerRadius = 6
        deviceNameField.layer.borderWidth = 1
        deviceNameField.layer.borderColor = ZedStyle.border.cgColor
        deviceNameField.leftView = UIView(frame: CGRect(x: 0, y: 0, width: 12, height: 1))
        deviceNameField.leftViewMode = .always
        deviceNameField.autocorrectionType = .no
        deviceNameField.autocapitalizationType = .words
        deviceNameField.returnKeyType = .done
        deviceNameField.delegate = self
        deviceNameField.accessibilityIdentifier = "tailscale-device-name"
        search.clearButtonMode = .whileEditing
        search.returnKeyType = .done
        search.delegate = self
        search.accessibilityIdentifier = "machine-search"
        search.addTarget(self, action: #selector(filter), for: .editingChanged)
        // UITextField sizes an image accessory from its intrinsic size. A fixed
        // container preserves the leading inset and the gap before the text.
        let searchAccessory = UIView(frame: CGRect(x: 0, y: 0, width: 40, height: 44))
        let icon = UIImageView(image: UIImage(systemName: "magnifyingglass",
            withConfiguration: UIImage.SymbolConfiguration(pointSize: 16, weight: .regular)))
        icon.tintColor = ZedStyle.muted
        icon.contentMode = .scaleAspectFit
        icon.frame = CGRect(x: 12, y: 14, width: 16, height: 16)
        searchAccessory.addSubview(icon)
        search.leftView = searchAccessory
        search.contentVerticalAlignment = .center
        search.leftViewMode = .always
        search.translatesAutoresizingMaskIntoConstraints = false
        note.font = ZedStyle.mono(10)
        note.textColor = ZedStyle.muted
        note.numberOfLines = 2
        note.translatesAutoresizingMaskIntoConstraints = false
        header.addSubview(search); header.addSubview(note)
        NSLayoutConstraint.activate([
            search.leadingAnchor.constraint(equalTo: header.leadingAnchor, constant: 18),
            search.trailingAnchor.constraint(equalTo: header.trailingAnchor, constant: -18),
            search.topAnchor.constraint(equalTo: header.topAnchor, constant: 18),
            search.heightAnchor.constraint(equalToConstant: 44),
            note.leadingAnchor.constraint(equalTo: search.leadingAnchor, constant: 2),
            note.trailingAnchor.constraint(equalTo: search.trailingAnchor),
            note.topAnchor.constraint(equalTo: search.bottomAnchor, constant: 12),
        ])
        tableView.tableHeaderView = header
        tableView.tableFooterView = footer()
        updateNote()
    }

    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        if isBeingDismissed || navigationController?.isBeingDismissed == true {
            Self.current = nil
        }
    }

    func update(_ payload: PickerPayload) {
        self.payload = payload
        guard isViewLoaded else { return }
        updateNote()
        tableView.reloadData()
        tableView.tableFooterView = footer()
        navigationItem.rightBarButtonItem?.isEnabled = !payload.refreshing
        deviceSettings?.reload()
    }

    private func updateNote() {
        if payload.refreshing { note.text = "Refreshing Tailscale machines…" }
        else if !payload.message.isEmpty { note.text = payload.message }
        else if !payload.tailnet_ready { note.text = "Connect to Tailscale to see your machines." }
        else if !query.isEmpty {
            let count = saved.count + machines.count
            if count == 0 {
                note.text = "No results for “\(query)”.\nTry another name, hostname, or IP address."
            } else {
                note.text = "\(count) \(count == 1 ? "result" : "results") for “\(query)”\nChoose a computer to connect over SSH."
            }
        }
        else if payload.roster.machines.isEmpty { note.text = "No computers are available in this tailnet yet." }
        else {
            let age = max(0, Date().timeIntervalSince1970 - Double(payload.roster.updated_at))
            let time = age < 60 ? "just now" : age < 3600 ? "\(Int(age / 60))m ago" : "\(Int(age / 3600))h ago"
            note.text = "Choose a computer to connect over SSH.\nTailscale machines · updated \(time)"
        }
    }

    private var machines: [Machine] {
        payload.roster.machines.filter { machine in
            !payload.saved.contains(where: { $0.matches(machine) })
                && (query.isEmpty || "\(machine.name) \(machine.hostname) \(machine.address)".localizedCaseInsensitiveContains(query))
        }
    }
    private var saved: [SavedHost] {
        payload.saved.filter { query.isEmpty || "\($0.name) \($0.address)".localizedCaseInsensitiveContains(query) }
    }
    override func numberOfSections(in tableView: UITableView) -> Int { 2 }
    override func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int { section == 0 ? saved.count : machines.count }
    override func tableView(_ tableView: UITableView, viewForHeaderInSection section: Int) -> UIView? {
        guard (section == 0 ? saved.count : machines.count) > 0 else { return nil }
        let label = UILabel()
        label.font = ZedStyle.mono(10, medium: true)
        label.textColor = ZedStyle.muted
        label.text = section == 0 ? "  SAVED CONNECTIONS" : "  COMPUTERS"
        label.backgroundColor = ZedStyle.background
        return label
    }
    override func tableView(_ tableView: UITableView, heightForHeaderInSection section: Int) -> CGFloat { (section == 0 ? saved.count : machines.count) > 0 ? 34 : .leastNormalMagnitude }
    override func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = UITableViewCell(style: .subtitle, reuseIdentifier: nil)
        cell.backgroundColor = ZedStyle.background
        let selected = UIView(); selected.backgroundColor = ZedStyle.surface; cell.selectedBackgroundView = selected
        cell.textLabel?.font = ZedStyle.mono(13, medium: true)
        cell.textLabel?.textColor = ZedStyle.text
        cell.detailTextLabel?.font = ZedStyle.mono(10)
        cell.detailTextLabel?.textColor = ZedStyle.muted
        cell.imageView?.tintColor = ZedStyle.muted
        if indexPath.section == 0 {
            let host = saved[indexPath.row]
            cell.textLabel?.text = host.name
            cell.detailTextLabel?.text = host.address
            cell.imageView?.image = UIImage(systemName: "terminal")
            cell.accessoryType = .disclosureIndicator
            cell.accessibilityHint = "Connect using this saved SSH connection."
            cell.accessibilityIdentifier = "saved-\(host.address)"
        } else {
            let machine = machines[indexPath.row]
            cell.textLabel?.text = machine.name
            let ready = machine.online
            let status = ready ? "Online in Tailscale" : "Offline in Tailscale"
            cell.detailTextLabel?.text = "\(machine.os) · \(status)"
            cell.imageView?.image = UIImage(systemName: machine.os.lowercased() == "macos" ? (machine.name.lowercased().contains("macbook") ? "laptopcomputer" : "desktopcomputer") : "server.rack")
            cell.accessoryType = .disclosureIndicator
            cell.accessibilityHint = "Open SSH connection details for this computer."
            cell.accessibilityIdentifier = "machine-\(machine.address)"
            cell.accessibilityLabel = "\(machine.name), \(machine.os), \(status)"
        }
        return cell
    }
    override func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)
        if indexPath.section == 0 { connect(saved[indexPath.row]); return }
        let machine = machines[indexPath.row]
        if let host = payload.saved.first(where: { $0.matches(machine) }) { connect(host); return }
        let detail = ConnectionEditor(machine: machine, publicKey: payload.public_key)
        navigationController?.pushViewController(detail, animated: true)
    }

    private func connect(_ host: SavedHost) {
        Self.current = nil
        // A fast SSH probe can finish during the dismissal animation. Submit
        // only once the terminal is again the stable presenting controller.
        dismiss(animated: true) {
            host.name.withCString { name in host.address.withCString { address in host.command.withCString { command in terminal_add_host(name, address, command) } } }
        }
    }
    // A signed-in machine list contains only connection choices. Device/account
    // controls live behind the gear button, outside the search results.
    private func footer() -> UIView {
        payload.tailnet_ready ? UIView(frame: .zero) : deviceSettingsForm()
    }

    @objc private func showDeviceSettings() {
        let controller = MachineDeviceSettings { [weak self] in
            self?.deviceSettingsForm() ?? UIView()
        }
        deviceSettings = controller
        navigationController?.pushViewController(controller, animated: view.window != nil)
    }

    private func deviceSettingsForm() -> UIView {
        let connected = payload.tailnet_ready
        let footer = UIView(frame: CGRect(x: 0, y: 0, width: 390, height: connected ? 360 : 304))
        footer.backgroundColor = ZedStyle.background
        if !deviceNameField.isFirstResponder {
            deviceNameField.text = UserDefaults.standard.string(forKey: "ized-terminal.tailscale-device-name") ?? defaultTailscaleDeviceName()
        }
        func label(_ text: String, size: CGFloat = 11) -> UILabel {
            let label = UILabel()
            label.text = text; label.numberOfLines = 0
            label.font = ZedStyle.mono(size); label.textColor = ZedStyle.muted
            label.translatesAutoresizingMaskIntoConstraints = false
            footer.addSubview(label)
            return label
        }
        let heading = label(connected ? "THIS DEVICE" : "CONNECT THIS DEVICE", size: 10)
        let introduction = label("How this device appears in Tailscale.")
        let fieldLabel = label("DEVICE NAME", size: 10)
        deviceNameField.accessibilityLabel = "This device’s Tailscale name"
        deviceNameField.translatesAutoresizingMaskIntoConstraints = false
        footer.addSubview(deviceNameField)
        let kind = UIDevice.current.userInterfaceIdiom == .pad ? "iPad" : "iPhone"
        let explanation = label("This names your \(kind), not the computer you connect to.")
        let action = UIButton(type: .system)
        let loginPending = payload.message == "Waiting for Tailscale sign-in…"
        action.setTitle(connected ? "Save device name" : loginPending ? "Waiting for Tailscale…" : "Connect to Tailscale", for: .normal)
        action.titleLabel?.font = ZedStyle.mono(12, medium: true)
        action.setTitleColor(ZedStyle.text, for: .normal)
        action.backgroundColor = ZedStyle.surface
        action.layer.cornerRadius = 6
        action.layer.borderWidth = 1; action.layer.borderColor = ZedStyle.border.cgColor
        action.isEnabled = !payload.refreshing && !loginPending
        action.addTarget(self, action: connected ? #selector(saveDeviceName) : #selector(connectTailscale), for: .touchUpInside)
        action.accessibilityIdentifier = connected ? "save-tailscale-device-name" : "connect-tailscale"
        action.translatesAutoresizingMaskIntoConstraints = false
        footer.addSubview(action)
        NSLayoutConstraint.activate([
            heading.leadingAnchor.constraint(equalTo: footer.leadingAnchor, constant: 18),
            heading.topAnchor.constraint(equalTo: footer.topAnchor, constant: 24),
            introduction.leadingAnchor.constraint(equalTo: heading.leadingAnchor),
            introduction.trailingAnchor.constraint(equalTo: footer.trailingAnchor, constant: -18),
            introduction.topAnchor.constraint(equalTo: heading.bottomAnchor, constant: 10),
            fieldLabel.leadingAnchor.constraint(equalTo: heading.leadingAnchor),
            fieldLabel.topAnchor.constraint(equalTo: introduction.bottomAnchor, constant: 24),
            deviceNameField.leadingAnchor.constraint(equalTo: heading.leadingAnchor),
            deviceNameField.trailingAnchor.constraint(equalTo: introduction.trailingAnchor),
            deviceNameField.topAnchor.constraint(equalTo: fieldLabel.bottomAnchor, constant: 8),
            deviceNameField.heightAnchor.constraint(equalToConstant: 44),
            explanation.leadingAnchor.constraint(equalTo: heading.leadingAnchor),
            explanation.trailingAnchor.constraint(equalTo: introduction.trailingAnchor),
            explanation.topAnchor.constraint(equalTo: deviceNameField.bottomAnchor, constant: 10),
            action.leadingAnchor.constraint(equalTo: heading.leadingAnchor),
            action.trailingAnchor.constraint(equalTo: introduction.trailingAnchor),
            action.topAnchor.constraint(equalTo: explanation.bottomAnchor, constant: 20),
            action.heightAnchor.constraint(equalToConstant: 44),
        ])
        if connected {
            let signOut = UIButton(type: .system)
            signOut.setTitle("Sign out of Tailscale", for: .normal)
            signOut.titleLabel?.font = ZedStyle.mono(11)
            signOut.setTitleColor(.systemRed, for: .normal)
            signOut.addTarget(self, action: #selector(confirmSignOut), for: .touchUpInside)
            signOut.accessibilityIdentifier = "sign-out-tailscale"
            signOut.translatesAutoresizingMaskIntoConstraints = false
            footer.addSubview(signOut)
            NSLayoutConstraint.activate([
                signOut.leadingAnchor.constraint(equalTo: heading.leadingAnchor),
                signOut.topAnchor.constraint(equalTo: action.bottomAnchor, constant: 24),
                signOut.heightAnchor.constraint(equalToConstant: 44),
            ])
        }
        return footer
    }
    @objc private func connectTailscale() { Task { await EmbeddedTailscale.shared.signIn(deviceName: deviceNameField.text ?? "") } }
    @objc private func saveDeviceName() { deviceNameField.resignFirstResponder(); Task { await EmbeddedTailscale.shared.updateDeviceName(deviceNameField.text ?? "") } }
    @objc private func confirmSignOut() {
        let alert = UIAlertController(
            title: "Sign out of Tailscale?",
            message: "iZed Terminal will need to sign in again to reach your tailnet.",
            preferredStyle: .alert
        )
        alert.addAction(UIAlertAction(title: "Cancel", style: .cancel))
        alert.addAction(UIAlertAction(title: "Sign Out", style: .destructive) { _ in
            Task { await EmbeddedTailscale.shared.signOut() }
        })
        (navigationController?.topViewController ?? self).present(alert, animated: true)
    }
    @objc private func refresh() { terminal_refresh_machines() }
    @objc private func filter() {
        query = (search.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        updateNote()
        tableView.reloadData()
    }
    func textFieldShouldReturn(_ textField: UITextField) -> Bool { textField.resignFirstResponder(); return true }
    @objc private func close() { Self.current = nil; dismiss(animated: true) }
}

private final class MachineDeviceSettings: UIViewController {
    private let makeForm: () -> UIView
    private let scroll = UIScrollView()
    private var form: UIView?
    init(makeForm: @escaping () -> UIView) {
        self.makeForm = makeForm
        super.init(nibName: nil, bundle: nil)
    }
    required init?(coder: NSCoder) { fatalError() }
    override func viewDidLoad() {
        super.viewDidLoad()
        title = "Device settings"
        view.backgroundColor = ZedStyle.background
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.keyboardDismissMode = .interactive
        view.addSubview(scroll)
        NSLayoutConstraint.activate([
            scroll.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            scroll.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor),
            scroll.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: view.trailingAnchor),
        ])
        reload()
    }
    func reload() {
        guard isViewLoaded else { return }
        form?.removeFromSuperview()
        let content = makeForm()
        form = content
        let height = content.bounds.height
        content.translatesAutoresizingMaskIntoConstraints = false
        scroll.addSubview(content)
        NSLayoutConstraint.activate([
            content.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor),
            content.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor),
            content.leadingAnchor.constraint(equalTo: scroll.contentLayoutGuide.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: scroll.contentLayoutGuide.trailingAnchor),
            content.widthAnchor.constraint(equalTo: scroll.frameLayoutGuide.widthAnchor),
            content.heightAnchor.constraint(equalToConstant: height),
        ])
    }
}

private final class ConnectionEditor: UIViewController {
    let machine: Machine
    let publicKey: String
    let username = UITextField()
    let command = UITextField()
    let error = UILabel()
    init(machine: Machine, publicKey: String) {
        self.machine = machine; self.publicKey = publicKey
        super.init(nibName: nil, bundle: nil)
    }
    required init?(coder: NSCoder) { fatalError() }
    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = ZedStyle.background
        title = "Connect"
        let scroll = UIScrollView(); scroll.translatesAutoresizingMaskIntoConstraints = false; scroll.keyboardDismissMode = .interactive
        view.addSubview(scroll)
        let stack = UIStackView(); stack.axis = .vertical; stack.spacing = 12; stack.translatesAutoresizingMaskIntoConstraints = false
        scroll.addSubview(stack)
        NSLayoutConstraint.activate([
            scroll.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor), scroll.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor), scroll.leadingAnchor.constraint(equalTo: view.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            stack.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor, constant: 26), stack.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor, constant: -28), stack.leadingAnchor.constraint(equalTo: scroll.frameLayoutGuide.leadingAnchor, constant: 22), stack.trailingAnchor.constraint(equalTo: scroll.frameLayoutGuide.trailingAnchor, constant: -22),
        ])
        func label(_ text: String, _ size: CGFloat = 11, muted: Bool = true) -> UILabel {
            let label = UILabel(); label.text = text; label.numberOfLines = 0; label.font = ZedStyle.mono(size); label.textColor = muted ? ZedStyle.muted : ZedStyle.text; return label
        }
        func field(_ field: UITextField, _ placeholder: String, id: String) {
            field.placeholder = placeholder; field.font = ZedStyle.mono(14); field.textColor = ZedStyle.text
            field.backgroundColor = ZedStyle.surface; field.layer.cornerRadius = 5; field.layer.borderWidth = 1; field.layer.borderColor = ZedStyle.border.cgColor
            field.autocapitalizationType = .none; field.autocorrectionType = .no; field.keyboardType = .asciiCapable
            field.leftView = UIView(frame: CGRect(x: 0, y: 0, width: 12, height: 44)); field.leftViewMode = .always
            field.heightAnchor.constraint(equalToConstant: 48).isActive = true; field.accessibilityIdentifier = id
            stack.addArrangedSubview(field)
        }
        stack.addArrangedSubview(label(machine.name, 18, muted: false))
        stack.addArrangedSubview(label("\(machine.os) · \(machine.hostname.isEmpty ? machine.address : machine.hostname)", 10))
        stack.setCustomSpacing(30, after: stack.arrangedSubviews.last!)
        stack.addArrangedSubview(label("SSH USERNAME"))
        field(username, "Your account on this machine", id: "ssh-username")
        stack.addArrangedSubview(label("We’ll remember this for next time.", 10))
        let advanced = UIButton(type: .system)
        advanced.setTitle("Launch command (optional)", for: .normal); advanced.contentHorizontalAlignment = .left; advanced.titleLabel?.font = ZedStyle.mono(11); advanced.tintColor = ZedStyle.muted
        stack.addArrangedSubview(advanced)
        field(command, "e.g. tmux new-session -A -s phone", id: "connection-command")
        command.isHidden = true
        advanced.addAction(UIAction { [weak self] _ in self?.command.isHidden.toggle() }, for: .touchUpInside)
        error.font = ZedStyle.mono(11); error.textColor = .systemRed; error.numberOfLines = 0; stack.addArrangedSubview(error)
        let connect = UIButton(type: .system)
        connect.setTitle("Open terminal  →", for: .normal); connect.setTitleColor(ZedStyle.text, for: .normal)
        connect.titleLabel?.font = ZedStyle.mono(14, medium: true); connect.backgroundColor = UIColor(white: 0.12, alpha: 1)
        connect.layer.cornerRadius = 5; connect.layer.borderWidth = 1; connect.layer.borderColor = ZedStyle.border.cgColor
        connect.heightAnchor.constraint(equalToConstant: 50).isActive = true; connect.accessibilityIdentifier = "open-terminal"
        connect.addTarget(self, action: #selector(connectNow), for: .touchUpInside); stack.addArrangedSubview(connect)
        let key = UIButton(type: .system); key.setTitle("Copy this phone’s public SSH key", for: .normal); key.titleLabel?.font = ZedStyle.mono(10); key.tintColor = ZedStyle.muted
        key.heightAnchor.constraint(equalToConstant: 44).isActive = true
        key.addAction(UIAction { [weak self, weak key] _ in UIPasteboard.general.string = self?.publicKey; key?.setTitle("Public key copied", for: .normal) }, for: .touchUpInside)
        stack.addArrangedSubview(key)
    }
    @objc private func connectNow() {
        let user = (username.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !user.isEmpty && !user.contains("@") && user.rangeOfCharacter(from: .whitespacesAndNewlines) == nil else { error.text = "Enter your SSH username."; return }
        let target = "\(user)@\(machine.address.contains(":") ? "[\(machine.address)]" : machine.address)"
        let displayName = machine.name
        let launch = command.text ?? ""
        view.endEditing(true)
        MachinePicker.current = nil
        navigationController?.dismiss(animated: true) {
            displayName.withCString { n in target.withCString { a in launch.withCString { c in terminal_add_host(n, a, c) } } }
        }
    }
}
