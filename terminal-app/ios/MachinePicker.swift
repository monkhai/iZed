import UIKit
import Network

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
    let source: String
    let username: String
    let updated_at: UInt64
}
private struct PickerPayload: Decodable {
    let roster: Roster
    let saved: [SavedHost]
    let public_key: String
    let message: String
    let refreshing: Bool
    let has_session: Bool
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
    private let note = UILabel()
    private var checks: [String: NWConnection] = [:]
    private var reachability: [String: Bool] = [:]
    private var query = ""

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
        navigationItem.rightBarButtonItem = UIBarButtonItem(image: UIImage(systemName: "arrow.clockwise"), style: .plain, target: self, action: #selector(refresh))
        navigationItem.rightBarButtonItem?.accessibilityLabel = "Refresh machines"
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
        search.clearButtonMode = .whileEditing
        search.returnKeyType = .done
        search.delegate = self
        search.accessibilityIdentifier = "machine-search"
        search.addTarget(self, action: #selector(filter), for: .editingChanged)
        let icon = UIImageView(image: UIImage(systemName: "magnifyingglass"))
        icon.tintColor = ZedStyle.muted
        icon.contentMode = .center
        icon.frame = CGRect(x: 0, y: 0, width: 40, height: 44)
        search.leftView = icon
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
        checkSSH()
    }

    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        if navigationController?.presentingViewController == nil {
            Self.current = nil
            checks.values.forEach { $0.cancel() }
        }
    }

    func update(_ payload: PickerPayload) {
        self.payload = payload
        guard isViewLoaded else { return }
        updateNote()
        tableView.reloadData()
        navigationItem.rightBarButtonItem?.isEnabled = !payload.refreshing
        checkSSH()
    }

    private func updateNote() {
        if payload.refreshing { note.text = "Refreshing through your paired Mac…" }
        else if !payload.message.isEmpty { note.text = payload.message }
        else if payload.roster.machines.isEmpty { note.text = "Uses the Tailscale app already on your iPhone." }
        else {
            let age = max(0, Date().timeIntervalSince1970 - Double(payload.roster.updated_at))
            let time = age < 60 ? "just now" : age < 3600 ? "\(Int(age / 60))m ago" : "\(Int(age / 3600))h ago"
            note.text = "Your Tailscale machines · synced \(time)\nUses your existing Tailscale connection"
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
        label.text = section == 0 ? "  RECENT CONNECTIONS" : "  ON YOUR TAILNET"
        label.backgroundColor = ZedStyle.background
        return label
    }
    override func tableView(_ tableView: UITableView, heightForHeaderInSection section: Int) -> CGFloat { (section == 0 ? saved.count : machines.count) > 0 ? 34 : 0 }
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
            if let machine = payload.roster.machines.first(where: { host.matches($0) }), reachability[machine.address] == true {
                let dot = UIImageView(image: UIImage(systemName: "circle.fill"))
                dot.tintColor = ZedStyle.green; dot.frame = CGRect(x: 0, y: 0, width: 7, height: 7); cell.accessoryView = dot
            }
            cell.accessibilityIdentifier = "saved-\(host.address)"
        } else {
            let machine = machines[indexPath.row]
            cell.textLabel?.text = machine.name
            let ready = reachability[machine.address]
            let status = ready == true ? "SSH ready" : ready == false ? "No SSH response" : "Checking SSH…"
            cell.detailTextLabel?.text = "\(machine.os) · \(status)"
            cell.imageView?.image = UIImage(systemName: machine.os.lowercased() == "macos" ? (machine.name.lowercased().contains("macbook") ? "laptopcomputer" : "desktopcomputer") : "server.rack")
            let dot = UIImageView(image: UIImage(systemName: ready == true ? "circle.fill" : "chevron.right"))
            dot.tintColor = ready == true ? ZedStyle.green : ZedStyle.muted
            dot.frame = CGRect(x: 0, y: 0, width: ready == true ? 7 : 10, height: ready == true ? 7 : 13)
            cell.accessoryView = dot
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
        let detail = ConnectionEditor(machine: machine, username: payload.roster.username, publicKey: payload.public_key)
        navigationController?.pushViewController(detail, animated: true)
    }

    private func connect(_ host: SavedHost) {
        checks.values.forEach { $0.cancel() }
        Self.current = nil
        // A fast SSH probe can finish during the dismissal animation. Submit
        // only once the terminal is again the stable presenting controller.
        dismiss(animated: true) {
            host.name.withCString { name in host.address.withCString { address in host.command.withCString { command in terminal_add_host(name, address, command) } } }
        }
    }
    private func checkSSH() {
        for machine in payload.roster.machines where checks[machine.address] == nil {
            let connection = NWConnection(host: NWEndpoint.Host(machine.address), port: 22, using: .tcp)
            checks[machine.address] = connection
            connection.stateUpdateHandler = { [weak self, weak connection] state in
                guard let self else { return }
                let result: Bool?
                switch state { case .ready: result = true; case .failed: result = false; default: result = nil }
                guard let result else { return }
                DispatchQueue.main.async {
                    self.reachability[machine.address] = result
                    self.tableView.reloadData()
                }
                connection?.cancel()
            }
            connection.start(queue: DispatchQueue.global(qos: .utility))
            DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self, weak connection] in
                guard let self, self.reachability[machine.address] == nil else { return }
                self.reachability[machine.address] = false
                connection?.cancel()
                self.tableView.reloadData()
            }
        }
    }

    private func footer() -> UIView {
        let footer = UIView(frame: CGRect(x: 0, y: 0, width: 430, height: 150))
        let add = UIButton(type: .system)
        add.setTitle("＋  Add manually", for: .normal)
        add.titleLabel?.font = ZedStyle.mono(12)
        add.setTitleColor(ZedStyle.muted, for: .normal)
        add.addTarget(self, action: #selector(manual), for: .touchUpInside)
        add.accessibilityIdentifier = "add-manually"
        add.translatesAutoresizingMaskIntoConstraints = false
        footer.addSubview(add)
        let label = UILabel()
        label.text = "Uses Tailscale on this iPhone."
        label.font = ZedStyle.mono(10)
        label.textColor = ZedStyle.muted
        label.numberOfLines = 0
        label.translatesAutoresizingMaskIntoConstraints = false
        footer.addSubview(label)
        NSLayoutConstraint.activate([
            add.leadingAnchor.constraint(equalTo: footer.leadingAnchor, constant: 18), add.topAnchor.constraint(equalTo: footer.topAnchor, constant: 12), add.heightAnchor.constraint(equalToConstant: 44),
            label.leadingAnchor.constraint(equalTo: add.leadingAnchor), label.trailingAnchor.constraint(equalTo: footer.trailingAnchor, constant: -18), label.topAnchor.constraint(equalTo: add.bottomAnchor, constant: 20),
        ])
        return footer
    }
    @objc private func manual() { navigationController?.pushViewController(ConnectionEditor(machine: nil, username: payload.roster.username, publicKey: payload.public_key), animated: true) }
    @objc private func refresh() { checks.values.forEach { $0.cancel() }; checks.removeAll(); reachability.removeAll(); checkSSH(); terminal_refresh_machines() }
    @objc private func filter() { query = search.text ?? ""; tableView.reloadData() }
    func textFieldShouldReturn(_ textField: UITextField) -> Bool { textField.resignFirstResponder(); return true }
    @objc private func close() { checks.values.forEach { $0.cancel() }; Self.current = nil; dismiss(animated: true) }
}

private final class ConnectionEditor: UIViewController {
    let machine: Machine?
    let publicKey: String
    let username = UITextField()
    let address = UITextField()
    let name = UITextField()
    let command = UITextField()
    let error = UILabel()
    init(machine: Machine?, username: String, publicKey: String) {
        self.machine = machine; self.publicKey = publicKey
        super.init(nibName: nil, bundle: nil)
        self.username.text = username
    }
    required init?(coder: NSCoder) { fatalError() }
    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = ZedStyle.background
        title = machine == nil ? "Add connection" : "Connect"
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
        if let machine {
            stack.addArrangedSubview(label(machine.name, 18, muted: false))
            stack.addArrangedSubview(label("\(machine.os) · \(machine.hostname.isEmpty ? machine.address : machine.hostname)", 10))
            stack.setCustomSpacing(30, after: stack.arrangedSubviews.last!)
            stack.addArrangedSubview(label("SSH USERNAME"))
            field(username, "Your account on this Mac", id: "ssh-username")
            stack.addArrangedSubview(label("We’ll remember this for next time.", 10))
        } else {
            stack.addArrangedSubview(label("NAME")); field(name, "studio", id: "connection-name")
            stack.addArrangedSubview(label("SSH ADDRESS")); field(address, "user@hostname:22", id: "connection-address")
        }
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
        let target: String
        let displayName: String
        if let machine {
            let user = (username.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            guard !user.isEmpty && !user.contains("@") && user.rangeOfCharacter(from: .whitespacesAndNewlines) == nil else { error.text = "Enter your SSH username."; return }
            target = "\(user)@\(machine.address.contains(":") ? "[\(machine.address)]" : machine.address)"
            displayName = machine.name
        } else {
            target = (address.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            guard target.contains("@") else { error.text = "Use user@hostname, with an optional :port."; return }
            displayName = (name.text ?? "").isEmpty ? target : name.text!
        }
        let launch = command.text ?? ""
        view.endEditing(true)
        MachinePicker.current = nil
        navigationController?.dismiss(animated: true) {
            displayName.withCString { n in target.withCString { a in launch.withCString { c in terminal_add_host(n, a, c) } } }
        }
    }
}
